//! Model download tests, headless, against a local fake HTTP server: no real download.
//!
//! The real files are checked by the `#[ignore]`d tests at the end:
//! `cargo test -p trama-core --test models -- --ignored`.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use tempfile::TempDir;
use trama_core::models::*;

const PATIENCE: Duration = Duration::from_secs(10);
const PLENTY: u64 = u64::MAX / 2;

// --- Fake server -------------------------------------------------------------------

/// A request the server received.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Request {
    path: String,
    range: Option<String>,
}

#[derive(Default)]
struct ServerState {
    files: HashMap<String, Vec<u8>>,
    /// Path → path it redirects to.
    redirects: HashMap<String, String>,
    /// Path → status answered instead of the file.
    statuses: HashMap<String, u16>,
    /// Close the connection after this many body bytes, once.
    drop_after: Option<usize>,
    /// Stop after this many body bytes, once, until `release` is set.
    hold_after: Option<usize>,
    release: bool,
    /// Answer 200 with the whole file even to range requests.
    ignore_range: bool,
    requests: Vec<Request>,
}

#[derive(Clone)]
struct Server {
    base: String,
    state: Arc<Mutex<ServerState>>,
}

impl Server {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(ServerState::default()));
        let shared = Arc::clone(&state);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let state = Arc::clone(&shared);
                std::thread::spawn(move || serve(stream, &state));
            }
        });
        Self { base, state }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    fn put(&self, path: &str, content: Vec<u8>) {
        self.state
            .lock()
            .unwrap()
            .files
            .insert(path.to_owned(), content);
    }

    fn with(&self, change: impl FnOnce(&mut ServerState)) {
        change(&mut self.state.lock().unwrap());
    }

    fn requests(&self) -> Vec<Request> {
        self.state.lock().unwrap().requests.clone()
    }

    fn requests_for(&self, path: &str) -> Vec<Request> {
        self.requests()
            .into_iter()
            .filter(|r| r.path == path)
            .collect()
    }
}

fn serve(stream: TcpStream, state: &Mutex<ServerState>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut first = String::new();
    if reader.read_line(&mut first).is_err() {
        return;
    }
    let path = first.split_whitespace().nth(1).unwrap_or("/").to_owned();
    let mut range = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("range")
        {
            range = Some(value.trim().to_owned());
        }
    }
    let mut out = stream;

    let (content, start, drop_after, hold_after) = {
        let mut st = state.lock().unwrap();
        st.requests.push(Request {
            path: path.clone(),
            range: range.clone(),
        });
        if let Some(target) = st.redirects.get(&path) {
            let _ = write!(
                out,
                "HTTP/1.1 302 Found\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            return;
        }
        if let Some(status) = st.statuses.get(&path) {
            let _ = write!(
                out,
                "HTTP/1.1 {status} Nope\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            return;
        }
        let Some(content) = st.files.get(&path).cloned() else {
            let _ = write!(
                out,
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            return;
        };
        let start = match (&range, st.ignore_range) {
            (Some(range), false) => range
                .strip_prefix("bytes=")
                .and_then(|r| r.strip_suffix('-'))
                .and_then(|s| s.parse::<usize>().ok()),
            _ => None,
        };
        (content, start, st.drop_after.take(), st.hold_after.take())
    };

    let len = content.len();
    let body = match start {
        Some(start) if start >= len => {
            let _ = write!(
                out,
                "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{len}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            return;
        }
        Some(start) => {
            let _ = write!(
                out,
                "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{}/{len}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                len - 1,
                len - start
            );
            &content[start..]
        }
        None => {
            let _ = write!(
                out,
                "HTTP/1.1 200 OK\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n"
            );
            &content[..]
        }
    };
    if let Some(n) = drop_after {
        let _ = out.write_all(&body[..n.min(body.len())]);
        let _ = out.flush();
        let _ = out.shutdown(std::net::Shutdown::Both);
        return;
    }
    if let Some(n) = hold_after {
        let n = n.min(body.len());
        let _ = out.write_all(&body[..n]);
        let _ = out.flush();
        while !state.lock().unwrap().release {
            std::thread::sleep(Duration::from_millis(5));
        }
        let _ = out.write_all(&body[n..]);
        return;
    }
    let _ = out.write_all(body);
}

// --- Helpers -----------------------------------------------------------------------

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Deterministic content that differs at every offset.
fn content(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| ((i * 31 + i / 251) as u8).wrapping_add(seed))
        .collect()
}

/// A model with a large file (served through a redirect) and a small one.
struct Fixture {
    _dir: TempDir,
    root: std::path::PathBuf,
    server: Server,
    spec: ModelSpec,
    big: Vec<u8>,
    small: Vec<u8>,
}

const BIG_LEN: usize = 1_000_000;

fn fixture() -> Fixture {
    let server = Server::start();
    let big = content(BIG_LEN, 1);
    let small = content(5_000, 7);
    server.put("/cdn/big.onnx", big.clone());
    server.put("/small.txt", small.clone());
    server.with(|s| {
        s.redirects
            .insert("/big.onnx".into(), "/cdn/big.onnx".into());
    });
    let spec = ModelSpec {
        id: "test-model".into(),
        files: vec![
            ModelFile {
                name: "big.onnx".into(),
                url: server.url("/big.onnx"),
                size: big.len() as u64,
                sha256: sha256(&big),
            },
            ModelFile {
                name: "small.txt".into(),
                url: server.url("/small.txt"),
                size: small.len() as u64,
                sha256: sha256(&small),
            },
        ],
    };
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Models");
    Fixture {
        _dir: dir,
        root,
        server,
        spec,
        big,
        small,
    }
}

impl Fixture {
    fn store(&self) -> ModelStore {
        ModelStore::new(&self.root, self.spec.clone())
            .with_free_space(|_| Ok(PLENTY))
            .with_retry_delay(Duration::ZERO)
    }

    fn total(&self) -> u64 {
        (self.big.len() + self.small.len()) as u64
    }
}

fn download(store: &ModelStore) -> std::result::Result<Vec<Progress>, ModelError> {
    let seen = Mutex::new(Vec::new());
    store.download(&AtomicBool::new(false), &|p| seen.lock().unwrap().push(p))?;
    Ok(seen.into_inner().unwrap())
}

fn state(store: &ModelStore) -> ModelState {
    store.status().unwrap().state
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

// --- Store -------------------------------------------------------------------------

#[test]
fn downloads_and_verifies_a_model() {
    let fx = fixture();
    let store = fx.store();
    let status = store.status().unwrap();
    assert_eq!(status.state, ModelState::Missing);
    assert_eq!(status.downloaded_bytes, 0);
    assert_eq!(status.total_bytes, fx.total());
    assert!(
        !store.dir().exists(),
        "nothing is created before a download"
    );

    let progress = download(&store).unwrap();

    assert_eq!(read(&store.file("big.onnx")), fx.big);
    assert_eq!(read(&store.file("small.txt")), fx.small);
    let status = store.status().unwrap();
    assert_eq!(status.state, ModelState::Ready);
    assert_eq!(status.downloaded_bytes, fx.total());
    assert_eq!(status.error_code, None);
    let sums = std::fs::read_to_string(store.dir().join("SHA256SUMS")).unwrap();
    assert!(sums.contains(&format!("{}  big.onnx\n", sha256(&fx.big))));

    // Progress goes up to the total, never backwards.
    assert!(
        progress
            .windows(2)
            .all(|w| w[0].downloaded <= w[1].downloaded)
    );
    assert_eq!(progress.last().unwrap().downloaded, fx.total());
    assert!(progress.len() > 10, "{} progress steps", progress.len());

    // A ready model is not downloaded again.
    let before = fx.server.requests().len();
    download(&store).unwrap();
    assert_eq!(fx.server.requests().len(), before);
}

#[test]
fn resumes_after_the_connection_drops() {
    let fx = fixture();
    fx.server.with(|s| s.drop_after = Some(300_000));
    let store = fx.store();

    download(&store).unwrap();

    assert_eq!(read(&store.file("big.onnx")), fx.big);
    assert_eq!(state(&store), ModelState::Ready);
    // The second request asked for the rest only, and the range survived the redirect.
    let requests = fx.server.requests_for("/cdn/big.onnx");
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert_eq!(requests[0].range, None);
    assert_eq!(requests[1].range.as_deref(), Some("bytes=300000-"));
}

#[test]
fn resumes_an_interrupted_download_after_a_restart() {
    let fx = fixture();
    let store = fx.store();
    let cancel = AtomicBool::new(false);
    let err = store
        .download(&cancel, &|p| {
            if p.downloaded >= 400_000 {
                cancel.store(true, Ordering::Relaxed);
            }
        })
        .unwrap_err();
    assert!(matches!(err, ModelError::Cancelled), "{err:?}");

    let status = store.status().unwrap();
    assert_eq!(status.state, ModelState::Missing);
    let kept = status.downloaded_bytes;
    assert!(kept >= 400_000 && kept < BIG_LEN as u64, "{kept}");
    assert!(store.dir().join("big.onnx.part").exists());
    drop(store);

    // A new store over the same folder, as after an app restart.
    let store = fx.store();
    let progress = download(&store).unwrap();
    assert_eq!(progress[0].downloaded, kept);
    assert_eq!(read(&store.file("big.onnx")), fx.big);
    assert_eq!(state(&store), ModelState::Ready);
    let requests = fx.server.requests_for("/cdn/big.onnx");
    assert_eq!(
        requests.last().unwrap().range,
        Some(format!("bytes={kept}-"))
    );
}

#[test]
fn a_wrong_checksum_marks_the_model_corrupt_until_downloaded_again() {
    let fx = fixture();
    // The server sends different bytes of the same size.
    fx.server.put("/small.txt", content(fx.small.len(), 99));
    let store = fx.store();

    let err = download(&store).unwrap_err();
    assert!(
        matches!(&err, ModelError::ChecksumMismatch { file } if file == "small.txt"),
        "{err:?}"
    );
    assert_eq!(err.code(), "checksum_mismatch");
    let status = store.status().unwrap();
    assert_eq!(status.state, ModelState::Corrupt);
    assert_eq!(status.error_code, Some("checksum_mismatch"));
    assert!(!store.file("small.txt").exists());
    assert!(!store.dir().join("small.txt.part").exists());
    // The good file is kept.
    assert_eq!(read(&store.file("big.onnx")), fx.big);

    // Upstream fixed: the next download fetches only the bad file.
    fx.server.put("/small.txt", fx.small.clone());
    let before = fx.server.requests_for("/cdn/big.onnx").len();
    download(&store).unwrap();
    assert_eq!(state(&store), ModelState::Ready);
    assert_eq!(fx.server.requests_for("/cdn/big.onnx").len(), before);
    assert_eq!(read(&store.file("small.txt")), fx.small);
}

#[test]
fn a_file_damaged_on_disk_is_detected_and_downloaded_again() {
    let fx = fixture();
    let store = fx.store();
    download(&store).unwrap();

    // Same size, one byte changed: only a full check notices.
    let mut bytes = fx.big.clone();
    bytes[123_456] ^= 0xff;
    std::fs::write(store.file("big.onnx"), &bytes).unwrap();
    assert_eq!(state(&store), ModelState::Ready);

    let status = store.verify().unwrap();
    assert_eq!(status.state, ModelState::Corrupt);
    assert_eq!(status.error_code, Some("checksum_mismatch"));
    assert!(!store.file("big.onnx").exists());
    assert_eq!(state(&store), ModelState::Corrupt);

    let small_requests = fx.server.requests_for("/small.txt").len();
    download(&store).unwrap();
    assert_eq!(state(&store), ModelState::Ready);
    assert_eq!(read(&store.file("big.onnx")), fx.big);
    assert_eq!(fx.server.requests_for("/small.txt").len(), small_requests);
    assert_eq!(store.verify().unwrap().state, ModelState::Ready);
}

#[test]
fn a_file_that_changed_size_on_disk_is_corrupt() {
    let fx = fixture();
    let store = fx.store();
    download(&store).unwrap();
    std::fs::write(store.file("small.txt"), b"short").unwrap();

    assert_eq!(state(&store), ModelState::Corrupt);
    download(&store).unwrap();
    assert_eq!(state(&store), ModelState::Ready);
    assert_eq!(read(&store.file("small.txt")), fx.small);
}

#[test]
fn a_bad_partial_file_is_downloaded_again_from_the_start() {
    let fx = fixture();
    let store = fx.store();
    std::fs::create_dir_all(store.dir()).unwrap();
    std::fs::write(store.dir().join("big.onnx.part"), content(200_000, 42)).unwrap();

    download(&store).unwrap();

    assert_eq!(read(&store.file("big.onnx")), fx.big);
    assert_eq!(state(&store), ModelState::Ready);
    let ranges: Vec<_> = fx
        .server
        .requests_for("/cdn/big.onnx")
        .into_iter()
        .map(|r| r.range)
        .collect();
    assert_eq!(ranges, vec![Some("bytes=200000-".to_owned()), None]);
}

#[test]
fn a_server_that_ignores_ranges_restarts_the_file() {
    let fx = fixture();
    let store = fx.store();
    std::fs::create_dir_all(store.dir()).unwrap();
    std::fs::write(store.dir().join("big.onnx.part"), &fx.big[..100_000]).unwrap();
    fx.server.with(|s| s.ignore_range = true);

    download(&store).unwrap();

    assert_eq!(read(&store.file("big.onnx")), fx.big);
    assert_eq!(fx.server.requests_for("/cdn/big.onnx").len(), 1);
}

#[test]
fn a_complete_partial_file_is_verified_without_downloading() {
    let fx = fixture();
    let store = fx.store();
    std::fs::create_dir_all(store.dir()).unwrap();
    std::fs::write(store.dir().join("big.onnx.part"), &fx.big).unwrap();

    download(&store).unwrap();

    assert_eq!(state(&store), ModelState::Ready);
    assert!(fx.server.requests_for("/big.onnx").is_empty());
}

#[test]
fn not_enough_space_stops_before_downloading() {
    let fx = fixture();
    let available = Arc::new(Mutex::new(0u64));
    let probe = Arc::clone(&available);
    let store = ModelStore::new(&fx.root, fx.spec.clone())
        .with_free_space(move |_| Ok(*probe.lock().unwrap()))
        .with_retry_delay(Duration::ZERO);

    let err = download(&store).unwrap_err();
    match err {
        ModelError::NotEnoughSpace { needed, available } => {
            assert_eq!(needed, fx.total() + SPACE_MARGIN);
            assert_eq!(available, 0);
        }
        other => panic!("{other:?}"),
    }
    assert!(fx.server.requests().is_empty());
    assert_eq!(state(&store), ModelState::Missing);

    // Only the bytes still missing count.
    std::fs::write(store.dir().join("big.onnx.part"), &fx.big[..600_000]).unwrap();
    let needed = fx.total() - 600_000 + SPACE_MARGIN;
    *available.lock().unwrap() = needed - 1;
    assert!(matches!(
        download(&store).unwrap_err(),
        ModelError::NotEnoughSpace { .. }
    ));
    *available.lock().unwrap() = needed;
    download(&store).unwrap();
    assert_eq!(state(&store), ModelState::Ready);
}

#[test]
fn a_missing_file_on_the_server_is_a_permanent_error() {
    let fx = fixture();
    fx.server.with(|s| {
        s.files.remove("/small.txt");
    });
    let store = fx.store();

    let err = download(&store).unwrap_err();
    assert!(
        matches!(err, ModelError::HttpStatus { status: 404, .. }),
        "{err:?}"
    );
    assert_eq!(err.code(), "http_status");
    assert_eq!(fx.server.requests_for("/small.txt").len(), 1);
    assert_eq!(state(&store), ModelState::Missing);
}

#[test]
fn a_busy_server_is_retried_then_reported() {
    let fx = fixture();
    fx.server.with(|s| {
        s.statuses.insert("/small.txt".into(), 503);
    });
    let store = fx.store();

    let err = download(&store).unwrap_err();
    assert!(
        matches!(err, ModelError::HttpStatus { status: 503, .. }),
        "{err:?}"
    );
    assert_eq!(fx.server.requests_for("/small.txt").len(), 4);
}

#[test]
fn a_file_of_the_wrong_size_is_rejected() {
    let fx = fixture();
    fx.server.put("/small.txt", content(4_000, 7));
    let store = fx.store();

    let err = download(&store).unwrap_err();
    assert!(
        matches!(
            err,
            ModelError::SizeMismatch {
                expected: 5_000,
                actual: 4_000,
                ..
            }
        ),
        "{err:?}"
    );
    assert_eq!(err.code(), "size_mismatch");
}

#[test]
fn an_unreachable_server_is_a_network_error() {
    let fx = fixture();
    // Nothing listens on this port once the listener is dropped.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut spec = fx.spec.clone();
    spec.files[0].url = format!("http://127.0.0.1:{port}/big.onnx");
    let store = ModelStore::new(&fx.root, spec)
        .with_free_space(|_| Ok(PLENTY))
        .with_retry_delay(Duration::ZERO);

    let err = download(&store).unwrap_err();
    assert_eq!(err.code(), "network", "{err:?}");
}

#[test]
fn removing_deletes_the_model_and_partial_files() {
    let fx = fixture();
    let store = fx.store();
    download(&store).unwrap();
    std::fs::write(store.dir().join("other.part"), b"x").unwrap();

    store.remove().unwrap();

    assert!(!store.dir().exists());
    assert!(fx.root.exists(), "only the model folder goes");
    assert_eq!(state(&store), ModelState::Missing);
    store.remove().unwrap();
}

// --- Manager -----------------------------------------------------------------------

struct Events(Receiver<ModelStatus>);

impl Events {
    fn sink() -> (Arc<dyn ModelEventSink>, Self) {
        let (tx, rx) = channel();
        let tx = Mutex::new(tx);
        let sink = move |status: ModelStatus| {
            let _ = tx.lock().unwrap().send(status);
        };
        (Arc::new(sink), Self(rx))
    }

    #[track_caller]
    fn wait_for(&self, want: impl Fn(&ModelStatus) -> bool) -> Vec<ModelStatus> {
        let deadline = Instant::now() + PATIENCE;
        let mut seen = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let Ok(status) = self.0.recv_timeout(left) else {
                panic!("status not received; seen: {seen:#?}");
            };
            let done = want(&status);
            seen.push(status);
            if done {
                return seen;
            }
        }
    }
}

fn is(state: ModelState) -> impl Fn(&ModelStatus) -> bool {
    move |status| status.state == state
}

#[test]
fn the_manager_downloads_in_the_background() {
    let fx = fixture();
    let (sink, events) = Events::sink();
    let manager = ModelManager::new(fx.store(), sink);
    assert_eq!(manager.status().unwrap().state, ModelState::Missing);

    let started = manager.download().unwrap();
    assert_eq!(started.state, ModelState::Downloading);
    // Asking again while it runs changes nothing.
    assert_eq!(manager.download().unwrap().state, ModelState::Downloading);

    let seen = events.wait_for(is(ModelState::Ready));
    assert!(
        seen.iter()
            .any(|s| s.state == ModelState::Downloading && s.downloaded_bytes > 0)
    );
    assert_eq!(manager.status().unwrap().state, ModelState::Ready);
    assert_eq!(
        fx.server.requests_for("/cdn/big.onnx").len(),
        1,
        "one download only"
    );
}

#[test]
fn the_manager_cancels_and_resumes() {
    let fx = fixture();
    fx.server.with(|s| s.hold_after = Some(300_000));
    let (sink, events) = Events::sink();
    let manager = ModelManager::new(fx.store(), sink);

    manager.download().unwrap();
    events.wait_for(|s| s.state == ModelState::Downloading && s.downloaded_bytes >= 300_000);
    manager.cancel().unwrap();
    fx.server.with(|s| s.release = true);

    let seen = events.wait_for(|s| s.state != ModelState::Downloading);
    let stopped = seen.last().unwrap();
    assert_eq!(stopped.state, ModelState::Missing);
    assert_eq!(stopped.error_code, None, "a cancel is not an error");
    let kept = stopped.downloaded_bytes;
    assert!(kept >= 300_000 && kept < BIG_LEN as u64, "{kept}");

    manager.download().unwrap();
    events.wait_for(is(ModelState::Ready));
    let requests = fx.server.requests_for("/cdn/big.onnx");
    assert_eq!(
        requests.last().unwrap().range,
        Some(format!("bytes={kept}-"))
    );
}

#[test]
fn the_manager_reports_a_failed_download() {
    let fx = fixture();
    fx.server.with(|s| {
        s.files.remove("/small.txt");
    });
    let (sink, events) = Events::sink();
    let manager = ModelManager::new(fx.store(), sink);

    manager.download().unwrap();
    let seen = events.wait_for(|s| s.state != ModelState::Downloading);
    let failed = seen.last().unwrap();
    assert_eq!(failed.state, ModelState::Missing);
    assert_eq!(failed.error_code, Some("http_status"));
    assert_eq!(manager.status().unwrap().error_code, Some("http_status"));
}

#[test]
fn the_manager_removes_the_model_even_while_downloading() {
    let fx = fixture();
    fx.server.with(|s| s.hold_after = Some(100_000));
    let (sink, events) = Events::sink();
    let manager = ModelManager::new(fx.store(), sink);

    manager.download().unwrap();
    events.wait_for(|s| s.downloaded_bytes >= 100_000);
    let server = fx.server.clone();
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        server.with(|s| s.release = true);
    });
    let status = manager.remove().unwrap();
    releaser.join().unwrap();

    assert_eq!(status.state, ModelState::Missing);
    assert_eq!(status.downloaded_bytes, 0);
    assert!(!manager.store().dir().exists());
}

#[test]
fn the_manager_verifies_the_model() {
    let fx = fixture();
    let (sink, events) = Events::sink();
    let manager = ModelManager::new(fx.store(), sink);
    manager.download().unwrap();
    events.wait_for(is(ModelState::Ready));

    std::fs::write(manager.store().file("small.txt"), content(5_000, 1)).unwrap();
    let status = manager.verify().unwrap();
    assert_eq!(status.state, ModelState::Corrupt);
    assert_eq!(status.error_code, Some("checksum_mismatch"));

    manager.download().unwrap();
    events.wait_for(is(ModelState::Ready));
}

// --- Real files (network, manual) ---------------------------------------------------

/// Checks that every pinned URL still serves a file of the pinned size, with ranges.
#[test]
#[ignore = "network: run manually"]
fn the_pinned_urls_serve_the_pinned_sizes() {
    let spec = transcription_model();
    for file in &spec.files {
        let response = ureq::get(&file.url)
            .header("Range", "bytes=0-0")
            .call()
            .unwrap();
        assert_eq!(response.status().as_u16(), 206, "{}", file.url);
        let range = response.headers()["content-range"].to_str().unwrap();
        assert_eq!(
            range,
            format!("bytes 0-0/{}", file.size),
            "{} has another size",
            file.name
        );
    }
}

/// Downloads the real transcription model (about 2.5 GB) into `TRAMA_MODELS`, or a
/// temporary folder.
#[test]
#[ignore = "network: downloads about 2.5 GB"]
fn the_real_transcription_model_downloads() {
    let tmp = tempfile::tempdir().unwrap();
    let root = std::env::var_os("TRAMA_MODELS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| tmp.path().to_owned());
    let store = ModelStore::new(root, transcription_model());
    store
        .download(&AtomicBool::new(false), &|p| {
            eprint!("\r{:.1}%", p.fraction() * 100.0);
        })
        .unwrap();
    eprintln!();
    assert_eq!(store.verify().unwrap().state, ModelState::Ready);
}
