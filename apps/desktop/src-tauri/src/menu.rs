//! The native menu bar: the standard app, Edit and Window menus, plus the shell commands
//! (File › New Note, Record Meeting; View › Toggle Sidebar; Go › Home, Sources, Back,
//! Command Bar; Trama › Settings).
//!
//! The menu does not run the shell commands itself: choosing one sends
//! [`MENU_COMMAND_EVENT`] to the main window, whose action dispatcher runs the same action
//! as the shortcut (see `src/shell/actions.ts`). The frontend reports back what can run and
//! what the labels depend on with [`set_menu_state`], and the items follow.

use std::sync::{Mutex, PoisonError};

use serde::Deserialize;
use tauri::menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use trama_core::i18n::Localizer;

use crate::quit;

/// Event sent to the main window when a shell command is chosen in the menu. The payload is
/// the command id.
pub const MENU_COMMAND_EVENT: &str = "menu-command";

/// The window whose dispatcher runs the shell commands.
const MAIN_WINDOW: &str = "main";

/// A shell command listed in the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellCommand {
    NewNote,
    Record,
    ToggleSidebar,
    Home,
    Sources,
    Back,
    CommandBar,
    Settings,
}

impl ShellCommand {
    pub const ALL: [ShellCommand; 8] = [
        ShellCommand::NewNote,
        ShellCommand::Record,
        ShellCommand::ToggleSidebar,
        ShellCommand::Home,
        ShellCommand::Sources,
        ShellCommand::Back,
        ShellCommand::CommandBar,
        ShellCommand::Settings,
    ];

    /// Id of the menu item, the same as the frontend's action id.
    pub fn id(self) -> &'static str {
        match self {
            ShellCommand::NewNote => "newNote",
            ShellCommand::Record => "record",
            ShellCommand::ToggleSidebar => "toggleSidebar",
            ShellCommand::Home => "home",
            ShellCommand::Sources => "sources",
            ShellCommand::Back => "back",
            ShellCommand::CommandBar => "commandBar",
            ShellCommand::Settings => "settings",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|command| command.id() == id)
    }

    /// The shortcut, the same as the webview's (`src/shell/shortcuts.ts`).
    fn accelerator(self) -> Option<&'static str> {
        match self {
            ShellCommand::NewNote => Some("CmdOrCtrl+N"),
            ShellCommand::Record => Some("Alt+CmdOrCtrl+R"),
            ShellCommand::ToggleSidebar => Some("CmdOrCtrl+\\"),
            ShellCommand::Back => Some("CmdOrCtrl+["),
            ShellCommand::CommandBar => Some("CmdOrCtrl+K"),
            ShellCommand::Settings => Some("CmdOrCtrl+,"),
            ShellCommand::Home | ShellCommand::Sources => None,
        }
    }

    /// Message id of the label, which may depend on the state.
    fn label(self, state: &MenuState) -> &'static str {
        match self {
            ShellCommand::NewNote => "menu-new-note",
            ShellCommand::Record if state.recording => "menu-stop-recording",
            ShellCommand::Record => "menu-record-meeting",
            ShellCommand::ToggleSidebar if state.sidebar_collapsed => "menu-show-sidebar",
            ShellCommand::ToggleSidebar => "menu-hide-sidebar",
            ShellCommand::Home => "menu-home",
            ShellCommand::Sources => "menu-sources",
            ShellCommand::Back => "menu-back",
            ShellCommand::CommandBar => "menu-command-bar",
            ShellCommand::Settings => "menu-settings",
        }
    }
}

/// What the shell commands depend on, as the frontend reports it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuState {
    /// Ids of the commands that can run now; unknown ids are ignored.
    pub enabled: Vec<String>,
    pub recording: bool,
    pub sidebar_collapsed: bool,
}

impl Default for MenuState {
    /// Before the frontend reports: everything can run except Back, with no history yet.
    fn default() -> Self {
        Self {
            enabled: ShellCommand::ALL
                .into_iter()
                .filter(|command| *command != ShellCommand::Back)
                .map(|command| command.id().to_owned())
                .collect(),
            recording: false,
            sidebar_collapsed: false,
        }
    }
}

impl MenuState {
    fn is_enabled(&self, command: ShellCommand) -> bool {
        self.enabled.iter().any(|id| id == command.id())
    }
}

/// How a command's item looks in a given state.
#[derive(Debug, PartialEq, Eq)]
struct ItemLook {
    label: &'static str,
    enabled: bool,
}

fn look(command: ShellCommand, state: &MenuState) -> ItemLook {
    ItemLook {
        label: command.label(state),
        enabled: state.is_enabled(command),
    }
}

/// Every label a command can show, in any state.
fn label_ids() -> Vec<&'static str> {
    let mut ids = Vec::new();
    for recording in [false, true] {
        for sidebar_collapsed in [false, true] {
            let state = MenuState {
                enabled: Vec::new(),
                recording,
                sidebar_collapsed,
            };
            ids.extend(ShellCommand::ALL.into_iter().map(|c| c.label(&state)));
        }
    }
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// The menu items of the shell commands, kept to update them in place, with their labels
/// in the menu's language.
struct Items<R: Runtime> {
    labels: Vec<(&'static str, String)>,
    items: Vec<(ShellCommand, MenuItem<R>)>,
}

impl<R: Runtime> Items<R> {
    fn label(&self, id: &str) -> String {
        self.labels
            .iter()
            .find(|(label, _)| *label == id)
            .map_or_else(|| id.to_owned(), |(_, text)| text.clone())
    }
}

/// The state of the shell commands and their items in the current menu.
pub struct ShellMenu<R: Runtime> {
    state: Mutex<MenuState>,
    items: Mutex<Option<Items<R>>>,
}

impl<R: Runtime> Default for ShellMenu<R> {
    fn default() -> Self {
        Self {
            state: Mutex::new(MenuState::default()),
            items: Mutex::new(None),
        }
    }
}

/// Builds the menu bar in the language of `l10n` and sets it, keeping the current state of
/// the shell commands. Called at start and when the interface language changes.
pub fn install<R: Runtime>(app: &AppHandle<R>, l10n: &Localizer) -> tauri::Result<()> {
    let shell = app.state::<ShellMenu<R>>();
    let state = shell
        .state
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let mut items = Vec::new();
    for command in ShellCommand::ALL {
        let look = look(command, &state);
        let item = MenuItem::with_id(
            app,
            command.id(),
            l10n.message(look.label, None),
            look.enabled,
            command.accelerator(),
        )?;
        items.push((command, item));
    }
    let menu = build(app, l10n, &items)?;
    app.set_menu(menu)?;
    let labels = label_ids()
        .into_iter()
        .map(|id| (id, l10n.message(id, None)))
        .collect();
    *shell.items.lock().unwrap_or_else(PoisonError::into_inner) = Some(Items { labels, items });
    Ok(())
}

/// The whole menu bar, with every label taken from the catalogs.
fn build<R: Runtime>(
    app: &AppHandle<R>,
    l10n: &Localizer,
    items: &[(ShellCommand, MenuItem<R>)],
) -> tauri::Result<Menu<R>> {
    let t = |id: &str| Some(l10n.message(id, None));
    let separator = || PredefinedMenuItem::separator(app);
    let item = |command: ShellCommand| {
        &items
            .iter()
            .find(|(c, _)| *c == command)
            .expect("every command has an item")
            .1
    };

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
            item(ShellCommand::Settings),
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
    let file_submenu = Submenu::with_items(
        app,
        l10n.message("menu-file", None),
        true,
        &[item(ShellCommand::NewNote), item(ShellCommand::Record)],
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
    let view_submenu = Submenu::with_items(
        app,
        l10n.message("menu-view", None),
        true,
        &[
            item(ShellCommand::ToggleSidebar),
            &separator()?,
            &PredefinedMenuItem::fullscreen(app, t("menu-fullscreen").as_deref())?,
        ],
    )?;
    let go_submenu = Submenu::with_items(
        app,
        l10n.message("menu-go", None),
        true,
        &[
            item(ShellCommand::Home),
            item(ShellCommand::Sources),
            &separator()?,
            item(ShellCommand::Back),
            &separator()?,
            item(ShellCommand::CommandBar),
        ],
    )?;
    let window_submenu = Submenu::with_items(
        app,
        l10n.message("menu-window", None),
        true,
        &[
            &PredefinedMenuItem::minimize(app, t("menu-minimize").as_deref())?,
            &PredefinedMenuItem::maximize(app, t("menu-zoom").as_deref())?,
            &separator()?,
            &PredefinedMenuItem::close_window(app, t("menu-close-window").as_deref())?,
        ],
    )?;
    Menu::with_items(
        app,
        &[
            &app_submenu,
            &file_submenu,
            &edit_submenu,
            &view_submenu,
            &go_submenu,
            &window_submenu,
        ],
    )
}

/// Sends a shell command chosen in the menu to the main window; other menu events are
/// ignored.
pub fn handle_event<R: Runtime>(app: &AppHandle<R>, id: &str) {
    let Some(command) = ShellCommand::from_id(id) else {
        return;
    };
    if let Err(err) = app.emit_to(MAIN_WINDOW, MENU_COMMAND_EVENT, command.id()) {
        eprintln!("trama: cannot send the menu command {id}: {err}");
    }
}

/// Updates the shell commands of the menu: which can run, and the labels that depend on
/// the state (Record Meeting or Stop Recording, Show or Hide Sidebar).
#[tauri::command]
pub fn set_menu_state<R: Runtime>(app: AppHandle<R>, state: MenuState) -> Result<(), String> {
    let shell = app.state::<ShellMenu<R>>();
    let previous = std::mem::replace(
        &mut *shell.state.lock().unwrap_or_else(PoisonError::into_inner),
        state.clone(),
    );
    let guard = shell.items.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(items) = guard.as_ref() else {
        return Ok(());
    };
    for (command, item) in &items.items {
        let (before, now) = (look(*command, &previous), look(*command, &state));
        let result = if before.label == now.label {
            Ok(())
        } else {
            item.set_text(items.label(now.label))
        }
        .and_then(|()| {
            if before.enabled == now.enabled {
                Ok(())
            } else {
                item.set_enabled(now.enabled)
            }
        });
        if let Err(err) = result {
            eprintln!("trama: cannot update the menu item {}: {err}", command.id());
            return Err(crate::settings::INTERNAL_ERROR.to_owned());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(enabled: &[&str], recording: bool, sidebar_collapsed: bool) -> MenuState {
        MenuState {
            enabled: enabled.iter().map(|id| (*id).to_owned()).collect(),
            recording,
            sidebar_collapsed,
        }
    }

    #[test]
    fn ids_match_the_frontend_actions() {
        for command in ShellCommand::ALL {
            assert_eq!(ShellCommand::from_id(command.id()), Some(command));
        }
        assert_eq!(ShellCommand::from_id(quit::QUIT_MENU_ID), None);
        assert_eq!(ShellCommand::from_id("unknown"), None);
        // The ids are the action ids in src/shell/actions.ts.
        let actions = include_str!("../../src/shell/actions.ts");
        for command in ShellCommand::ALL {
            assert!(
                actions.contains(&format!("'{}'", command.id())),
                "{} is not an action of the frontend",
                command.id()
            );
        }
    }

    #[test]
    fn shortcuts_match_the_issue() {
        let shortcut = |command| ShellCommand::accelerator(command);
        assert_eq!(shortcut(ShellCommand::ToggleSidebar), Some("CmdOrCtrl+\\"));
        assert_eq!(shortcut(ShellCommand::Back), Some("CmdOrCtrl+["));
        assert_eq!(shortcut(ShellCommand::Settings), Some("CmdOrCtrl+,"));
        assert_eq!(shortcut(ShellCommand::CommandBar), Some("CmdOrCtrl+K"));
        assert_eq!(shortcut(ShellCommand::Record), Some("Alt+CmdOrCtrl+R"));
        assert_eq!(shortcut(ShellCommand::NewNote), Some("CmdOrCtrl+N"));
        assert_eq!(shortcut(ShellCommand::Home), None);
        assert_eq!(shortcut(ShellCommand::Sources), None);
    }

    #[test]
    fn back_is_disabled_until_there_is_history() {
        let start = MenuState::default();
        assert!(!look(ShellCommand::Back, &start).enabled);
        for command in ShellCommand::ALL
            .into_iter()
            .filter(|c| *c != ShellCommand::Back)
        {
            assert!(look(command, &start).enabled, "{command:?}");
        }
        let with_history = state(&["back", "home"], false, false);
        assert!(look(ShellCommand::Back, &with_history).enabled);
        assert!(!look(ShellCommand::Record, &with_history).enabled);
    }

    #[test]
    fn labels_follow_the_state() {
        let idle = state(&[], false, false);
        let busy = state(&["record", "bogus"], true, true);
        assert_eq!(
            look(ShellCommand::Record, &idle).label,
            "menu-record-meeting"
        );
        assert_eq!(
            look(ShellCommand::Record, &busy).label,
            "menu-stop-recording"
        );
        assert_eq!(
            look(ShellCommand::ToggleSidebar, &idle).label,
            "menu-hide-sidebar"
        );
        assert_eq!(
            look(ShellCommand::ToggleSidebar, &busy).label,
            "menu-show-sidebar"
        );
        assert!(look(ShellCommand::Record, &busy).enabled);
    }

    #[test]
    fn the_state_comes_from_the_frontend_in_camel_case() {
        let parsed: MenuState = serde_json::from_value(serde_json::json!({
            "enabled": ["back"],
            "recording": true,
            "sidebarCollapsed": true,
        }))
        .unwrap();
        assert_eq!(parsed, state(&["back"], true, true));
    }

    #[test]
    fn every_command_label_is_translated() {
        for locale in ["en", "it"] {
            let l10n = Localizer::new(&[locale], crate::CATALOGS).unwrap();
            for id in label_ids() {
                assert_ne!(l10n.message(id, None), id, "{id} is missing in {locale}");
            }
        }
        let l10n = Localizer::new(&["it"], crate::CATALOGS).unwrap();
        assert_eq!(l10n.message("menu-settings", None), "Impostazioni…");
    }
}
