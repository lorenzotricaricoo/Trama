//! Trama desktop app: a thin Tauri shell over `trama-core`.
//!
//! The shell owns windows, menus and the bridge to the frontend. Everything else
//! belongs to the core library (principle A1).

mod dock_icon;
mod jobs;
mod meetings;
mod menu;
mod models;
mod notes;
mod providers;
mod quit;
mod recording;
mod settings;
mod summary;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use tauri::{AppHandle, Manager, RunEvent, Runtime, State, WindowEvent};
use trama_core::config::{AppConfig, CONFIG_FILE, config_notice};
use trama_core::i18n::{Catalog, Localizer, system_locales};
use trama_core::jobs::{DeleteAudio, JobHandler, Runner, Transcribe};
use trama_core::providers::{ClientConfig, Keychain};
use trama_core::storage::Library;
use trama_core::summary::Summarize;
use trama_core::transcription::SherpaLoader;

/// Catalogs used by the shell itself (native menus, notifications). The frontend loads its
/// own.
const CATALOGS: &[Catalog] = &[
    Catalog {
        locale: "en",
        name: "menu",
        source: include_str!("../../../../locales/en/menu.ftl"),
    },
    Catalog {
        locale: "it",
        name: "menu",
        source: include_str!("../../../../locales/it/menu.ftl"),
    },
    Catalog {
        locale: "en",
        name: "shell",
        source: include_str!("../../../../locales/en/shell.ftl"),
    },
    Catalog {
        locale: "it",
        name: "shell",
        source: include_str!("../../../../locales/it/shell.ftl"),
    },
];

/// Overrides the system language in development, for example `TRAMA_LOCALE=en`.
const LOCALE_OVERRIDE_ENV: &str = "TRAMA_LOCALE";

/// Overrides the library folder, for example to keep development data apart from the
/// installed app: `TRAMA_LIBRARY=/tmp/trama-dev`.
const LIBRARY_OVERRIDE_ENV: &str = "TRAMA_LIBRARY";

/// Name of the library folder inside the app data folder.
const LIBRARY_DIR: &str = "Library";

/// State shared with Tauri commands.
struct AppState {
    /// The interface locale; it changes when the user picks another language.
    locale: Mutex<String>,
}

/// Locales to negotiate: the override if set, then the language chosen in Settings,
/// otherwise the system preferences.
fn requested_locales(config: &AppConfig) -> Vec<String> {
    match std::env::var(LOCALE_OVERRIDE_ENV) {
        Ok(locale) if !locale.trim().is_empty() => vec![locale],
        _ => match &config.interface_language {
            Some(language) => vec![language.clone()],
            None => system_locales(),
        },
    }
}

/// The library folder: the override if set, then the location chosen in Settings,
/// otherwise inside the app data folder
/// (`~/Library/Application Support/<identifier>/Library` on macOS). Returns the folder
/// and whether the override decided it.
fn library_dir(config: &AppConfig, default: &Path) -> (PathBuf, bool) {
    match std::env::var_os(LIBRARY_OVERRIDE_ENV) {
        Some(dir) if !dir.is_empty() => (PathBuf::from(dir), true),
        _ => (
            config
                .library_dir
                .clone()
                .unwrap_or_else(|| default.to_path_buf()),
            false,
        ),
    }
}

/// Reads the app configuration from the app data folder. A file that can't be read is set
/// aside (or, failing that, never overwritten) and reported as a startup notice.
fn load_settings<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<settings::Settings> {
    let data_dir = app.path().app_data_dir()?;
    let path = data_dir.join(CONFIG_FILE);
    let loaded = AppConfig::load(&path);
    let notices = loaded
        .notice
        .map(|code| settings::StartupNotice::new(code, Some(&path)))
        .into_iter()
        .collect();
    let default_library = data_dir.join(LIBRARY_DIR);
    let (open_library, library_overridden) = library_dir(&loaded.config, &default_library);
    Ok(settings::Settings {
        path,
        config: Mutex::new(loaded.config),
        open_library,
        default_library,
        library_overridden,
        home: app.path().home_dir().ok(),
        save_blocked: loaded.save_blocked,
        notices,
    })
}

/// What opening the library at start gave.
struct LibraryOpening<T> {
    /// The open library, if any could be opened.
    core: Option<T>,
    /// The folder of the open library (the one tried last when none opened).
    dir: PathBuf,
    notice: Option<settings::StartupNotice>,
}

/// Opens the library in `wanted`. If it can't be opened (a disconnected disk, a folder
/// without permission, a newer version of Trama, another copy holding it) the default
/// library is opened instead when `may_fall_back`, and the user is told; the chosen
/// location stays saved, so it is tried again at the next start. Never fails: without
/// any library the app still starts and says so.
fn open_with_fallback<T, E: std::fmt::Display>(
    wanted: &Path,
    default: &Path,
    may_fall_back: bool,
    mut open: impl FnMut(&Path) -> Result<T, E>,
) -> LibraryOpening<T> {
    let err = match open(wanted) {
        Ok(core) => {
            return LibraryOpening {
                core: Some(core),
                dir: wanted.to_path_buf(),
                notice: None,
            };
        }
        Err(err) => err,
    };
    eprintln!(
        "trama: cannot open the library in {}: {err}",
        wanted.display()
    );
    let failed = |dir: &Path| LibraryOpening {
        core: None,
        dir: dir.to_path_buf(),
        notice: Some(settings::StartupNotice::new(
            config_notice::LIBRARY_FAILED,
            Some(dir),
        )),
    };
    if !may_fall_back || wanted == default {
        return failed(wanted);
    }
    match open(default) {
        Ok(core) => LibraryOpening {
            core: Some(core),
            dir: default.to_path_buf(),
            notice: Some(settings::StartupNotice::new(
                config_notice::LIBRARY_UNAVAILABLE,
                Some(wanted),
            )),
        },
        Err(err) => {
            eprintln!(
                "trama: cannot open the default library in {} either: {err}",
                default.display()
            );
            failed(default)
        }
    }
}

/// Negotiates the interface locale from the configuration and translates the native
/// menus with it. Returns the locale.
fn apply_locale<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<String> {
    let config = app.state::<settings::Settings>().config().clone();
    let l10n = Localizer::new(&requested_locales(&config), CATALOGS)
        .expect("the embedded menu catalogs are valid (checked by tests)");
    let locale = l10n.locale().to_string();
    menu::install(app, &l10n)?;
    if let Some(recorder) = app.try_state::<recording::Recorder>() {
        recorder.set_texts(recording_texts(&l10n));
    }
    match app.try_state::<AppState>() {
        Some(state) => {
            *state.locale.lock().unwrap_or_else(PoisonError::into_inner) = locale.clone();
        }
        None => {
            app.manage(AppState {
                locale: Mutex::new(locale.clone()),
            });
        }
    }
    Ok(locale)
}

/// Opens the library and starts the job runner with the handlers available so far.
fn start_core<R: Runtime>(
    app: &AppHandle<R>,
    library_dir: &Path,
) -> Result<jobs::Core, Box<dyn std::error::Error>> {
    let library = Library::open(library_dir)?;
    // The meeting page plays the tracks through the asset protocol, from this library only.
    app.asset_protocol_scope()
        .allow_directory(library.root().join(trama_core::storage::AUDIO_DIR), true)?;
    let recovery = library.recovery();
    if !recovery.is_empty() {
        eprintln!("trama: recovered after an unclean stop: {recovery:?}");
    }
    let library = Arc::new(Mutex::new(library));
    // The transcription model is loaded from the folder the model manager downloads into.
    let transcribe = Transcribe::new(SherpaLoader::new(models::models_dir(app)?));
    let handlers: Vec<Arc<dyn JobHandler>> = vec![
        Arc::new(DeleteAudio),
        Arc::new(transcribe),
        Arc::new(Summarize::new(Arc::new(Keychain), ClientConfig::default())),
    ];
    let runner = Runner::start(
        Arc::clone(&library),
        handlers,
        jobs::event_sink(app.clone()),
    );
    Ok(jobs::Core { library, runner })
}

/// The interface locale negotiated by the core, so the shell and the frontend agree.
#[tauri::command]
fn app_locale(state: State<'_, AppState>) -> String {
    state
        .locale
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// Texts of the recording panel and its notifications.
fn recording_texts(l10n: &Localizer) -> recording::Texts {
    let t = |id: &str| l10n.message(id, None);
    recording::Texts {
        panel_title: t("recording-panel-title"),
        ready_title: t("meeting-ready-title"),
        ready_body: t("meeting-ready-body"),
        failed_title: t("meeting-failed-title"),
        failed_body: t("meeting-failed-body"),
    }
}

/// Starts the desktop app.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(quit::Quit::default())
        .manage(recording::Recorder::default())
        .manage(menu::ShellMenu::<tauri::Wry>::default())
        .invoke_handler(tauri::generate_handler![
            app_locale,
            jobs::active_jobs,
            jobs::retry_job,
            jobs::cancel_job,
            models::model_status,
            models::download_model,
            models::cancel_model_download,
            models::verify_model,
            models::remove_model,
            settings::app_settings,
            settings::set_interface_language,
            settings::set_appearance,
            settings::set_reduce_transparency,
            settings::set_reduce_motion,
            settings::set_microphone,
            settings::set_global_shortcut,
            settings::input_devices,
            settings::choose_library_folder,
            settings::set_library_location,
            settings::reveal_library,
            settings::library_settings,
            settings::set_library_settings,
            settings::audio_retention_impact,
            settings::held_audio,
            settings::apply_retention_to_held_audio,
            settings::budget_status,
            providers::list_providers,
            providers::add_provider,
            providers::update_provider,
            providers::remove_provider,
            providers::test_provider,
            providers::set_model_settings,
            providers::task_models,
            providers::set_task_model,
            settings::startup_notices,
            summary::source_summary,
            summary::regenerate_summary,
            summary::set_source_title,
            summary::edit_summary,
            meetings::retranscribe,
            notes::create_note,
            notes::save_note,
            notes::get_note,
            notes::set_note_title,
            notes::open_link,
            notes::list_sources,
            meetings::get_meeting,
            recording::start_recording,
            recording::stop_recording,
            recording::recording_status,
            recording::show_recording_panel,
            recording::set_recording_panel_compact,
            recording::add_recording_note,
            recording::update_recording_note,
            recording::delete_recording_note,
            recording::recording_notes,
            menu::set_menu_state,
            quit::quit_ready,
            quit::quit_cancelled
        ])
        .on_menu_event(|app, event| {
            if event.id() == quit::QUIT_MENU_ID {
                quit::request(app);
            } else {
                menu::handle_event(app, event.id().as_ref());
            }
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.state::<quit::Quit>().may_close() {
                    return;
                }
                api.prevent_close();
                if window.label() == recording::RECORDING_WINDOW {
                    // Closing the panel only hides it: the recording goes on, and the
                    // main window can show the panel again.
                    if let Err(err) = window.hide() {
                        eprintln!("trama: cannot hide the recording panel: {err}");
                    }
                } else {
                    // Closing the main window quits the app: let the editor save first.
                    quit::request(window.app_handle());
                }
            }
        })
        .setup(move |app| {
            let mut settings = load_settings(app.handle())?;
            // The vibrancy follows the app's appearance, so it must match the chosen theme.
            let theme = settings::native_theme(&settings.config().theme);
            app.set_theme(theme);
            dock_icon::show(app.handle(), &settings.config().accent);
            // A library chosen with TRAMA_LIBRARY is never swapped for the real one.
            let opening = open_with_fallback(
                &settings.open_library,
                &settings.default_library,
                !settings.library_overridden,
                |dir| start_core(app.handle(), dir),
            );
            settings.open_library = opening.dir;
            settings.notices.extend(opening.notice);
            app.manage(settings);
            apply_locale(app.handle())?;
            if let Some(core) = opening.core {
                app.manage(core);
            }
            app.manage(models::start(app.handle())?);
            if let Some(window) = app.get_webview_window("main") {
                window.set_title(&app.package_info().name)?;
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building the Trama desktop app")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                // Before the runner stops, so the transcription is queued for next time.
                recording::stop_on_exit(app);
                if let Some(core) = app.try_state::<jobs::Core>() {
                    // Unfinished jobs stay queued and resume at the next start.
                    core.runner.stop();
                }
                if let Some(models) = app.try_state::<models::Models>() {
                    // A partial download stays on disk and resumes when asked again.
                    models.transcription.shutdown();
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Message ids of the standard items of the menu (see `menu::build`).
    const MENU_IDS: &[&str] = &[
        "menu-about",
        "menu-services",
        "menu-hide",
        "menu-hide-others",
        "menu-show-all",
        "menu-quit",
        "menu-file",
        "menu-edit",
        "menu-view",
        "menu-go",
        "menu-undo",
        "menu-redo",
        "menu-cut",
        "menu-copy",
        "menu-paste",
        "menu-select-all",
        "menu-window",
        "menu-minimize",
        "menu-zoom",
        "menu-fullscreen",
        "menu-close-window",
    ];

    #[test]
    fn every_menu_label_is_translated() {
        for locale in ["en", "it"] {
            let l10n = Localizer::new(&[locale], CATALOGS).unwrap();
            assert_eq!(l10n.locale().to_string(), locale);
            for id in MENU_IDS {
                let label = l10n.message(id, None);
                assert_ne!(label, *id, "{id} is missing in {locale}");
            }
        }
    }

    /// Info.plist strings shown to the user, translated in `macos/<lang>.lproj`.
    const USAGE_KEYS: &[&str] = &[
        "NSMicrophoneUsageDescription",
        "NSAudioCaptureUsageDescription",
    ];

    #[test]
    fn permission_prompts_are_translated() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let info_plist = std::fs::read_to_string(dir.join("Info.plist")).unwrap();
        for key in USAGE_KEYS {
            assert!(
                info_plist.contains(&format!("<key>{key}</key>")),
                "{key} missing in Info.plist"
            );
        }
        for locale in trama_core::i18n::SUPPORTED_LOCALES {
            let path = dir.join(format!("macos/{locale}.lproj/InfoPlist.strings"));
            let strings = std::fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("missing {}", path.display()));
            for key in USAGE_KEYS {
                assert!(
                    strings.contains(&format!("\"{key}\" = \"")),
                    "{key} missing in {locale}"
                );
            }
        }
    }

    /// Message ids used by [`recording_texts`].
    const SHELL_IDS: &[&str] = &[
        "recording-panel-title",
        "meeting-ready-title",
        "meeting-ready-body",
        "meeting-failed-title",
        "meeting-failed-body",
    ];

    #[test]
    fn every_shell_text_is_translated() {
        for locale in ["en", "it"] {
            let l10n = Localizer::new(&[locale], CATALOGS).unwrap();
            for id in SHELL_IDS {
                let text = l10n.message(id, None);
                assert_ne!(text, *id, "{id} is missing in {locale}");
            }
        }
        let l10n = Localizer::new(&["it"], CATALOGS).unwrap();
        assert_eq!(
            l10n.message("recording-panel-title", None),
            "Trama — Registrazione"
        );
    }

    #[test]
    fn a_library_that_cannot_be_opened_falls_back_to_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let default = dir.path().join("Library");
        // A file where the chosen library should be: like a disk that is gone.
        let broken = dir.path().join("Gone");
        std::fs::write(&broken, b"not a folder").unwrap();

        let opening = open_with_fallback(&broken, &default, true, |d: &Path| Library::open(d));
        assert!(opening.core.is_some());
        assert_eq!(opening.dir, default);
        let notice = opening.notice.unwrap();
        assert_eq!(notice.code, config_notice::LIBRARY_UNAVAILABLE);
        assert_eq!(notice.path, Some(settings::path_text(&broken)));

        // A library that opens gives no notice.
        drop(opening.core);
        let opening = open_with_fallback(&default, &default, true, |d: &Path| Library::open(d));
        assert!(opening.core.is_some() && opening.notice.is_none());
    }

    #[test]
    fn without_any_library_the_app_still_starts() {
        let dir = tempfile::tempdir().unwrap();
        let broken = dir.path().join("Gone");
        std::fs::write(&broken, b"not a folder").unwrap();
        let default = dir.path().join("AlsoGone");
        std::fs::write(&default, b"not a folder").unwrap();

        let opening = open_with_fallback(&broken, &default, true, |d: &Path| Library::open(d));
        assert!(opening.core.is_none());
        assert_eq!(opening.notice.unwrap().code, config_notice::LIBRARY_FAILED);

        // TRAMA_LIBRARY in development: no silent switch to the real library.
        let ok_default = dir.path().join("Library");
        let opening = open_with_fallback(&broken, &ok_default, false, |d: &Path| Library::open(d));
        assert!(opening.core.is_none());
        assert_eq!(opening.dir, broken);
        assert!(!ok_default.exists());
    }

    #[test]
    fn labels_use_the_brand_name() {
        let l10n = Localizer::new(&["it"], CATALOGS).unwrap();
        assert_eq!(l10n.message("menu-quit", None), "Esci da Trama");
    }
}
