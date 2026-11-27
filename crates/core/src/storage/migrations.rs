//! Versioned schema migrations (principle A3).
//!
//! Each migration is a SQL script embedded at build time. They are applied in order,
//! each in its own transaction, and `PRAGMA user_version` records how many have run.
//! Migrations are append-only: once released, a script is never edited.

use rusqlite::Connection;

use super::error::{Result, StorageError};

/// The schema migrations, in order. Migration `n` (1-based) brings the schema to version `n`.
pub(crate) const MIGRATIONS: &[&str] = &[include_str!("migrations/0001_initial.sql")];

/// Schema version written by this build.
pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

/// Current schema version of the database.
pub(crate) fn schema_version(conn: &Connection) -> Result<u32> {
    Ok(conn.pragma_query_value(None, "user_version", |row| row.get(0))?)
}

/// Brings the database up to date with `migrations`. Refuses a database written by a
/// newer schema, and fails without changes if a migration leaves broken foreign keys.
pub(crate) fn migrate(conn: &mut Connection, migrations: &[&str]) -> Result<()> {
    let supported = migrations.len() as u32;
    let found = schema_version(conn)?;
    if found > supported {
        return Err(StorageError::NewerSchema { found, supported });
    }
    for (index, script) in migrations.iter().enumerate().skip(found as usize) {
        let version = index as u32 + 1;
        let tx = conn.transaction()?;
        tx.execute_batch(script)?;
        let broken: Option<String> = tx
            .query_row(
                "SELECT \"table\" FROM pragma_foreign_key_check LIMIT 1",
                [],
                |row| row.get(0),
            )
            .map(Some)
            .or_else(|err| match err {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                err => Err(err),
            })?;
        if let Some(table) = broken {
            return Err(StorageError::Corrupt(format!(
                "migration {version} leaves broken foreign keys in {table}"
            )));
        }
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        conn
    }

    fn tables(conn: &Connection) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_schema WHERE type = 'table' ORDER BY name")
            .unwrap();
        stmt.query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn fresh_database_gets_every_migration() {
        let mut conn = memory();
        migrate(&mut conn, MIGRATIONS).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), SCHEMA_VERSION);
        let tables = tables(&conn);
        for table in [
            "source",
            "source_version",
            "meeting",
            "track",
            "transcript_segment",
            "recording_note",
            "summary",
            "setting",
            "segment_search",
            "text_search",
        ] {
            assert!(tables.iter().any(|t| t == table), "missing table {table}");
        }
        let integrity: String = conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .unwrap();
        assert_eq!(integrity, "ok");
    }

    #[test]
    fn migrating_twice_changes_nothing() {
        let mut conn = memory();
        migrate(&mut conn, MIGRATIONS).unwrap();
        migrate(&mut conn, MIGRATIONS).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn upgrades_keep_existing_data() {
        let v1 = "CREATE TABLE item (id INTEGER PRIMARY KEY, name TEXT NOT NULL);";
        let v2 = "ALTER TABLE item ADD COLUMN note TEXT NOT NULL DEFAULT '';";
        let mut conn = memory();
        migrate(&mut conn, &[v1]).unwrap();
        conn.execute("INSERT INTO item (name) VALUES ('kept')", [])
            .unwrap();

        migrate(&mut conn, &[v1, v2]).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), 2);
        let (name, note): (String, String) = conn
            .query_row("SELECT name, note FROM item", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!((name.as_str(), note.as_str()), ("kept", ""));
    }

    #[test]
    fn refuses_a_newer_schema() {
        let mut conn = memory();
        conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .unwrap();
        let err = migrate(&mut conn, MIGRATIONS).unwrap_err();
        assert!(matches!(
            err,
            StorageError::NewerSchema { found, supported }
                if found == SCHEMA_VERSION + 1 && supported == SCHEMA_VERSION
        ));
    }

    #[test]
    fn a_failing_migration_is_rolled_back() {
        let v1 = "CREATE TABLE item (id INTEGER PRIMARY KEY);";
        let broken = "CREATE TABLE other (id INTEGER PRIMARY KEY); SELECT nope FROM nowhere;";
        let mut conn = memory();
        migrate(&mut conn, &[v1]).unwrap();
        assert!(migrate(&mut conn, &[v1, broken]).is_err());
        assert_eq!(schema_version(&conn).unwrap(), 1);
        assert!(!tables(&conn).iter().any(|t| t == "other"));
    }

    #[test]
    fn a_migration_breaking_foreign_keys_fails() {
        let v1 = "CREATE TABLE parent (id INTEGER PRIMARY KEY);
                  CREATE TABLE child (parent_id INTEGER REFERENCES parent (id));";
        let orphan = "INSERT INTO child VALUES (42);";
        let mut conn = memory();
        // Foreign keys are checked at the end of the migration, not per statement.
        conn.pragma_update(None, "foreign_keys", false).unwrap();
        migrate(&mut conn, &[v1]).unwrap();
        let err = migrate(&mut conn, &[v1, orphan]).unwrap_err();
        assert!(matches!(err, StorageError::Corrupt(_)));
        assert_eq!(schema_version(&conn).unwrap(), 1);
    }
}
