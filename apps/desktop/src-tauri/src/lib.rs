//! Trama desktop app: a thin Tauri shell over `trama-core`.
//!
//! The shell owns windows, menus and the bridge to the frontend. Everything else
//! belongs to the core library (principle A1).

mod jobs;
mod models;
mod providers;
mod settings;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use tauri::menu::{AboutMetadata, Menu, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Manager, RunEvent, Runtime, State};
use trama_core::config::{AppConfig, CONFIG_FILE};
use trama_core::i18n::{Catalog, Localizer, system_locales};
use trama_core::jobs::{DeleteAudio, JobHandler, Runner};
use trama_core::storage::Library;

/// Catalogs used by the shell itself (native menus). The frontend loads its own.
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

/// Reads the app configuration from the app data folder.
fn load_settings<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<settings::Settings> {
    let data_dir = app.path().app_data_dir()?;
    let path = data_dir.join(CONFIG_FILE);
    let config = AppConfig::load(&path).unwrap_or_else(|err| {
        eprintln!(
            "trama: cannot read {}: {err}; using the defaults",
            path.display()
        );
        AppConfig::default()
    });
    let default_library = data_dir.join(LIBRARY_DIR);
    let (open_library, library_overridden) = library_dir(&config, &default_library);
    Ok(settings::Settings {
        path,
        config: Mutex::new(config),
        open_library,
        default_library,
        library_overridden,
        home: app.path().home_dir().ok(),
    })
}

/// Negotiates the interface locale from the configuration and translates the native
/// menus with it. Returns the locale.
fn apply_locale<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<String> {
    let config = app.state::<settings::Settings>().config().clone();
    let l10n = Localizer::new(&requested_locales(&config), CATALOGS)
        .expect("the embedded menu catalogs are valid (checked by tests)");
    let locale = l10n.locale().to_string();
    app.set_menu(app_menu(app, &l10n)?)?;
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
    let recovery = library.recovery();
    if !recovery.is_empty() {
        eprintln!("trama: recovered after an unclean stop: {recovery:?}");
    }
    let library = Arc::new(Mutex::new(library));
    let handlers: Vec<Arc<dyn JobHandler>> = vec![Arc::new(DeleteAudio)];
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
            &PredefinedMenuItem::quit(app, t("menu-quit").as_deref())?,
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
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
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
            settings::set_microphone,
            settings::set_global_shortcut,
            settings::input_devices,
            settings::choose_library_folder,
            settings::set_library_location,
            settings::reveal_library,
            settings::library_settings,
            settings::set_library_settings,
            settings::budget_status,
            providers::list_providers,
            providers::add_provider,
            providers::update_provider,
            providers::remove_provider,
            providers::test_provider,
            providers::set_model_settings,
            providers::task_models,
            providers::set_task_model
        ])
        .setup(move |app| {
            let settings = load_settings(app.handle())?;
            let library_dir = settings.open_library.clone();
            app.manage(settings);
            apply_locale(app.handle())?;
            app.manage(start_core(app.handle(), &library_dir)?);
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

    #[test]
    fn labels_use_the_brand_name() {
        let l10n = Localizer::new(&["it"], CATALOGS).unwrap();
        assert_eq!(l10n.message("menu-quit", None), "Esci da Trama");
    }
}
