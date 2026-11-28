//! Removes a meeting's audio from disk, keeping its transcript, when the audio retention
//! setting says so (see [`crate::storage::Library::audio_due_for_deletion`]).

use std::io::ErrorKind;
use std::path::Path;

use super::{JobContext, JobError, JobHandler};
use crate::storage::{Id, Job, JobKind, Library};

/// Handler for [`JobKind::DeleteAudio`]: deletes the tracks and timing files of a meeting
/// and its audio folder, then records that the audio is gone. Files already missing are
/// not an error, so the job can be repeated safely.
///
/// It deletes only what the retention setting says to delete at the time it runs: after
/// a successful transcription, once the period has passed, and not audio on hold. A job
/// that is no longer due (the setting changed meanwhile) finishes without deleting.
/// The runner queues these jobs on its own ([`JobHandler::due`]); every deletion is logged.
#[derive(Debug, Default, Clone, Copy)]
pub struct DeleteAudio;

/// Error code when a file can't be removed (permissions, disk errors).
pub const DELETE_FAILED: &str = "audio_delete_failed";

impl JobHandler for DeleteAudio {
    fn kind(&self) -> JobKind {
        JobKind::DeleteAudio
    }

    fn due(&self, library: &Library) -> crate::storage::Result<Vec<Id>> {
        library.audio_due_for_deletion()
    }

    fn run(&self, job: &Job, ctx: &JobContext) -> Result<(), JobError> {
        let id = job.target_id;
        let (root, files, retention) = {
            let library = ctx.library();
            if !library.is_audio_due(id)? {
                eprintln!(
                    "trama: kept the audio of meeting {id}: the retention setting doesn't delete it now"
                );
                return Ok(());
            }
            let files: Vec<_> = library
                .tracks(id)?
                .into_iter()
                .flat_map(|track| std::iter::once(track.audio_path).chain(track.timing_path))
                .collect();
            let retention = library.library_settings()?.audio_retention;
            (library.root().to_path_buf(), files, retention)
        };

        let total = files.len() + 1;
        for (done, file) in files.iter().enumerate() {
            remove(&root.join(file), |p| std::fs::remove_file(p))?;
            ctx.progress(done as f32 / total as f32);
        }
        remove(&root.join(Library::meeting_audio_dir(id)), |p| {
            std::fs::remove_dir_all(p)
        })?;

        ctx.library().mark_audio_deleted(id)?;
        eprintln!(
            "trama: deleted the audio of meeting {id} ({} files, audio retention {})",
            files.len(),
            retention.as_str()
        );
        ctx.progress(1.0);
        Ok(())
    }
}

/// Queues the deletion of a meeting's audio if the retention setting says to delete it
/// now; called once its transcription is stored, so "never" deletes it right away.
pub(super) fn enqueue_if_due(ctx: &JobContext, meeting_id: Id) -> crate::storage::Result<()> {
    if ctx.library().is_audio_due(meeting_id)? {
        ctx.enqueue(JobKind::DeleteAudio, meeting_id)?;
    }
    Ok(())
}

fn remove(path: &Path, op: fn(&Path) -> std::io::Result<()>) -> Result<(), JobError> {
    match op(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(JobError::retryable(
            DELETE_FAILED,
            format!("{}: {err}", path.display()),
        )),
    }
}
