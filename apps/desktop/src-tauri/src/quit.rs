//! Quitting without losing typing: Quit (⌘Q) and closing the window first ask the frontend
//! to write what it has not saved yet (an editor waiting for a pause in the typing).
//!
//! - Everything saved: the frontend calls [`quit_ready`] and the app exits.
//! - A save failed (disk full, database busy): the frontend calls [`quit_cancelled`], the
//!   app stays open with the text on screen and the error shown. Quitting again soon after
//!   quits anyway (the frontend decides, see `src/notes/flush.ts`).
//! - No answer within [`SAVE_TIMEOUT`] (a stuck frontend): the app exits anyway.
//!
//! The macOS Dock's Quit and logging out terminate the app without asking; the editor
//! saves within a fraction of a second of the last keystroke, so at most that is lost.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, Runtime, State};

/// Event sent to the frontend when the user asks to quit.
pub const QUIT_REQUESTED_EVENT: &str = "quit-requested";

/// Id of the Quit item of the native menu.
pub const QUIT_MENU_ID: &str = "quit";

/// How long the frontend has to save before the app exits anyway. Longer than the core's
/// wait for a busy database (5 s), so a slow save can still finish.
const SAVE_TIMEOUT: Duration = Duration::from_secs(6);

/// Where quitting stands.
#[derive(Default)]
pub struct Quit {
    /// The frontend has been asked to save and has not answered yet.
    requested: AtomicBool,
    /// Counts requests, so the timeout of a cancelled request does nothing.
    request: AtomicU64,
    /// The app is exiting: windows close without asking again.
    exiting: AtomicBool,
}

impl Quit {
    /// Whether a window may close now. Until the app is exiting, closing asks to quit.
    pub fn may_close(&self) -> bool {
        self.exiting.load(Ordering::SeqCst)
    }
}

/// Exits the app, letting windows close.
fn exit<R: Runtime>(app: &AppHandle<R>) {
    app.state::<Quit>().exiting.store(true, Ordering::SeqCst);
    app.exit(0);
}

/// Asks the frontend to save, then exits when it answers or after [`SAVE_TIMEOUT`].
pub fn request<R: Runtime>(app: &AppHandle<R>) {
    let quit = app.state::<Quit>();
    if quit.requested.swap(true, Ordering::SeqCst) {
        return;
    }
    let request = quit.request.fetch_add(1, Ordering::SeqCst) + 1;
    if let Err(err) = app.emit(QUIT_REQUESTED_EVENT, ()) {
        eprintln!("trama: cannot ask the frontend to save before quitting: {err}");
        exit(app);
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(SAVE_TIMEOUT);
        let quit = app.state::<Quit>();
        let still_waiting =
            quit.requested.load(Ordering::SeqCst) && quit.request.load(Ordering::SeqCst) == request;
        if still_waiting {
            exit(&app);
        }
    });
}

/// Called by the frontend once everything is saved.
#[tauri::command]
pub fn quit_ready<R: Runtime>(app: AppHandle<R>, quit: State<'_, Quit>) {
    // Only after a request: a stray call must not close the app.
    if quit.requested.load(Ordering::SeqCst) {
        exit(&app);
    }
}

/// Called by the frontend when a save failed: the app stays open.
#[tauri::command]
pub fn quit_cancelled(quit: State<'_, Quit>) {
    quit.requested.store(false, Ordering::SeqCst);
}
