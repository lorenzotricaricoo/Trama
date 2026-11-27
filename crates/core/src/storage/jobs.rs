//! The persistent job queue and recovery after a crash.
//!
//! Jobs are rows in the `job` table, so they survive a restart. A job moves through
//! `queued → running → done`; a failure either puts it back in the queue with a delay
//! (retryable errors, up to `max_attempts`) or marks it `failed` with an error code the
//! interface translates (principle A4: no silent failures). The runner that executes
//! jobs lives in [`crate::jobs`].

use rusqlite::{Connection, OptionalExtension, Row, params};

use super::{
    Id, Library, Result, SourceKind, StorageError, Timestamp, enum_column, id_column,
    meeting_error, new_id,
};

/// Attempts before a job that keeps failing is marked `failed`.
pub const JOB_MAX_ATTEMPTS: u32 = 5;

/// Delay before the first retry; it doubles with each attempt.
pub const JOB_RETRY_BASE_MS: i64 = 30 * 1000;

/// Longest delay between two attempts.
pub const JOB_RETRY_MAX_MS: i64 = 60 * 60 * 1000;

/// Error code of a job that was running when the app stopped, after its last attempt.
pub const JOB_ERROR_INTERRUPTED: &str = "interrupted";

text_enum! {
    /// What a job does to its target source.
    JobKind {
        /// Transcribe a meeting's audio.
        Transcribe = "transcribe",
        /// Summarize and title a source.
        Summarize = "summarize",
        /// Remove a meeting's audio from disk.
        DeleteAudio = "delete_audio",
    }
}

text_enum! {
    /// Where a job is in its life.
    JobState {
        /// Waiting to run, from `run_after` on.
        Queued = "queued",
        Running = "running",
        Done = "done",
        /// Gave up: a permanent error, or no attempts left.
        Failed = "failed",
        /// Stopped by the user.
        Cancelled = "cancelled",
    }
}

/// A unit of background work on one source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub id: Id,
    pub kind: JobKind,
    /// The source the job works on.
    pub target_id: Id,
    pub state: JobState,
    /// Attempts started so far, including the running one.
    pub attempts: u32,
    pub max_attempts: u32,
    /// A queued job doesn't run before this time.
    pub run_after: Timestamp,
    /// Code of the last error, translated by the interface.
    pub error_code: Option<String>,
    /// Technical detail of the last error, for diagnostics.
    pub error_detail: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub finished_at: Option<Timestamp>,
}

impl Job {
    /// Queued or running.
    pub fn is_active(&self) -> bool {
        matches!(self.state, JobState::Queued | JobState::Running)
    }
}

/// What [`Library::open`] found and repaired from a previous run that did not stop cleanly.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recovery {
    /// Jobs that were running, put back in the queue in their original place.
    pub requeued_jobs: Vec<Id>,
    /// Jobs that were running on their last attempt, marked failed.
    pub failed_jobs: Vec<Id>,
    /// Meetings that were recording, marked failed with
    /// [`meeting_error::RECORDING_INTERRUPTED`].
    pub interrupted_recordings: Vec<Id>,
}

impl Recovery {
    pub fn is_empty(&self) -> bool {
        self.requeued_jobs.is_empty()
            && self.failed_jobs.is_empty()
            && self.interrupted_recordings.is_empty()
    }
}

/// Delay before attempt `attempts + 1`, after `attempts` failed ones.
pub fn retry_delay(attempts: u32) -> i64 {
    let doublings = attempts.saturating_sub(1).min(20);
    (JOB_RETRY_BASE_MS << doublings).min(JOB_RETRY_MAX_MS)
}

const JOB_COLUMNS: &str = "id, kind, target_id, state, attempts, max_attempts, run_after, \
     error_code, error_detail, created_at, updated_at, finished_at";

fn job_row(row: &Row<'_>) -> rusqlite::Result<Job> {
    Ok(Job {
        id: id_column(row, 0)?,
        kind: enum_column(row, 1)?,
        target_id: id_column(row, 2)?,
        state: enum_column(row, 3)?,
        attempts: row.get(4)?,
        max_attempts: row.get(5)?,
        run_after: row.get(6)?,
        error_code: row.get(7)?,
        error_detail: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
        finished_at: row.get(11)?,
    })
}

fn load_job(conn: &Connection, id: Id) -> Result<Job> {
    conn.query_row(
        &format!("SELECT {JOB_COLUMNS} FROM job WHERE id = ?1"),
        [id.to_string()],
        job_row,
    )
    .optional()?
    .ok_or_else(|| StorageError::not_found("job", id))
}

fn query_jobs(conn: &Connection, filter: &str, param: Option<String>) -> Result<Vec<Job>> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT {JOB_COLUMNS} FROM job WHERE {filter} ORDER BY created_at, id"
    ))?;
    let rows = match param {
        Some(param) => stmt.query_map([param], job_row)?,
        None => stmt.query_map([], job_row)?,
    };
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// SQL list of the kinds, for an `IN (…)` filter. Kinds are fixed identifiers, never user input.
fn kinds_list(kinds: &[JobKind]) -> String {
    kinds
        .iter()
        .map(|kind| format!("'{}'", kind.as_str()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Repairs the state left by a process that stopped without finishing its work. Only the
/// owner of the library calls it, when opening it, so nothing else can be running.
pub(super) fn recover(conn: &mut Connection, now: Timestamp) -> Result<Recovery> {
    let tx = conn.transaction()?;
    let running = query_jobs(&tx, "state = 'running'", None)?;
    let mut recovery = Recovery::default();
    for job in running {
        if job.attempts >= job.max_attempts {
            tx.execute(
                "UPDATE job SET state = 'failed', error_code = ?2, error_detail = NULL,
                     updated_at = ?3, finished_at = ?3 WHERE id = ?1",
                params![job.id.to_string(), JOB_ERROR_INTERRUPTED, now],
            )?;
            recovery.failed_jobs.push(job.id);
        } else {
            tx.execute(
                "UPDATE job SET state = 'queued', updated_at = ?2 WHERE id = ?1",
                params![job.id.to_string(), now],
            )?;
            recovery.requeued_jobs.push(job.id);
        }
    }
    let recordings: Vec<String> = {
        let mut stmt = tx.prepare(
            "SELECT source_id FROM meeting WHERE status = 'recording' ORDER BY source_id",
        )?;
        stmt.query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    for id in recordings {
        tx.execute(
            "UPDATE meeting SET status = 'failed', error_code = ?2 WHERE source_id = ?1",
            params![id, meeting_error::RECORDING_INTERRUPTED],
        )?;
        tx.execute(
            "UPDATE source SET updated_at = ?2 WHERE id = ?1",
            params![id, now],
        )?;
        recovery.interrupted_recordings.push(super::parse_id(&id)?);
    }
    tx.commit()?;
    Ok(recovery)
}

impl Library {
    /// What opening the library repaired from a run that did not stop cleanly.
    pub fn recovery(&self) -> &Recovery {
        &self.recovery
    }

    /// Queues a job on a source. If the same kind of job is already queued or running for
    /// that source, returns it instead of adding another.
    pub fn enqueue_job(&mut self, kind: JobKind, target_id: Id) -> Result<Job> {
        let now = self.now();
        let tx = self.conn.transaction()?;
        let source = super::sources::load_source(&tx, target_id)?;
        if source.deleted_at.is_some() {
            return Err(StorageError::not_found("source", target_id));
        }
        if matches!(kind, JobKind::Transcribe | JobKind::DeleteAudio)
            && source.kind != SourceKind::Meeting
        {
            return Err(StorageError::Invalid("this job only applies to meetings"));
        }
        let existing = tx
            .query_row(
                &format!(
                    "SELECT {JOB_COLUMNS} FROM job
                     WHERE kind = ?1 AND target_id = ?2 AND state IN ('queued', 'running')"
                ),
                params![kind.as_str(), target_id.to_string()],
                job_row,
            )
            .optional()?;
        let job = match existing {
            Some(job) => job,
            None => {
                let id = new_id();
                tx.execute(
                    "INSERT INTO job (id, kind, target_id, state, max_attempts, run_after, created_at, updated_at)
                     VALUES (?1, ?2, ?3, 'queued', ?4, ?5, ?5, ?5)",
                    params![id.to_string(), kind.as_str(), target_id.to_string(), JOB_MAX_ATTEMPTS, now],
                )?;
                load_job(&tx, id)?
            }
        };
        tx.commit()?;
        Ok(job)
    }

    /// A job by id.
    pub fn job(&self, id: Id) -> Result<Job> {
        load_job(&self.conn, id)
    }

    /// Queued and running jobs, oldest first.
    pub fn active_jobs(&self) -> Result<Vec<Job>> {
        query_jobs(&self.conn, "state IN ('queued', 'running')", None)
    }

    /// Every job of a source, oldest first.
    pub fn jobs_for(&self, target_id: Id) -> Result<Vec<Job>> {
        query_jobs(&self.conn, "target_id = ?1", Some(target_id.to_string()))
    }

    /// Takes the next due job of one of `kinds` and marks it running. Jobs on sources in
    /// the trash wait until the source is restored (or purged with it).
    pub fn claim_job(&mut self, kinds: &[JobKind]) -> Result<Option<Job>> {
        if kinds.is_empty() {
            return Ok(None);
        }
        let now = self.now();
        let tx = self.conn.transaction()?;
        let next: Option<String> = tx
            .query_row(
                &format!(
                    "SELECT job.id FROM job JOIN source ON source.id = job.target_id
                     WHERE job.state = 'queued' AND job.run_after <= ?1
                       AND job.kind IN ({}) AND source.deleted_at IS NULL
                     ORDER BY job.run_after, job.created_at, job.id LIMIT 1",
                    kinds_list(kinds)
                ),
                [now],
                |row| row.get(0),
            )
            .optional()?;
        let Some(id) = next else {
            return Ok(None);
        };
        tx.execute(
            "UPDATE job SET state = 'running', attempts = attempts + 1, updated_at = ?2 WHERE id = ?1",
            params![id, now],
        )?;
        let job = load_job(&tx, super::parse_id(&id)?)?;
        tx.commit()?;
        Ok(Some(job))
    }

    /// When the next queued job of one of `kinds` becomes due, if any.
    pub fn next_job_due(&self, kinds: &[JobKind]) -> Result<Option<Timestamp>> {
        if kinds.is_empty() {
            return Ok(None);
        }
        Ok(self.conn.query_row(
            &format!(
                "SELECT MIN(job.run_after) FROM job JOIN source ON source.id = job.target_id
                 WHERE job.state = 'queued' AND job.kind IN ({}) AND source.deleted_at IS NULL",
                kinds_list(kinds)
            ),
            [],
            |row| row.get(0),
        )?)
    }

    /// Marks a running job as done. A job cancelled meanwhile stays cancelled.
    pub fn finish_job(&mut self, id: Id) -> Result<Job> {
        let now = self.now();
        self.conn.execute(
            "UPDATE job SET state = 'done', error_code = NULL, error_detail = NULL,
                 updated_at = ?2, finished_at = ?2
             WHERE id = ?1 AND state = 'running'",
            params![id.to_string(), now],
        )?;
        load_job(&self.conn, id)
    }

    /// Records a failed attempt of a running job. A retryable error puts it back in the
    /// queue after [`retry_delay`] while attempts are left; otherwise it is marked failed.
    pub fn fail_job(
        &mut self,
        id: Id,
        error_code: &str,
        error_detail: &str,
        retryable: bool,
    ) -> Result<Job> {
        let now = self.now();
        let job = load_job(&self.conn, id)?;
        if job.state != JobState::Running {
            return Ok(job);
        }
        if retryable && job.attempts < job.max_attempts {
            self.conn.execute(
                "UPDATE job SET state = 'queued', run_after = ?2, error_code = ?3, error_detail = ?4,
                     updated_at = ?5 WHERE id = ?1",
                params![
                    id.to_string(),
                    now + retry_delay(job.attempts),
                    error_code,
                    error_detail,
                    now
                ],
            )?;
        } else {
            self.conn.execute(
                "UPDATE job SET state = 'failed', error_code = ?2, error_detail = ?3,
                     updated_at = ?4, finished_at = ?4 WHERE id = ?1",
                params![id.to_string(), error_code, error_detail, now],
            )?;
        }
        load_job(&self.conn, id)
    }

    /// Puts a running job back in the queue without counting the attempt, because the app
    /// is stopping, not because the job failed. It keeps its place in the queue.
    pub fn release_job(&mut self, id: Id) -> Result<Job> {
        let now = self.now();
        self.conn.execute(
            "UPDATE job SET state = 'queued', attempts = MAX(attempts - 1, 0), updated_at = ?2
             WHERE id = ?1 AND state = 'running'",
            params![id.to_string(), now],
        )?;
        load_job(&self.conn, id)
    }

    /// Cancels a queued or running job. A running job is told to stop by the runner.
    pub fn cancel_job(&mut self, id: Id) -> Result<Job> {
        let now = self.now();
        let job = load_job(&self.conn, id)?;
        if !job.is_active() {
            return Err(StorageError::Invalid(
                "only queued or running jobs can be cancelled",
            ));
        }
        self.conn.execute(
            "UPDATE job SET state = 'cancelled', updated_at = ?2, finished_at = ?2 WHERE id = ?1",
            params![id.to_string(), now],
        )?;
        load_job(&self.conn, id)
    }

    /// Queues a failed or cancelled job again, with all its attempts.
    pub fn retry_job(&mut self, id: Id) -> Result<Job> {
        let now = self.now();
        let job = load_job(&self.conn, id)?;
        if !matches!(job.state, JobState::Failed | JobState::Cancelled) {
            return Err(StorageError::Invalid(
                "only failed or cancelled jobs can be retried",
            ));
        }
        let duplicate = self.conn.execute(
            "UPDATE job SET state = 'queued', attempts = 0, run_after = ?2, updated_at = ?2,
                 finished_at = NULL
             WHERE id = ?1",
            params![id.to_string(), now],
        );
        match duplicate {
            Ok(_) => load_job(&self.conn, id),
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Err(StorageError::Invalid("the same job is already queued"))
            }
            Err(err) => Err(err.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_delay_doubles_up_to_a_limit() {
        assert_eq!(retry_delay(1), 30_000);
        assert_eq!(retry_delay(2), 60_000);
        assert_eq!(retry_delay(3), 120_000);
        assert_eq!(retry_delay(10), JOB_RETRY_MAX_MS);
        assert_eq!(retry_delay(u32::MAX), JOB_RETRY_MAX_MS);
        assert_eq!(retry_delay(0), 30_000);
    }
}
