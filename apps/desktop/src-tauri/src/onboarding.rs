//! First launch (flow F1): whether the onboarding is shown, where it resumes, and the
//! microphone and system audio permissions it asks for.
//!
//! **Who sees it.** The onboarding is shown only to a new install: at start, when
//! `settings.json` does not exist yet and the library has no database, `onboarding` is
//! stored as `"welcome"`. From then on the stored value decides: a step name resumes the
//! onboarding there, `"done"` (finished or skipped) never shows it again. A configuration
//! without the key, which is what every install made before the onboarding has, counts as
//! done, so an existing library is never greeted as new. The user can always open it again
//! from the command bar ("Set up Trama").
//!
//! **Permissions.** Reading the microphone status never asks anything. Asking is always the
//! user's click: the microphone through AVFoundation, system audio by the inaudible probe
//! of [`CaptureBackend::probe_system_audio_permission`], which makes macOS ask the first
//! time. Debug builds can replace both with `TRAMA_FAKE_PERMISSIONS=<microphone>,<system
//! audio>` (for example `not_determined,unknown`), so the steps can be tried without
//! touching the real permissions; an answer then turns into `granted`.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;
use tauri::{AppHandle, State};
use trama_core::capture::{CaptureBackend, PermissionState};
use trama_core::config::ONBOARDING_DONE;

use crate::settings::{INTERNAL_ERROR, Settings, blocking};

/// The step stored for a new install.
pub const FIRST_STEP: &str = "welcome";

/// Overrides the permissions in debug builds; see the module documentation.
const FAKE_PERMISSIONS_ENV: &str = "TRAMA_FAKE_PERMISSIONS";

/// Whether this start is a new install: nothing was ever configured on this Mac and the
/// library is empty, and the onboarding was never recorded.
pub fn is_new_install(recorded: Option<&str>, config_existed: bool, library: &Path) -> bool {
    recorded.is_none()
        && !config_existed
        && !library.join(trama_core::storage::DATABASE_FILE).exists()
}

/// Stores the first step for a new install. Called at start, before the library is
/// opened (opening it creates the database).
pub fn prepare(settings: &Settings, config_existed: bool) {
    let recorded = settings.config().onboarding.clone();
    if !is_new_install(recorded.as_deref(), config_existed, &settings.open_library) {
        return;
    }
    // A file that cannot be written only means the onboarding shows again next time.
    if let Err(code) = settings.update(|config| config.set_onboarding(FIRST_STEP)) {
        eprintln!("trama: cannot record the onboarding ({code})");
    }
}

/// The step the onboarding resumes at; `None` when it is not shown at start.
fn pending(recorded: Option<&str>) -> Option<String> {
    recorded
        .filter(|step| *step != ONBOARDING_DONE)
        .map(str::to_owned)
}

/// Where the onboarding stands, as the frontend sees it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingState {
    /// The step to show at start; `None`: the onboarding is done (or predates this install).
    pub pending: Option<String>,
}

/// Where the onboarding stands.
#[tauri::command]
pub fn onboarding_state(settings: State<'_, Settings>) -> OnboardingState {
    OnboardingState {
        pending: pending(settings.config().onboarding.as_deref()),
    }
}

/// Records the step reached, or `"done"` when the onboarding is finished or skipped.
#[tauri::command]
pub fn set_onboarding_step(settings: State<'_, Settings>, step: String) -> Result<(), String> {
    settings
        .update(|config| config.set_onboarding(&step))
        .map(drop)
}

/// A permission as the frontend names it.
fn permission_name(state: PermissionState) -> &'static str {
    match state {
        PermissionState::Granted => "granted",
        PermissionState::Denied => "denied",
        PermissionState::Restricted => "restricted",
        PermissionState::NotDetermined => "not_determined",
        PermissionState::Unknown => "unknown",
    }
}

fn parse_permission(name: &str) -> Option<PermissionState> {
    Some(match name.trim() {
        "granted" => PermissionState::Granted,
        "denied" => PermissionState::Denied,
        "restricted" => PermissionState::Restricted,
        "not_determined" => PermissionState::NotDetermined,
        "unknown" => PermissionState::Unknown,
        _ => return None,
    })
}

/// Permissions that stand in for the real ones in debug builds.
#[derive(Debug, Clone, Copy, PartialEq)]
struct FakePermissions {
    microphone: PermissionState,
    system_audio: PermissionState,
}

fn parse_fake(value: &str) -> Option<FakePermissions> {
    let (microphone, system_audio) = value.split_once(',')?;
    Some(FakePermissions {
        microphone: parse_permission(microphone)?,
        system_audio: parse_permission(system_audio)?,
    })
}

/// Where the permissions come from: macOS, or the fake of a debug build.
pub struct Permissions {
    fake: Option<Mutex<FakePermissions>>,
}

impl Permissions {
    /// The real permissions; in a debug build, the fake ones if `TRAMA_FAKE_PERMISSIONS`
    /// says so.
    pub fn from_env() -> Self {
        let fake = if cfg!(debug_assertions) {
            std::env::var(FAKE_PERMISSIONS_ENV)
                .ok()
                .and_then(|value| parse_fake(&value))
        } else {
            None
        };
        if fake.is_some() {
            eprintln!("trama: using the permissions of {FAKE_PERMISSIONS_ENV}");
        }
        Self {
            fake: fake.map(Mutex::new),
        }
    }

    fn fake(&self) -> Option<std::sync::MutexGuard<'_, FakePermissions>> {
        self.fake
            .as_ref()
            .map(|fake| fake.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn microphone(&self) -> PermissionState {
        match self.fake() {
            Some(fake) => fake.microphone,
            None => real_backend().map_or(PermissionState::Unknown, |b| b.microphone_permission()),
        }
    }

    fn request_microphone(&self) -> PermissionState {
        match self.fake() {
            Some(mut fake) => {
                if fake.microphone == PermissionState::NotDetermined {
                    fake.microphone = PermissionState::Granted;
                }
                fake.microphone
            }
            None => real_backend().map_or(PermissionState::Unknown, |b| {
                b.request_microphone_permission()
            }),
        }
    }

    fn check_system_audio(&self) -> PermissionState {
        match self.fake() {
            Some(mut fake) => {
                if matches!(
                    fake.system_audio,
                    PermissionState::NotDetermined | PermissionState::Unknown
                ) {
                    fake.system_audio = PermissionState::Granted;
                }
                fake.system_audio
            }
            None => real_backend().map_or(PermissionState::Unknown, |b| {
                b.probe_system_audio_permission()
            }),
        }
    }
}

#[cfg(target_os = "macos")]
fn real_backend() -> Option<Arc<dyn CaptureBackend>> {
    Some(Arc::new(trama_core::capture::macos::MacBackend::new()))
}

#[cfg(not(target_os = "macos"))]
fn real_backend() -> Option<Arc<dyn CaptureBackend>> {
    None
}

/// The microphone permission, without asking.
#[tauri::command]
pub fn microphone_permission(permissions: State<'_, Permissions>) -> &'static str {
    permission_name(permissions.microphone())
}

/// Asks for the microphone if macOS has not asked yet, and waits for the answer.
#[tauri::command]
pub async fn request_microphone_permission(app: AppHandle) -> Result<&'static str, String> {
    use tauri::Manager;
    blocking(app, |app| {
        Ok(permission_name(
            app.state::<Permissions>().request_microphone(),
        ))
    })
    .await
}

/// Checks system audio with the inaudible probe (about a second). The first time, macOS
/// asks the user; until they answer the result is not `granted`.
#[tauri::command]
pub async fn check_system_audio_permission(app: AppHandle) -> Result<&'static str, String> {
    use tauri::Manager;
    blocking(app, |app| {
        Ok(permission_name(
            app.state::<Permissions>().check_system_audio(),
        ))
    })
    .await
}

/// Panes of System Settings › Privacy & Security that the onboarding opens.
fn privacy_pane(pane: &str) -> Option<&'static str> {
    match pane {
        "microphone" => {
            Some("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
        }
        // "Screen & System Audio Recording", where "System Audio Recording Only" is listed.
        "system_audio" => {
            Some("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
        }
        _ => None,
    }
}

/// Opens the pane of System Settings where a permission is changed.
#[tauri::command]
pub fn open_privacy_settings(pane: String) -> Result<(), String> {
    let Some(url) = privacy_pane(&pane) else {
        return Err(INTERNAL_ERROR.to_owned());
    };
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("/usr/bin/open")
            .arg(url)
            .spawn()
            .map(drop)
            .map_err(|err| {
                eprintln!("trama: cannot open System Settings: {err}");
                INTERNAL_ERROR.to_owned()
            })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = url;
        Err(INTERNAL_ERROR.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_new_install_starts_the_onboarding() {
        let dir = tempfile::tempdir().unwrap();
        let library = dir.path().join("Library");
        assert!(is_new_install(None, false, &library));
        // Settings were saved on this Mac: an install from before the onboarding.
        assert!(!is_new_install(None, true, &library));
        // A recorded step decides by itself.
        assert!(!is_new_install(Some("model"), false, &library));
        assert!(!is_new_install(Some(ONBOARDING_DONE), false, &library));
        // A library with its database: someone has used Trama here already.
        std::fs::create_dir_all(&library).unwrap();
        std::fs::write(library.join(trama_core::storage::DATABASE_FILE), b"").unwrap();
        assert!(!is_new_install(None, false, &library));
    }

    #[test]
    fn the_stored_step_says_where_to_resume() {
        assert_eq!(pending(None), None);
        assert_eq!(pending(Some(ONBOARDING_DONE)), None);
        assert_eq!(pending(Some("model")), Some("model".to_owned()));
    }

    #[test]
    fn fake_permissions_are_read_and_answer_once() {
        assert_eq!(parse_fake("granted"), None);
        assert_eq!(parse_fake("granted,maybe"), None);
        let fake = parse_fake("not_determined, unknown").unwrap();
        assert_eq!(fake.microphone, PermissionState::NotDetermined);
        assert_eq!(fake.system_audio, PermissionState::Unknown);

        let permissions = Permissions {
            fake: Some(Mutex::new(fake)),
        };
        assert_eq!(permissions.microphone(), PermissionState::NotDetermined);
        assert_eq!(permissions.request_microphone(), PermissionState::Granted);
        assert_eq!(permissions.check_system_audio(), PermissionState::Granted);

        // A refusal stays: only System Settings changes it.
        let denied = Permissions {
            fake: Some(Mutex::new(parse_fake("denied,denied").unwrap())),
        };
        assert_eq!(denied.request_microphone(), PermissionState::Denied);
        assert_eq!(denied.check_system_audio(), PermissionState::Denied);
    }

    #[test]
    fn every_permission_has_a_frontend_name() {
        for state in [
            PermissionState::Granted,
            PermissionState::Denied,
            PermissionState::Restricted,
            PermissionState::NotDetermined,
            PermissionState::Unknown,
        ] {
            assert_eq!(parse_permission(permission_name(state)), Some(state));
        }
    }

    #[test]
    fn only_known_privacy_panes_open() {
        assert!(
            privacy_pane("microphone")
                .unwrap()
                .ends_with("Privacy_Microphone")
        );
        assert!(privacy_pane("system_audio").is_some());
        assert_eq!(privacy_pane("camera"), None);
    }
}
