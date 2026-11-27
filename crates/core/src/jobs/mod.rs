//! Background work: the runner that executes queued jobs and reports their progress.
//!
//! Jobs are stored by the library ([`crate::storage::Job`]), so they survive a restart.
//! The [`Runner`] owns one worker thread that claims due jobs, hands each one to the
//! [`JobHandler`] registered for its kind, and records the outcome. Every state change and
//! progress step goes to an [`EventSink`]: the desktop shell forwards them to the
//! frontend as events, the tests collect them.
//!
//! Jobs run one at a time: transcription uses the whole machine, and the others are short.

mod delete_audio;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

pub use delete_audio::DeleteAudio;

use crate::storage::{Id, Job, JobKind, Library, Result, StorageError};

/// The library shared by the runner and the app.
pub type SharedLibrary = Arc<Mutex<Library>>;

/// Locks the library, even if a handler panicked while holding it: the database stays
/// consistent because every change runs in a transaction.
pub fn lock(library: &SharedLibrary) -> MutexGuard<'_, Library> {
    library.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Error codes set by the runner itself.
pub mod error_code {
    /// A handler panicked; a bug.
    pub const PANICKED: &str = "panicked";
    /// The runner could not store the outcome of a job.
    pub const STORAGE: &str = "storage";
}

/// Longest the worker sleeps without checking the queue, in case a wake-up is missed.
const IDLE_WAIT: Duration = Duration::from_secs(30);

/// Why an attempt failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobError {
    /// Stable code the interface translates, for example `"model_missing"`.
    pub code: String,
    /// Technical detail for diagnostics; not shown as is.
    pub detail: String,
    /// Whether trying again later may succeed (a busy resource, a network error).
    pub retryable: bool,
}

impl JobError {
    /// An error worth retrying later.
    pub fn retryable(code: &str, detail: impl ToString) -> Self {
        Self {
            code: code.to_owned(),
            detail: detail.to_string(),
            retryable: true,
        }
    }

    /// An error that retrying won't fix.
    pub fn permanent(code: &str, detail: impl ToString) -> Self {
        Self {
            code: code.to_owned(),
            detail: detail.to_string(),
            retryable: false,
        }
    }

    /// The handler stopped because [`JobContext::should_stop`] said so.
    pub fn stopped() -> Self {
        Self::retryable("stopped", "")
    }
}

impl From<StorageError> for JobError {
    fn from(err: StorageError) -> Self {
        Self::retryable(error_code::STORAGE, err)
    }
}

/// Executes the jobs of one kind.
pub trait JobHandler: Send + Sync {
    /// The kind of job this handler runs.
    fn kind(&self) -> JobKind;

    /// Runs one attempt. Long work reports progress through `ctx` and returns
    /// [`JobError::stopped`] soon after [`JobContext::should_stop`] becomes true.
    fn run(&self, job: &Job, ctx: &JobContext) -> std::result::Result<(), JobError>;
}

/// A change worth telling the interface about.
#[derive(Debug, Clone, PartialEq)]
pub struct JobEvent {
    /// The job as stored after the change.
    pub job: Job,
    /// Progress of a running job, from 0 to 1, when known.
    pub progress: Option<f32>,
}

/// Receives job events. Must not block: it is called from the worker thread.
pub trait EventSink: Send + Sync {
    fn send(&self, event: JobEvent);
}

impl<F: Fn(JobEvent) + Send + Sync> EventSink for F {
    fn send(&self, event: JobEvent) {
        self(event)
    }
}

/// What a handler can use while it runs.
pub struct JobContext {
    library: SharedLibrary,
    job: Job,
    events: Arc<dyn EventSink>,
    stop: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    last_percent: AtomicI32,
}

impl JobContext {
    /// The library. Don't hold the lock during long work: the app needs it too.
    pub fn library(&self) -> MutexGuard<'_, Library> {
        lock(&self.library)
    }

    /// Reports progress, from 0 to 1. Only whole-percent changes are sent on.
    pub fn progress(&self, fraction: f32) {
        let fraction = if fraction.is_nan() {
            0.0
        } else {
            fraction.clamp(0.0, 1.0)
        };
        let percent = (fraction * 100.0).round() as i32;
        if self.last_percent.swap(percent, Ordering::Relaxed) != percent {
            self.events.send(JobEvent {
                job: self.job.clone(),
                progress: Some(fraction),
            });
        }
    }

    /// True when the app is stopping or the user cancelled the job.
    pub fn should_stop(&self) -> bool {
        self.stop.load(Ordering::Relaxed) || self.cancelled.load(Ordering::Relaxed)
    }
}

struct Shared {
    library: SharedLibrary,
    handlers: Vec<Arc<dyn JobHandler>>,
    events: Arc<dyn EventSink>,
    stop: Arc<AtomicBool>,
    wake: Mutex<bool>,
    wake_signal: Condvar,
    /// The running job and its cancellation flag.
    current: Mutex<Option<(Id, Arc<AtomicBool>)>>,
}

impl Shared {
    fn kinds(&self) -> Vec<JobKind> {
        self.handlers.iter().map(|h| h.kind()).collect()
    }

    fn handler(&self, kind: JobKind) -> Option<Arc<dyn JobHandler>> {
        self.handlers.iter().find(|h| h.kind() == kind).cloned()
    }

    fn emit(&self, job: Job, progress: Option<f32>) {
        self.events.send(JobEvent { job, progress });
    }

    fn notify(&self) {
        *self.wake.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.wake_signal.notify_all();
    }

    /// Sleeps until notified, stopping, or `timeout`.
    fn wait(&self, timeout: Duration) {
        let mut woken = self.wake.lock().unwrap_or_else(PoisonError::into_inner);
        if !*woken && !self.stop.load(Ordering::SeqCst) {
            woken = self
                .wake_signal
                .wait_timeout(woken, timeout)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        *woken = false;
    }
}

/// Runs queued jobs on a worker thread until stopped.
pub struct Runner {
    shared: Arc<Shared>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl std::fmt::Debug for Runner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runner").finish_non_exhaustive()
    }
}

impl Runner {
    /// Starts the worker. Only jobs whose kind has a handler are claimed; the others wait
    /// in the queue for a build that can run them.
    pub fn start(
        library: SharedLibrary,
        handlers: Vec<Arc<dyn JobHandler>>,
        events: Arc<dyn EventSink>,
    ) -> Self {
        let shared = Arc::new(Shared {
            library,
            handlers,
            events,
            stop: Arc::new(AtomicBool::new(false)),
            wake: Mutex::new(false),
            wake_signal: Condvar::new(),
            current: Mutex::new(None),
        });
        let worker = {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("trama-jobs".into())
                .spawn(move || work(&shared))
                .expect("the job worker thread starts")
        };
        Self {
            shared,
            worker: Mutex::new(Some(worker)),
        }
    }

    /// Queues a job (or returns the same one already queued) and wakes the worker.
    pub fn enqueue(&self, kind: JobKind, target_id: Id) -> Result<Job> {
        let job = lock(&self.shared.library).enqueue_job(kind, target_id)?;
        self.shared.emit(job.clone(), None);
        self.shared.notify();
        Ok(job)
    }

    /// Queues a failed or cancelled job again.
    pub fn retry(&self, id: Id) -> Result<Job> {
        let job = lock(&self.shared.library).retry_job(id)?;
        self.shared.emit(job.clone(), None);
        self.shared.notify();
        Ok(job)
    }

    /// Cancels a job; if it is running, its handler is told to stop.
    pub fn cancel(&self, id: Id) -> Result<Job> {
        let job = lock(&self.shared.library).cancel_job(id)?;
        if let Some((running, flag)) = &*self
            .shared
            .current
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            && *running == id
        {
            flag.store(true, Ordering::SeqCst);
        }
        self.shared.emit(job.clone(), None);
        Ok(job)
    }

    /// Wakes the worker, for example after time moved on in a test.
    pub fn notify(&self) {
        self.shared.notify();
    }

    /// Asks the running job to stop, waits for the worker to exit, and leaves unfinished
    /// jobs in the queue for the next start. Called automatically on drop.
    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.shared.notify();
        let worker = self
            .worker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(worker) = worker {
            // A panic outside a handler is a bug; it has been reported on stderr already.
            let _ = worker.join();
        }
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The worker loop.
fn work(shared: &Shared) {
    let kinds = shared.kinds();
    while !shared.stop.load(Ordering::SeqCst) {
        let claimed = lock(&shared.library).claim_job(&kinds);
        match claimed {
            Ok(Some(job)) => run(shared, job),
            Ok(None) => {
                let wait = {
                    let library = lock(&shared.library);
                    match library.next_job_due(&kinds) {
                        Ok(Some(due)) => {
                            let ms = (due - library.now()).clamp(0, IDLE_WAIT.as_millis() as i64);
                            Duration::from_millis(ms as u64)
                        }
                        _ => IDLE_WAIT,
                    }
                };
                shared.wait(wait);
            }
            Err(err) => {
                // The database is unusable; keep trying, but say so (principle A4).
                eprintln!("trama: cannot read the job queue: {err}");
                shared.wait(IDLE_WAIT);
            }
        }
    }
}

/// Runs one attempt of a claimed job and records the outcome.
fn run(shared: &Shared, job: Job) {
    let cancelled = Arc::new(AtomicBool::new(false));
    *shared
        .current
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some((job.id, Arc::clone(&cancelled)));
    shared.emit(job.clone(), Some(0.0));

    let outcome = match shared.handler(job.kind) {
        Some(handler) => {
            let ctx = JobContext {
                library: Arc::clone(&shared.library),
                job: job.clone(),
                events: Arc::clone(&shared.events),
                stop: Arc::clone(&shared.stop),
                cancelled: Arc::clone(&cancelled),
                last_percent: AtomicI32::new(0),
            };
            catch_unwind(AssertUnwindSafe(|| handler.run(&job, &ctx))).unwrap_or_else(|panic| {
                let detail = panic
                    .downcast_ref::<&str>()
                    .map(|s| (*s).to_owned())
                    .or_else(|| panic.downcast_ref::<String>().cloned())
                    .unwrap_or_default();
                Err(JobError::permanent(error_code::PANICKED, detail))
            })
        }
        // Not reachable: only kinds with a handler are claimed.
        None => Err(JobError::permanent("no_handler", job.kind.as_str())),
    };
    *shared
        .current
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = None;

    let stored = {
        let mut library = lock(&shared.library);
        if cancelled.load(Ordering::SeqCst) {
            // Cancelled by the user: already stored and announced.
            return;
        }
        match outcome {
            Ok(()) => library.finish_job(job.id),
            Err(_) if shared.stop.load(Ordering::SeqCst) => library.release_job(job.id),
            Err(err) => library.fail_job(job.id, &err.code, &err.detail, err.retryable),
        }
    };
    match stored {
        Ok(job) => shared.emit(job, None),
        Err(err) => eprintln!("trama: cannot store the outcome of job {}: {err}", job.id),
    }
}
