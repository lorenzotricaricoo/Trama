//! Trama desktop app: a thin Tauri shell over `trama-core`.
//!
//! The shell owns windows, menus and the bridge to the frontend. Everything else
//! belongs to the core library (principle A1).

use tauri::menu::{AboutMetadata, Menu, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Manager, Runtime, State};
use trama_core::i18n::{Catalog, Localizer, system_locales};

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

/// The interface locale negotiated by the core, so the shell and the frontend agree.
#[tauri::command]
fn app_locale(state: State<'_, AppState>) -> String {
    state.locale.clone()
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
    let l10n = Localizer::new(&requested_locales(), CATALOGS)
        .expect("the embedded menu catalogs are valid (checked by tests)");
    let locale = l10n.locale().to_string();

    tauri::Builder::default()
        .manage(AppState { locale })
        .invoke_handler(tauri::generate_handler![app_locale])
        .setup(move |app| {
            let menu = app_menu(app.handle(), &l10n)?;
            app.set_menu(menu)?;
            if let Some(window) = app.get_webview_window("main") {
                window.set_title(&app.package_info().name)?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the Trama desktop app");
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
