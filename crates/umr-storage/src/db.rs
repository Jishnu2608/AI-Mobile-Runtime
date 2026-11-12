//! The SQLite database: opening, pragmas and migrations.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

use crate::migrations::{latest_version, Migration, MIGRATIONS};

/// File name of the database inside the data root.
pub const DB_FILE: &str = "umr.db";

/// Storage errors with user-facing messages.
#[derive(Debug)]
pub enum StorageError {
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
    /// The database was written by a newer version of the product.
    TooNew {
        found: u32,
        supported: u32,
    },
    /// A migration failed; the database was left at `at_version` (the transaction rolled back).
    MigrationFailed {
        version: u32,
        description: &'static str,
        at_version: u32,
        error: rusqlite::Error,
    },
    /// A request that does not match the stored data (e.g. an unknown id).
    Invalid(String),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(e) => write!(f, "database error: {e}"),
            Self::Io(e) => write!(f, "database file error: {e}"),
            Self::TooNew { found, supported } => write!(
                f,
                "the database has schema version {found}, but this version of the product supports up to {supported}; \
                 update the product or remove the database"
            ),
            Self::MigrationFailed { version, description, at_version, error } => {
                write!(f, "database upgrade to version {version} ({description}) failed (still at {at_version}): {error}")
            }
            Self::Invalid(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for StorageError {}

impl From<rusqlite::Error> for StorageError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e)
    }
}

impl From<std::io::Error> for StorageError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// An open, migrated database.
#[derive(Debug)]
pub struct Db {
    conn: Connection,
    path: PathBuf,
}

impl Db {
    /// Opens (creating if needed) `<root>/umr.db` and brings the schema up to date.
    pub fn open_in(root: &Path) -> Result<Self, StorageError> {
        Self::open(&root.join(DB_FILE))
    }

    /// Opens a database file and applies all pending migrations.
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        Self::open_with(path, MIGRATIONS)
    }

    pub(crate) fn open_with(path: &Path, migrations: &[Migration]) -> Result<Self, StorageError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let mut db = Self {
            conn,
            path: path.to_path_buf(),
        };
        db.migrate(migrations)?;
        Ok(db)
    }

    /// Current schema version (`PRAGMA user_version`).
    pub fn schema_version(&self) -> Result<u32, StorageError> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    /// Database file path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Journal mode in effect (expected: `wal`).
    pub fn journal_mode(&self) -> Result<String, StorageError> {
        Ok(self
            .conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))?)
    }

    /// Runs `PRAGMA quick_check`; returns `true` when the database is consistent.
    pub fn quick_check(&self) -> Result<bool, StorageError> {
        let result: String = self
            .conn
            .query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        Ok(result == "ok")
    }

    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }

    pub(crate) fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    fn migrate(&mut self, migrations: &[Migration]) -> Result<(), StorageError> {
        let current = self.schema_version()?;
        let supported = migrations.last().map_or(0, |m| m.version);
        if current > supported {
            return Err(StorageError::TooNew {
                found: current,
                supported,
            });
        }
        for m in migrations.iter().filter(|m| m.version > current) {
            let at = self.schema_version()?;
            let tx = self.conn.transaction()?;
            let result = tx
                .execute_batch(m.sql)
                .and_then(|()| tx.pragma_update(None, "user_version", m.version));
            match result {
                Ok(()) => tx.commit()?,
                Err(error) => {
                    drop(tx); // rolls back
                    return Err(StorageError::MigrationFailed {
                        version: m.version,
                        description: m.description,
                        at_version: at,
                        error,
                    });
                }
            }
        }
        Ok(())
    }
}

/// Highest schema version supported by this build.
#[must_use]
pub fn supported_schema_version() -> u32 {
    latest_version()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn temp_db_path(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("umr-storage-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(DB_FILE)
    }

    fn cleanup(path: &Path) {
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn new_database_is_migrated_to_latest_in_wal_mode() {
        let p = temp_db_path("new");
        let db = Db::open(&p).unwrap();
        assert_eq!(db.schema_version().unwrap(), supported_schema_version());
        assert_eq!(db.journal_mode().unwrap(), "wal");
        assert!(db.quick_check().unwrap());
        let created: String = db
            .conn()
            .query_row("SELECT value FROM meta WHERE key='created_unix'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(created.parse::<u64>().unwrap() > 1_700_000_000);
        drop(db);
        cleanup(&p);
    }

    #[test]
    fn reopening_is_idempotent() {
        let p = temp_db_path("reopen");
        drop(Db::open(&p).unwrap());
        let db = Db::open(&p).unwrap();
        assert_eq!(db.schema_version().unwrap(), supported_schema_version());
        let n: i64 = db
            .conn()
            .query_row("SELECT count(*) FROM meta", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "migration 1 must not run twice");
        drop(db);
        cleanup(&p);
    }

    #[test]
    fn newer_database_is_refused() {
        let p = temp_db_path("too-new");
        {
            let c = Connection::open(&p).unwrap();
            c.pragma_update(None, "user_version", 999).unwrap();
        }
        match Db::open(&p) {
            Err(StorageError::TooNew { found: 999, .. }) => {}
            other => panic!("expected TooNew, got {other:?}"),
        }
        cleanup(&p);
    }

    #[test]
    fn failing_migration_rolls_back_and_keeps_previous_version() {
        const GOOD: Migration = Migration {
            version: 1,
            description: "good",
            sql: "CREATE TABLE a(x INTEGER) STRICT;",
        };
        const BAD: Migration = Migration {
            version: 2,
            description: "bad",
            sql: "CREATE TABLE b(y INTEGER) STRICT; THIS IS NOT SQL;",
        };
        let p = temp_db_path("rollback");
        match Db::open_with(&p, &[GOOD, BAD]) {
            Err(StorageError::MigrationFailed {
                version: 2,
                at_version: 1,
                ..
            }) => {}
            other => panic!("expected MigrationFailed, got {other:?}"),
        }
        let c = Connection::open(&p).unwrap();
        let v: u32 = c
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 1);
        let b_exists: i64 = c
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='b'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(b_exists, 0, "partial migration must be rolled back");
        drop(c);
        cleanup(&p);
    }
}
