//! The model download as a background task for the app.
//!
//! A model download has no source row, so it does not fit the job queue (jobs target a
//! source). It is a task of its own: one worker thread at a time, cancellable, with its
//! state sent to an event sink after every change, like the job runner does. Nothing is
//! stored in the database: the state lives on disk, next to the model, so after a
//! restart the next download resumes from the partial files.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;

use super::{ModelError, ModelState, ModelStatus, ModelStore, Progress, Result};

/// Receives the model status after every change. Must not block: it is called from the
/// download thread.
pub trait ModelEventSink: Send + Sync {
    fn send(&self, status: ModelStatus);
}

impl<F: Fn(ModelStatus) + Send + Sync> ModelEventSink for F {
    fn send(&self, status: ModelStatus) {
        self(status)
    }
}

struct Running {
    id: u64,
    cancel: Arc<AtomicBool>,
    progress: Progress,
    worker: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct Inner {
    running: Option<Running>,
    /// Code of the last failed download, until the next one starts.
    last_error: Option<&'static str>,
    next_id: u64,
}

struct Shared {
    store: ModelStore,
    events: Arc<dyn ModelEventSink>,
    inner: Mutex<Inner>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn status(&self, inner: &Inner) -> Result<ModelStatus> {
        if let Some(running) = &inner.running {
            return Ok(ModelStatus {
                state: ModelState::Downloading,
                downloaded_bytes: running.progress.downloaded,
                total_bytes: running.progress.total,
                error_code: None,
            });
        }
        let mut status = self.store.status()?;
        if status.error_code.is_none() {
            status.error_code = inner.last_error;
        }
        Ok(status)
    }

    /// Sends the current status; a status that can't be read is reported, not sent.
    fn emit(&self) {
        let status = {
            let inner = self.lock();
            self.status(&inner)
        };
        match status {
            Ok(status) => self.events.send(status),
            Err(err) => eprintln!("trama: cannot read the model status: {err}"),
        }
    }
}

/// Downloads, checks and removes one model in the background.
pub struct ModelManager {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for ModelManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelManager")
            .field("store", &self.shared.store)
            .finish_non_exhaustive()
    }
}

impl ModelManager {
    pub fn new(store: ModelStore, events: Arc<dyn ModelEventSink>) -> Self {
        Self {
            shared: Arc::new(Shared {
                store,
                events,
                inner: Mutex::new(Inner::default()),
            }),
        }
    }

    /// The store, for the paths of a ready model.
    pub fn store(&self) -> &ModelStore {
        &self.shared.store
    }

    pub fn status(&self) -> Result<ModelStatus> {
        let inner = self.shared.lock();
        self.shared.status(&inner)
    }

    /// Starts downloading in the background, or resumes; does nothing if a download is
    /// already running. Progress and the outcome arrive as events.
    pub fn download(&self) -> Result<ModelStatus> {
        let status = {
            let mut inner = self.shared.lock();
            if inner.running.is_none() {
                let on_disk = self.shared.store.status()?;
                if on_disk.state != ModelState::Ready {
                    inner.last_error = None;
                    inner.next_id += 1;
                    let id = inner.next_id;
                    let cancel = Arc::new(AtomicBool::new(false));
                    let worker = {
                        let shared = Arc::clone(&self.shared);
                        let cancel = Arc::clone(&cancel);
                        std::thread::Builder::new()
                            .name("trama-model-download".into())
                            .spawn(move || work(&shared, id, &cancel))
                            .map_err(|err| ModelError::io(&self.shared.store.dir(), err))?
                    };
                    inner.running = Some(Running {
                        id,
                        cancel,
                        progress: Progress {
                            downloaded: on_disk.downloaded_bytes,
                            total: on_disk.total_bytes,
                        },
                        worker: Some(worker),
                    });
                }
            }
            self.shared.status(&inner)?
        };
        self.shared.events.send(status.clone());
        Ok(status)
    }

    /// Asks a running download to stop. The partial files stay, so the next download
    /// resumes; the final status arrives as an event when the download thread exits.
    pub fn cancel(&self) -> Result<ModelStatus> {
        let inner = self.shared.lock();
        if let Some(running) = &inner.running {
            running.cancel.store(true, Ordering::SeqCst);
        }
        self.shared.status(&inner)
    }

    /// Hashes every file again (several seconds for the transcription model). A file
    /// that doesn't match is deleted and the model becomes corrupt.
    pub fn verify(&self) -> Result<ModelStatus> {
        let status = {
            let mut inner = self.shared.lock();
            if inner.running.is_some() {
                return self.shared.status(&inner);
            }
            let verified = self.shared.store.verify()?;
            inner.last_error = verified.error_code;
            self.shared.status(&inner)?
        };
        self.shared.events.send(status.clone());
        Ok(status)
    }

    /// Stops a running download, waits for it, and deletes the model from disk.
    pub fn remove(&self) -> Result<ModelStatus> {
        self.stop_and_wait();
        let status = {
            let mut inner = self.shared.lock();
            if inner.running.is_none() {
                self.shared.store.remove()?;
                inner.last_error = None;
            }
            self.shared.status(&inner)?
        };
        self.shared.events.send(status.clone());
        Ok(status)
    }

    /// Stops a running download and waits for its thread; partial files stay for the
    /// next start. Called automatically on drop.
    pub fn shutdown(&self) {
        self.stop_and_wait();
    }

    fn stop_and_wait(&self) {
        let worker = {
            let mut inner = self.shared.lock();
            inner.running.as_mut().and_then(|running| {
                running.cancel.store(true, Ordering::SeqCst);
                running.worker.take()
            })
        };
        if let Some(worker) = worker {
            // A panic in the download is a bug; it has been reported on stderr already.
            let _ = worker.join();
        }
    }
}

impl Drop for ModelManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// The download thread.
fn work(shared: &Shared, id: u64, cancel: &AtomicBool) {
    let on_progress = |progress: Progress| {
        {
            let mut inner = shared.lock();
            match inner.running.as_mut() {
                Some(running) if running.id == id => running.progress = progress,
                _ => return,
            }
        }
        shared.events.send(ModelStatus {
            state: ModelState::Downloading,
            downloaded_bytes: progress.downloaded,
            total_bytes: progress.total,
            error_code: None,
        });
    };
    let outcome = shared.store.download(cancel, &on_progress);
    {
        let mut inner = shared.lock();
        if inner.running.as_ref().is_some_and(|r| r.id == id) {
            inner.running = None;
        }
        inner.last_error = match &outcome {
            Ok(()) | Err(ModelError::Cancelled) => None,
            Err(err) => {
                // Surfaced to the interface through the status, and logged (A4).
                eprintln!("trama: model download failed: {err}");
                Some(err.code())
            }
        };
    }
    shared.emit();
}
