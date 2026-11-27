//! Removes a meeting's audio from disk, keeping its transcript.

use std::io::ErrorKind;
use std::path::Path;

use super::{JobContext, JobError, JobHandler};
use crate::storage::{Job, JobKind, Library};

/// Handler for [`JobKind::DeleteAudio`]: deletes the tracks and timing files of a meeting
/// and its audio folder, then records that the audio is gone. Files already missing are
/// not an error, so the job can be repeated safely.
#[derive(Debug, Default, Clone, Copy)]
pub struct DeleteAudio;

/// Error code when a file can't be removed (permissions, disk errors).
pub const DELETE_FAILED: &str = "audio_delete_failed";

impl JobHandler for DeleteAudio {
    fn kind(&self) -> JobKind {
        JobKind::DeleteAudio
    }

    fn run(&self, job: &Job, ctx: &JobContext) -> Result<(), JobError> {
        let (root, files) = {
            let library = ctx.library();
            let files: Vec<_> = library
                .tracks(job.target_id)?
                .into_iter()
                .flat_map(|track| std::iter::once(track.audio_path).chain(track.timing_path))
                .collect();
            (library.root().to_path_buf(), files)
        };

        let total = files.len() + 1;
        for (done, file) in files.iter().enumerate() {
            remove(&root.join(file), |p| std::fs::remove_file(p))?;
            ctx.progress(done as f32 / total as f32);
        }
        remove(&root.join(Library::meeting_audio_dir(job.target_id)), |p| {
            std::fs::remove_dir_all(p)
        })?;

        ctx.library().mark_audio_deleted(job.target_id)?;
        ctx.progress(1.0);
        Ok(())
    }
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
