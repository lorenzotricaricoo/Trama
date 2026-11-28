//! App configuration: preferences of the app on this Mac, outside the library.
//!
//! The library holds the preferences that travel with the user's data
//! ([`crate::storage::LibrarySettings`]). What belongs to this Mac lives here, in a small
//! JSON file next to the default library location, because some of it is needed before
//! the library can be opened (its location, the interface language for the menus):
//!
//! ```text
//! ~/Library/Application Support/io.github.lorenzotricaricoo.trama/
//! ├── settings.json   this file
//! ├── Library/        the default library
//! └── Models/
//! ```
//!
//! The core takes the file's path as a parameter, like [`crate::storage::Library::open`]
//! takes the library folder. Reading never fails on bad content: a value that can't be
//! read gives its default, and keys this version doesn't know are kept when saving.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::i18n::SUPPORTED_LOCALES;
use crate::storage::DATABASE_FILE;

/// Name of the configuration file inside the app data folder.
pub const CONFIG_FILE: &str = "settings.json";

/// Themes: follow macOS, or always light or dark.
pub const THEMES: &[&str] = &["system", "light", "dark"];

/// Accent colours of the visual direction (`acc-<name>` in the design tokens).
pub const ACCENTS: &[&str] = &[
    "lime", "blue", "purple", "pink", "red", "orange", "yellow", "green", "graphite",
];

/// Global shortcut to start and stop recording until the user picks another: ⌥⌘R, the
/// same keys as inside the app.
pub const DEFAULT_GLOBAL_SHORTCUT: &str = "Alt+Super+KeyR";

/// Modifiers of a shortcut, in the order they are written.
const MODIFIERS: &[&str] = &["Control", "Alt", "Shift", "Super"];

/// Why a value is refused, as stable codes the interface translates.
pub mod config_error {
    /// Not one of the supported interface languages.
    pub const UNSUPPORTED_LANGUAGE: &str = "unsupported_language";
    /// Not one of [`super::THEMES`] or [`super::ACCENTS`].
    pub const UNKNOWN_APPEARANCE: &str = "unknown_appearance";
    /// The shortcut has no modifier besides Shift, or a key that can't be used.
    pub const INVALID_SHORTCUT: &str = "invalid_shortcut";
    /// The library location is not an absolute path.
    pub const LIBRARY_NOT_ABSOLUTE: &str = "library_not_absolute";
    /// The library location is a file.
    pub const LIBRARY_NOT_A_FOLDER: &str = "library_not_a_folder";
    /// The folder has other files and no library: Trama would mix its files with them.
    pub const LIBRARY_FOLDER_NOT_EMPTY: &str = "library_folder_not_empty";
    /// The folder is synced by iCloud Drive or another cloud service, which can corrupt
    /// an open database.
    pub const LIBRARY_SYNCED: &str = "library_synced";
    /// The configuration file could not be written.
    pub const CONFIG_WRITE: &str = "config_write";

    /// Every code, for the translation tests.
    pub const ALL: &[&str] = &[
        UNSUPPORTED_LANGUAGE,
        UNKNOWN_APPEARANCE,
        INVALID_SHORTCUT,
        LIBRARY_NOT_ABSOLUTE,
        LIBRARY_NOT_A_FOLDER,
        LIBRARY_FOLDER_NOT_EMPTY,
        LIBRARY_SYNCED,
        CONFIG_WRITE,
    ];
}

/// Warnings about a library location that is accepted anyway.
pub mod config_warning {
    /// `~/Documents` or `~/Desktop`, which iCloud Drive syncs when "Desktop & Documents
    /// Folders" is on.
    pub const LIBRARY_MAYBE_SYNCED: &str = "library_maybe_synced";
    /// The folder already holds a library, which Trama will open as it is.
    pub const LIBRARY_EXISTS: &str = "library_exists";

    /// Every code, for the translation tests.
    pub const ALL: &[&str] = &[LIBRARY_MAYBE_SYNCED, LIBRARY_EXISTS];
}

/// Preferences of the app on this Mac.
#[derive(Debug, Clone, PartialEq)]
pub struct AppConfig {
    /// Folder of the library; `None` for the default location. A change applies at the
    /// next start: the open library is never moved or closed behind the user's back.
    pub library_dir: Option<PathBuf>,
    /// Interface language (`en`, `it`); `None` follows macOS.
    pub interface_language: Option<String>,
    /// One of [`THEMES`].
    pub theme: String,
    /// One of [`ACCENTS`].
    pub accent: String,
    /// Microphone by device id; `None` follows the system default.
    pub microphone: Option<String>,
    /// Global shortcut to start and stop recording, as `Modifier+…+Code` with
    /// `KeyboardEvent.code` names (for example `Alt+Super+KeyR`).
    pub global_shortcut: String,
    /// Keys written by another version of Trama, kept as they are.
    other: Map<String, Value>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            library_dir: None,
            interface_language: None,
            theme: THEMES[0].to_owned(),
            accent: ACCENTS[0].to_owned(),
            microphone: None,
            global_shortcut: DEFAULT_GLOBAL_SHORTCUT.to_owned(),
            other: Map::new(),
        }
    }
}

const KEY_LIBRARY_DIR: &str = "libraryDir";
const KEY_INTERFACE_LANGUAGE: &str = "interfaceLanguage";
const KEY_THEME: &str = "theme";
const KEY_ACCENT: &str = "accent";
const KEY_MICROPHONE: &str = "microphone";
const KEY_GLOBAL_SHORTCUT: &str = "globalShortcut";
const KNOWN_KEYS: &[&str] = &[
    KEY_LIBRARY_DIR,
    KEY_INTERFACE_LANGUAGE,
    KEY_THEME,
    KEY_ACCENT,
    KEY_MICROPHONE,
    KEY_GLOBAL_SHORTCUT,
];

impl AppConfig {
    /// Reads the configuration at `path`. A missing file, or one that isn't JSON, gives
    /// the defaults; each value that can't be used gives its own default.
    pub fn load(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(err),
        };
        Ok(Self::parse(&text))
    }

    /// Reads a configuration from its JSON text, as [`AppConfig::load`] does.
    pub fn parse(text: &str) -> Self {
        let Ok(Value::Object(mut map)) = serde_json::from_str::<Value>(text) else {
            return Self::default();
        };
        let defaults = Self::default();
        let string = |map: &Map<String, Value>, key: &str| {
            map.get(key).and_then(Value::as_str).map(str::to_owned)
        };
        let config = Self {
            library_dir: string(&map, KEY_LIBRARY_DIR)
                .map(PathBuf::from)
                .filter(|dir| dir.is_absolute()),
            interface_language: string(&map, KEY_INTERFACE_LANGUAGE)
                .filter(|tag| is_supported_language(tag)),
            theme: string(&map, KEY_THEME)
                .filter(|theme| THEMES.contains(&theme.as_str()))
                .unwrap_or(defaults.theme),
            accent: string(&map, KEY_ACCENT)
                .filter(|accent| ACCENTS.contains(&accent.as_str()))
                .unwrap_or(defaults.accent),
            microphone: string(&map, KEY_MICROPHONE).filter(|id| !id.is_empty()),
            global_shortcut: string(&map, KEY_GLOBAL_SHORTCUT)
                .filter(|shortcut| is_valid_shortcut(shortcut))
                .unwrap_or(defaults.global_shortcut),
            other: Map::new(),
        };
        map.retain(|key, _| !KNOWN_KEYS.contains(&key.as_str()));
        Self {
            other: map,
            ..config
        }
    }

    /// The configuration as JSON text.
    pub fn to_json(&self) -> String {
        let mut map = self.other.clone();
        let mut put = |key: &str, value: Option<String>| {
            if let Some(value) = value {
                map.insert(key.to_owned(), Value::String(value));
            }
        };
        put(
            KEY_LIBRARY_DIR,
            self.library_dir
                .as_ref()
                .map(|dir| dir.to_string_lossy().into_owned()),
        );
        put(KEY_INTERFACE_LANGUAGE, self.interface_language.clone());
        put(KEY_THEME, Some(self.theme.clone()));
        put(KEY_ACCENT, Some(self.accent.clone()));
        put(KEY_MICROPHONE, self.microphone.clone());
        put(KEY_GLOBAL_SHORTCUT, Some(self.global_shortcut.clone()));
        let mut text =
            serde_json::to_string_pretty(&Value::Object(map)).unwrap_or_else(|_| "{}".to_owned());
        text.push('\n');
        text
    }

    /// Writes the configuration to `path`, replacing the file in one step so a crash
    /// never leaves half a file.
    pub fn save(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let path = path.as_ref();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let temp = path.with_extension("json.tmp");
        {
            let mut file = fs::File::create(&temp)?;
            file.write_all(self.to_json().as_bytes())?;
            file.sync_all()?;
        }
        fs::rename(&temp, path)
    }

    /// Sets the interface language; `None` follows macOS.
    pub fn set_interface_language(&mut self, tag: Option<&str>) -> Result<(), &'static str> {
        match tag {
            Some(tag) if !is_supported_language(tag) => Err(config_error::UNSUPPORTED_LANGUAGE),
            tag => {
                self.interface_language = tag.map(str::to_owned);
                Ok(())
            }
        }
    }

    /// Sets the theme and the accent colour.
    pub fn set_appearance(&mut self, theme: &str, accent: &str) -> Result<(), &'static str> {
        if !THEMES.contains(&theme) || !ACCENTS.contains(&accent) {
            return Err(config_error::UNKNOWN_APPEARANCE);
        }
        self.theme = theme.to_owned();
        self.accent = accent.to_owned();
        Ok(())
    }

    /// Sets the global shortcut.
    pub fn set_global_shortcut(&mut self, shortcut: &str) -> Result<(), &'static str> {
        if !is_valid_shortcut(shortcut) {
            return Err(config_error::INVALID_SHORTCUT);
        }
        self.global_shortcut = shortcut.to_owned();
        Ok(())
    }
}

/// Whether `tag` is one of the interface languages.
pub fn is_supported_language(tag: &str) -> bool {
    SUPPORTED_LOCALES.iter().any(|locale| *locale == tag)
}

/// Whether `shortcut` is `Modifier+…+Code`: modifiers from Control, Alt, Shift and Super,
/// each once and in that order, at least one of them other than Shift, then one key by
/// its `KeyboardEvent.code` name: a letter (`KeyR`), a digit (`Digit1`), a function key
/// (`F5`), `Space` or a punctuation key.
pub fn is_valid_shortcut(shortcut: &str) -> bool {
    let parts: Vec<&str> = shortcut.split('+').collect();
    let Some((key, modifiers)) = parts.split_last() else {
        return false;
    };
    let mut last = None;
    for modifier in modifiers {
        let Some(index) = MODIFIERS.iter().position(|m| m == modifier) else {
            return false;
        };
        if last.is_some_and(|last| index <= last) {
            return false;
        }
        last = Some(index);
    }
    let has_modifier = modifiers.iter().any(|m| *m != "Shift");
    has_modifier && is_shortcut_key(key)
}

fn is_shortcut_key(code: &str) -> bool {
    const PUNCTUATION: &[&str] = &[
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
    ];
    let single = |rest: &str, valid: fn(char) -> bool| {
        let mut chars = rest.chars();
        matches!((chars.next(), chars.next()), (Some(c), None) if valid(c))
    };
    if let Some(letter) = code.strip_prefix("Key") {
        return single(letter, |c| c.is_ascii_uppercase());
    }
    if let Some(digit) = code.strip_prefix("Digit") {
        return single(digit, |c| c.is_ascii_digit());
    }
    if let Some(number) = code.strip_prefix('F') {
        return number
            .parse::<u8>()
            .is_ok_and(|n| (1..=20).contains(&n) && !number.starts_with('0'));
    }
    PUNCTUATION.contains(&code)
}

/// Checks a folder chosen for the library. Returns a [`config_warning`] code when the
/// folder is accepted with a caveat, or a [`config_error`] code when it is refused. `home`
/// is the user's home folder, to recognise synced folders.
pub fn check_library_dir(
    dir: &Path,
    home: Option<&Path>,
) -> Result<Option<&'static str>, &'static str> {
    if !dir.is_absolute() {
        return Err(config_error::LIBRARY_NOT_ABSOLUTE);
    }
    if let Some(home) = home {
        let synced = ["Library/Mobile Documents", "Library/CloudStorage"];
        if synced.iter().any(|sub| dir.starts_with(home.join(sub))) {
            return Err(config_error::LIBRARY_SYNCED);
        }
    }
    let mut warning = None;
    if dir.exists() {
        if !dir.is_dir() {
            return Err(config_error::LIBRARY_NOT_A_FOLDER);
        }
        if dir.join(DATABASE_FILE).is_file() {
            warning = Some(config_warning::LIBRARY_EXISTS);
        } else {
            let mut entries = fs::read_dir(dir).map_err(|_| config_error::LIBRARY_NOT_A_FOLDER)?;
            // Finder's hidden files don't count.
            let has_files = entries.any(|entry| {
                entry.is_ok_and(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
            });
            if has_files {
                return Err(config_error::LIBRARY_FOLDER_NOT_EMPTY);
            }
        }
    }
    if warning.is_none()
        && let Some(home) = home
        && ["Documents", "Desktop"]
            .iter()
            .any(|sub| dir.starts_with(home.join(sub)))
    {
        warning = Some(config_warning::LIBRARY_MAYBE_SYNCED);
    }
    Ok(warning)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_gives_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let config = AppConfig::load(dir.path().join(CONFIG_FILE)).unwrap();
        assert_eq!(config, AppConfig::default());
        assert_eq!(config.theme, "system");
        assert_eq!(config.global_shortcut, DEFAULT_GLOBAL_SHORTCUT);
    }

    #[test]
    fn settings_survive_a_save_and_a_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join(CONFIG_FILE);
        let mut config = AppConfig::default();
        config.set_interface_language(Some("it")).unwrap();
        config.set_appearance("dark", "blue").unwrap();
        config.set_global_shortcut("Control+Shift+KeyM").unwrap();
        config.microphone = Some("BuiltInMicrophoneDevice".into());
        config.library_dir = Some(PathBuf::from("/Volumes/Data/Trama"));
        config.save(&path).unwrap();
        assert_eq!(AppConfig::load(&path).unwrap(), config);
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn bad_values_fall_back_one_by_one_and_unknown_keys_are_kept() {
        let config = AppConfig::parse(
            r#"{"theme":"neon","accent":"blue","interfaceLanguage":"fr",
                "libraryDir":"relative/path","globalShortcut":"KeyR","future":{"x":1}}"#,
        );
        assert_eq!(config.theme, "system");
        assert_eq!(config.accent, "blue");
        assert_eq!(config.interface_language, None);
        assert_eq!(config.library_dir, None);
        assert_eq!(config.global_shortcut, DEFAULT_GLOBAL_SHORTCUT);
        let saved: Value = serde_json::from_str(&config.to_json()).unwrap();
        assert_eq!(saved["future"]["x"], 1);
        assert_eq!(AppConfig::parse("not json"), AppConfig::default());
        assert_eq!(AppConfig::parse("[1, 2]"), AppConfig::default());
    }

    #[test]
    fn values_are_checked_when_set() {
        let mut config = AppConfig::default();
        assert_eq!(
            config.set_interface_language(Some("de")),
            Err(config_error::UNSUPPORTED_LANGUAGE)
        );
        config.set_interface_language(None).unwrap();
        assert_eq!(
            config.set_appearance("dark", "teal"),
            Err(config_error::UNKNOWN_APPEARANCE)
        );
        assert_eq!(
            config.set_global_shortcut("Shift+KeyR"),
            Err(config_error::INVALID_SHORTCUT)
        );
        assert_eq!(config, AppConfig::default());
    }

    #[test]
    fn shortcuts_need_a_modifier_and_a_usable_key() {
        for shortcut in [
            "Alt+Super+KeyR",
            "Control+KeyA",
            "Control+Alt+Shift+Super+Digit1",
            "Super+F5",
            "Alt+Space",
            "Shift+Super+Slash",
        ] {
            assert!(is_valid_shortcut(shortcut), "{shortcut}");
        }
        for shortcut in [
            "",
            "KeyR",
            "Shift+KeyR",
            "Super+Alt+KeyR",
            "Alt+Alt+KeyR",
            "Alt+Super",
            "Alt+Keyr",
            "Alt+KeyRR",
            "Alt+F0",
            "Alt+F21",
            "Alt+F05",
            "Alt+Enter",
            "Hyper+KeyR",
        ] {
            assert!(!is_valid_shortcut(shortcut), "{shortcut}");
        }
    }

    #[test]
    fn library_folders_are_checked() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        assert_eq!(
            check_library_dir(Path::new("relative"), Some(home)),
            Err(config_error::LIBRARY_NOT_ABSOLUTE)
        );
        // A folder that doesn't exist yet is created at the next start.
        assert_eq!(check_library_dir(&home.join("new"), Some(home)), Ok(None));

        let empty = home.join("empty");
        fs::create_dir(&empty).unwrap();
        fs::write(empty.join(".DS_Store"), b"").unwrap();
        assert_eq!(check_library_dir(&empty, Some(home)), Ok(None));

        let busy = home.join("busy");
        fs::create_dir(&busy).unwrap();
        fs::write(busy.join("notes.txt"), b"x").unwrap();
        assert_eq!(
            check_library_dir(&busy, Some(home)),
            Err(config_error::LIBRARY_FOLDER_NOT_EMPTY)
        );
        assert_eq!(
            check_library_dir(&busy.join("notes.txt"), Some(home)),
            Err(config_error::LIBRARY_NOT_A_FOLDER)
        );

        let existing = home.join("existing");
        fs::create_dir(&existing).unwrap();
        fs::write(existing.join(DATABASE_FILE), b"").unwrap();
        assert_eq!(
            check_library_dir(&existing, Some(home)),
            Ok(Some(config_warning::LIBRARY_EXISTS))
        );

        assert_eq!(
            check_library_dir(&home.join("Library/Mobile Documents/Trama"), Some(home)),
            Err(config_error::LIBRARY_SYNCED)
        );
        assert_eq!(
            check_library_dir(&home.join("Documents/Trama"), Some(home)),
            Ok(Some(config_warning::LIBRARY_MAYBE_SYNCED))
        );
    }
}
