//! Bridge between the core's notes and sources and the frontend: Tauri commands that wrap
//! the storage API. Notes are saved automatically by the editor; the core decides when an
//! edit updates the open version and when it starts a new one.

use serde::{Deserialize, Serialize};
use tauri::State;
use trama_core::jobs::lock;
use trama_core::storage::{
    HitTarget, Id, Library, SearchHit, Source, SourceCursor, SourceKind, SourceListItem,
    SourceResult, Timestamp, TitleOrigin, Version,
};

use crate::jobs::Core;

/// Longest preview shown for a source without a title, in characters.
const PREVIEW_CHARS: usize = 120;

/// Most sources returned by one call to [`list_sources`].
const MAX_SOURCES: u32 = 500;

/// Most sources returned by one call to [`search_sources`].
const MAX_RESULTS: u32 = 100;

/// Longest search accepted, in characters; longer input is cut.
const MAX_QUERY_CHARS: usize = 200;

/// Longest title a note keeps, in characters.
const MAX_TITLE_CHARS: usize = 200;

/// A note as the editor sees it: its current text.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NotePayload {
    pub id: String,
    pub title: String,
    /// Markdown text of the current version.
    pub body: String,
    pub version_number: u32,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl NotePayload {
    fn new(source: &Source, version: Option<&Version>) -> Self {
        Self {
            id: source.id.to_string(),
            title: source.title.clone(),
            body: version.and_then(|v| v.body.clone()).unwrap_or_default(),
            version_number: version.map_or(0, |v| v.number),
            created_at: source.created_at,
            updated_at: source.updated_at,
        }
    }
}

/// What a save stored: the version holding the text and when it was written.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SavedPayload {
    pub version_number: u32,
    pub saved_at: Timestamp,
}

/// A row of the Sources list.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SourcePayload {
    pub id: String,
    /// `meeting` or `note`.
    pub kind: &'static str,
    /// Empty until a summary or the user gives one.
    pub title: String,
    pub occurred_at: Timestamp,
    pub updated_at: Timestamp,
    /// For a note: the first line of its text, shown while it has no title.
    pub preview: Option<String>,
    /// For a meeting: `recording`, `processing`, `ready` or `failed`.
    pub status: Option<&'static str>,
    /// For a meeting: its length, once known.
    pub duration_ms: Option<i64>,
}

/// One place a search found, in a source.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HitPayload {
    /// `title`, `transcript`, `note` (the text of a note) or `recordingNote`.
    pub kind: &'static str,
    /// The text around the match; each match is between U+0002 and U+0003.
    pub excerpt: String,
    /// Where it is in a meeting, from its start.
    pub at_ms: Option<i64>,
}

/// A source found by a search, with its best hits.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SearchResultPayload {
    pub source: SourcePayload,
    pub hits: Vec<HitPayload>,
}

/// How many sources there are of each kind, for the filters of Sources.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SourceCountsPayload {
    pub meetings: u32,
    pub notes: u32,
}

/// Where a page of sources ends; passed back as `before` to get the next page.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CursorPayload {
    pub occurred_at: Timestamp,
    pub id: String,
}

/// A page of the Sources list.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SourcePagePayload {
    pub sources: Vec<SourcePayload>,
    /// The cursor of the next page; `None` on the last one.
    pub next: Option<CursorPayload>,
}

/// The first line with text, without the Markdown marks that open it (`#`, `>`, `-`, …),
/// shortened to [`PREVIEW_CHARS`]. `None` for an empty text.
pub fn preview(body: &str) -> Option<String> {
    let line = body
        .lines()
        .map(|line| {
            line.trim_start_matches(|c: char| c.is_whitespace() || "#>-*+".contains(c))
                .trim()
        })
        .find(|line| !line.is_empty())?;
    let mut text: String = line.chars().take(PREVIEW_CHARS).collect();
    if line.chars().count() > PREVIEW_CHARS {
        text.push('…');
    }
    Some(text)
}

fn source_payload(item: &SourceListItem) -> SourcePayload {
    let source = &item.source;
    SourcePayload {
        id: source.id.to_string(),
        kind: source.kind.as_str(),
        title: source.title.clone(),
        occurred_at: source.occurred_at,
        updated_at: source.updated_at,
        preview: item.text_start.as_deref().and_then(preview),
        status: item.meeting.map(|meeting| meeting.status.as_str()),
        duration_ms: item.meeting.and_then(|meeting| meeting.duration_ms),
    }
}

fn hit_payload(hit: &SearchHit) -> HitPayload {
    HitPayload {
        kind: match hit.target {
            HitTarget::Title => "title",
            HitTarget::Segment { .. } => "transcript",
            HitTarget::Note { .. } => "note",
            HitTarget::RecordingNote { .. } => "recordingNote",
        },
        excerpt: hit.excerpt.clone(),
        at_ms: hit.at_ms,
    }
}

fn result_payload(result: &SourceResult) -> SearchResultPayload {
    SearchResultPayload {
        source: source_payload(&result.item),
        hits: result.hits.iter().map(hit_payload).collect(),
    }
}

fn parse_id(id: &str) -> Result<Id, String> {
    Id::parse_str(id).map_err(|_| format!("invalid note id {id:?}"))
}

/// A title as the user typed it, on one line, without surrounding spaces and at most
/// [`MAX_TITLE_CHARS`] long.
pub fn clean_title(title: &str) -> String {
    let line = title.split_whitespace().collect::<Vec<_>>().join(" ");
    line.chars().take(MAX_TITLE_CHARS).collect()
}

/// Creates a note with its first title and text, typed before the note existed. An empty
/// title is left to the summary.
#[tauri::command]
pub fn create_note(
    core: State<'_, Core>,
    title: Option<String>,
    body: String,
) -> Result<NotePayload, String> {
    let mut library = lock(&core.library);
    let title = clean_title(title.as_deref().unwrap_or_default());
    let source = library
        .create_note(&title, &body)
        .map_err(|err| err.to_string())?;
    let version = library
        .current_version(source.id)
        .map_err(|err| err.to_string())?;
    Ok(NotePayload::new(&source, version.as_ref()))
}

/// Saves the text of a note (autosave). The write is committed before this returns.
#[tauri::command]
pub fn save_note(core: State<'_, Core>, id: String, body: String) -> Result<SavedPayload, String> {
    let version = lock(&core.library)
        .save_note(parse_id(&id)?, &body)
        .map_err(|err| err.to_string())?;
    Ok(SavedPayload {
        version_number: version.number,
        saved_at: version.edited_at,
    })
}

/// Renames a note. An empty title gives the title back to the summary.
pub fn rename_note(library: &mut Library, id: Id, title: &str) -> Result<Source, String> {
    let source = library.source(id).map_err(|err| err.to_string())?;
    if source.kind != SourceKind::Note || source.deleted_at.is_some() {
        return Err(format!("note {id} not found"));
    }
    let title = clean_title(title);
    let renamed = if title.is_empty() {
        library.reset_title(id)
    } else {
        library.set_title(id, &title, TitleOrigin::User)
    };
    renamed.map_err(|err| err.to_string())
}

/// Sets the title of a note, as typed above its text; returns the title kept.
#[tauri::command]
pub fn set_note_title(core: State<'_, Core>, id: String, title: String) -> Result<String, String> {
    let id = parse_id(&id)?;
    rename_note(&mut lock(&core.library), id, &title).map(|source| source.title)
}

/// Opens a link of a note's preview in the default browser or mail app. Only web and mail
/// links: anything else could start a program.
#[tauri::command]
pub fn open_link(url: String) -> Result<(), String> {
    let allowed = ["https://", "http://", "mailto:"]
        .iter()
        .any(|scheme| url.len() > scheme.len() && url[..scheme.len()].eq_ignore_ascii_case(scheme));
    if !allowed || url.chars().any(char::is_control) {
        return Err("unsupported_link".to_owned());
    }
    #[cfg(target_os = "macos")]
    let opened = std::process::Command::new("/usr/bin/open")
        .arg("--")
        .arg(&url)
        .status();
    #[cfg(not(target_os = "macos"))]
    let opened: std::io::Result<std::process::ExitStatus> =
        Err(std::io::Error::other("not supported on this platform yet"));
    match opened {
        Ok(status) if status.success() => Ok(()),
        other => {
            eprintln!("trama: cannot open a link: {other:?}");
            Err("link_not_opened".to_owned())
        }
    }
}

/// A note with the text of its current version.
#[tauri::command]
pub fn get_note(core: State<'_, Core>, id: String) -> Result<NotePayload, String> {
    let id = parse_id(&id)?;
    let library = lock(&core.library);
    let source = library.source(id).map_err(|err| err.to_string())?;
    if source.kind != SourceKind::Note || source.deleted_at.is_some() {
        return Err(format!("note {id} not found"));
    }
    let version = library.current_version(id).map_err(|err| err.to_string())?;
    Ok(NotePayload::new(&source, version.as_ref()))
}

/// Sources not in the trash, newest first: all of them, or one `kind` (`meeting` or
/// `note`), a page of at most `limit`. `before` is the `next` cursor of the previous page.
/// Used by the Sources screen.
#[tauri::command]
pub fn list_sources(
    core: State<'_, Core>,
    kind: Option<String>,
    before: Option<CursorPayload>,
    limit: Option<u32>,
) -> Result<SourcePagePayload, String> {
    let kind = kind
        .map(|kind| kind.parse::<SourceKind>())
        .transpose()
        .map_err(|err| err.to_string())?;
    let before = before
        .map(|cursor| {
            Ok::<_, String>(SourceCursor {
                occurred_at: cursor.occurred_at,
                id: parse_id(&cursor.id)?,
            })
        })
        .transpose()?;
    let limit = limit.unwrap_or(MAX_SOURCES).clamp(1, MAX_SOURCES);
    // One query under the lock; the payloads are built after releasing it.
    let items = lock(&core.library)
        .list_source_items(kind, before, limit)
        .map_err(|err| err.to_string())?;
    Ok(page(&items, limit))
}

/// Sources containing every word of `query` (in the title, the text, the transcript or the
/// notes typed while recording), best first, optionally of one `kind`, at most `limit`.
/// Used by Sources and the command bar.
#[tauri::command]
pub fn search_sources(
    core: State<'_, Core>,
    query: String,
    kind: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<SearchResultPayload>, String> {
    let kind = kind
        .map(|kind| kind.parse::<SourceKind>())
        .transpose()
        .map_err(|err| err.to_string())?;
    let query: String = query.chars().take(MAX_QUERY_CHARS).collect();
    let limit = limit.unwrap_or(MAX_RESULTS).clamp(1, MAX_RESULTS);
    let results = lock(&core.library)
        .search_sources(&query, kind, limit)
        .map_err(|err| err.to_string())?;
    Ok(results.iter().map(result_payload).collect())
}

/// How many meetings and notes are not in the trash.
#[tauri::command]
pub fn source_counts(core: State<'_, Core>) -> Result<SourceCountsPayload, String> {
    let counts = lock(&core.library)
        .source_counts()
        .map_err(|err| err.to_string())?;
    Ok(SourceCountsPayload {
        meetings: counts.meetings,
        notes: counts.notes,
    })
}

fn page(items: &[SourceListItem], limit: u32) -> SourcePagePayload {
    let full = u32::try_from(items.len()).is_ok_and(|len| len >= limit);
    SourcePagePayload {
        sources: items.iter().map(source_payload).collect(),
        next: items.last().filter(|_| full).map(|item| CursorPayload {
            occurred_at: item.source.occurred_at,
            id: item.source.id.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_results_say_where_each_hit_is() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut library = Library::open(dir.path()).unwrap();
        let meeting = library.create_meeting("it", 0).unwrap();
        library
            .add_recording_note(meeting.id, 42_000, "budget")
            .unwrap();
        library.create_note("Budget", "").unwrap();
        let results: Vec<_> = library
            .search_sources("budget", None, 10)
            .unwrap()
            .iter()
            .map(result_payload)
            .collect();
        assert_eq!(results[0].source.kind, "note");
        assert_eq!(results[0].hits[0].kind, "title");
        assert_eq!(results[0].source.status, None);
        assert_eq!(results[1].source.kind, "meeting");
        assert_eq!(results[1].source.status, Some("recording"));
        assert_eq!(results[1].hits[0].kind, "recordingNote");
        assert_eq!(results[1].hits[0].at_ms, Some(42_000));
    }

    #[test]
    fn titles_are_kept_on_one_short_line() {
        assert_eq!(clean_title("  Weekly\n sync\t "), "Weekly sync");
        assert_eq!(clean_title("   "), "");
        assert_eq!(
            clean_title(&"è".repeat(300)).chars().count(),
            MAX_TITLE_CHARS
        );
    }

    #[test]
    fn renaming_a_note_and_clearing_the_title() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut library = Library::open(dir.path()).unwrap();
        let note = library.create_note("", "text").unwrap();
        let renamed = rename_note(&mut library, note.id, "  Piano Q1 ").unwrap();
        assert_eq!(renamed.title, "Piano Q1");
        assert_eq!(renamed.title_origin, TitleOrigin::User);
        let cleared = rename_note(&mut library, note.id, " ").unwrap();
        assert_eq!(cleared.title, "");
        assert_eq!(cleared.title_origin, TitleOrigin::Auto);
        assert!(rename_note(&mut library, Id::now_v7(), "x").is_err());
    }

    #[test]
    fn only_web_and_mail_links_open() {
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "https://",
            "ssh://h",
            "x",
        ] {
            assert_eq!(open_link(url.into()).unwrap_err(), "unsupported_link");
        }
    }

    #[test]
    fn preview_is_the_first_line_with_text() {
        assert_eq!(preview(""), None);
        assert_eq!(preview("\n  \n# \n"), None);
        assert_eq!(
            preview("\n\n## Weekly sync  \nbody").as_deref(),
            Some("Weekly sync")
        );
        assert_eq!(preview("- [ ] call back").as_deref(), Some("[ ] call back"));
        assert_eq!(preview("> perché no").as_deref(), Some("perché no"));
        let long = "è".repeat(PREVIEW_CHARS + 5);
        let shortened = preview(&long).unwrap();
        assert_eq!(shortened.chars().count(), PREVIEW_CHARS + 1);
        assert!(shortened.ends_with('…'));
    }

    #[test]
    fn payloads_use_the_frontend_names() {
        let saved = serde_json::to_value(SavedPayload {
            version_number: 2,
            saved_at: 5,
        })
        .unwrap();
        assert_eq!(saved["versionNumber"], 2);
        assert_eq!(saved["savedAt"], 5);

        let source = serde_json::to_value(SourcePayload {
            id: "x".into(),
            kind: "note",
            title: String::new(),
            occurred_at: 1,
            updated_at: 2,
            preview: Some("Weekly sync".into()),
            status: Some("ready"),
            duration_ms: Some(60_000),
        })
        .unwrap();
        assert_eq!(source["durationMs"], 60_000);
        assert_eq!(source["status"], "ready");
        assert_eq!(source["occurredAt"], 1);
        assert_eq!(source["updatedAt"], 2);
        assert_eq!(source["preview"], "Weekly sync");
    }

    #[test]
    fn pages_end_with_a_cursor_until_the_last() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut library = Library::open(dir.path()).unwrap();
        for i in 0..3 {
            library.create_note("", &format!("note {i}")).unwrap();
        }
        let first = page(&library.list_source_items(None, None, 2).unwrap(), 2);
        let next = first.next.clone().expect("a full page has a next cursor");
        let json = serde_json::to_value(&first).unwrap();
        assert_eq!(json["next"]["occurredAt"], next.occurred_at);
        let before = SourceCursor {
            occurred_at: next.occurred_at,
            id: Id::parse_str(&next.id).unwrap(),
        };
        let last = page(
            &library.list_source_items(None, Some(before), 2).unwrap(),
            2,
        );
        assert_eq!(last.sources.len(), 1);
        assert_eq!(last.next, None);
    }

    #[test]
    fn notes_round_trip_through_the_payloads() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut library = Library::open(dir.path()).unwrap();
        let note = library.create_note("", "# First").unwrap();
        library.save_note(note.id, "# First\n\nmore").unwrap();
        let version = library.current_version(note.id).unwrap();
        let payload = NotePayload::new(&library.source(note.id).unwrap(), version.as_ref());
        assert_eq!(payload.body, "# First\n\nmore");
        assert_eq!(payload.version_number, 1);
        let items = library.list_source_items(None, None, 10).unwrap();
        let row = source_payload(&items[0]);
        assert_eq!(row.kind, "note");
        assert_eq!(row.preview.as_deref(), Some("First"));
    }
}
