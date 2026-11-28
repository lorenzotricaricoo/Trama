//! Storage: the user's library on disk.
//!
//! A library is a folder with one SQLite database and the meetings' audio. One process
//! owns it at a time (principle A2): [`Library::open`] takes an exclusive lock, and every
//! other client goes through the owner's API. The data model is described in
//! `docs/architecture/data-model.md`.

// First, so its `text_enum!` macro is visible to the modules below.
#[macro_use]
mod model;

mod error;
mod jobs;
mod meetings;
mod migrations;
mod providers;
mod search;
mod settings;
mod sources;
mod usage;

use std::fs::{File, TryLockError};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};

pub use error::{Result, StorageError};
pub use jobs::*;
pub use migrations::SCHEMA_VERSION;
pub use model::*;
pub use providers::*;
pub use settings::*;
pub use usage::*;

/// Database file inside the library folder.
pub const DATABASE_FILE: &str = "trama.db";
/// Lock file held by the process that owns the library.
pub const LOCK_FILE: &str = "trama.lock";
/// Folder with the meetings' audio, one subfolder per meeting.
pub const AUDIO_DIR: &str = "audio";

/// Days a source stays in the trash before it is removed for good.
pub const TRASH_RETENTION_DAYS: i64 = 30;

/// Pause after which a new edit to a note starts a new version.
pub const NOTE_VERSION_PAUSE_MS: i64 = 10 * 60 * 1000;

/// How long to wait for a busy database before failing.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Source of the current time, replaceable in tests.
pub trait Clock: Send {
    /// UTC milliseconds since the Unix epoch.
    fn now(&self) -> Timestamp;
}

/// The system clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        i64::try_from(since_epoch.as_millis()).unwrap_or(i64::MAX)
    }
}

/// An open library, owned by this process until dropped.
pub struct Library {
    root: PathBuf,
    conn: Connection,
    clock: Box<dyn Clock>,
    recovery: Recovery,
    // Held for the lifetime of the library; the OS releases it when the file is closed.
    _lock: File,
}

impl std::fmt::Debug for Library {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Library")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

impl Library {
    /// Opens the library in `root`, creating it if needed, and brings its schema up to date.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_clock(root, Box::new(SystemClock))
    }

    /// Like [`Library::open`], with the given clock.
    pub fn open_with_clock(root: impl AsRef<Path>, clock: Box<dyn Clock>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(root.join(AUDIO_DIR))?;

        let lock = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(root.join(LOCK_FILE))?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(StorageError::Locked(root)),
            Err(TryLockError::Error(err)) => return Err(err.into()),
        }

        let mut conn = Connection::open(root.join(DATABASE_FILE))?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        // Migrations check foreign keys themselves once they are done, so a migration
        // can rebuild a table; normal operation enforces them on every statement.
        conn.pragma_update(None, "foreign_keys", false)?;
        migrations::migrate(&mut conn, migrations::MIGRATIONS)?;
        conn.pragma_update(None, "foreign_keys", true)?;
        let recovery = jobs::recover(&mut conn, clock.now())?;

        Ok(Self {
            root,
            conn,
            clock,
            recovery,
            _lock: lock,
        })
    }

    /// The library folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Folder for the audio of one meeting, relative to the library root.
    pub fn meeting_audio_dir(meeting_id: Id) -> PathBuf {
        Path::new(AUDIO_DIR).join(meeting_id.to_string())
    }

    /// Schema version of the open database.
    pub fn schema_version(&self) -> Result<u32> {
        migrations::schema_version(&self.conn)
    }

    /// Writes a consistent copy of the database to `dest`, which must not exist. Safe
    /// while the library is in use; the audio folder is copied separately.
    pub fn backup_database(&self, dest: impl AsRef<Path>) -> Result<()> {
        let dest = dest.as_ref();
        let dest = dest
            .to_str()
            .ok_or(StorageError::Invalid("backup path is not valid UTF-8"))?;
        self.conn.execute("VACUUM INTO ?1", [dest])?;
        Ok(())
    }

    /// A library setting, if set.
    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM setting WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    /// Sets a library setting. Never use it for secrets: they belong in the Keychain.
    pub fn set_setting(&mut self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO setting (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// The current time, from the library's clock.
    pub fn now(&self) -> Timestamp {
        self.clock.now()
    }
}

/// Checks that a path stored in the library is relative and stays inside it.
fn library_relative(path: &Path) -> Result<String> {
    let inside = !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)));
    if !inside {
        return Err(StorageError::Invalid(
            "audio paths must be relative to the library",
        ));
    }
    path.to_str()
        .map(str::to_owned)
        .ok_or(StorageError::Invalid("audio paths must be valid UTF-8"))
}

/// Reads a UUID stored as text.
fn parse_id(text: &str) -> Result<Id> {
    Id::parse_str(text).map_err(|_| StorageError::Corrupt(format!("invalid id {text:?}")))
}

/// Reads a text column as a UUID inside a row mapper.
fn id_column(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Id> {
    let text: String = row.get(index)?;
    Id::parse_str(&text).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, err.into())
    })
}

/// Reads a text column as one of the stored enums inside a row mapper.
fn enum_column<T>(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<T>
where
    T: std::str::FromStr<Err = StorageError>,
{
    let text: String = row.get(index)?;
    text.parse().map_err(|err: StorageError| {
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, err.into())
    })
}

/// A new identifier.
fn new_id() -> Id {
    Id::now_v7()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_paths_stay_inside() {
        assert!(library_relative(Path::new("audio/x/me.wav")).is_ok());
        assert!(library_relative(Path::new("/tmp/me.wav")).is_err());
        assert!(library_relative(Path::new("audio/../../me.wav")).is_err());
        assert!(library_relative(Path::new("./me.wav")).is_err());
        assert!(library_relative(Path::new("")).is_err());
    }

    #[test]
    fn ids_round_trip() {
        let id = new_id();
        assert_eq!(parse_id(&id.to_string()).unwrap(), id);
        assert!(parse_id("nope").is_err());
    }
}
