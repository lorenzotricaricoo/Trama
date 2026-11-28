-- Audio retention: see docs/architecture/data-model.md (Meeting) and
-- docs/design/information-architecture.md#audio-retention.
--
-- Audio recorded before this version is put on hold: the retention setting deletes it
-- only after the user confirms, in Settings › Data, that the setting applies to it too.
-- Meetings recorded from now on follow the setting without asking.
ALTER TABLE meeting ADD COLUMN audio_on_hold INTEGER NOT NULL DEFAULT 0 CHECK (audio_on_hold IN (0, 1));
UPDATE meeting SET audio_on_hold = 1 WHERE audio_deleted_at IS NULL;
