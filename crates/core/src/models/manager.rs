//! The model download and check as background tasks for the app.
//!
//! A model download has no source row, so it does not fit the job queue (jobs target a
//! source). It is a task of its own: one worker thread at a time (a download or a
//! check), cancellable, with the status sent to an event sink after every change, like
//! the job runner does. Nothing is stored in the database: the state lives on disk, next
//! to the model, so after a restart the next download resumes from the partial files.
//!
//! Every event is sent while the manager's lock is held, so events arrive in the order
//! of the changes: a fast failure can't be overtaken by the status announcing the start.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use super::{ModelError, ModelState, ModelStatus, ModelStore, Progress, Result};

/// Error code of a task that panicked; a bug.
pub const PANICKED: &str = "panicked";

/// Receives the model status after every change. It is called with the manager's lock
/// held, so it must not block and must not call the manager.
pub trait ModelEventSink: Send + Sync {
    fn send(&self, status: ModelStatus);
}

impl<F: Fn(ModelStatus) + Send + Sync> ModelEventSink for F {
    fn send(&self, status: ModelStatus) {
        self(status)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Task {
    Download,
    Verify,
}

struct Running {
    id: u64,
    task: Task,
    cancel: Arc<AtomicBool>,
    downloaded: u64,
    total: u64,
    progress: f32,
}

#[derive(Default)]
struct Inner {
    running: Option<Running>,
    /// A removal is waiting for the running task to stop: no new task may start.
    removing: bool,
    /// Code of the last failed task, until the next one starts.
    last_error: Option<&'static str>,
    next_id: u64,
}

struct Shared {
    store: ModelStore,
    events: Arc<dyn ModelEventSink>,
    inner: Mutex<Inner>,
    /// Signalled when a task ends.
    idle: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The status; reads only file sizes, never hashes.
    fn status(&self, inner: &Inner) -> Result<ModelStatus> {
        if let Some(running) = &inner.running {
            return Ok(ModelStatus {
                state: match running.task {
                    Task::Download => ModelState::Downloading,
                    Task::Verify => ModelState::Verifying,
                },
                downloaded_bytes: running.downloaded,
                total_bytes: running.total,
                error_code: None,
                progress: Some(running.progress),
            });
        }
        let mut status = self.store.status()?;
        if status.error_code.is_none() {
            status.error_code = inner.last_error;
        }
        Ok(status)
    }

    /// Sends the current status; the lock must be held, which keeps events in order.
    fn emit(&self, inner: &Inner) -> Result<ModelStatus> {
        let status = self.status(inner)?;
        self.events.send(status.clone());
        Ok(status)
    }

    /// Like [`Self::emit`], for the worker, which has no caller to return an error to.
    fn emit_or_log(&self, inner: &Inner) {
        if let Err(err) = self.emit(inner) {
            eprintln!("trama: cannot read the model status: {err}");
        }
    }

    /// Cancels the running task and waits until it has ended.
    fn stop_and_wait<'a>(&'a self, mut inner: MutexGuard<'a, Inner>) -> MutexGuard<'a, Inner> {
        while let Some(running) = &inner.running {
            running.cancel.store(true, Ordering::SeqCst);
            inner = self
                .idle
                .wait(inner)
                .unwrap_or_else(PoisonError::into_inner);
        }
        inner
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
                idle: Condvar::new(),
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

    /// Starts downloading in the background, or resumes. Does nothing if the model is
    /// ready or a task is already running. Progress and the outcome arrive as events.
    pub fn download(&self) -> Result<ModelStatus> {
        self.start(Task::Download)
    }

    /// Starts hashing every file again in the background (several seconds for the
    /// transcription model). A file that doesn't match is deleted and the model becomes
    /// corrupt. Does nothing if a task is already running.
    pub fn verify(&self) -> Result<ModelStatus> {
        self.start(Task::Verify)
    }

    fn start(&self, task: Task) -> Result<ModelStatus> {
        let mut inner = self.shared.lock();
        if inner.running.is_some() || inner.removing {
            return self.shared.status(&inner);
        }
        let on_disk = self.shared.store.status()?;
        if task == Task::Download && on_disk.state == ModelState::Ready {
            return Ok(on_disk);
        }
        inner.next_id += 1;
        let id = inner.next_id;
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let shared = Arc::clone(&self.shared);
            let cancel = Arc::clone(&cancel);
            std::thread::Builder::new()
                .name("trama-model".into())
                .spawn(move || work(&shared, id, task, &cancel))
                .map_err(|err| ModelError::io(&self.shared.store.dir(), err))?;
        }
        inner.last_error = None;
        inner.running = Some(Running {
            id,
            task,
            cancel,
            downloaded: on_disk.downloaded_bytes,
            total: on_disk.total_bytes,
            progress: 0.0,
        });
        // The worker can't report anything before this: it needs the lock.
        self.shared.emit(&inner)
    }

    /// Asks the running task to stop. The partial files stay, so the next download
    /// resumes; the final status arrives as an event when the task has stopped.
    pub fn cancel(&self) -> Result<ModelStatus> {
        let inner = self.shared.lock();
        if let Some(running) = &inner.running {
            running.cancel.store(true, Ordering::SeqCst);
        }
        self.shared.status(&inner)
    }

    /// Stops the running task, waits for it, and deletes the model from disk. No task
    /// can start until the model is gone. Waiting may take as long as a stalled read.
    pub fn remove(&self) -> Result<ModelStatus> {
        let mut inner = self.shared.lock();
        inner.removing = true;
        let mut inner = self.shared.stop_and_wait(inner);
        inner.removing = false;
        self.shared.store.remove()?;
        inner.last_error = None;
        self.shared.emit(&inner)
    }

    /// Stops the running task and waits for it; partial files stay for the next start.
    /// Called automatically on drop.
    pub fn shutdown(&self) {
        let inner = self.shared.lock();
        drop(self.shared.stop_and_wait(inner));
    }
}

impl Drop for ModelManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// The worker thread of one task.
fn work(shared: &Shared, id: u64, task: Task, cancel: &AtomicBool) {
    let on_progress = |progress: Progress| {
        let mut inner = shared.lock();
        match inner.running.as_mut() {
            Some(running) if running.id == id => {
                if task == Task::Download {
                    running.downloaded = progress.downloaded;
                }
                running.progress = progress.fraction();
            }
            _ => return,
        }
        shared.emit_or_log(&inner);
    };
    let outcome = catch_unwind(AssertUnwindSafe(|| match task {
        Task::Download => shared.store.download(cancel, &on_progress),
        Task::Verify => shared.store.verify(cancel, &on_progress).map(|_| ()),
    }));

    let mut inner = shared.lock();
    if inner.running.as_ref().is_some_and(|r| r.id == id) {
        inner.running = None;
    }
    inner.last_error = match &outcome {
        Ok(Ok(())) | Ok(Err(ModelError::Cancelled)) => None,
        Ok(Err(err)) => {
            // Surfaced to the interface through the status, and logged (A4).
            eprintln!("trama: model {task:?} failed: {err}");
            Some(err.code())
        }
        Err(_) => {
            // The panic message is already on stderr.
            eprintln!("trama: model {task:?} panicked");
            Some(PANICKED)
        }
    };
    // Waiters wake once the lock is released, after the final event.
    shared.idle.notify_all();
    shared.emit_or_log(&inner);
}
