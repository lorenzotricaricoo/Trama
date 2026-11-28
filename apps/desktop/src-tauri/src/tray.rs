//! The menu bar item: record or stop a meeting, a new note, what is being processed and
//! the latest meetings, plus Open, Settings and Quit. It works with the main window closed.
//!
//! The icon is a template image (macOS tints it for the menu bar); it turns into the red
//! record dot only while recording, since red is reserved for recording. While a meeting
//! is transcribed its progress shows next to the icon.
//!
//! Updates go through one worker thread: job events can arrive while the job runner holds
//! the library, and menu changes wait for the main thread, so the event sink never waits.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Mutex, PoisonError};

use fluent_bundle::FluentArgs;
use tauri::image::Image;
use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use trama_core::i18n::Localizer;
use trama_core::jobs::lock;
use trama_core::storage::{Id, JobKind, SourceKind};

use crate::activity::{Activity, Processing, percent};
use crate::background;
use crate::menu::{MENU_COMMAND_EVENT, ShellCommand};

/// Id of the menu bar item.
pub const TRAY_ID: &str = "trama";

/// Sent to the main window to open a meeting chosen in the menu bar item. The payload is
/// the meeting id.
pub const OPEN_MEETING_EVENT: &str = "open-meeting";

/// How many of the latest meetings the menu lists.
const LATEST_MEETINGS: u32 = 5;

/// The template icon, 18 pt at 2x.
const ICON: &[u8] = include_bytes!("../icons/tray/tray.png");

/// The icon while recording: the red record dot.
const RECORDING_ICON: &[u8] = include_bytes!("../icons/tray/tray-recording.png");

/// Prefix of the ids of the menu bar item's entries, apart from the app menu's.
const ID_PREFIX: &str = "tray:";

/// An entry of the menu that runs something.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Record,
    NewNote,
    Open,
    Settings,
    Quit,
    Meeting(Id),
}

impl Command {
    fn id(self) -> String {
        let name = match self {
            Command::Record => "record".to_owned(),
            Command::NewNote => "new-note".to_owned(),
            Command::Open => "open".to_owned(),
            Command::Settings => "settings".to_owned(),
            Command::Quit => "quit".to_owned(),
            Command::Meeting(id) => format!("meeting:{id}"),
        };
        format!("{ID_PREFIX}{name}")
    }

    fn parse(id: &str) -> Option<Self> {
        match id.strip_prefix(ID_PREFIX)? {
            "record" => Some(Command::Record),
            "new-note" => Some(Command::NewNote),
            "open" => Some(Command::Open),
            "settings" => Some(Command::Settings),
            "quit" => Some(Command::Quit),
            other => Id::parse_str(other.strip_prefix("meeting:")?)
                .ok()
                .map(Command::Meeting),
        }
    }
}

/// A meeting being processed, as the menu lists it.
#[derive(Debug, Clone, PartialEq)]
struct Row {
    job_id: Id,
    meeting_id: Id,
    /// Empty while the meeting has no title yet.
    title: String,
    job: Processing,
}

/// Everything the menu shows; rebuilt only when it changes (apart from progress).
#[derive(Debug, Clone, PartialEq)]
struct Model {
    recording: bool,
    shortcut: Option<String>,
    processing: Vec<Row>,
    /// Latest meetings, newest first, with their titles (empty while untitled).
    meetings: Vec<(Id, String)>,
    locale: String,
}

impl Model {
    /// The model without progress, to tell a rebuild from a progress update.
    fn structure(&self) -> Model {
        let mut model = self.clone();
        for row in &mut model.processing {
            row.job.progress = None;
        }
        model
    }
}

fn title_or_untitled(l10n: &Localizer, title: &str) -> String {
    if title.is_empty() {
        l10n.message("tray-meeting-untitled", None)
    } else {
        title.to_owned()
    }
}

/// What a meeting being processed is doing, for example "Transcribing 62%".
fn status_text(l10n: &Localizer, job: &Processing) -> String {
    if job.retrying {
        return l10n.message("tray-retrying", None);
    }
    if !job.running {
        return l10n.message("tray-waiting", None);
    }
    match (job.kind, job.progress) {
        (JobKind::Transcribe, Some(progress)) => {
            let mut args = FluentArgs::new();
            args.set("percent", percent(progress));
            l10n.message("tray-transcribing-percent", Some(&args))
        }
        (JobKind::Transcribe, None) => l10n.message("tray-transcribing", None),
        _ => l10n.message("tray-summarizing", None),
    }
}

/// The entry of a meeting being processed: "Weekly sync — Transcribing 62%".
fn row_text(l10n: &Localizer, row: &Row) -> String {
    let mut args = FluentArgs::new();
    args.set("title", title_or_untitled(l10n, &row.title));
    args.set("status", status_text(l10n, &row.job));
    l10n.message("tray-processing-row", Some(&args))
}

/// The text next to the icon: the transcription progress, never while recording.
fn icon_title(model: &Model) -> Option<String> {
    if model.recording {
        return None;
    }
    model
        .processing
        .iter()
        .find(|row| row.job.running && row.job.kind == JobKind::Transcribe)
        .and_then(|row| row.job.progress)
        .map(|progress| format!("{}%", percent(progress)))
}

/// The tooltip of the icon.
fn tooltip(l10n: &Localizer, model: &Model) -> String {
    if model.recording {
        l10n.message("tray-tooltip-recording", None)
    } else if let Some(row) = model.processing.first() {
        row_text(l10n, row)
    } else {
        l10n.message("tray-tooltip", None)
    }
}

/// The menu in use and its entries of meetings being processed, to update their progress.
struct Built<R: Runtime> {
    model: Model,
    rows: Vec<(Id, MenuItem<R>)>,
}

/// A request to the worker.
enum Update {
    /// Read everything again and rebuild the menu if it changed.
    Refresh,
    /// Only the progress of a meeting being processed moved.
    Progress,
}

/// The channel to the worker that keeps the menu bar item up to date.
pub struct Tray {
    updates: Mutex<Sender<Update>>,
}

impl Tray {
    fn send(&self, update: Update) {
        let sent = self
            .updates
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .send(update);
        if sent.is_err() {
            eprintln!("trama: the menu bar item stopped updating");
        }
    }
}

/// Asks for the menu to follow a change (recording, jobs, language, shortcut).
pub fn refresh<R: Runtime>(app: &AppHandle<R>) {
    if let Some(tray) = app.try_state::<Tray>() {
        tray.send(Update::Refresh);
    }
}

/// Asks for the progress of the meetings being processed to be shown again.
pub fn progress<R: Runtime>(app: &AppHandle<R>) {
    if let Some(tray) = app.try_state::<Tray>() {
        tray.send(Update::Progress);
    }
}

/// Adds the menu bar item and starts its worker. Called once the library, the settings
/// and the locale are ready.
pub fn install<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let icon = TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(ICON)?)
        .icon_as_template(true)
        .show_menu_on_left_click(true)
        .build(app)?;
    let (sender, receiver) = channel();
    app.manage(Tray {
        updates: Mutex::new(sender),
    });
    let handle = app.clone();
    std::thread::Builder::new()
        .name("trama-tray".into())
        .spawn(move || work(&handle, &icon, &receiver))?;
    refresh(app);
    Ok(())
}

/// The worker: applies the updates in order, taking every waiting one at once.
fn work<R: Runtime>(app: &AppHandle<R>, icon: &TrayIcon<R>, updates: &Receiver<Update>) {
    let mut built: Option<Built<R>> = None;
    while let Ok(first) = updates.recv() {
        let mut rebuild = matches!(first, Update::Refresh);
        while let Ok(next) = updates.try_recv() {
            rebuild |= matches!(next, Update::Refresh);
        }
        let Some(l10n) = crate::localizer(app) else {
            continue;
        };
        let result = if rebuild || built.is_none() {
            update_all(app, icon, &l10n, &mut built)
        } else {
            update_progress(app, icon, &l10n, &mut built)
        };
        if let Err(err) = result {
            eprintln!("trama: cannot update the menu bar item: {err}");
        }
    }
}

/// Reads what the menu shows.
fn read_model<R: Runtime>(app: &AppHandle<R>) -> Model {
    let state = app.state::<Activity>().state().clone();
    let shortcut = app
        .try_state::<crate::shortcut::GlobalShortcut>()
        .and_then(|shortcut| shortcut.registered());
    let locale = app
        .try_state::<crate::AppState>()
        .map(|state| state.locale())
        .unwrap_or_default();
    let mut processing: Vec<Row> = state
        .jobs
        .iter()
        .map(|(job_id, job)| Row {
            job_id: *job_id,
            meeting_id: job.target_id,
            title: String::new(),
            job: *job,
        })
        .collect();
    let mut meetings = Vec::new();
    if let Some(core) = app.try_state::<crate::jobs::Core>() {
        let library = lock(&core.library);
        for row in &mut processing {
            row.title = library
                .source(row.meeting_id)
                .map(|source| source.title)
                .unwrap_or_default();
        }
        match library.list_sources(Some(SourceKind::Meeting), None, LATEST_MEETINGS) {
            Ok(sources) => {
                meetings = sources
                    .into_iter()
                    .map(|source| (source.id, source.title))
                    .collect();
            }
            Err(err) => eprintln!("trama: cannot list the latest meetings: {err}"),
        }
    }
    Model {
        recording: state.recording,
        shortcut,
        processing,
        meetings,
        locale,
    }
}

/// Rebuilds the menu if anything but progress changed, then updates the icon.
fn update_all<R: Runtime>(
    app: &AppHandle<R>,
    icon: &TrayIcon<R>,
    l10n: &Localizer,
    built: &mut Option<Built<R>>,
) -> tauri::Result<()> {
    let model = read_model(app);
    let recording_before = built.as_ref().map(|built| built.model.recording);
    let same = built
        .as_ref()
        .is_some_and(|built| built.model.structure() == model.structure());
    if same {
        return update_progress(app, icon, l10n, built);
    }
    let (menu, rows) = build_menu(app, l10n, &model)?;
    icon.set_menu(Some(menu))?;
    if recording_before != Some(model.recording) {
        let png = if model.recording {
            RECORDING_ICON
        } else {
            ICON
        };
        icon.set_icon(Some(Image::from_bytes(png)?))?;
        icon.set_icon_as_template(!model.recording)?;
    }
    icon.set_title(icon_title(&model))?;
    icon.set_tooltip(Some(tooltip(l10n, &model)))?;
    *built = Some(Built { model, rows });
    Ok(())
}

/// Updates the progress of the meetings being processed, in place.
fn update_progress<R: Runtime>(
    app: &AppHandle<R>,
    icon: &TrayIcon<R>,
    l10n: &Localizer,
    built: &mut Option<Built<R>>,
) -> tauri::Result<()> {
    let Some(built) = built.as_mut() else {
        return Ok(());
    };
    let state = app.state::<Activity>().state().clone();
    for row in &mut built.model.processing {
        let Some(job) = state.jobs.get(&row.job_id) else {
            continue;
        };
        let before = row_text(l10n, row);
        row.job = *job;
        let now = row_text(l10n, row);
        if before != now
            && let Some((_, item)) = built.rows.iter().find(|(id, _)| *id == row.job_id)
        {
            item.set_text(now)?;
        }
    }
    icon.set_title(icon_title(&built.model))?;
    icon.set_tooltip(Some(tooltip(l10n, &built.model)))
}

/// The labels of the menu, in order, as the user reads them: entries, `—` for separators
/// and `[…]` around entries that cannot be chosen. Used by the tests.
#[cfg(test)]
fn outline(l10n: &Localizer, model: &Model) -> Vec<String> {
    entries(l10n, model)
        .into_iter()
        .map(|entry| match entry {
            Entry::Separator => "—".to_owned(),
            Entry::Item {
                text,
                command: None,
                ..
            } => format!("[{text}]"),
            Entry::Item { text, .. } => text,
        })
        .collect()
}

/// An entry of the menu, before it becomes a native one.
enum Entry {
    Separator,
    Item {
        text: String,
        /// `None` for a heading or a placeholder that cannot be chosen.
        command: Option<Command>,
        accelerator: Option<String>,
        /// The job of a meeting being processed, to update its progress later.
        job: Option<Id>,
    },
}

fn item(text: String, command: Command) -> Entry {
    Entry::Item {
        text,
        command: Some(command),
        accelerator: None,
        job: None,
    }
}

fn heading(text: String) -> Entry {
    Entry::Item {
        text,
        command: None,
        accelerator: None,
        job: None,
    }
}

/// The entries of the menu for `model`.
fn entries(l10n: &Localizer, model: &Model) -> Vec<Entry> {
    let t = |id: &str| l10n.message(id, None);
    let record = if model.recording {
        "menu-stop-recording"
    } else {
        "menu-record-meeting"
    };
    let mut entries = vec![
        Entry::Item {
            text: t(record),
            command: Some(Command::Record),
            accelerator: model
                .shortcut
                .as_deref()
                .and_then(crate::shortcut::accelerator),
            job: None,
        },
        item(t("menu-new-note"), Command::NewNote),
        Entry::Separator,
    ];
    if !model.processing.is_empty() {
        entries.push(heading(t("tray-processing")));
        for row in &model.processing {
            entries.push(Entry::Item {
                text: row_text(l10n, row),
                command: Some(Command::Meeting(row.meeting_id)),
                accelerator: None,
                job: Some(row.job_id),
            });
        }
        entries.push(Entry::Separator);
    }
    entries.push(heading(t("tray-latest-meetings")));
    if model.meetings.is_empty() {
        entries.push(heading(t("tray-no-meetings")));
    }
    for (id, title) in &model.meetings {
        entries.push(item(title_or_untitled(l10n, title), Command::Meeting(*id)));
    }
    entries.extend([
        Entry::Separator,
        item(t("tray-open"), Command::Open),
        item(t("menu-settings"), Command::Settings),
        Entry::Separator,
        item(t("menu-quit"), Command::Quit),
    ]);
    entries
}

type Rows<R> = Vec<(Id, MenuItem<R>)>;

/// The native menu for `model`, with the entries of the meetings being processed.
fn build_menu<R: Runtime>(
    app: &AppHandle<R>,
    l10n: &Localizer,
    model: &Model,
) -> tauri::Result<(Menu<R>, Rows<R>)> {
    let menu = Menu::new(app)?;
    let mut rows = Vec::new();
    for (index, entry) in entries(l10n, model).into_iter().enumerate() {
        match entry {
            Entry::Separator => menu.append(&PredefinedMenuItem::separator(app)?)?,
            Entry::Item {
                text,
                command,
                accelerator,
                job,
            } => {
                // Headings get an id of their own that runs nothing.
                let id = command.map_or_else(|| format!("{ID_PREFIX}heading-{index}"), Command::id);
                let entry =
                    MenuItem::with_id(app, id, text, command.is_some(), accelerator.as_deref())?;
                menu.append(&entry as &dyn IsMenuItem<R>)?;
                if let Some(job) = job {
                    rows.push((job, entry));
                }
            }
        }
    }
    Ok((menu, rows))
}

/// Runs an entry of the menu bar item. Returns `false` for an id that is not one.
pub fn handle_event<R: Runtime>(app: &AppHandle<R>, id: &str) -> bool {
    if !id.starts_with(ID_PREFIX) {
        return false;
    }
    let Some(command) = Command::parse(id) else {
        return true;
    };
    let to_main = |event: &str, payload: String| {
        background::show_main(app);
        if let Err(err) = app.emit_to(background::MAIN_WINDOW, event, payload) {
            eprintln!("trama: cannot send {event} to the main window: {err}");
        }
    };
    match command {
        Command::Record => crate::recording::toggle(app),
        Command::NewNote => to_main(MENU_COMMAND_EVENT, ShellCommand::NewNote.id().to_owned()),
        Command::Settings => to_main(MENU_COMMAND_EVENT, ShellCommand::Settings.id().to_owned()),
        Command::Open => background::show_main(app),
        Command::Quit => crate::quit::request(app),
        Command::Meeting(meeting_id) => to_main(OPEN_MEETING_EVENT, meeting_id.to_string()),
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l10n(locale: &str) -> Localizer {
        Localizer::new(&[locale], crate::CATALOGS).unwrap()
    }

    fn processing(kind: JobKind, running: bool, progress: Option<f32>) -> Processing {
        Processing {
            kind,
            target_id: Id::now_v7(),
            running,
            retrying: false,
            progress,
        }
    }

    fn row(title: &str, job: Processing) -> Row {
        Row {
            job_id: Id::now_v7(),
            meeting_id: job.target_id,
            title: title.to_owned(),
            job,
        }
    }

    fn idle() -> Model {
        Model {
            recording: false,
            shortcut: Some("Alt+Super+KeyR".into()),
            processing: Vec::new(),
            meetings: Vec::new(),
            locale: "en".into(),
        }
    }

    #[test]
    fn ids_round_trip() {
        let meeting = Id::now_v7();
        for command in [
            Command::Record,
            Command::NewNote,
            Command::Open,
            Command::Settings,
            Command::Quit,
            Command::Meeting(meeting),
        ] {
            assert_eq!(Command::parse(&command.id()), Some(command));
        }
        assert_eq!(Command::parse("tray:heading-3"), None);
        assert_eq!(Command::parse("tray:meeting:nope"), None);
        // The app menu's ids are not the menu bar item's.
        assert_eq!(Command::parse("record"), None);
        assert_eq!(Command::parse(crate::quit::QUIT_MENU_ID), None);
    }

    #[test]
    fn an_idle_menu_records_and_lists_no_meetings() {
        let outline = outline(&l10n("en"), &idle());
        assert_eq!(
            outline,
            [
                "Record Meeting",
                "New Note",
                "—",
                "[Latest Meetings]",
                "[No meetings yet]",
                "—",
                "Open Trama",
                "Settings…",
                "—",
                "Quit Trama",
            ]
        );
    }

    #[test]
    fn the_menu_shows_the_processing_and_the_latest_meetings() {
        let mut model = idle();
        model.recording = true;
        model.processing = vec![
            row(
                "Weekly sync",
                processing(JobKind::Transcribe, true, Some(0.625)),
            ),
            row("", processing(JobKind::Summarize, false, None)),
        ];
        model.meetings = vec![
            (Id::now_v7(), "Weekly sync".into()),
            (Id::now_v7(), String::new()),
        ];
        let outline = outline(&l10n("it"), &model);
        assert_eq!(
            outline,
            [
                "Ferma la registrazione",
                "Nuova nota",
                "—",
                "[In elaborazione]",
                "Weekly sync — Trascrizione 62%",
                "Meeting senza titolo — In attesa",
                "—",
                "[Ultimi meeting]",
                "Weekly sync",
                "Meeting senza titolo",
                "—",
                "Apri Trama",
                "Impostazioni…",
                "—",
                "Esci da Trama",
            ]
        );
    }

    #[test]
    fn every_status_is_translated() {
        for locale in ["en", "it"] {
            let l10n = l10n(locale);
            let mut retrying = processing(JobKind::Summarize, false, None);
            retrying.retrying = true;
            for job in [
                processing(JobKind::Transcribe, true, Some(0.1)),
                processing(JobKind::Transcribe, true, None),
                processing(JobKind::Summarize, true, None),
                processing(JobKind::Transcribe, false, None),
                retrying,
            ] {
                let text = row_text(&l10n, &row("", job));
                assert!(!text.contains("tray-"), "{text} in {locale}");
            }
            for id in ["tray-tooltip", "tray-tooltip-recording", "tray-no-meetings"] {
                assert_ne!(l10n.message(id, None), id, "{id} is missing in {locale}");
            }
        }
    }

    #[test]
    fn progress_shows_next_to_the_icon_only_while_transcribing() {
        let mut model = idle();
        assert_eq!(icon_title(&model), None);
        model.processing = vec![row("", processing(JobKind::Transcribe, true, Some(0.42)))];
        assert_eq!(icon_title(&model).as_deref(), Some("42%"));
        model.recording = true;
        assert_eq!(icon_title(&model), None);
    }

    #[test]
    fn progress_alone_does_not_rebuild_the_menu() {
        let mut model = idle();
        model.processing = vec![row("", processing(JobKind::Transcribe, true, Some(0.1)))];
        let mut moved = model.clone();
        moved.processing[0].job.progress = Some(0.5);
        assert_eq!(model.structure(), moved.structure());
        moved.processing[0].job.running = false;
        assert_ne!(model.structure(), moved.structure());
    }

    #[test]
    fn the_icons_are_square_18_pt_at_2x() {
        for png in [ICON, RECORDING_ICON] {
            let image = Image::from_bytes(png).unwrap();
            assert_eq!((image.width(), image.height()), (36, 36));
        }
        // The template icon has no colour: only black with transparency.
        let image = Image::from_bytes(ICON).unwrap();
        assert!(
            image
                .rgba()
                .chunks(4)
                .all(|px| px[3] == 0 || (px[0] == 0 && px[1] == 0 && px[2] == 0))
        );
    }
}
