-- Data model v1: sources, versions, meetings, search and settings.
-- See docs/architecture/data-model.md. Identifiers are UUIDv7 text, timestamps are
-- UTC milliseconds, times inside a meeting are milliseconds from its start.

CREATE TABLE source (
    id           TEXT PRIMARY KEY NOT NULL,
    kind         TEXT NOT NULL CHECK (kind IN ('meeting', 'note')),
    title        TEXT NOT NULL DEFAULT '',
    title_origin TEXT NOT NULL DEFAULT 'auto' CHECK (title_origin IN ('auto', 'user')),
    language     TEXT,
    occurred_at  INTEGER NOT NULL,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL,
    deleted_at   INTEGER
) STRICT;

CREATE INDEX source_by_date ON source (occurred_at DESC) WHERE deleted_at IS NULL;
CREATE INDEX source_trash ON source (deleted_at) WHERE deleted_at IS NOT NULL;

CREATE TABLE source_version (
    id         TEXT PRIMARY KEY NOT NULL,
    source_id  TEXT NOT NULL REFERENCES source (id) ON DELETE CASCADE,
    number     INTEGER NOT NULL CHECK (number >= 1),
    origin     TEXT NOT NULL CHECK (origin IN ('transcription', 'editor')),
    engine     TEXT,
    body       TEXT,
    created_at INTEGER NOT NULL,
    edited_at  INTEGER NOT NULL,
    sealed_at  INTEGER,
    UNIQUE (source_id, number),
    CHECK (origin = 'editor' OR (body IS NULL AND engine IS NOT NULL))
) STRICT;

-- A sealed version never changes again.
CREATE TRIGGER source_version_sealed_update
BEFORE UPDATE ON source_version
WHEN OLD.sealed_at IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'sealed version');
END;

CREATE TABLE meeting (
    source_id        TEXT PRIMARY KEY NOT NULL REFERENCES source (id) ON DELETE CASCADE,
    status           TEXT NOT NULL CHECK (status IN ('recording', 'processing', 'ready', 'failed')),
    duration_ms      INTEGER CHECK (duration_ms >= 0),
    audio_deleted_at INTEGER
) STRICT;

CREATE TABLE track (
    id          TEXT PRIMARY KEY NOT NULL,
    meeting_id  TEXT NOT NULL REFERENCES meeting (source_id) ON DELETE CASCADE,
    role        TEXT NOT NULL CHECK (role IN ('me', 'others')),
    audio_path  TEXT NOT NULL,
    timing_path TEXT,
    device      TEXT,
    UNIQUE (meeting_id, role)
) STRICT;

CREATE TABLE transcript_segment (
    id         INTEGER PRIMARY KEY,
    version_id TEXT NOT NULL REFERENCES source_version (id) ON DELETE CASCADE,
    seq        INTEGER NOT NULL,
    role       TEXT NOT NULL CHECK (role IN ('me', 'others')),
    start_ms   INTEGER NOT NULL CHECK (start_ms >= 0),
    end_ms     INTEGER NOT NULL CHECK (end_ms >= start_ms),
    text       TEXT NOT NULL,
    UNIQUE (version_id, seq)
) STRICT;

-- Segments belong to transcription versions and are written before the version is sealed.
CREATE TRIGGER transcript_segment_insert_check
BEFORE INSERT ON transcript_segment
BEGIN
    SELECT RAISE(ABORT, 'segments need an open transcription version')
    WHERE NOT EXISTS (
        SELECT 1 FROM source_version
        WHERE id = NEW.version_id AND origin = 'transcription' AND sealed_at IS NULL
    );
END;

CREATE TRIGGER transcript_segment_update_check
BEFORE UPDATE ON transcript_segment
BEGIN
    SELECT RAISE(ABORT, 'transcript segments never change');
END;

CREATE TABLE recording_note (
    id         TEXT PRIMARY KEY NOT NULL,
    meeting_id TEXT NOT NULL REFERENCES meeting (source_id) ON DELETE CASCADE,
    at_ms      INTEGER NOT NULL CHECK (at_ms >= 0),
    text       TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

CREATE INDEX recording_note_by_time ON recording_note (meeting_id, at_ms);

CREATE TABLE summary (
    id             TEXT PRIMARY KEY NOT NULL,
    version_id     TEXT NOT NULL REFERENCES source_version (id) ON DELETE CASCADE,
    title          TEXT NOT NULL,
    body           TEXT NOT NULL,
    key_points     TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(key_points)),
    language       TEXT NOT NULL,
    model          TEXT NOT NULL,
    prompt_version TEXT NOT NULL,
    created_at     INTEGER NOT NULL
) STRICT;

CREATE INDEX summary_by_version ON summary (version_id, created_at DESC);

CREATE TABLE setting (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
) STRICT;

-- Full-text search. Accents and case are ignored.
--
-- Transcript segments are the bulk of the text: their index uses the segment table as
-- external content, keyed by the segment id, which never changes.
CREATE VIRTUAL TABLE segment_search USING fts5 (
    text,
    content = 'transcript_segment',
    content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER segment_search_insert AFTER INSERT ON transcript_segment
BEGIN
    INSERT INTO segment_search (rowid, text) VALUES (NEW.id, NEW.text);
END;

CREATE TRIGGER segment_search_delete AFTER DELETE ON transcript_segment
BEGIN
    INSERT INTO segment_search (segment_search, rowid, text) VALUES ('delete', OLD.id, OLD.text);
END;

-- Note bodies and recording notes are few and change often: their index stores the
-- text itself. `kind` says what `ref` points to: 'note' (a version id) or
-- 'recording_note' (a recording note id).
CREATE VIRTUAL TABLE text_search USING fts5 (
    text,
    source_id UNINDEXED,
    kind UNINDEXED,
    ref UNINDEXED,
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER text_search_note_insert AFTER INSERT ON source_version
WHEN NEW.body IS NOT NULL
BEGIN
    INSERT INTO text_search (text, source_id, kind, ref)
    VALUES (NEW.body, NEW.source_id, 'note', NEW.id);
END;

CREATE TRIGGER text_search_note_update AFTER UPDATE OF body ON source_version
BEGIN
    DELETE FROM text_search WHERE kind = 'note' AND ref = OLD.id;
    INSERT INTO text_search (text, source_id, kind, ref)
    SELECT NEW.body, NEW.source_id, 'note', NEW.id WHERE NEW.body IS NOT NULL;
END;

CREATE TRIGGER text_search_note_delete AFTER DELETE ON source_version
BEGIN
    DELETE FROM text_search WHERE kind = 'note' AND ref = OLD.id;
END;

CREATE TRIGGER text_search_recording_note_insert AFTER INSERT ON recording_note
BEGIN
    INSERT INTO text_search (text, source_id, kind, ref)
    VALUES (NEW.text, NEW.meeting_id, 'recording_note', NEW.id);
END;

CREATE TRIGGER text_search_recording_note_update AFTER UPDATE OF text ON recording_note
BEGIN
    DELETE FROM text_search WHERE kind = 'recording_note' AND ref = OLD.id;
    INSERT INTO text_search (text, source_id, kind, ref)
    VALUES (NEW.text, NEW.meeting_id, 'recording_note', NEW.id);
END;

CREATE TRIGGER text_search_recording_note_delete AFTER DELETE ON recording_note
BEGIN
    DELETE FROM text_search WHERE kind = 'recording_note' AND ref = OLD.id;
END;
