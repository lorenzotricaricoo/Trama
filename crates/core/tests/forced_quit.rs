//! A note survives a forced quit: a process saves an edit and is killed with SIGKILL,
//! without closing the library; opening the library again finds the edit, in the note
//! and in search.
//!
//! The test runs its own binary again as the process to kill: the child is this same
//! test, started with [`CHILD_ENV`] pointing at the library folder.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use tempfile::TempDir;
use trama_core::storage::{Id, Library};

/// Set in the child process to the library folder it writes to.
const CHILD_ENV: &str = "TRAMA_FORCED_QUIT_LIBRARY";

/// Printed by the child once its last edit is saved, followed by the note id.
const SAVED: &str = "trama-note-saved ";

const LAST_EDIT: &str = "# Weekly sync\n\nThe last line typed before the kill: perché no";

/// What the child does: saves a note through the normal API, says so, then waits to be
/// killed. The library is never dropped, so nothing is closed or checkpointed.
fn child(dir: &Path) -> ! {
    let mut library = Library::open(dir).expect("child opens the library");
    let note = library
        .create_note("", "# Weekly sync")
        .expect("child creates a note");
    library
        .save_note(note.id, LAST_EDIT)
        .expect("child saves the edit");
    let mut out = std::io::stdout().lock();
    writeln!(out, "{SAVED}{}", note.id).expect("child writes to stdout");
    out.flush().expect("child flushes stdout");
    // Killed here. The deadline only stops an orphan if the parent died first.
    std::thread::sleep(Duration::from_secs(60));
    std::process::exit(1);
}

#[test]
fn a_saved_note_survives_a_forced_quit() {
    if let Some(dir) = std::env::var_os(CHILD_ENV) {
        child(Path::new(&dir));
    }

    let dir = TempDir::new().unwrap();
    let mut process = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "a_saved_note_survives_a_forced_quit",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_ENV, dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let stdout = process.stdout.take().unwrap();
    let id = BufReader::new(stdout)
        .lines()
        .map(|line| line.expect("child output is text"))
        // The test harness may print its own text before the marker on the same line.
        .find_map(|line| line.split_once(SAVED).map(|(_, id)| id.trim().to_owned()))
        .expect("the child saved a note before exiting");
    let id = Id::parse_str(&id).unwrap();

    // SIGKILL on Unix: no destructors, no atexit, like a forced quit or a crash.
    process.kill().unwrap();
    let status = process.wait().unwrap();
    assert!(!status.success(), "the child was killed, not finished");

    let library = Library::open(dir.path()).expect("the lock is released by the kill");
    let version = library.current_version(id).unwrap().unwrap();
    assert_eq!(version.body.as_deref(), Some(LAST_EDIT));
    let hits = library.search("perche", 10).unwrap();
    assert_eq!(
        hits.iter().map(|hit| hit.source_id).collect::<Vec<_>>(),
        [id]
    );
}
