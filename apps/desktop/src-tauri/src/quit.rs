//! Quitting without losing typing: Quit (⌘Q) and closing the window first ask the frontend
//! to write what it has not saved yet (an editor waiting for a pause in the typing), then
//! exit. A frontend that does not answer in time does not keep the app open.
//!
//! The macOS Dock's Quit and logging out terminate the app without asking; the editor
//! saves within a fraction of a second of the last keystroke, so at most that is lost.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, Runtime, State};

/// Event sent to the frontend when the user asks to quit.
pub const QUIT_REQUESTED_EVENT: &str = "quit-requested";

/// Id of the Quit item of the native menu.
pub const QUIT_MENU_ID: &str = "quit";

/// How long the frontend has to save before the app exits anyway.
const SAVE_TIMEOUT: Duration = Duration::from_secs(2);

/// Where quitting stands.
#[derive(Default)]
pub struct Quit {
    /// The frontend has been asked to save.
    requested: AtomicBool,
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
    if app.state::<Quit>().requested.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Err(err) = app.emit(QUIT_REQUESTED_EVENT, ()) {
        eprintln!("trama: cannot ask the frontend to save before quitting: {err}");
        exit(app);
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(SAVE_TIMEOUT);
        exit(&app);
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
