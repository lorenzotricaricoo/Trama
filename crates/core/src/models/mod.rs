//! Downloaded models: fetching them from a pinned URL, checking them, and removing them.
//!
//! The transcription model (about 2.5 GB) is not bundled with the app (ADR 0007): the
//! user downloads it on purpose. [`ModelStore`] keeps one model in its own folder inside
//! the models folder it is given, like [`crate::storage::Library::open`] takes the
//! library folder. Models live outside the library: they are not user data, and a
//! library backup should not carry them.
//!
//! A download is safe to interrupt. Each file is written to `<name>.part` and resumed
//! with an HTTP range request; it gets its final name only after its SHA-256 matches the
//! pinned one. When every file is in place the store writes `SHA256SUMS`, which marks
//! the model ready without hashing 2.5 GB at every start. A file that fails its checksum
//! is deleted and the model is reported corrupt until it is downloaded again.
//!
//! [`ModelManager`] runs the download on a background thread for the app, with progress
//! events and cancellation.

mod catalog;
mod download;
mod manager;

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};

pub use catalog::{TRANSCRIPTION_MODEL_ID, files, transcription_model};
pub use manager::{ModelEventSink, ModelManager};

/// Marker written when every file is in place and verified: one `<sha256>  <name>` line
/// per file, the format `shasum -a 256 -c` reads.
const READY_MARKER: &str = "SHA256SUMS";

/// Marker written when a file failed its checksum: the names of the files involved.
const CORRUPT_MARKER: &str = "CORRUPT";

/// Extension of a file being downloaded.
const PART_EXTENSION: &str = "part";

/// Free space left on the disk after the download, so the system is not starved.
pub const SPACE_MARGIN: u64 = 256 * 1024 * 1024;

/// One file of a model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFile {
    /// Name inside the model folder.
    pub name: String,
    /// Where to download it from.
    pub url: String,
    /// Size in bytes.
    pub size: u64,
    /// Expected SHA-256, lowercase hex.
    pub sha256: String,
}

/// A model: the files that make it up, all pinned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSpec {
    /// Folder name inside the models folder; changes with every pinned version.
    pub id: String,
    pub files: Vec<ModelFile>,
}

impl ModelSpec {
    /// Bytes of all the files together.
    pub fn total_size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    /// The content of the ready marker for this model.
    fn checksums(&self) -> String {
        self.files
            .iter()
            .map(|f| format!("{}  {}\n", f.sha256, f.name))
            .collect()
    }
}

/// State of a model, as the interface shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelState {
    /// Not on disk, or only partly (a download can resume).
    Missing,
    /// Being downloaded now.
    Downloading,
    /// On disk and verified.
    Ready,
    /// A file failed its checksum or changed on disk; download it again.
    Corrupt,
}

impl ModelState {
    /// Stable name for the interface.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Downloading => "downloading",
            Self::Ready => "ready",
            Self::Corrupt => "corrupt",
        }
    }
}

/// What the interface needs to show a model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelStatus {
    pub state: ModelState,
    /// Bytes already on disk, including partial files.
    pub downloaded_bytes: u64,
    /// Bytes of the whole model.
    pub total_bytes: u64,
    /// Code of the last failed download or check, for the interface to translate.
    pub error_code: Option<&'static str>,
}

/// Progress of a download, in bytes of the whole model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub downloaded: u64,
    pub total: u64,
}

impl Progress {
    /// From 0 to 1.
    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            1.0
        } else {
            (self.downloaded as f64 / self.total as f64) as f32
        }
    }
}

/// Why a download or a check failed.
#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("not enough free space: {needed} bytes needed, {available} available")]
    NotEnoughSpace { needed: u64, available: u64 },
    #[error("network error while downloading {file}: {detail}")]
    Network { file: String, detail: String },
    #[error("the server answered {status} for {file}")]
    HttpStatus { file: String, status: u16 },
    #[error("{file} has {actual} bytes on the server, {expected} expected")]
    SizeMismatch {
        file: String,
        expected: u64,
        actual: u64,
    },
    #[error("{file} does not match its checksum")]
    ChecksumMismatch { file: String },
    #[error("the download was cancelled")]
    Cancelled,
    #[error("cannot use {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

impl ModelError {
    /// Stable code for the interface to translate.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotEnoughSpace { .. } => "not_enough_space",
            Self::Network { .. } => "network",
            Self::HttpStatus { .. } => "http_status",
            Self::SizeMismatch { .. } => "size_mismatch",
            Self::ChecksumMismatch { .. } => "checksum_mismatch",
            Self::Cancelled => "cancelled",
            Self::Io { .. } => "io",
        }
    }

    /// Whether trying again soon may work: a dropped connection or a busy server.
    fn retryable(&self) -> bool {
        match self {
            Self::Network { .. } => true,
            Self::HttpStatus { status, .. } => *status == 429 || *status >= 500,
            _ => false,
        }
    }

    fn io(path: &Path, source: io::Error) -> Self {
        Self::Io {
            path: path.to_owned(),
            source,
        }
    }
}

/// Result of model operations.
pub type Result<T> = std::result::Result<T, ModelError>;

/// Measures the free space available to the app at a path.
pub type FreeSpace = dyn Fn(&Path) -> io::Result<u64> + Send + Sync;

/// One model on disk: its state, its download, its removal.
pub struct ModelStore {
    root: PathBuf,
    spec: ModelSpec,
    agent: ureq::Agent,
    free_space: Arc<FreeSpace>,
    retry_delay: Duration,
}

impl std::fmt::Debug for ModelStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelStore")
            .field("root", &self.root)
            .field("spec", &self.spec.id)
            .finish_non_exhaustive()
    }
}

impl ModelStore {
    /// A store for `spec` inside `models_dir`. Nothing is touched on disk until a
    /// download starts.
    pub fn new(models_dir: impl Into<PathBuf>, spec: ModelSpec) -> Self {
        Self {
            root: models_dir.into(),
            spec,
            agent: download::agent(),
            free_space: Arc::new(|path: &Path| fs4::available_space(path)),
            retry_delay: Duration::from_secs(2),
        }
    }

    /// Replaces the free-space probe, for tests.
    pub fn with_free_space(
        mut self,
        probe: impl Fn(&Path) -> io::Result<u64> + Send + Sync + 'static,
    ) -> Self {
        self.free_space = Arc::new(probe);
        self
    }

    /// Changes the first pause before retrying after a network error (it doubles at
    /// every retry), for tests.
    pub fn with_retry_delay(mut self, delay: Duration) -> Self {
        self.retry_delay = delay;
        self
    }

    pub fn spec(&self) -> &ModelSpec {
        &self.spec
    }

    /// The model folder.
    pub fn dir(&self) -> PathBuf {
        self.root.join(&self.spec.id)
    }

    /// Path of one file of the model; only usable when the model is ready.
    pub fn file(&self, name: &str) -> PathBuf {
        self.dir().join(name)
    }

    fn part(&self, file: &ModelFile) -> PathBuf {
        self.dir().join(format!("{}.{PART_EXTENSION}", file.name))
    }

    /// The state on disk, without hashing: never [`ModelState::Downloading`] (only the
    /// [`ModelManager`] knows that).
    pub fn status(&self) -> Result<ModelStatus> {
        let dir = self.dir();
        let total = self.spec.total_size();
        let status = |state, downloaded| ModelStatus {
            state,
            downloaded_bytes: downloaded,
            total_bytes: total,
            error_code: (state == ModelState::Corrupt).then_some("checksum_mismatch"),
        };
        if !dir.exists() {
            return Ok(status(ModelState::Missing, 0));
        }
        let mut downloaded = 0;
        let mut complete = true;
        let mut corrupt = dir.join(CORRUPT_MARKER).exists();
        for file in &self.spec.files {
            match file_len(&self.file(&file.name))? {
                Some(len) if len == file.size => downloaded += len,
                // A verified file that changed size afterwards.
                Some(_) => {
                    corrupt = true;
                    complete = false;
                }
                None => {
                    complete = false;
                    let part = file_len(&self.part(file))?.unwrap_or(0);
                    downloaded += part.min(file.size);
                }
            }
        }
        if corrupt {
            return Ok(status(ModelState::Corrupt, downloaded));
        }
        let marker = self.dir().join(READY_MARKER);
        let verified = match fs::read_to_string(&marker) {
            Ok(content) => content == self.spec.checksums(),
            Err(err) if err.kind() == io::ErrorKind::NotFound => false,
            Err(err) => return Err(ModelError::io(&marker, err)),
        };
        let state = if complete && verified {
            ModelState::Ready
        } else {
            ModelState::Missing
        };
        Ok(status(state, downloaded))
    }

    /// Downloads what is missing, resuming partial files, and verifies every file.
    ///
    /// Network errors are retried a few times, resuming where the transfer stopped;
    /// after that the error is returned and a later call resumes. `cancel` stops the
    /// download soon after it becomes true, keeping what was downloaded.
    pub fn download(&self, cancel: &AtomicBool, progress: &dyn Fn(Progress)) -> Result<()> {
        let dir = self.dir();
        fs::create_dir_all(&dir).map_err(|err| ModelError::io(&dir, err))?;
        remove_if_exists(&dir.join(CORRUPT_MARKER))?;

        // Files still needed, and the bytes already there.
        let mut needed = Vec::new();
        let mut done = 0;
        for file in &self.spec.files {
            let path = self.file(&file.name);
            match file_len(&path)? {
                Some(len) if len == file.size => done += len,
                Some(_) => {
                    remove_if_exists(&path)?;
                    needed.push(file);
                }
                None => {
                    let part = file_len(&self.part(file))?.unwrap_or(0);
                    if part > file.size {
                        remove_if_exists(&self.part(file))?;
                    } else {
                        done += part;
                    }
                    needed.push(file);
                }
            }
        }

        let total = self.spec.total_size();
        let remaining = total - done;
        if remaining > 0 {
            let available = (self.free_space)(&dir).map_err(|err| ModelError::io(&dir, err))?;
            let wanted = remaining + SPACE_MARGIN;
            if available < wanted {
                return Err(ModelError::NotEnoughSpace {
                    needed: wanted,
                    available,
                });
            }
        }

        let mut reporter = download::Reporter::new(total, done, progress);
        reporter.report(true);
        for file in needed {
            self.fetch_with_retries(file, cancel, &mut reporter)?;
        }
        self.write_ready_marker()
    }

    /// Downloads one file, retrying network errors and a resumed file that failed its
    /// checksum (the old part may have come from a different upstream file).
    fn fetch_with_retries(
        &self,
        file: &ModelFile,
        cancel: &AtomicBool,
        reporter: &mut download::Reporter<'_>,
    ) -> Result<()> {
        const MAX_FAILED_ATTEMPTS: u32 = 4;
        let part = self.part(file);
        let mut failed = 0;
        let mut delay = self.retry_delay;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(ModelError::Cancelled);
            }
            let before = reporter.downloaded();
            match download::fetch(&self.agent, file, &part, cancel, reporter) {
                Ok(download::Fetched::Verified) => break,
                Ok(download::Fetched::ResumedBadChecksum) => {
                    // The part is gone: the next attempt starts this file over.
                    failed += 1;
                    if failed >= MAX_FAILED_ATTEMPTS {
                        return Err(self.mark_corrupt(file));
                    }
                }
                Ok(download::Fetched::BadChecksum) => return Err(self.mark_corrupt(file)),
                Err(err) if err.retryable() => {
                    // Attempts that moved forward don't count against the limit.
                    if reporter.downloaded() == before {
                        failed += 1;
                    }
                    if failed >= MAX_FAILED_ATTEMPTS {
                        return Err(err);
                    }
                    sleep_unless_cancelled(delay, cancel)?;
                    delay = delay.saturating_mul(2);
                }
                Err(err) => return Err(err),
            }
        }
        let path = self.file(&file.name);
        fs::rename(&part, &path).map_err(|err| ModelError::io(&path, err))
    }

    /// Records that `file` failed its checksum, and returns the error to report.
    fn mark_corrupt(&self, file: &ModelFile) -> ModelError {
        let marker = self.dir().join(CORRUPT_MARKER);
        let mut content = fs::read_to_string(&marker).unwrap_or_default();
        content.push_str(&file.name);
        content.push('\n');
        match write_atomically(&marker, content.as_bytes()) {
            Ok(()) => ModelError::ChecksumMismatch {
                file: file.name.clone(),
            },
            Err(err) => err,
        }
    }

    fn write_ready_marker(&self) -> Result<()> {
        write_atomically(
            &self.dir().join(READY_MARKER),
            self.spec.checksums().as_bytes(),
        )
    }

    /// Hashes every downloaded file again. A file that doesn't match is deleted and the
    /// model becomes corrupt; a complete model that matches becomes ready.
    pub fn verify(&self) -> Result<ModelStatus> {
        let mut bad = Vec::new();
        let mut complete = true;
        for file in &self.spec.files {
            let path = self.file(&file.name);
            match file_len(&path)? {
                Some(len) if len == file.size => {
                    if hash_file(&path)? != file.sha256 {
                        bad.push(file);
                    }
                }
                Some(_) => bad.push(file),
                None => complete = false,
            }
        }
        if !bad.is_empty() {
            remove_if_exists(&self.dir().join(READY_MARKER))?;
            for file in bad {
                remove_if_exists(&self.file(&file.name))?;
                if let err @ ModelError::Io { .. } = self.mark_corrupt(file) {
                    return Err(err);
                }
            }
        } else if complete {
            self.write_ready_marker()?;
        }
        self.status()
    }

    /// Deletes the model folder, partial files included.
    pub fn remove(&self) -> Result<()> {
        let dir = self.dir();
        match fs::remove_dir_all(&dir) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(ModelError::io(&dir, err)),
        }
    }
}

fn file_len(path: &Path) -> Result<Option<u64>> {
    match fs::metadata(path) {
        Ok(meta) => Ok(Some(meta.len())),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(ModelError::io(path, err)),
    }
}

fn remove_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(ModelError::io(path, err)),
    }
}

/// Writes a small file so that it is either all there or not there at all.
fn write_atomically(path: &Path, content: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    let write = || -> io::Result<()> {
        let mut file = File::create(&tmp)?;
        file.write_all(content)?;
        file.sync_all()?;
        fs::rename(&tmp, path)
    };
    write().map_err(|err| ModelError::io(path, err))
}

/// SHA-256 of a whole file, lowercase hex.
fn hash_file(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    hash_into(path, &mut hasher, u64::MAX).map_err(|err| ModelError::io(path, err))?;
    Ok(hex(&hasher.finalize()))
}

/// Feeds up to `limit` bytes of a file to `hasher`.
fn hash_into(path: &Path, hasher: &mut Sha256, limit: u64) -> io::Result<u64> {
    let mut reader = File::open(path)?.take(limit);
    let mut buf = vec![0; download::CHUNK];
    let mut read = 0;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            return Ok(read);
        }
        hasher.update(&buf[..n]);
        read += n as u64;
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sleep_unless_cancelled(delay: Duration, cancel: &AtomicBool) -> Result<()> {
    let step = Duration::from_millis(50);
    let mut left = delay;
    while !left.is_zero() {
        if cancel.load(Ordering::Relaxed) {
            return Err(ModelError::Cancelled);
        }
        let now = left.min(step);
        std::thread::sleep(now);
        left -= now;
    }
    Ok(())
}
