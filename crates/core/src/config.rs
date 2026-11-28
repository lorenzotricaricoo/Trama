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
//! takes the library folder. Reading never fails: a value that can't be used gives its
//! default, and keys this version doesn't know are kept when saving. A file that can't be
//! read at all is set aside as `settings.json.corrupt` before anything is saved over it,
//! and the app is told with a [`config_notice`].

use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

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

/// The onboarding was finished, or skipped: it is not shown again at start.
pub const ONBOARDING_DONE: &str = "done";

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
    /// The folder, or the folder that would hold it, can't be written.
    pub const LIBRARY_NOT_WRITABLE: &str = "library_not_writable";
    /// The configuration file could not be written.
    pub const CONFIG_WRITE: &str = "config_write";
    /// Not an onboarding step name.
    pub const INVALID_ONBOARDING_STEP: &str = "invalid_onboarding_step";

    /// Every code, for the translation tests.
    pub const ALL: &[&str] = &[
        UNSUPPORTED_LANGUAGE,
        UNKNOWN_APPEARANCE,
        INVALID_SHORTCUT,
        LIBRARY_NOT_ABSOLUTE,
        LIBRARY_NOT_A_FOLDER,
        LIBRARY_FOLDER_NOT_EMPTY,
        LIBRARY_SYNCED,
        LIBRARY_NOT_WRITABLE,
        CONFIG_WRITE,
        INVALID_ONBOARDING_STEP,
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

/// Problems found at start, for the app to tell the user once: Trama started anyway.
pub mod config_notice {
    /// The configuration file couldn't be read: it was kept as `settings.json.corrupt`
    /// and the defaults are used.
    pub const SETTINGS_RESET: &str = "settings_reset";
    /// The configuration file couldn't be read nor set aside: the defaults are used and
    /// changes are not saved, so the file is not overwritten.
    pub const SETTINGS_UNREADABLE: &str = "settings_unreadable";
    /// The chosen library couldn't be opened (a disconnected disk, no permission, a newer
    /// version of Trama, another copy of Trama using it): the default library is open.
    pub const LIBRARY_UNAVAILABLE: &str = "library_unavailable";
    /// No library could be opened.
    pub const LIBRARY_FAILED: &str = "library_failed";

    /// Every code, for the translation tests.
    pub const ALL: &[&str] = &[
        SETTINGS_RESET,
        SETTINGS_UNREADABLE,
        LIBRARY_UNAVAILABLE,
        LIBRARY_FAILED,
    ];
}

/// Suffix of a configuration file set aside because it couldn't be read.
pub const CORRUPT_SUFFIX: &str = "corrupt";

/// A configuration read at start, with what went wrong reading it.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedConfig {
    pub config: AppConfig,
    /// A [`config_notice`] code when the file couldn't be read.
    pub notice: Option<&'static str>,
    /// The unreadable file is still in place: saving would overwrite it.
    pub save_blocked: bool,
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
    /// Opaque window even when macOS keeps transparency on (macOS's own Reduce
    /// transparency always wins).
    pub reduce_transparency: bool,
    /// No transitions, even when macOS keeps motion on (macOS's own Reduce motion always
    /// wins).
    pub reduce_motion: bool,
    /// Microphone by device id; `None` follows the system default.
    pub microphone: Option<String>,
    /// Global shortcut to start and stop recording, as `Modifier+…+Code` with
    /// `KeyboardEvent.code` names (for example `Alt+Super+KeyR`).
    pub global_shortcut: String,
    /// Where the onboarding stands: the step to resume at, or [`ONBOARDING_DONE`]. `None`
    /// when this file never recorded it, which is the case of every install made before
    /// the onboarding existed.
    pub onboarding: Option<String>,
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
            reduce_transparency: false,
            reduce_motion: false,
            microphone: None,
            global_shortcut: DEFAULT_GLOBAL_SHORTCUT.to_owned(),
            onboarding: None,
            other: Map::new(),
        }
    }
}

const KEY_LIBRARY_DIR: &str = "libraryDir";
const KEY_INTERFACE_LANGUAGE: &str = "interfaceLanguage";
const KEY_THEME: &str = "theme";
const KEY_ACCENT: &str = "accent";
const KEY_REDUCE_TRANSPARENCY: &str = "reduceTransparency";
const KEY_REDUCE_MOTION: &str = "reduceMotion";
const KEY_MICROPHONE: &str = "microphone";
const KEY_GLOBAL_SHORTCUT: &str = "globalShortcut";
const KEY_ONBOARDING: &str = "onboarding";
const KNOWN_KEYS: &[&str] = &[
    KEY_LIBRARY_DIR,
    KEY_INTERFACE_LANGUAGE,
    KEY_THEME,
    KEY_ACCENT,
    KEY_REDUCE_TRANSPARENCY,
    KEY_REDUCE_MOTION,
    KEY_MICROPHONE,
    KEY_GLOBAL_SHORTCUT,
    KEY_ONBOARDING,
];

impl AppConfig {
    /// Reads the configuration at `path`; never fails. A missing file gives the defaults,
    /// and each value that can't be used gives its own default. A file that can't be read
    /// or isn't a JSON object is renamed `settings.json.corrupt`, so a later save doesn't
    /// destroy it; if even that fails, saving is blocked.
    pub fn load(path: impl AsRef<Path>) -> LoadedConfig {
        let path = path.as_ref();
        let ok = |config| LoadedConfig {
            config,
            notice: None,
            save_blocked: false,
        };
        let problem = match fs::read_to_string(path) {
            Ok(text) => match Self::try_parse(&text) {
                Some(config) => return ok(config),
                None => "not a JSON object".to_owned(),
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return ok(Self::default()),
            Err(err) => err.to_string(),
        };
        let aside = corrupt_path(path);
        match fs::rename(path, &aside) {
            Ok(()) => {
                eprintln!(
                    "trama: cannot read {} ({problem}); kept as {} and using the defaults",
                    path.display(),
                    aside.display()
                );
                LoadedConfig {
                    config: Self::default(),
                    notice: Some(config_notice::SETTINGS_RESET),
                    save_blocked: false,
                }
            }
            Err(err) => {
                eprintln!(
                    "trama: cannot read {} ({problem}) nor set it aside ({err}); \
                     using the defaults without saving",
                    path.display()
                );
                LoadedConfig {
                    config: Self::default(),
                    notice: Some(config_notice::SETTINGS_UNREADABLE),
                    save_blocked: true,
                }
            }
        }
    }

    /// Reads a configuration from its JSON text: the defaults if it isn't a JSON object.
    pub fn parse(text: &str) -> Self {
        Self::try_parse(text).unwrap_or_default()
    }

    /// Reads a configuration from its JSON text; `None` if it isn't a JSON object.
    fn try_parse(text: &str) -> Option<Self> {
        let Ok(Value::Object(mut map)) = serde_json::from_str::<Value>(text) else {
            return None;
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
            reduce_transparency: map
                .get(KEY_REDUCE_TRANSPARENCY)
                .and_then(Value::as_bool)
                .unwrap_or(defaults.reduce_transparency),
            reduce_motion: map
                .get(KEY_REDUCE_MOTION)
                .and_then(Value::as_bool)
                .unwrap_or(defaults.reduce_motion),
            microphone: string(&map, KEY_MICROPHONE).filter(|id| !id.is_empty()),
            global_shortcut: string(&map, KEY_GLOBAL_SHORTCUT)
                .filter(|shortcut| is_valid_shortcut(shortcut))
                .unwrap_or(defaults.global_shortcut),
            onboarding: string(&map, KEY_ONBOARDING).filter(|step| is_onboarding_step(step)),
            other: Map::new(),
        };
        map.retain(|key, _| !KNOWN_KEYS.contains(&key.as_str()));
        Some(Self {
            other: map,
            ..config
        })
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
        put(KEY_ONBOARDING, self.onboarding.clone());
        map.insert(
            KEY_REDUCE_TRANSPARENCY.to_owned(),
            Value::Bool(self.reduce_transparency),
        );
        map.insert(
            KEY_REDUCE_MOTION.to_owned(),
            Value::Bool(self.reduce_motion),
        );
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

    /// Records where the onboarding stands: a step name, or [`ONBOARDING_DONE`].
    pub fn set_onboarding(&mut self, step: &str) -> Result<(), &'static str> {
        if !is_onboarding_step(step) {
            return Err(config_error::INVALID_ONBOARDING_STEP);
        }
        self.onboarding = Some(step.to_owned());
        Ok(())
    }
}

/// Whether `step` can name an onboarding step: 1 to 32 lowercase letters or `_`. The
/// frontend owns the list of steps, so a name an older or newer version wrote is kept
/// and an unknown one simply starts from the beginning.
pub fn is_onboarding_step(step: &str) -> bool {
    (1..=32).contains(&step.len()) && step.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
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

/// Where a configuration file that couldn't be read is set aside.
pub fn corrupt_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(CORRUPT_SUFFIX);
    path.with_file_name(name)
}

/// `path` with every symbolic link and `..` resolved as the system would, also when its
/// last parts don't exist yet: each existing prefix is resolved, then `..` goes up from
/// there. A prefix that can't be resolved (no permission) is kept as written.
pub fn resolve_path(path: &Path) -> PathBuf {
    let mut resolved = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            Component::Normal(part) => {
                resolved.push(part);
                if fs::symlink_metadata(&resolved).is_ok()
                    && let Ok(real) = fs::canonicalize(&resolved)
                {
                    resolved = real;
                }
            }
            other => resolved.push(other),
        }
    }
    resolved
}

/// A folder accepted for the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedLibraryDir {
    /// The folder with links and `..` resolved: what is stored and compared.
    pub path: PathBuf,
    /// A [`config_warning`] code when it is accepted with a caveat.
    pub warning: Option<&'static str>,
}

/// Checks a folder chosen for the library: an absolute path, empty or new or holding a
/// library, outside the folders cloud services sync, and writable (a test file is
/// created and removed in it, or in the nearest existing folder above it). Links and `..`
/// are resolved first, in it and in `home` (the user's home folder, to recognise synced
/// folders). Refused with a [`config_error`] code.
pub fn check_library_dir(
    dir: &Path,
    home: Option<&Path>,
) -> Result<CheckedLibraryDir, &'static str> {
    if !dir.is_absolute() {
        return Err(config_error::LIBRARY_NOT_ABSOLUTE);
    }
    let dir = resolve_path(dir);
    let home = home.map(resolve_path);
    if let Some(home) = &home {
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
            let mut entries = fs::read_dir(&dir).map_err(|_| config_error::LIBRARY_NOT_A_FOLDER)?;
            // Finder's hidden files don't count.
            let has_files = entries.any(|entry| {
                entry.is_ok_and(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
            });
            if has_files {
                return Err(config_error::LIBRARY_FOLDER_NOT_EMPTY);
            }
        }
    }
    if !can_write_in(&dir) {
        return Err(config_error::LIBRARY_NOT_WRITABLE);
    }
    if warning.is_none()
        && let Some(home) = &home
        && ["Documents", "Desktop"]
            .iter()
            .any(|sub| dir.starts_with(home.join(sub)))
    {
        warning = Some(config_warning::LIBRARY_MAYBE_SYNCED);
    }
    Ok(CheckedLibraryDir { path: dir, warning })
}

/// Whether a file can be created in `dir`, or in the nearest existing folder above it
/// (where the missing folders would be created). Nothing is left behind.
fn can_write_in(dir: &Path) -> bool {
    let Some(existing) = dir.ancestors().find(|ancestor| ancestor.is_dir()) else {
        return false;
    };
    let probe = existing.join(format!(".trama-write-check-{}", std::process::id()));
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(file) => {
            drop(file);
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_gives_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = AppConfig::load(dir.path().join(CONFIG_FILE));
        assert_eq!((loaded.notice, loaded.save_blocked), (None, false));
        let config = loaded.config;
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
        config.reduce_transparency = true;
        config.reduce_motion = true;
        config.library_dir = Some(PathBuf::from("/Volumes/Data/Trama"));
        config.set_onboarding("permissions").unwrap();
        config.save(&path).unwrap();
        assert_eq!(AppConfig::load(&path).config, config);
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn the_onboarding_step_is_a_short_name() {
        let mut config = AppConfig::default();
        assert_eq!(config.onboarding, None);
        config.set_onboarding(ONBOARDING_DONE).unwrap();
        config.set_onboarding("system_audio").unwrap();
        assert_eq!(config.onboarding.as_deref(), Some("system_audio"));
        for bad in ["", "Done", "step-1", "../x", &"a".repeat(33)] {
            assert_eq!(
                config.set_onboarding(bad),
                Err(config_error::INVALID_ONBOARDING_STEP),
                "{bad}"
            );
        }
        assert_eq!(config.onboarding.as_deref(), Some("system_audio"));
        // A stored value that is not a step name counts as never recorded.
        assert_eq!(AppConfig::parse(r#"{"onboarding":"<b>"}"#).onboarding, None);
        assert_eq!(
            AppConfig::parse(r#"{"onboarding":"done"}"#)
                .onboarding
                .as_deref(),
            Some(ONBOARDING_DONE)
        );
    }

    #[test]
    fn bad_values_fall_back_one_by_one_and_unknown_keys_are_kept() {
        let config = AppConfig::parse(
            r#"{"theme":"neon","accent":"blue","interfaceLanguage":"fr",
                "libraryDir":"relative/path","globalShortcut":"KeyR",
                "reduceTransparency":"yes","reduceMotion":1,"future":{"x":1}}"#,
        );
        assert!(!config.reduce_transparency);
        assert!(!config.reduce_motion);
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

    /// The warning of a folder that is accepted, or the refusal.
    fn warning(dir: &Path, home: &Path) -> Result<Option<&'static str>, &'static str> {
        check_library_dir(dir, Some(home)).map(|checked| checked.warning)
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
        assert_eq!(warning(&home.join("new"), home), Ok(None));
        assert!(!home.join("new").exists());

        let empty = home.join("empty");
        fs::create_dir(&empty).unwrap();
        fs::write(empty.join(".DS_Store"), b"").unwrap();
        assert_eq!(warning(&empty, home), Ok(None));
        // The write check leaves nothing behind.
        assert_eq!(fs::read_dir(&empty).unwrap().count(), 1);

        let busy = home.join("busy");
        fs::create_dir(&busy).unwrap();
        fs::write(busy.join("notes.txt"), b"x").unwrap();
        assert_eq!(
            warning(&busy, home),
            Err(config_error::LIBRARY_FOLDER_NOT_EMPTY)
        );
        assert_eq!(
            warning(&busy.join("notes.txt"), home),
            Err(config_error::LIBRARY_NOT_A_FOLDER)
        );

        let existing = home.join("existing");
        fs::create_dir(&existing).unwrap();
        fs::write(existing.join(DATABASE_FILE), b"").unwrap();
        assert_eq!(
            warning(&existing, home),
            Ok(Some(config_warning::LIBRARY_EXISTS))
        );

        assert_eq!(
            warning(&home.join("Library/Mobile Documents/Trama"), home),
            Err(config_error::LIBRARY_SYNCED)
        );
        assert_eq!(
            warning(&home.join("Documents/Trama"), home),
            Ok(Some(config_warning::LIBRARY_MAYBE_SYNCED))
        );
    }

    #[cfg(unix)]
    #[test]
    fn links_and_dot_dot_cannot_hide_a_synced_folder() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let icloud = home.join("Library/Mobile Documents/com~apple~CloudDocs");
        fs::create_dir_all(&icloud).unwrap();

        // A link outside the synced folder that points into it.
        let link = root.path().join("shortcut");
        std::os::unix::fs::symlink(&icloud, &link).unwrap();
        assert_eq!(
            warning(&link.join("Trama"), &home),
            Err(config_error::LIBRARY_SYNCED)
        );

        // `..` that climbs into it, also through folders that don't exist yet.
        let sneaky = home.join("Projects/missing/../../Library/Mobile Documents/Trama");
        assert_eq!(warning(&sneaky, &home), Err(config_error::LIBRARY_SYNCED));

        // A home reached through a link is recognised too.
        let home_link = root.path().join("home-link");
        std::os::unix::fs::symlink(&home, &home_link).unwrap();
        assert_eq!(
            warning(&icloud.join("Trama"), &home_link),
            Err(config_error::LIBRARY_SYNCED)
        );

        // What is stored is the resolved folder.
        let target = root.path().join("data");
        fs::create_dir(&target).unwrap();
        let data_link = root.path().join("data-link");
        std::os::unix::fs::symlink(&target, &data_link).unwrap();
        let checked = check_library_dir(&data_link.join("x/../Trama"), Some(&home)).unwrap();
        assert_eq!(
            checked.path,
            fs::canonicalize(&target).unwrap().join("Trama")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_that_cannot_be_written_is_refused() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let locked = root.path().join("locked");
        fs::create_dir(&locked).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
        // Root can write anywhere: nothing to check then.
        let writable = fs::write(locked.join("probe"), b"").is_ok();
        if !writable {
            assert_eq!(
                warning(&locked, root.path()),
                Err(config_error::LIBRARY_NOT_WRITABLE)
            );
            // A new folder inside it couldn't be created either.
            assert_eq!(
                warning(&locked.join("Trama"), root.path()),
                Err(config_error::LIBRARY_NOT_WRITABLE)
            );
        }
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn an_unreadable_file_is_set_aside_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        fs::write(&path, b"{\"theme\": \"dark\", oops").unwrap();

        let loaded = AppConfig::load(&path);
        assert_eq!(loaded.config, AppConfig::default());
        assert_eq!(loaded.notice, Some(config_notice::SETTINGS_RESET));
        assert!(!loaded.save_blocked);
        let aside = dir.path().join("settings.json.corrupt");
        assert_eq!(
            fs::read_to_string(&aside).unwrap(),
            "{\"theme\": \"dark\", oops"
        );

        // Saving afterwards writes a new file and keeps the damaged one.
        loaded.config.save(&path).unwrap();
        assert!(aside.exists());
        let again = AppConfig::load(&path);
        assert_eq!(again.notice, None);
    }

    #[test]
    fn notices_and_errors_have_distinct_codes() {
        let mut codes: Vec<&str> = config_error::ALL
            .iter()
            .chain(config_warning::ALL)
            .chain(config_notice::ALL)
            .copied()
            .collect();
        let count = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), count);
    }
}
