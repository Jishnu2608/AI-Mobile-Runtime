//! Installer records: the install journal (INS-003), licence acceptances (INS-004) and the
//! audit log.

use rusqlite::{params, OptionalExtension, Row};

use crate::db::{Db, StorageError};

/// State of an install transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxnState {
    Started,
    Downloaded,
    Extracted,
    /// The staging folder is being moved into place; the next step records the component.
    Committing,
    Committed,
    Failed,
    RolledBack,
}

impl TxnState {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Downloaded => "downloaded",
            Self::Extracted => "extracted",
            Self::Committing => "committing",
            Self::Committed => "committed",
            Self::Failed => "failed",
            Self::RolledBack => "rolled-back",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "started" => Self::Started,
            "downloaded" => Self::Downloaded,
            "extracted" => Self::Extracted,
            "committing" => Self::Committing,
            "committed" => Self::Committed,
            "failed" => Self::Failed,
            "rolled-back" => Self::RolledBack,
            _ => return None,
        })
    }

    /// Finished: nothing left to recover.
    #[must_use]
    pub fn is_final(self) -> bool {
        matches!(self, Self::Committed | Self::Failed | Self::RolledBack)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalEntry {
    pub id: String,
    pub component: String,
    pub target: String,
    pub staging: Option<String>,
    pub state: TxnState,
    pub error: Option<String>,
    pub started_unix: i64,
    pub updated_unix: i64,
}

const JOURNAL_COLUMNS: &str =
    "id, component, target, staging, state, error, started_unix, updated_unix";

fn journal_row(r: &Row<'_>) -> rusqlite::Result<JournalEntry> {
    let state: String = r.get(4)?;
    Ok(JournalEntry {
        id: r.get(0)?,
        component: r.get(1)?,
        target: r.get(2)?,
        staging: r.get(3)?,
        state: TxnState::parse(&state).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                4,
                rusqlite::types::Type::Text,
                format!("bad state '{state}'").into(),
            )
        })?,
        error: r.get(5)?,
        started_unix: r.get(6)?,
        updated_unix: r.get(7)?,
    })
}

impl Db {
    /// Starts a journal entry.
    pub fn journal_begin(&self, e: &JournalEntry) -> Result<(), StorageError> {
        self.conn().execute(
            &format!("INSERT INTO install_journal ({JOURNAL_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"),
            params![e.id, e.component, e.target, e.staging, e.state.as_str(), e.error, e.started_unix, e.updated_unix],
        )?;
        Ok(())
    }

    /// Moves a transaction to a new state. Returns whether it existed.
    pub fn journal_set(
        &self,
        id: &str,
        state: TxnState,
        error: Option<&str>,
        at_unix: i64,
    ) -> Result<bool, StorageError> {
        Ok(self.conn().execute(
            "UPDATE install_journal SET state = ?2, error = COALESCE(?3, error), updated_unix = ?4 WHERE id = ?1",
            params![id, state.as_str(), error, at_unix],
        )? == 1)
    }

    /// Transactions that did not finish (for recovery after a crash), oldest first.
    pub fn journal_unfinished(&self) -> Result<Vec<JournalEntry>, StorageError> {
        let mut stmt = self.conn().prepare(&format!(
            "SELECT {JOURNAL_COLUMNS} FROM install_journal
             WHERE state NOT IN ('committed','failed','rolled-back') ORDER BY started_unix, id"
        ))?;
        let rows = stmt.query_map([], journal_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// One journal entry.
    pub fn journal_entry(&self, id: &str) -> Result<Option<JournalEntry>, StorageError> {
        Ok(self
            .conn()
            .query_row(
                &format!("SELECT {JOURNAL_COLUMNS} FROM install_journal WHERE id = ?1"),
                [id],
                journal_row,
            )
            .optional()?)
    }

    /// Records that the user accepted a licence text (identified by its SHA-256).
    pub fn accept_licence(
        &self,
        licence_id: &str,
        text_sha256: &str,
        client_id: Option<&str>,
        at_unix: i64,
    ) -> Result<(), StorageError> {
        self.conn().execute(
            "INSERT INTO licence_acceptances (licence_id, text_sha256, accepted_unix, client_id) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(licence_id, text_sha256) DO UPDATE SET accepted_unix = excluded.accepted_unix, client_id = excluded.client_id",
            params![licence_id, text_sha256, at_unix, client_id],
        )?;
        Ok(())
    }

    /// Was exactly this licence text accepted?
    pub fn licence_accepted(
        &self,
        licence_id: &str,
        text_sha256: &str,
    ) -> Result<bool, StorageError> {
        Ok(self
            .conn()
            .query_row(
                "SELECT 1 FROM licence_acceptances WHERE licence_id = ?1 AND text_sha256 = ?2",
                params![licence_id, text_sha256],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Appends to the audit log.
    pub fn audit(
        &self,
        at_unix: i64,
        client_id: Option<&str>,
        action: &str,
        detail: Option<&str>,
    ) -> Result<(), StorageError> {
        self.conn().execute(
            "INSERT INTO audit_log (at_unix, client_id, action, detail) VALUES (?1, ?2, ?3, ?4)",
            params![at_unix, client_id, action, detail],
        )?;
        Ok(())
    }

    /// Number of audit entries with this action.
    pub fn audit_count(&self, action: &str) -> Result<i64, StorageError> {
        Ok(self.conn().query_row(
            "SELECT COUNT(*) FROM audit_log WHERE action = ?1",
            [action],
            |r| r.get(0),
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_db_path;

    fn entry(id: &str) -> JournalEntry {
        JournalEntry {
            id: id.into(),
            component: "jdk/temurin@17".into(),
            target: "D:\\x".into(),
            staging: Some("D:\\s".into()),
            state: TxnState::Started,
            error: None,
            started_unix: 1,
            updated_unix: 1,
        }
    }

    #[test]
    fn journal_lifecycle_and_unfinished_list() {
        let path = temp_db_path("journal");
        let db = Db::open(&path).unwrap();
        db.journal_begin(&entry("t1")).unwrap();
        db.journal_begin(&entry("t2")).unwrap();
        assert!(db.journal_set("t1", TxnState::Extracted, None, 2).unwrap());
        assert!(db
            .journal_set("t2", TxnState::Failed, Some("boom"), 3)
            .unwrap());
        let open = db.journal_unfinished().unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].state, TxnState::Extracted);
        assert_eq!(
            db.journal_entry("t2").unwrap().unwrap().error.as_deref(),
            Some("boom")
        );
        assert!(!db.journal_set("nope", TxnState::Failed, None, 4).unwrap());
        for s in [
            "started",
            "downloaded",
            "extracted",
            "committing",
            "committed",
            "failed",
            "rolled-back",
        ] {
            assert_eq!(TxnState::parse(s).unwrap().as_str(), s);
        }
    }

    #[test]
    fn licence_acceptance_is_bound_to_the_text() {
        let path = temp_db_path("licence");
        let db = Db::open(&path).unwrap();
        assert!(!db.licence_accepted("android-sdk-license", "aa").unwrap());
        db.accept_licence("android-sdk-license", "aa", Some("admin"), 5)
            .unwrap();
        assert!(db.licence_accepted("android-sdk-license", "aa").unwrap());
        assert!(
            !db.licence_accepted("android-sdk-license", "bb").unwrap(),
            "changed text needs a new acceptance"
        );
        db.audit(
            5,
            Some("admin"),
            "licence.accept",
            Some("android-sdk-license"),
        )
        .unwrap();
        assert_eq!(db.audit_count("licence.accept").unwrap(), 1);
    }
}
