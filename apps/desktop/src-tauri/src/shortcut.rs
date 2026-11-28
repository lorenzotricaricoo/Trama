//! The global shortcut that starts and stops recording from any app, without opening the
//! window (⌥⌘R by default, chosen in Settings › Recording).
//!
//! A shortcut is refused before it is registered when macOS or Trama already uses it
//! (`shortcut_reserved`), and when the system does not let Trama register it
//! (`shortcut_taken`); the previous one then stays. A stored shortcut that cannot be
//! registered at start is reported to Settings, so the user can pick another.

use std::sync::{Mutex, PoisonError};

use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

/// Error codes of this module; the interface translates them.
pub mod error_code {
    /// macOS or Trama already uses the shortcut.
    pub const RESERVED: &str = "shortcut_reserved";
    /// The system did not let Trama register the shortcut (another app may hold it).
    pub const TAKEN: &str = "shortcut_taken";
}

/// Shortcuts of macOS and of Trama's own menus, as the core stores them. Choosing one of
/// them would hide the action it already runs, everywhere.
const RESERVED: &[&str] = &[
    // Spotlight, Finder search, input sources, emoji, app switcher.
    "Super+Space",
    "Alt+Super+Space",
    "Control+Space",
    "Control+Alt+Space",
    "Control+Super+Space",
    "Super+Backquote",
    "Shift+Super+Backquote",
    // Screenshots and recordings.
    "Shift+Super+Digit3",
    "Shift+Super+Digit4",
    "Shift+Super+Digit5",
    "Shift+Super+Digit6",
    "Control+Shift+Super+Digit3",
    "Control+Shift+Super+Digit4",
    // Session, windows and the Dock.
    "Control+Super+KeyQ",
    "Shift+Super+KeyQ",
    "Alt+Shift+Super+KeyQ",
    "Control+Super+KeyF",
    "Alt+Super+KeyD",
    "Alt+Super+KeyH",
    "Alt+Super+KeyW",
    "Alt+Super+KeyM",
    // Text editing and Go › Go to Folder.
    "Shift+Super+KeyZ",
    "Shift+Super+KeyG",
];

/// Whether macOS or Trama already uses `shortcut` (stored as `Modifier+…+Code`). Every
/// shortcut with ⌘ as its only modifier is taken: apps use them all (⌘N, ⌘K, ⌘Q, ⌘,…).
pub fn is_reserved(shortcut: &str) -> bool {
    let parts: Vec<&str> = shortcut.split('+').collect();
    let command_only = parts.len() == 2 && parts[0] == "Super";
    command_only || RESERVED.contains(&shortcut)
}

/// The registered shortcut and why the stored one could not be registered, if it could
/// not.
#[derive(Default)]
pub struct GlobalShortcut {
    registered: Mutex<Option<String>>,
    error: Mutex<Option<&'static str>>,
}

impl GlobalShortcut {
    /// Why the stored shortcut is not listening, for Settings.
    pub fn error(&self) -> Option<&'static str> {
        *self.error.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The shortcut listening now, if any.
    pub fn registered(&self) -> Option<String> {
        self.registered
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// Registers `shortcut` to start and stop recording.
fn register<R: Runtime>(app: &AppHandle<R>, shortcut: &str) -> Result<(), String> {
    app.global_shortcut()
        .on_shortcut(shortcut, |app, _, event| {
            if event.state == ShortcutState::Pressed {
                crate::recording::toggle(app);
            }
        })
        .map_err(|err| {
            eprintln!("trama: cannot register the global shortcut {shortcut}: {err}");
            error_code::TAKEN.to_owned()
        })
}

/// Registers the stored shortcut at start. A failure is kept for Settings, never fatal.
pub fn start<R: Runtime>(app: &AppHandle<R>, shortcut: &str) {
    let state = app.state::<GlobalShortcut>();
    let result = register(app, shortcut);
    *state.error.lock().unwrap_or_else(PoisonError::into_inner) =
        result.err().map(|_| error_code::TAKEN);
    if state.error().is_none() {
        *state
            .registered
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(shortcut.to_owned());
    }
}

/// Listens for `shortcut` instead of the current one, then runs `save`. If the shortcut is
/// reserved or cannot be registered, or `save` fails, the current one stays.
pub fn replace<R: Runtime>(
    app: &AppHandle<R>,
    shortcut: &str,
    save: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    if is_reserved(shortcut) {
        return Err(error_code::RESERVED.to_owned());
    }
    let state = app.state::<GlobalShortcut>();
    let current = state.registered();
    if current.as_deref() == Some(shortcut) {
        return save();
    }
    let unregister = |old: &str| {
        if let Err(err) = app.global_shortcut().unregister(old) {
            eprintln!("trama: cannot unregister the global shortcut {old}: {err}");
        }
    };
    if let Some(old) = &current {
        unregister(old);
    }
    let restore = || {
        if let Some(old) = &current
            && register(app, old).is_err()
        {
            *state
                .registered
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = None;
        }
    };
    if let Err(code) = register(app, shortcut) {
        restore();
        return Err(code);
    }
    if let Err(code) = save() {
        unregister(shortcut);
        restore();
        return Err(code);
    }
    *state
        .registered
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(shortcut.to_owned());
    *state.error.lock().unwrap_or_else(PoisonError::into_inner) = None;
    Ok(())
}

/// The shortcut as a menu accelerator (the same names as the core's, which the menu
/// parser reads), to show it next to Record Meeting in the menu bar item.
pub fn accelerator(shortcut: &str) -> Option<String> {
    trama_core::config::is_valid_shortcut(shortcut).then(|| shortcut.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

    #[test]
    fn the_default_shortcut_is_free_and_parses() {
        let default = trama_core::config::DEFAULT_GLOBAL_SHORTCUT;
        assert!(!is_reserved(default));
        let parsed = Shortcut::from_str(default).unwrap();
        assert!(parsed.matches(Modifiers::ALT | Modifiers::SUPER, Code::KeyR));
    }

    #[test]
    fn every_stored_key_parses_as_a_global_shortcut() {
        for key in [
            "KeyA",
            "Digit0",
            "F5",
            "F20",
            "Space",
            "Minus",
            "Equal",
            "BracketLeft",
            "BracketRight",
            "Backslash",
            "Semicolon",
            "Quote",
            "Comma",
            "Period",
            "Slash",
            "Backquote",
        ] {
            let shortcut = format!("Control+Alt+Shift+Super+{key}");
            assert!(
                trama_core::config::is_valid_shortcut(&shortcut),
                "{shortcut}"
            );
            assert!(Shortcut::from_str(&shortcut).is_ok(), "{shortcut}");
            assert!(
                muda::accelerator::Accelerator::from_str(&shortcut).is_ok(),
                "{shortcut} as an accelerator"
            );
        }
    }

    #[test]
    fn system_and_app_shortcuts_are_reserved() {
        for shortcut in [
            "Super+KeyN",
            "Super+KeyQ",
            "Super+Comma",
            "Super+Space",
            "Shift+Super+Digit4",
            "Control+Super+KeyQ",
            "Control+Space",
        ] {
            assert!(is_reserved(shortcut), "{shortcut}");
        }
        for shortcut in [
            "Alt+Super+KeyR",
            "Control+Shift+KeyM",
            "Shift+Super+KeyR",
            "Control+Alt+KeyN",
        ] {
            assert!(!is_reserved(shortcut), "{shortcut}");
        }
    }

    #[test]
    fn every_reserved_shortcut_is_a_valid_one() {
        for shortcut in RESERVED {
            assert!(
                trama_core::config::is_valid_shortcut(shortcut),
                "{shortcut}"
            );
        }
    }
}
