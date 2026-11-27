-- Background jobs (transcription, summary, audio removal) and interrupted recordings.
-- See docs/architecture/data-model.md.

CREATE TABLE job (
    id           TEXT PRIMARY KEY NOT NULL,
    kind         TEXT NOT NULL CHECK (kind IN ('transcribe', 'summarize', 'delete_audio')),
    target_id    TEXT NOT NULL REFERENCES source (id) ON DELETE CASCADE,
    state        TEXT NOT NULL CHECK (state IN ('queued', 'running', 'done', 'failed', 'cancelled')),
    attempts     INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    max_attempts INTEGER NOT NULL CHECK (max_attempts >= 1),
    run_after    INTEGER NOT NULL,
    error_code   TEXT,
    error_detail TEXT,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL,
    finished_at  INTEGER
) STRICT;

-- At most one queued or running job of each kind per source.
CREATE UNIQUE INDEX job_active ON job (kind, target_id) WHERE state IN ('queued', 'running');
CREATE INDEX job_due ON job (run_after) WHERE state = 'queued';
CREATE INDEX job_by_target ON job (target_id);

-- Why a meeting failed, as a code the interface translates (for example
-- 'recording_interrupted'); empty unless the status is 'failed'.
ALTER TABLE meeting ADD COLUMN error_code TEXT;
