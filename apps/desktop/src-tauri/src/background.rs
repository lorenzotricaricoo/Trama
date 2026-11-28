//! Running in the background: closing the main window hides it and Trama stays in the menu
//! bar, with recording and processing going on, until the user quits. Without a window
//! Trama leaves the Dock (and the app switcher); opening the window brings it back, with
//! the Dock icon of the accent.
//!
//! Trama can also open at login, straight into the menu bar: the login item starts it
//! with [`BACKGROUND_ARG`].

use tauri::{AppHandle, Manager, Runtime};

/// Label of the main window.
pub const MAIN_WINDOW: &str = "main";

/// Argument of the login item: start in the menu bar, without the window.
pub const BACKGROUND_ARG: &str = "--background";

/// Name of the login item (`~/Library/LaunchAgents/<name>.plist`).
pub const LOGIN_ITEM_NAME: &str = "io.github.lorenzotricaricoo.trama";

/// Error code of a login item that could not be changed.
pub const LOGIN_ITEM_FAILED: &str = "login_item_failed";

/// Whether Trama was started by the login item.
pub fn started_in_background() -> bool {
    is_background(std::env::args())
}

fn is_background(mut args: impl Iterator<Item = String>) -> bool {
    args.any(|arg| arg == BACKGROUND_ARG)
}

/// Shows or hides Trama in the Dock.
fn set_dock<R: Runtime>(app: &AppHandle<R>, visible: bool) {
    #[cfg(target_os = "macos")]
    {
        if let Err(err) = app.set_dock_visibility(visible) {
            eprintln!("trama: cannot change the Dock visibility: {err}");
        }
        if visible {
            // Back in the Dock: with the icon of the accent, not the bundle's.
            let accent = app
                .try_state::<crate::settings::Settings>()
                .map(|settings| settings.config().accent.clone());
            if let Some(accent) = accent {
                crate::dock_icon::show(app, &accent);
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, visible);
}

/// Shows the main window in front, and Trama in the Dock.
pub fn show_main<R: Runtime>(app: &AppHandle<R>) {
    set_dock(app, true);
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    let result = window
        .show()
        .and_then(|()| window.unminimize())
        .and_then(|()| window.set_focus());
    if let Err(err) = result {
        eprintln!("trama: cannot show the main window: {err}");
    }
}

/// Hides the main window and leaves the Dock: Trama stays in the menu bar. The webview
/// stays loaded, so the editor keeps saving and quitting still lets it finish.
pub fn hide_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW)
        && let Err(err) = window.hide()
    {
        eprintln!("trama: cannot hide the main window: {err}");
    }
    set_dock(app, false);
}

/// Whether Trama opens at login.
#[tauri::command]
pub fn open_at_login<R: Runtime>(app: AppHandle<R>) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().map_err(|err| {
        eprintln!("trama: cannot read the login item: {err}");
        LOGIN_ITEM_FAILED.to_owned()
    })
}

/// Makes Trama open at login, in the menu bar, or stops it.
#[tauri::command]
pub fn set_open_at_login<R: Runtime>(app: AppHandle<R>, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let launcher = app.autolaunch();
    let result = if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    };
    result.map_err(|err| {
        eprintln!("trama: cannot change the login item: {err}");
        LOGIN_ITEM_FAILED.to_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_login_item_starts_in_the_background() {
        let args = |list: &[&str]| list.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        assert!(is_background(
            args(&["trama-desktop", BACKGROUND_ARG]).into_iter()
        ));
        assert!(!is_background(args(&["trama-desktop"]).into_iter()));
        assert!(!is_background(
            args(&["trama-desktop", "--backgrounds"]).into_iter()
        ));
    }
}
