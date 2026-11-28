//! Bridge between the settings and the frontend: the app configuration on this Mac
//! ([`AppConfig`]), the settings kept in the library and the month's spend on language
//! models. Every failure reaches the frontend as a code it translates (A5).

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime, State};
use trama_core::config::{AppConfig, config_error, resolve_path};
use trama_core::jobs::lock;
use trama_core::storage::{
    AudioRetention, BudgetStatus, HeldAudio, Library, LibrarySettings, RetentionImpact,
    StorageError, UsageSettings,
};

/// Code of a library that could not be read or written.
pub const STORAGE_ERROR: &str = "storage";
/// Code of a setting the core refused.
pub const INVALID_SETTING: &str = "invalid_setting";
/// Code of a change of audio retention that would delete existing audio and that the
/// user has not confirmed.
pub const AUDIO_DELETION_UNCONFIRMED: &str = "audio_deletion_unconfirmed";
/// Code of a command whose work did not finish (a panic): a bug.
pub const INTERNAL_ERROR: &str = "internal";

/// The app configuration and where the library is, owned by the app for its whole life.
pub struct Settings {
    /// The configuration file.
    pub path: PathBuf,
    pub config: Mutex<AppConfig>,
    /// The library opened at start.
    pub open_library: PathBuf,
    /// The library location when none is chosen.
    pub default_library: PathBuf,
    /// `TRAMA_LIBRARY` is set: the configured location is ignored.
    pub library_overridden: bool,
    /// The user's home folder, to recognise synced folders.
    pub home: Option<PathBuf>,
    /// The configuration file couldn't be read nor set aside: saving would destroy it.
    pub save_blocked: bool,
    /// Problems found at start, for the frontend to tell the user.
    pub notices: Vec<StartupNotice>,
}

/// A problem found at start (a `config_notice` code); Trama started anyway.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StartupNotice {
    pub code: &'static str,
    /// The folder or file it is about.
    pub path: Option<String>,
}

impl StartupNotice {
    pub fn new(code: &'static str, path: Option<&Path>) -> Self {
        Self {
            code,
            path: path.map(path_text),
        }
    }
}

impl Settings {
    pub fn config(&self) -> MutexGuard<'_, AppConfig> {
        self.config.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Applies `change` to the configuration and saves it; nothing changes if either fails.
    pub fn update(
        &self,
        change: impl FnOnce(&mut AppConfig) -> Result<(), &'static str>,
    ) -> Result<AppConfig, String> {
        let mut config = self.config();
        let mut next = config.clone();
        change(&mut next)?;
        if self.save_blocked {
            return Err(config_error::CONFIG_WRITE.to_owned());
        }
        next.save(&self.path).map_err(|err| {
            eprintln!("trama: cannot save {}: {err}", self.path.display());
            config_error::CONFIG_WRITE.to_owned()
        })?;
        *config = next.clone();
        Ok(next)
    }

    /// The library the app opens at the next start.
    pub fn next_library(&self) -> PathBuf {
        if self.library_overridden {
            return self.open_library.clone();
        }
        self.config()
            .library_dir
            .clone()
            .unwrap_or_else(|| self.default_library.clone())
    }
}

/// Where the library is, as the frontend sees it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LibraryLocation {
    /// The library open now.
    pub current: String,
    /// The library opened at the next start.
    pub next: String,
    pub default: String,
    /// Set by `TRAMA_LIBRARY` in development: it can't be changed here.
    pub overridden: bool,
    /// The next start opens another library.
    pub restart_required: bool,
}

/// The app configuration, as the frontend sees it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsPayload {
    /// `None` follows macOS.
    pub interface_language: Option<String>,
    pub theme: String,
    pub accent: String,
    pub reduce_transparency: bool,
    pub reduce_motion: bool,
    /// Microphone by device id; `None` follows the system default.
    pub microphone: Option<String>,
    pub global_shortcut: String,
    pub default_global_shortcut: &'static str,
    pub library: LibraryLocation,
}

pub fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn location(settings: &Settings) -> LibraryLocation {
    let next = settings.next_library();
    LibraryLocation {
        current: path_text(&settings.open_library),
        restart_required: resolve_path(&next) != resolve_path(&settings.open_library),
        next: path_text(&next),
        default: path_text(&settings.default_library),
        overridden: settings.library_overridden,
    }
}

fn payload(settings: &Settings) -> AppSettingsPayload {
    let config = settings.config().clone();
    AppSettingsPayload {
        interface_language: config.interface_language,
        theme: config.theme,
        accent: config.accent,
        reduce_transparency: config.reduce_transparency,
        reduce_motion: config.reduce_motion,
        microphone: config.microphone,
        global_shortcut: config.global_shortcut,
        default_global_shortcut: trama_core::config::DEFAULT_GLOBAL_SHORTCUT,
        library: location(settings),
    }
}

/// Runs `work` on a blocking thread: the library, the Keychain and the network must not
/// hold up the main thread.
pub async fn blocking<T: Send + 'static>(
    app: AppHandle,
    work: impl FnOnce(&AppHandle) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || work(&app))
        .await
        .map_err(|err| {
            eprintln!("trama: a settings command did not finish: {err}");
            INTERNAL_ERROR.to_owned()
        })?
}

/// A storage error as a code; the detail goes to the log.
pub fn storage_code(err: StorageError) -> String {
    eprintln!("trama: settings storage error: {err}");
    match err {
        StorageError::Invalid(_) => INVALID_SETTING.to_owned(),
        _ => STORAGE_ERROR.to_owned(),
    }
}

/// The app configuration.
#[tauri::command]
pub fn app_settings(settings: State<'_, Settings>) -> AppSettingsPayload {
    payload(&settings)
}

/// Sets the interface language (`None` follows macOS), translates the native menus again
/// and returns the locale now in use.
#[tauri::command]
pub fn set_interface_language<R: Runtime>(
    app: AppHandle<R>,
    settings: State<'_, Settings>,
    language: Option<String>,
) -> Result<String, String> {
    settings.update(|config| config.set_interface_language(language.as_deref()))?;
    crate::apply_locale(&app).map_err(|err| {
        eprintln!("trama: cannot translate the menus again: {err}");
        INTERNAL_ERROR.to_owned()
    })
}

/// Sets the theme and the accent colour.
#[tauri::command]
pub fn set_appearance(
    app: AppHandle,
    settings: State<'_, Settings>,
    theme: String,
    accent: String,
) -> Result<(), String> {
    let config = settings.update(|config| config.set_appearance(&theme, &accent))?;
    app.set_theme(native_theme(&config.theme));
    crate::dock_icon::show(&app, &config.accent);
    Ok(())
}

/// The macOS appearance for a theme: forcing it makes the window's vibrancy (sidebar,
/// glass) match the chosen theme instead of the system one. `None` follows the system.
pub fn native_theme(theme: &str) -> Option<tauri::Theme> {
    match theme {
        "light" => Some(tauri::Theme::Light),
        "dark" => Some(tauri::Theme::Dark),
        _ => None,
    }
}

/// Keeps the window opaque even when macOS keeps transparency on.
#[tauri::command]
pub fn set_reduce_transparency(settings: State<'_, Settings>, enabled: bool) -> Result<(), String> {
    settings
        .update(|config| {
            config.reduce_transparency = enabled;
            Ok(())
        })
        .map(drop)
}

/// Turns transitions off even when macOS keeps motion on.
#[tauri::command]
pub fn set_reduce_motion(settings: State<'_, Settings>, enabled: bool) -> Result<(), String> {
    settings
        .update(|config| {
            config.reduce_motion = enabled;
            Ok(())
        })
        .map(drop)
}

/// Sets the microphone (`None` follows the system default). The recording flow reads it.
#[tauri::command]
pub fn set_microphone(settings: State<'_, Settings>, device: Option<String>) -> Result<(), String> {
    settings
        .update(|config| {
            config.microphone = device.filter(|id| !id.is_empty());
            Ok(())
        })
        .map(drop)
}

/// Sets the global shortcut that starts and stops recording. It is stored here; the menu
/// bar item (#32) registers it with the system.
#[tauri::command]
pub fn set_global_shortcut(settings: State<'_, Settings>, shortcut: String) -> Result<(), String> {
    settings
        .update(|config| config.set_global_shortcut(&shortcut))
        .map(drop)
}

/// A microphone, as the frontend sees it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InputDevice {
    pub id: String,
    pub name: String,
}

/// The microphones connected now and the system default.
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct InputDevices {
    pub devices: Vec<InputDevice>,
    pub default_id: Option<String>,
}

/// The microphones connected now. Fails with a capture error code.
#[tauri::command]
pub async fn input_devices(app: AppHandle) -> Result<InputDevices, String> {
    blocking(app, |_| list_input_devices()).await
}

#[cfg(target_os = "macos")]
fn list_input_devices() -> Result<InputDevices, String> {
    use trama_core::capture::macos::MacBackend;
    use trama_core::capture::{CaptureBackend, Source};

    let backend = MacBackend::new();
    let to_code = |err: trama_core::capture::CaptureError| {
        eprintln!("trama: cannot list the microphones: {err}");
        err.code.to_owned()
    };
    let devices = backend.input_devices().map_err(to_code)?;
    let default_id = backend
        .default_device(Source::Microphone)
        .map_err(to_code)?
        .map(|device| device.id);
    Ok(InputDevices {
        devices: devices
            .into_iter()
            .map(|device| InputDevice {
                id: device.id,
                name: device.name,
            })
            .collect(),
        default_id,
    })
}

#[cfg(not(target_os = "macos"))]
fn list_input_devices() -> Result<InputDevices, String> {
    Ok(InputDevices::default())
}

/// A folder chosen for the library, checked.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChosenFolder {
    pub path: String,
    /// Accepted with a caveat (a `config_warning` code).
    pub warning: Option<&'static str>,
    /// Refused (a `config_error` code).
    pub error: Option<&'static str>,
}

/// Checks a folder; the path returned is the resolved one (links and `..` followed),
/// which is what gets stored.
fn check_folder(settings: &Settings, path: &Path) -> ChosenFolder {
    match trama_core::config::check_library_dir(path, settings.home.as_deref()) {
        Ok(checked) => ChosenFolder {
            path: path_text(&checked.path),
            warning: checked.warning,
            error: None,
        },
        Err(error) => ChosenFolder {
            path: path_text(path),
            warning: None,
            error: Some(error),
        },
    }
}

/// Asks for a folder for the library with the system dialog and checks it; nothing is
/// saved. `None` if the user cancelled.
#[tauri::command]
pub async fn choose_library_folder(app: AppHandle) -> Result<Option<ChosenFolder>, String> {
    blocking(app, |app| {
        use tauri_plugin_dialog::DialogExt;
        let settings = app.state::<Settings>();
        let start = settings
            .next_library()
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let picked = app
            .dialog()
            .file()
            .set_directory(start)
            .set_can_create_directories(true)
            .blocking_pick_folder();
        Ok(picked
            .and_then(|path| path.into_path().ok())
            .map(|path| check_folder(&settings, &path)))
    })
    .await
}

/// Sets where the library is opened at the next start (`None`: the default location).
/// The open library is not moved, copied or closed: the user is told a restart is needed.
#[tauri::command]
pub fn set_library_location(
    settings: State<'_, Settings>,
    path: Option<String>,
) -> Result<LibraryLocation, String> {
    set_location(&settings, path)
}

fn set_location(settings: &Settings, path: Option<String>) -> Result<LibraryLocation, String> {
    let dir = match path {
        Some(path) => {
            let checked =
                trama_core::config::check_library_dir(Path::new(&path), settings.home.as_deref())
                    .map_err(str::to_owned)?;
            // The default location is stored as "no choice", so it follows the app.
            let default = resolve_path(&settings.default_library);
            (checked.path != default).then_some(checked.path)
        }
        None => None,
    };
    settings.update(|config| {
        config.library_dir = dir;
        Ok(())
    })?;
    Ok(location(settings))
}

/// Problems found at start, to tell the user: a configuration file set aside, a library
/// that couldn't be opened.
#[tauri::command]
pub fn startup_notices(settings: State<'_, Settings>) -> Vec<StartupNotice> {
    settings.notices.clone()
}

/// Shows the open library in the Finder.
#[tauri::command]
pub fn reveal_library(settings: State<'_, Settings>) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("/usr/bin/open")
            .arg(&settings.open_library)
            .spawn()
            .map(drop)
            .map_err(|err| {
                eprintln!("trama: cannot show the library in the Finder: {err}");
                INTERNAL_ERROR.to_owned()
            })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = settings;
        Err(INTERNAL_ERROR.to_owned())
    }
}

/// The library settings, as the frontend sees them. Amounts are in millionths of the
/// currency unit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySettingsPayload {
    pub meeting_language: String,
    /// `never`, `7_days`, `30_days` or `always`.
    pub audio_retention: String,
    pub currency: String,
    pub monthly_budget_micros: Option<u64>,
    pub warn_percent: u8,
    pub enforce_budget: bool,
}

impl From<LibrarySettings> for LibrarySettingsPayload {
    fn from(settings: LibrarySettings) -> Self {
        Self {
            meeting_language: settings.meeting_language,
            audio_retention: settings.audio_retention.as_str().to_owned(),
            currency: settings.usage.currency,
            monthly_budget_micros: settings.usage.monthly_budget_micros,
            warn_percent: settings.usage.warn_percent,
            enforce_budget: settings.usage.enforce_budget,
        }
    }
}

impl TryFrom<LibrarySettingsPayload> for LibrarySettings {
    type Error = String;

    fn try_from(payload: LibrarySettingsPayload) -> Result<Self, Self::Error> {
        let audio_retention: AudioRetention = payload
            .audio_retention
            .parse()
            .map_err(|_| INVALID_SETTING.to_owned())?;
        Ok(Self {
            meeting_language: payload.meeting_language,
            audio_retention,
            usage: UsageSettings {
                currency: payload.currency,
                monthly_budget_micros: payload.monthly_budget_micros,
                warn_percent: payload.warn_percent,
                enforce_budget: payload.enforce_budget,
            },
        })
    }
}

/// The settings kept in the library.
#[tauri::command]
pub async fn library_settings(app: AppHandle) -> Result<LibrarySettingsPayload, String> {
    blocking(app, |app| {
        let core = crate::jobs::core_state(app)?;
        let settings = lock(&core.library)
            .library_settings()
            .map_err(storage_code)?;
        Ok(settings.into())
    })
    .await
}

/// Stores the settings kept in the library and returns them as stored.
///
/// A change of audio retention that would delete audio the user already has is refused
/// with [`AUDIO_DELETION_UNCONFIRMED`] unless `confirm_audio_deletion` says the user
/// confirmed it. Once a new retention is stored it applies to every meeting, including
/// the audio on hold, and the runner looks for audio to delete right away.
#[tauri::command]
pub async fn set_library_settings(
    app: AppHandle,
    settings: LibrarySettingsPayload,
    confirm_audio_deletion: Option<bool>,
) -> Result<LibrarySettingsPayload, String> {
    blocking(app, move |app| {
        let settings = LibrarySettings::try_from(settings)?;
        let core = crate::jobs::core_state(app)?;
        let (stored, retention_changed) = store_library_settings(
            &mut lock(&core.library),
            &settings,
            confirm_audio_deletion.unwrap_or(false),
        )?;
        if retention_changed {
            core.runner.request_sweep();
        }
        Ok(stored.into())
    })
    .await
}

/// Stores the library settings, applying the audio retention rules above. Returns the
/// settings as stored, and whether the retention changed.
fn store_library_settings(
    library: &mut Library,
    settings: &LibrarySettings,
    confirmed: bool,
) -> Result<(LibrarySettings, bool), String> {
    let current = library
        .library_settings()
        .map_err(storage_code)?
        .audio_retention;
    let changed = current != settings.audio_retention;
    if changed
        && !confirmed
        && !library
            .audio_retention_impact(settings.audio_retention)
            .map_err(storage_code)?
            .is_empty()
    {
        return Err(AUDIO_DELETION_UNCONFIRMED.to_owned());
    }
    let stored = library
        .set_library_settings(settings)
        .map_err(storage_code)?;
    if changed {
        let released = library.release_audio_holds().map_err(storage_code)?;
        eprintln!(
            "trama: audio retention changed from {} to {}{}",
            current.as_str(),
            stored.audio_retention.as_str(),
            if released > 0 {
                format!("; it now applies to {released} meetings recorded before")
            } else {
                String::new()
            }
        );
    }
    Ok((stored, changed))
}

/// What a retention setting would delete of the audio that exists now.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RetentionImpactPayload {
    /// Recordings whose audio would be deleted right away.
    pub delete_now: u32,
    /// Recordings whose audio would be deleted when their period ends.
    pub delete_later: u32,
    /// Days the setting keeps audio after the transcription; absent for "always".
    pub days: Option<u32>,
}

impl RetentionImpactPayload {
    fn new(impact: RetentionImpact, retention: AudioRetention) -> Self {
        Self {
            delete_now: impact.delete_now,
            delete_later: impact.delete_later,
            days: retention.days(),
        }
    }
}

/// What changing the audio retention to `retention` would delete, so the interface can
/// ask before.
#[tauri::command]
pub async fn audio_retention_impact(
    app: AppHandle,
    retention: String,
) -> Result<RetentionImpactPayload, String> {
    blocking(app, move |app| {
        let retention: AudioRetention =
            retention.parse().map_err(|_| INVALID_SETTING.to_owned())?;
        let core = crate::jobs::core_state(app)?;
        let impact = lock(&core.library)
            .audio_retention_impact(retention)
            .map_err(storage_code)?;
        Ok(RetentionImpactPayload::new(impact, retention))
    })
    .await
}

/// Audio recorded before audio retention existed, kept until the user confirms.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HeldAudioPayload {
    pub count: u32,
    /// What the current setting would delete of it.
    pub impact: RetentionImpactPayload,
}

fn held_payload(library: &Library) -> Result<HeldAudioPayload, String> {
    let HeldAudio { count, impact } = library.held_audio().map_err(storage_code)?;
    let retention = library
        .library_settings()
        .map_err(storage_code)?
        .audio_retention;
    Ok(HeldAudioPayload {
        count,
        impact: RetentionImpactPayload::new(impact, retention),
    })
}

/// The audio on hold, and what the current setting would delete of it.
#[tauri::command]
pub async fn held_audio(app: AppHandle) -> Result<HeldAudioPayload, String> {
    blocking(app, |app| {
        let core = crate::jobs::core_state(app)?;
        held_payload(&lock(&core.library))
    })
    .await
}

/// Applies the audio retention to the audio on hold, after the user confirmed; returns
/// what is left on hold (nothing).
#[tauri::command]
pub async fn apply_retention_to_held_audio(app: AppHandle) -> Result<HeldAudioPayload, String> {
    blocking(app, |app| {
        let core = crate::jobs::core_state(app)?;
        let held = {
            let mut library = lock(&core.library);
            let released = library.release_audio_holds().map_err(storage_code)?;
            eprintln!("trama: audio retention now applies to {released} meetings recorded before");
            held_payload(&library)?
        };
        core.runner.request_sweep();
        Ok(held)
    })
    .await
}

/// The month's spend on language models against the budget.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BudgetPayload {
    /// Start of the month (UTC milliseconds).
    pub period_start: i64,
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub spent_micros: u64,
    /// Requests whose model had no price: the real spend is higher.
    pub unpriced_requests: u64,
    pub budget_micros: Option<u64>,
    pub warn_percent: u8,
    pub enforce: bool,
    /// `no_budget`, `ok`, `warning` or `exceeded`.
    pub state: &'static str,
}

impl From<BudgetStatus> for BudgetPayload {
    fn from(status: BudgetStatus) -> Self {
        Self {
            period_start: status.period_start,
            requests: status.totals.requests,
            input_tokens: status.totals.input_tokens,
            output_tokens: status.totals.output_tokens,
            spent_micros: status.totals.cost_micros,
            unpriced_requests: status.totals.unpriced_requests,
            budget_micros: status.budget_micros,
            warn_percent: status.warn_percent,
            enforce: status.enforce,
            state: status.state.as_str(),
        }
    }
}

/// The month's spend on language models against the budget.
#[tauri::command]
pub async fn budget_status(app: AppHandle) -> Result<BudgetPayload, String> {
    blocking(app, |app| {
        let core = crate::jobs::core_state(app)?;
        let status = lock(&core.library).budget_status().map_err(storage_code)?;
        Ok(status.into())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_theme_follows_the_chosen_theme() {
        assert_eq!(native_theme("light"), Some(tauri::Theme::Light));
        assert_eq!(native_theme("dark"), Some(tauri::Theme::Dark));
        assert_eq!(native_theme("system"), None);
    }

    fn settings(dir: &Path) -> Settings {
        Settings {
            path: dir.join("settings.json"),
            config: Mutex::new(AppConfig::default()),
            open_library: dir.join("Library"),
            default_library: dir.join("Library"),
            library_overridden: false,
            home: None,
            save_blocked: false,
            notices: Vec::new(),
        }
    }

    #[test]
    fn a_refused_change_saves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let settings = settings(dir.path());
        let err = settings
            .update(|config| config.set_appearance("neon", "lime"))
            .unwrap_err();
        assert_eq!(err, config_error::UNKNOWN_APPEARANCE);
        assert!(!settings.path.exists());

        settings
            .update(|config| config.set_appearance("dark", "blue"))
            .unwrap();
        let saved = AppConfig::load(&settings.path).config;
        assert_eq!(
            (saved.theme.as_str(), saved.accent.as_str()),
            ("dark", "blue")
        );
    }

    #[test]
    fn a_new_library_location_waits_for_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let settings = settings(dir.path());
        assert!(!location(&settings).restart_required);
        settings
            .update(|config| {
                config.library_dir = Some(dir.path().join("Elsewhere"));
                Ok(())
            })
            .unwrap();
        let moved = location(&settings);
        assert!(moved.restart_required);
        assert_eq!(moved.current, path_text(&dir.path().join("Library")));
        assert_eq!(moved.next, path_text(&dir.path().join("Elsewhere")));
    }

    #[test]
    fn a_config_file_that_could_not_be_set_aside_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings {
            save_blocked: true,
            ..settings(dir.path())
        };
        let err = settings
            .update(|config| config.set_appearance("dark", "blue"))
            .unwrap_err();
        assert_eq!(err, config_error::CONFIG_WRITE);
        assert!(!settings.path.exists());
        assert_eq!(settings.config().theme, "system");
    }

    #[test]
    fn a_chosen_folder_is_stored_resolved() {
        let dir = tempfile::tempdir().unwrap();
        let settings = settings(dir.path());
        let chosen = dir.path().join("Data/x/../Trama");
        let location = set_location(&settings, Some(path_text(&chosen))).unwrap();
        let resolved = resolve_path(&dir.path().join("Data/Trama"));
        assert_eq!(location.next, path_text(&resolved));
        assert_eq!(settings.config().library_dir, Some(resolved));
        assert!(location.restart_required);

        // The default location, however it is written, is stored as no choice.
        let default = dir.path().join("Data/../Library");
        let location = set_location(&settings, Some(path_text(&default))).unwrap();
        assert_eq!(settings.config().library_dir, None);
        assert!(!location.restart_required);
    }

    #[test]
    fn payloads_use_the_frontend_names() {
        let dir = tempfile::tempdir().unwrap();
        let json = serde_json::to_value(payload(&settings(dir.path()))).unwrap();
        assert_eq!(json["theme"], "system");
        assert_eq!(json["globalShortcut"], "Alt+Super+KeyR");
        assert_eq!(json["library"]["restartRequired"], false);

        let library: LibrarySettingsPayload = LibrarySettings::default().into();
        let json = serde_json::to_value(&library).unwrap();
        assert_eq!(json["audioRetention"], "never");
        assert_eq!(json["meetingLanguage"], "en");
        assert!(json["monthlyBudgetMicros"].is_null());
        let back = LibrarySettings::try_from(library).unwrap();
        assert_eq!(back, LibrarySettings::default());

        let bad = LibrarySettingsPayload {
            audio_retention: "forever".into(),
            ..LibrarySettings::default().into()
        };
        assert_eq!(LibrarySettings::try_from(bad).unwrap_err(), INVALID_SETTING);

        let impact = RetentionImpactPayload::new(
            RetentionImpact {
                delete_now: 2,
                delete_later: 1,
            },
            AudioRetention::Days7,
        );
        let json = serde_json::to_value(&impact).unwrap();
        assert_eq!(json["deleteNow"], 2);
        assert_eq!(json["deleteLater"], 1);
        assert_eq!(json["days"], 7);
    }

    /// A library with one transcribed meeting that still has its audio.
    fn library_with_audio(dir: &Path) -> Library {
        use trama_core::storage::{MeetingStatus, NewSegment, NewTrack, Role};
        let mut library = Library::open(dir).unwrap();
        let mut settings = library.library_settings().unwrap();
        settings.audio_retention = AudioRetention::Always;
        library.set_library_settings(&settings).unwrap();
        let id = library.create_meeting("it", 0).unwrap().id;
        library
            .add_track(
                id,
                &NewTrack {
                    role: Role::Me,
                    audio_path: Library::meeting_audio_dir(id).join("me.wav"),
                    timing_path: None,
                    device: None,
                },
            )
            .unwrap();
        library
            .set_meeting_status(id, MeetingStatus::Processing, None)
            .unwrap();
        library
            .complete_transcription(
                id,
                "engine",
                &[NewSegment {
                    role: Role::Me,
                    start_ms: 0,
                    end_ms: 1,
                    text: "ciao".into(),
                }],
            )
            .unwrap();
        library
    }

    #[test]
    fn deleting_existing_audio_needs_a_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let mut library = library_with_audio(dir.path());
        let mut never = library.library_settings().unwrap();
        never.audio_retention = AudioRetention::Never;

        assert_eq!(
            store_library_settings(&mut library, &never, false).unwrap_err(),
            AUDIO_DELETION_UNCONFIRMED
        );
        assert_eq!(
            library.library_settings().unwrap().audio_retention,
            AudioRetention::Always
        );

        // Other settings save as usual while the retention stays.
        let mut currency = library.library_settings().unwrap();
        currency.usage.currency = "EUR".into();
        let (_, changed) = store_library_settings(&mut library, &currency, false).unwrap();
        assert!(!changed);

        let never = LibrarySettings {
            audio_retention: AudioRetention::Never,
            ..library.library_settings().unwrap()
        };
        let (stored, changed) = store_library_settings(&mut library, &never, true).unwrap();
        assert!(changed);
        assert_eq!(stored.audio_retention, AudioRetention::Never);
        assert_eq!(stored.usage.currency, "EUR");
        assert_eq!(library.audio_due_for_deletion().unwrap().len(), 1);

        // Keeping more deletes nothing: no confirmation needed.
        let mut always = stored.clone();
        always.audio_retention = AudioRetention::Always;
        assert!(store_library_settings(&mut library, &always, false).is_ok());
    }
}
