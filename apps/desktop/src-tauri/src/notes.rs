//! Bridge between the core's notes and sources and the frontend: Tauri commands that wrap
//! the storage API. Notes are saved automatically by the editor; the core decides when an
//! edit updates the open version and when it starts a new one.

use serde::{Deserialize, Serialize};
use tauri::State;
use trama_core::jobs::lock;
use trama_core::storage::{
    Id, Source, SourceCursor, SourceKind, SourceListItem, Timestamp, Version,
};

use crate::jobs::Core;

/// Longest preview shown for a source without a title, in characters.
const PREVIEW_CHARS: usize = 120;

/// Most sources returned by one call to [`list_sources`].
const MAX_SOURCES: u32 = 500;

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
    }
}

fn parse_id(id: &str) -> Result<Id, String> {
    Id::parse_str(id).map_err(|_| format!("invalid note id {id:?}"))
}

/// Creates a note with its first text, typed before the note existed.
#[tauri::command]
pub fn create_note(core: State<'_, Core>, body: String) -> Result<NotePayload, String> {
    let mut library = lock(&core.library);
    let source = library
        .create_note("", &body)
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
    use trama_core::storage::Library;

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
        })
        .unwrap();
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
