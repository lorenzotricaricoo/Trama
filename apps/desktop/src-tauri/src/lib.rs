//! Trama desktop app: a thin Tauri shell over `trama-core`.
//!
//! The shell owns windows, menus and the bridge to the frontend. Everything else
//! belongs to the core library (principle A1).

mod jobs;
mod meetings;
mod models;
mod notes;
mod quit;
mod recording;
mod summary;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tauri::menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Manager, RunEvent, Runtime, State, WindowEvent};
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
    locale: String,
}

/// Locales to negotiate: the override if set, otherwise the system preferences.
fn requested_locales() -> Vec<String> {
    match std::env::var(LOCALE_OVERRIDE_ENV) {
        Ok(locale) if !locale.trim().is_empty() => vec![locale],
        _ => system_locales(),
    }
}

/// The library folder: the override if set, otherwise inside the app data folder
/// (`~/Library/Application Support/<identifier>/Library` on macOS).
fn library_dir<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<PathBuf> {
    match std::env::var_os(LIBRARY_OVERRIDE_ENV) {
        Some(dir) if !dir.is_empty() => Ok(PathBuf::from(dir)),
        _ => Ok(app.path().app_data_dir()?.join(LIBRARY_DIR)),
    }
}

/// Opens the library and starts the job runner with the handlers available so far.
fn start_core<R: Runtime>(app: &AppHandle<R>) -> Result<jobs::Core, Box<dyn std::error::Error>> {
    let library = Library::open(library_dir(app)?)?;
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
    state.locale.clone()
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

/// The native menu, with every label taken from the catalogs.
fn app_menu<R: Runtime>(app: &AppHandle<R>, l10n: &Localizer) -> tauri::Result<Menu<R>> {
    let t = |id: &str| Some(l10n.message(id, None));
    let separator = || PredefinedMenuItem::separator(app);

    let about = AboutMetadata {
        version: Some(app.package_info().version.to_string()),
        ..Default::default()
    };
    let app_submenu = Submenu::with_items(
        app,
        app.package_info().name.clone(),
        true,
        &[
            &PredefinedMenuItem::about(app, t("menu-about").as_deref(), Some(about))?,
            &separator()?,
            &PredefinedMenuItem::services(app, t("menu-services").as_deref())?,
            &separator()?,
            &PredefinedMenuItem::hide(app, t("menu-hide").as_deref())?,
            &PredefinedMenuItem::hide_others(app, t("menu-hide-others").as_deref())?,
            &PredefinedMenuItem::show_all(app, t("menu-show-all").as_deref())?,
            &separator()?,
            // Not the predefined item: quitting first lets the editor save (see `quit`).
            &MenuItem::with_id(
                app,
                quit::QUIT_MENU_ID,
                l10n.message("menu-quit", None),
                true,
                Some("CmdOrCtrl+Q"),
            )?,
        ],
    )?;
    let edit_submenu = Submenu::with_items(
        app,
        l10n.message("menu-edit", None),
        true,
        &[
            &PredefinedMenuItem::undo(app, t("menu-undo").as_deref())?,
            &PredefinedMenuItem::redo(app, t("menu-redo").as_deref())?,
            &separator()?,
            &PredefinedMenuItem::cut(app, t("menu-cut").as_deref())?,
            &PredefinedMenuItem::copy(app, t("menu-copy").as_deref())?,
            &PredefinedMenuItem::paste(app, t("menu-paste").as_deref())?,
            &PredefinedMenuItem::select_all(app, t("menu-select-all").as_deref())?,
        ],
    )?;
    let window_submenu = Submenu::with_items(
        app,
        l10n.message("menu-window", None),
        true,
        &[
            &PredefinedMenuItem::minimize(app, t("menu-minimize").as_deref())?,
            &PredefinedMenuItem::maximize(app, t("menu-zoom").as_deref())?,
            &PredefinedMenuItem::fullscreen(app, t("menu-fullscreen").as_deref())?,
            &separator()?,
            &PredefinedMenuItem::close_window(app, t("menu-close-window").as_deref())?,
        ],
    )?;
    Menu::with_items(app, &[&app_submenu, &edit_submenu, &window_submenu])
}

/// Starts the desktop app.
pub fn run() {
    let l10n = Localizer::new(&requested_locales(), CATALOGS)
        .expect("the embedded menu catalogs are valid (checked by tests)");
    let locale = l10n.locale().to_string();

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .manage(AppState { locale })
        .manage(quit::Quit::default())
        .manage(recording::Recorder::new(recording_texts(&l10n)))
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
            summary::source_summary,
            summary::regenerate_summary,
            summary::set_source_title,
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
            quit::quit_ready,
            quit::quit_cancelled
        ])
        .on_menu_event(|app, event| {
            if event.id() == quit::QUIT_MENU_ID {
                quit::request(app);
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
            app.manage(start_core(app.handle())?);
            app.manage(models::start(app.handle())?);
            let menu = app_menu(app.handle(), &l10n)?;
            app.set_menu(menu)?;
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

    /// Message ids used by [`app_menu`].
    const MENU_IDS: &[&str] = &[
        "menu-about",
        "menu-services",
        "menu-hide",
        "menu-hide-others",
        "menu-show-all",
        "menu-quit",
        "menu-edit",
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
    fn labels_use_the_brand_name() {
        let l10n = Localizer::new(&["it"], CATALOGS).unwrap();
        assert_eq!(l10n.message("menu-quit", None), "Esci da Trama");
    }
}
