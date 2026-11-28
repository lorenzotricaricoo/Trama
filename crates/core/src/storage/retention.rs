//! Audio retention: which meetings' audio the setting deletes, and when.
//!
//! The audio of a meeting is deleted only after a successful transcription (the meeting
//! is `ready` with a sealed transcription version), once the period chosen in the
//! setting ([`AudioRetention`]) has passed since that transcription. A changed setting
//! applies to every meeting, counted from its transcription. The deletion itself is a
//! [`JobKind::DeleteAudio`](super::JobKind::DeleteAudio) job (see
//! [`crate::jobs::DeleteAudio`]), queued by the job runner when it is due.
//!
//! Audio recorded before this version is *on hold*: the setting leaves it alone until the
//! user confirms that it applies to it too ([`Library::release_audio_holds`]).

use rusqlite::{OptionalExtension, Row, params};

use super::{AudioRetention, Id, Library, Result, Timestamp, id_column};

/// Milliseconds in a day, the unit of the retention periods.
pub const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// What applying a retention setting would do to the audio that exists now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RetentionImpact {
    /// Recordings whose audio would be deleted right away.
    pub delete_now: u32,
    /// Recordings whose audio would be deleted later, when their period ends, and that
    /// would otherwise have been kept.
    pub delete_later: u32,
}

impl RetentionImpact {
    /// True when nothing existing would be deleted, so there is nothing to confirm.
    pub fn is_empty(&self) -> bool {
        self.delete_now == 0 && self.delete_later == 0
    }
}

/// Audio recorded before this version, kept until the user confirms.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HeldAudio {
    /// Meetings whose audio is on hold.
    pub count: u32,
    /// What applying the current setting to them would delete.
    pub impact: RetentionImpact,
}

/// When the audio of a meeting transcribed at `transcribed_at` should be deleted under
/// `retention`; `None` when it is kept.
pub fn audio_expiry(retention: AudioRetention, transcribed_at: Timestamp) -> Option<Timestamp> {
    retention
        .days()
        .map(|days| transcribed_at.saturating_add(i64::from(days) * DAY_MS))
}

/// A meeting the retention setting applies to: it has audio, and its transcription
/// succeeded.
#[derive(Debug, Clone, Copy)]
struct Candidate {
    id: Id,
    on_hold: bool,
    /// When the current transcription was stored.
    transcribed_at: Timestamp,
    /// An earlier deletion failed or was cancelled: the runner doesn't queue it again on
    /// its own (the user can retry the job).
    settled: bool,
}

/// Meetings in the library (not in the trash) that still have their audio files and a
/// successful transcription, and no transcription waiting to run again.
const CANDIDATES: &str = "
    SELECT m.source_id, m.audio_on_hold, v.transcribed_at,
           EXISTS (SELECT 1 FROM job j WHERE j.target_id = m.source_id
                   AND j.kind = 'delete_audio' AND j.state IN ('failed', 'cancelled'))
    FROM meeting m
    JOIN source s ON s.id = m.source_id
    JOIN (SELECT source_id, MAX(sealed_at) AS transcribed_at FROM source_version
          WHERE origin = 'transcription' AND sealed_at IS NOT NULL GROUP BY source_id) v
      ON v.source_id = m.source_id
    WHERE s.deleted_at IS NULL
      AND m.audio_deleted_at IS NULL
      AND m.status = 'ready'
      AND EXISTS (SELECT 1 FROM track t WHERE t.meeting_id = m.source_id)
      AND NOT EXISTS (SELECT 1 FROM job j WHERE j.target_id = m.source_id
                      AND j.kind = 'transcribe' AND j.state = 'queued')";

fn candidate_row(row: &Row<'_>) -> rusqlite::Result<Candidate> {
    Ok(Candidate {
        id: id_column(row, 0)?,
        on_hold: row.get(1)?,
        transcribed_at: row.get(2)?,
        settled: row.get(3)?,
    })
}

impl Library {
    fn retention_candidates(&self) -> Result<Vec<Candidate>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "{CANDIDATES} ORDER BY v.transcribed_at, m.source_id"
        ))?;
        let rows = stmt.query_map([], candidate_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn retention_candidate(&self, id: Id) -> Result<Option<Candidate>> {
        Ok(self
            .conn
            .query_row(
                &format!("{CANDIDATES} AND m.source_id = ?1"),
                [id.to_string()],
                candidate_row,
            )
            .optional()?)
    }

    /// Meetings whose audio the retention setting says to delete now and that have no
    /// deletion queued, running, failed or cancelled: what the job runner queues.
    pub fn audio_due_for_deletion(&self) -> Result<Vec<Id>> {
        let retention = self.library_settings()?.audio_retention;
        let now = self.now();
        let active: Vec<Id> = self
            .active_jobs()?
            .into_iter()
            .filter(|job| job.kind == super::JobKind::DeleteAudio)
            .map(|job| job.target_id)
            .collect();
        Ok(self
            .retention_candidates()?
            .into_iter()
            .filter(|c| !c.on_hold && !c.settled && !active.contains(&c.id))
            .filter(|c| audio_expiry(retention, c.transcribed_at).is_some_and(|at| at <= now))
            .map(|c| c.id)
            .collect())
    }

    /// Whether the retention setting says to delete the audio of this meeting now. The
    /// deletion job asks again just before deleting anything.
    pub fn is_audio_due(&self, meeting_id: Id) -> Result<bool> {
        let Some(candidate) = self.retention_candidate(meeting_id)? else {
            return Ok(false);
        };
        let retention = self.library_settings()?.audio_retention;
        Ok(!candidate.on_hold
            && audio_expiry(retention, candidate.transcribed_at).is_some_and(|at| at <= self.now()))
    }

    /// What changing the retention setting to `retention` would do to the audio that
    /// exists now, compared with the current setting. Audio on hold counts as kept by the
    /// current setting: changing the setting applies to it too, once confirmed.
    pub fn audio_retention_impact(&self, retention: AudioRetention) -> Result<RetentionImpact> {
        let current = self.library_settings()?.audio_retention;
        let now = self.now();
        let mut impact = RetentionImpact::default();
        for candidate in self.retention_candidates()? {
            let before = if candidate.on_hold {
                None
            } else {
                audio_expiry(current, candidate.transcribed_at)
            };
            match audio_expiry(retention, candidate.transcribed_at) {
                Some(at) if at <= now && before.is_none_or(|before| before > now) => {
                    impact.delete_now += 1;
                }
                Some(at) if at > now && before.is_none() => impact.delete_later += 1,
                _ => {}
            }
        }
        Ok(impact)
    }

    /// The audio on hold, and what the current setting would do to it once released.
    pub fn held_audio(&self) -> Result<HeldAudio> {
        let count: u32 = self.conn.query_row(
            "SELECT COUNT(*) FROM meeting m JOIN source s ON s.id = m.source_id
             WHERE m.audio_on_hold = 1 AND m.audio_deleted_at IS NULL AND s.deleted_at IS NULL",
            [],
            |row| row.get(0),
        )?;
        let retention = self.library_settings()?.audio_retention;
        let now = self.now();
        let mut impact = RetentionImpact::default();
        for candidate in self.retention_candidates()?.iter().filter(|c| c.on_hold) {
            match audio_expiry(retention, candidate.transcribed_at) {
                Some(at) if at <= now => impact.delete_now += 1,
                Some(_) => impact.delete_later += 1,
                None => {}
            }
        }
        Ok(HeldAudio { count, impact })
    }

    /// Lets the retention setting apply to the audio on hold, after the user confirmed.
    /// Returns how many meetings were released.
    pub fn release_audio_holds(&mut self) -> Result<usize> {
        Ok(self.conn.execute(
            "UPDATE meeting SET audio_on_hold = 0 WHERE audio_on_hold = 1",
            params![],
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expiry_follows_the_setting() {
        assert_eq!(audio_expiry(AudioRetention::Never, 1_000), Some(1_000));
        assert_eq!(
            audio_expiry(AudioRetention::Days7, 1_000),
            Some(1_000 + 7 * DAY_MS)
        );
        assert_eq!(audio_expiry(AudioRetention::Days30, 0), Some(30 * DAY_MS));
        assert_eq!(audio_expiry(AudioRetention::Always, 1_000), None);
        assert_eq!(
            audio_expiry(AudioRetention::Days7, i64::MAX),
            Some(i64::MAX)
        );
    }

    #[test]
    fn an_empty_impact_needs_no_confirmation() {
        assert!(RetentionImpact::default().is_empty());
        assert!(
            !RetentionImpact {
                delete_now: 0,
                delete_later: 1
            }
            .is_empty()
        );
    }
}
