//! Transfer of one file over HTTP, resuming a partial file with a range request and
//! hashing while writing.

use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};

use super::{ModelError, ModelFile, Progress, Result, file_len, hash_into, hex, remove_if_exists};

/// Bytes read and written at a time.
pub(super) const CHUNK: usize = 256 * 1024;

/// The HTTP client: no compression (sizes and ranges must refer to the file itself),
/// HTTP errors returned as responses, and a timeout on every read so a stalled
/// connection is noticed and resumed.
pub(super) fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .timeout_per_read(Some(Duration::from_secs(60)))
        .user_agent(concat!("Trama/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// Turns bytes written into progress callbacks, about one per thousandth of the model.
pub(super) struct Reporter<'a> {
    total: u64,
    downloaded: u64,
    last: u64,
    step: u64,
    sink: &'a dyn Fn(Progress),
}

impl<'a> Reporter<'a> {
    pub(super) fn new(total: u64, downloaded: u64, sink: &'a dyn Fn(Progress)) -> Self {
        Self {
            total,
            downloaded,
            last: downloaded,
            step: (total / 1000).max(1),
            sink,
        }
    }

    pub(super) fn downloaded(&self) -> u64 {
        self.downloaded
    }

    pub(super) fn report(&mut self, force: bool) {
        if force
            || self.downloaded.abs_diff(self.last) >= self.step
            || self.downloaded == self.total
        {
            self.last = self.downloaded;
            (self.sink)(Progress {
                downloaded: self.downloaded,
                total: self.total,
            });
        }
    }

    fn add(&mut self, bytes: u64) {
        self.downloaded += bytes;
        self.report(false);
    }

    fn sub(&mut self, bytes: u64) {
        self.downloaded = self.downloaded.saturating_sub(bytes);
        self.report(true);
    }
}

/// How a transfer ended, when it reached the end of the file.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Fetched {
    /// The part file is complete and matches its checksum.
    Verified,
    /// A fresh download doesn't match: upstream serves something else.
    BadChecksum,
    /// A resumed download doesn't match; the part was deleted, start over.
    ResumedBadChecksum,
}

/// Downloads `file` into `part`, continuing from the bytes already there, which the
/// reporter has already counted.
pub(super) fn fetch(
    agent: &ureq::Agent,
    file: &ModelFile,
    part: &Path,
    cancel: &AtomicBool,
    reporter: &mut Reporter<'_>,
) -> Result<Fetched> {
    let io_err = |err| ModelError::io(part, err);
    let mut have = file_len(part)?.unwrap_or(0);

    if have < file.size {
        let mut request = agent.get(&file.url);
        if have > 0 {
            request = request.header("Range", format!("bytes={have}-"));
        }
        let response = request.call().map_err(|err| network(file, err))?;
        match response.status().as_u16() {
            206 => {
                let range = header(&response, "content-range").and_then(parse_content_range);
                match range {
                    Some((start, total)) if start == have => {
                        if let Some(total) = total
                            && total != file.size
                        {
                            return Err(size_mismatch(file, total));
                        }
                    }
                    _ => {
                        restart(part, &mut have, reporter)?;
                        return Err(network(file, "the server sent another range"));
                    }
                }
            }
            200 => {
                if let Some(len) = header(&response, "content-length").and_then(|v| v.parse().ok())
                    && len != file.size
                {
                    return Err(size_mismatch(file, len));
                }
                // The server ignored the range: start over.
                restart(part, &mut have, reporter)?;
            }
            416 => {
                restart(part, &mut have, reporter)?;
                return Err(network(file, "range not satisfiable"));
            }
            status => {
                return Err(ModelError::HttpStatus {
                    file: file.name.clone(),
                    status,
                });
            }
        }

        let mut hasher = Sha256::new();
        if have > 0 {
            hash_into(part, &mut hasher, have).map_err(io_err)?;
        }
        let resumed = have > 0;
        let mut out = OpenOptions::new()
            .create(true)
            .append(true)
            .open(part)
            .map_err(io_err)?;
        let mut body = response.into_body().into_reader();
        let mut buf = vec![0; CHUNK];
        while have < file.size {
            if cancel.load(Ordering::Relaxed) {
                return Err(ModelError::Cancelled);
            }
            let n = match body.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n as u64,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) => return Err(network(file, err)),
            };
            if have + n > file.size {
                drop(out);
                restart(part, &mut have, reporter)?;
                return Err(size_mismatch(file, have + n));
            }
            let chunk = &buf[..n as usize];
            out.write_all(chunk).map_err(io_err)?;
            hasher.update(chunk);
            have += n;
            reporter.add(n);
        }
        if have < file.size {
            return Err(network(file, "the connection closed before the end"));
        }
        out.sync_all().map_err(io_err)?;
        return finish(file, part, hasher, resumed, reporter);
    }

    // The part is complete already (stopped between the last byte and the rename).
    let mut hasher = Sha256::new();
    hash_into(part, &mut hasher, have).map_err(io_err)?;
    finish(file, part, hasher, true, reporter)
}

fn finish(
    file: &ModelFile,
    part: &Path,
    hasher: Sha256,
    resumed: bool,
    reporter: &mut Reporter<'_>,
) -> Result<Fetched> {
    if hex(&hasher.finalize()) == file.sha256 {
        return Ok(Fetched::Verified);
    }
    remove_if_exists(part)?;
    reporter.sub(file.size);
    Ok(if resumed {
        Fetched::ResumedBadChecksum
    } else {
        Fetched::BadChecksum
    })
}

/// Drops a partial file to download it again from the start.
fn restart(part: &Path, have: &mut u64, reporter: &mut Reporter<'_>) -> Result<()> {
    remove_if_exists(part)?;
    reporter.sub(*have);
    *have = 0;
    Ok(())
}

fn header<'r>(response: &'r ureq::http::Response<ureq::Body>, name: &str) -> Option<&'r str> {
    response.headers().get(name)?.to_str().ok()
}

/// Parses `bytes <start>-<end>/<total>` into the start and, if known, the total.
fn parse_content_range(value: &str) -> Option<(u64, Option<u64>)> {
    let rest = value.trim().strip_prefix("bytes ")?;
    let (range, total) = rest.split_once('/')?;
    let (start, _end) = range.split_once('-')?;
    let total = match total.trim() {
        "*" => None,
        total => Some(total.parse().ok()?),
    };
    Some((start.trim().parse().ok()?, total))
}

fn network(file: &ModelFile, detail: impl ToString) -> ModelError {
    ModelError::Network {
        file: file.name.clone(),
        detail: detail.to_string(),
    }
}

fn size_mismatch(file: &ModelFile, actual: u64) -> ModelError {
    ModelError::SizeMismatch {
        file: file.name.clone(),
        expected: file.size,
        actual,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_range_is_parsed() {
        assert_eq!(
            parse_content_range("bytes 100-199/200"),
            Some((100, Some(200)))
        );
        assert_eq!(parse_content_range("bytes 5-9/*"), Some((5, None)));
        assert_eq!(parse_content_range("bytes */200"), None);
        assert_eq!(parse_content_range("items 1-2/3"), None);
    }

    #[test]
    fn progress_is_reported_about_every_thousandth() {
        let seen = std::cell::RefCell::new(Vec::new());
        let sink = |p: Progress| seen.borrow_mut().push(p.downloaded);
        let mut reporter = Reporter::new(10_000, 0, &sink);
        for _ in 0..100 {
            reporter.add(5);
        }
        reporter.sub(500);
        let expected: Vec<u64> = (1..=50).map(|i| i * 10).chain([0]).collect();
        assert_eq!(*seen.borrow(), expected);
    }
}
