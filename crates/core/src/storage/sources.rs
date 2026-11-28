//! Sources, their versions, notes and the trash.

use std::path::PathBuf;

use rusqlite::{Connection, OptionalExtension, Row, params};

use super::{
    Id, LIST_TEXT_CHARS, Library, NOTE_VERSION_PAUSE_MS, NewSummary, Purged, Result, Source,
    SourceCursor, SourceKind, SourceListItem, StorageError, Summary, Timestamp, TitleOrigin,
    Version, VersionOrigin, enum_column, id_column, new_id, parse_id,
};

const SOURCE_COLUMNS: &str =
    "id, kind, title, title_origin, language, occurred_at, created_at, updated_at, deleted_at";

const VERSION_COLUMNS: &str =
    "id, source_id, number, origin, engine, body, created_at, edited_at, sealed_at";

const SUMMARY_COLUMNS: &str =
    "id, version_id, title, body, key_points, language, model, prompt_version, created_at";

pub(super) fn source_row(row: &Row<'_>) -> rusqlite::Result<Source> {
    Ok(Source {
        id: id_column(row, 0)?,
        kind: enum_column(row, 1)?,
        title: row.get(2)?,
        title_origin: enum_column(row, 3)?,
        language: row.get(4)?,
        occurred_at: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        deleted_at: row.get(8)?,
    })
}

pub(super) fn version_row(row: &Row<'_>) -> rusqlite::Result<Version> {
    Ok(Version {
        id: id_column(row, 0)?,
        source_id: id_column(row, 1)?,
        number: row.get(2)?,
        origin: enum_column(row, 3)?,
        engine: row.get(4)?,
        body: row.get(5)?,
        created_at: row.get(6)?,
        edited_at: row.get(7)?,
        sealed_at: row.get(8)?,
    })
}

fn summary_row(row: &Row<'_>) -> rusqlite::Result<Summary> {
    let key_points: String = row.get(4)?;
    let key_points = serde_json::from_str(&key_points).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, err.into())
    })?;
    Ok(Summary {
        id: id_column(row, 0)?,
        version_id: id_column(row, 1)?,
        title: row.get(2)?,
        body: row.get(3)?,
        key_points,
        language: row.get(5)?,
        model: row.get(6)?,
        prompt_version: row.get(7)?,
        created_at: row.get(8)?,
    })
}

/// Loads a source, in the trash or not.
pub(super) fn load_source(conn: &Connection, id: Id) -> Result<Source> {
    conn.query_row(
        &format!("SELECT {SOURCE_COLUMNS} FROM source WHERE id = ?1"),
        [id.to_string()],
        source_row,
    )
    .optional()?
    .ok_or_else(|| StorageError::not_found("source", id))
}

/// Loads a source that is not in the trash.
pub(super) fn live_source(conn: &Connection, id: Id, kind: SourceKind) -> Result<Source> {
    let source = load_source(conn, id)?;
    if source.kind != kind || source.deleted_at.is_some() {
        return Err(StorageError::not_found(kind.as_str(), id));
    }
    Ok(source)
}

pub(super) fn current_version(conn: &Connection, source_id: Id) -> Result<Option<Version>> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT {VERSION_COLUMNS} FROM source_version
                 WHERE source_id = ?1 ORDER BY number DESC LIMIT 1"
            ),
            [source_id.to_string()],
            version_row,
        )
        .optional()?)
}

pub(super) fn load_version(conn: &Connection, id: Id) -> Result<Version> {
    conn.query_row(
        &format!("SELECT {VERSION_COLUMNS} FROM source_version WHERE id = ?1"),
        [id.to_string()],
        version_row,
    )
    .optional()?
    .ok_or_else(|| StorageError::not_found("version", id))
}

/// Inserts a new version after the current one and returns it.
pub(super) fn insert_version(
    conn: &Connection,
    source_id: Id,
    origin: VersionOrigin,
    engine: Option<&str>,
    body: Option<&str>,
    now: Timestamp,
) -> Result<Version> {
    let id = new_id();
    conn.execute(
        "INSERT INTO source_version (id, source_id, number, origin, engine, body, created_at, edited_at)
         VALUES (?1, ?2,
                 (SELECT COALESCE(MAX(number), 0) + 1 FROM source_version WHERE source_id = ?2),
                 ?3, ?4, ?5, ?6, ?6)",
        params![id.to_string(), source_id.to_string(), origin.as_str(), engine, body, now],
    )?;
    load_version(conn, id)
}

pub(super) fn touch_source(conn: &Connection, id: Id, now: Timestamp) -> Result<()> {
    conn.execute(
        "UPDATE source SET updated_at = ?2 WHERE id = ?1",
        params![id.to_string(), now],
    )?;
    Ok(())
}

impl Library {
    /// A source, including one in the trash.
    pub fn source(&self, id: Id) -> Result<Source> {
        load_source(&self.conn, id)
    }

    /// Sources not in the trash, newest first. `kind` filters by kind; `before` pages
    /// through older sources, after the last one of the previous page.
    pub fn list_sources(
        &self,
        kind: Option<SourceKind>,
        before: Option<SourceCursor>,
        limit: u32,
    ) -> Result<Vec<Source>> {
        Ok(self
            .list_source_items(kind, before, limit)?
            .into_iter()
            .map(|item| item.source)
            .collect())
    }

    /// Like [`Library::list_sources`], with the start of each note's current text (up to
    /// [`LIST_TEXT_CHARS`] characters), read in the same query.
    pub fn list_source_items(
        &self,
        kind: Option<SourceKind>,
        before: Option<SourceCursor>,
        limit: u32,
    ) -> Result<Vec<SourceListItem>> {
        let columns = SOURCE_COLUMNS
            .split(", ")
            .map(|column| format!("s.{column}"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {columns}, substr(v.body, 1, ?5) FROM source s
             LEFT JOIN source_version v ON v.source_id = s.id
                 AND v.number = (SELECT MAX(number) FROM source_version WHERE source_id = s.id)
             WHERE s.deleted_at IS NULL
               AND (?1 IS NULL OR s.kind = ?1)
               AND (?2 IS NULL OR (s.occurred_at, s.id) < (?2, ?3))
             ORDER BY s.occurred_at DESC, s.id DESC
             LIMIT ?4"
        ))?;
        let rows = stmt.query_map(
            params![
                kind.map(SourceKind::as_str),
                before.map(|cursor| cursor.occurred_at),
                before.map(|cursor| cursor.id.to_string()),
                limit,
                LIST_TEXT_CHARS
            ],
            |row| {
                let text_start: Option<String> = row.get(9)?;
                Ok(SourceListItem {
                    source: source_row(row)?,
                    text_start: text_start.filter(|text| !text.is_empty()),
                })
            },
        )?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Sets the title. An automatic title never replaces one written by the user.
    pub fn set_title(&mut self, id: Id, title: &str, origin: TitleOrigin) -> Result<Source> {
        let now = self.now();
        let source = load_source(&self.conn, id)?;
        if origin == TitleOrigin::Auto && source.title_origin == TitleOrigin::User {
            return Ok(source);
        }
        self.conn.execute(
            "UPDATE source SET title = ?2, title_origin = ?3, updated_at = ?4 WHERE id = ?1",
            params![id.to_string(), title, origin.as_str(), now],
        )?;
        load_source(&self.conn, id)
    }

    /// Every version of a source, oldest first.
    pub fn versions(&self, source_id: Id) -> Result<Vec<Version>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {VERSION_COLUMNS} FROM source_version WHERE source_id = ?1 ORDER BY number"
        ))?;
        let rows = stmt.query_map([source_id.to_string()], version_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The current (highest-numbered) version of a source, if it has one.
    pub fn current_version(&self, source_id: Id) -> Result<Option<Version>> {
        current_version(&self.conn, source_id)
    }

    /// A version by id.
    pub fn version(&self, id: Id) -> Result<Version> {
        load_version(&self.conn, id)
    }

    /// Creates a note with its first, open version. An empty title is filled later by
    /// the summary.
    pub fn create_note(&mut self, title: &str, body: &str) -> Result<Source> {
        let now = self.now();
        let id = new_id();
        let origin = if title.is_empty() {
            TitleOrigin::Auto
        } else {
            TitleOrigin::User
        };
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO source (id, kind, title, title_origin, occurred_at, created_at, updated_at)
             VALUES (?1, 'note', ?2, ?3, ?4, ?4, ?4)",
            params![id.to_string(), title, origin.as_str(), now],
        )?;
        insert_version(&tx, id, VersionOrigin::Editor, None, Some(body), now)?;
        tx.commit()?;
        load_source(&self.conn, id)
    }

    /// Saves the text of a note (autosave). Edits within [`NOTE_VERSION_PAUSE_MS`] of the
    /// previous one update the open version; after a longer pause, or once the version is
    /// sealed, the text starts a new version. Returns the version holding the text.
    pub fn save_note(&mut self, id: Id, body: &str) -> Result<Version> {
        let now = self.now();
        let tx = self.conn.transaction()?;
        live_source(&tx, id, SourceKind::Note)?;
        let current = current_version(&tx, id)?;
        let version = match current {
            Some(v) if v.body.as_deref() == Some(body) => v,
            Some(v) if !v.is_sealed() && now - v.edited_at < NOTE_VERSION_PAUSE_MS => {
                tx.execute(
                    "UPDATE source_version SET body = ?2, edited_at = ?3 WHERE id = ?1",
                    params![v.id.to_string(), body, now],
                )?;
                touch_source(&tx, id, now)?;
                load_version(&tx, v.id)?
            }
            previous => {
                if let Some(v) = previous.filter(|v| !v.is_sealed()) {
                    seal(&tx, v.id, v.edited_at)?;
                }
                touch_source(&tx, id, now)?;
                insert_version(&tx, id, VersionOrigin::Editor, None, Some(body), now)?
            }
        };
        tx.commit()?;
        Ok(version)
    }

    /// Seals the open version of a source, if any, so it can be cited (the curator calls
    /// this before reading a note). Returns the current version, sealed.
    pub fn seal_current_version(&mut self, source_id: Id) -> Result<Option<Version>> {
        let now = self.now();
        let Some(version) = current_version(&self.conn, source_id)? else {
            return Ok(None);
        };
        if version.is_sealed() {
            return Ok(Some(version));
        }
        seal(&self.conn, version.id, now)?;
        Ok(Some(load_version(&self.conn, version.id)?))
    }

    /// Stores a summary of a version. If the source's title is automatic, the summary's
    /// title replaces it.
    pub fn add_summary(&mut self, version_id: Id, summary: &NewSummary) -> Result<Summary> {
        let now = self.now();
        let id = new_id();
        let key_points = serde_json::to_string(&summary.key_points)
            .map_err(|err| StorageError::Corrupt(err.to_string()))?;
        let tx = self.conn.transaction()?;
        let version = load_version(&tx, version_id)?;
        tx.execute(
            "INSERT INTO summary (id, version_id, title, body, key_points, language, model, prompt_version, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id.to_string(),
                version_id.to_string(),
                summary.title,
                summary.body,
                key_points,
                summary.language,
                summary.model,
                summary.prompt_version,
                now
            ],
        )?;
        tx.execute(
            "UPDATE source SET title = ?2, updated_at = ?3 WHERE id = ?1 AND title_origin = 'auto'",
            params![version.source_id.to_string(), summary.title, now],
        )?;
        let stored = tx.query_row(
            &format!("SELECT {SUMMARY_COLUMNS} FROM summary WHERE id = ?1"),
            [id.to_string()],
            summary_row,
        )?;
        tx.commit()?;
        Ok(stored)
    }

    /// The newest summary of the source's current version.
    pub fn latest_summary(&self, source_id: Id) -> Result<Option<Summary>> {
        Ok(self
            .conn
            .query_row(
                &format!(
                    "SELECT {SUMMARY_COLUMNS} FROM summary WHERE version_id = (
                         SELECT id FROM source_version WHERE source_id = ?1
                         ORDER BY number DESC LIMIT 1)
                     ORDER BY created_at DESC, id DESC LIMIT 1"
                ),
                [source_id.to_string()],
                summary_row,
            )
            .optional()?)
    }

    /// Moves a source to the trash. It disappears from lists and search.
    pub fn trash(&mut self, id: Id) -> Result<()> {
        let now = self.now();
        let changed = self.conn.execute(
            "UPDATE source SET deleted_at = ?2 WHERE id = ?1 AND deleted_at IS NULL",
            params![id.to_string(), now],
        )?;
        if changed == 0 {
            load_source(&self.conn, id)?;
        }
        Ok(())
    }

    /// Takes a source out of the trash.
    pub fn restore(&mut self, id: Id) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE source SET deleted_at = NULL WHERE id = ?1",
            [id.to_string()],
        )?;
        if changed == 0 {
            return Err(StorageError::not_found("source", id));
        }
        Ok(())
    }

    /// Sources in the trash, most recently deleted first.
    pub fn list_trash(&self) -> Result<Vec<Source>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {SOURCE_COLUMNS} FROM source WHERE deleted_at IS NOT NULL
             ORDER BY deleted_at DESC, id DESC"
        ))?;
        let rows = stmt.query_map([], source_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Removes for good the sources trashed before `cutoff`, with everything that belongs
    /// to them. Returns their audio paths, which the caller deletes from disk.
    pub fn purge_trash(&mut self, cutoff: Timestamp) -> Result<Purged> {
        let tx = self.conn.transaction()?;
        let sources: Vec<Id> = {
            let mut stmt = tx.prepare(
                "SELECT id FROM source WHERE deleted_at IS NOT NULL AND deleted_at < ?1 ORDER BY id",
            )?;
            let ids = stmt
                .query_map([cutoff], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            ids.iter().map(|id| parse_id(id)).collect::<Result<_>>()?
        };
        let mut audio_paths = Vec::new();
        for id in &sources {
            let mut stmt = tx.prepare(
                "SELECT audio_path, timing_path FROM track WHERE meeting_id = ?1 ORDER BY role",
            )?;
            let rows = stmt.query_map([id.to_string()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })?;
            for row in rows {
                let (audio, timing) = row?;
                audio_paths.push(PathBuf::from(audio));
                audio_paths.extend(timing.map(PathBuf::from));
            }
            if load_source(&tx, *id)?.kind == SourceKind::Meeting {
                audio_paths.push(Library::meeting_audio_dir(*id));
            }
            tx.execute("DELETE FROM source WHERE id = ?1", [id.to_string()])?;
        }
        tx.commit()?;
        Ok(Purged {
            sources,
            audio_paths,
        })
    }
}

fn seal(conn: &Connection, version_id: Id, at: Timestamp) -> Result<()> {
    conn.execute(
        "UPDATE source_version SET sealed_at = ?2 WHERE id = ?1 AND sealed_at IS NULL",
        params![version_id.to_string(), at],
    )?;
    Ok(())
}
