//! History of environment scans (ENV-006). Reports are stored as JSON text.

use rusqlite::{params, OptionalExtension};

use crate::db::{Db, StorageError};

/// Number of scans kept; older ones are deleted when a new one is recorded.
pub const KEEP_SCANS: i64 = 50;

impl Db {
    /// Stores a scan report and prunes old ones. Returns the new row id.
    pub fn record_env_scan(
        &self,
        scanned_unix: i64,
        report_json: &str,
    ) -> Result<i64, StorageError> {
        self.conn().execute(
            "INSERT INTO env_scans (scanned_unix, report_json) VALUES (?1, ?2)",
            params![scanned_unix, report_json],
        )?;
        let id = self.conn().last_insert_rowid();
        self.conn().execute(
            "DELETE FROM env_scans WHERE id NOT IN (SELECT id FROM env_scans ORDER BY id DESC LIMIT ?1)",
            [KEEP_SCANS],
        )?;
        Ok(id)
    }

    /// The most recent scan: `(scanned_unix, report_json)`.
    pub fn latest_env_scan(&self) -> Result<Option<(i64, String)>, StorageError> {
        Ok(self
            .conn()
            .query_row(
                "SELECT scanned_unix, report_json FROM env_scans ORDER BY id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }

    /// Number of stored scans.
    pub fn env_scan_count(&self) -> Result<i64, StorageError> {
        Ok(self
            .conn()
            .query_row("SELECT count(*) FROM env_scans", [], |r| r.get(0))?)
    }
}

#[cfg(test)]
mod tests {
    use crate::db::tests::temp_db_path;
    use crate::db::Db;

    #[test]
    fn record_latest_and_prune() {
        let p = temp_db_path("env-scans");
        let db = Db::open(&p).unwrap();
        assert_eq!(db.latest_env_scan().unwrap(), None);
        for i in 0..(super::KEEP_SCANS + 5) {
            db.record_env_scan(1_000 + i, &format!("{{\"n\":{i}}}"))
                .unwrap();
        }
        assert_eq!(db.env_scan_count().unwrap(), super::KEEP_SCANS);
        let (at, json) = db.latest_env_scan().unwrap().unwrap();
        assert_eq!(at, 1_000 + super::KEEP_SCANS + 4);
        assert!(json.contains(&(super::KEEP_SCANS + 4).to_string()));
        drop(db);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }
}
