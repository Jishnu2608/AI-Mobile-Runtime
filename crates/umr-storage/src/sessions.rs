//! Session recovery journal (step 2.6): one row per runtime session plus every state change.
//! On start the Core reads the sessions that were still active and settles them (their
//! emulators ended with the previous Core, because they lived in its Job Objects).

use rusqlite::{params, OptionalExtension, Row};
use serde::Serialize;

use crate::db::{Db, StorageError};

/// States that count as finished (everything else is active).
pub const FINISHED_STATES: [&str; 2] = ["idle", "stopped"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionRow {
    pub id: String,
    pub profile: String,
    pub avd: String,
    pub project: Option<String>,
    /// Android package the session runs, if any.
    pub app: Option<String>,
    pub state: String,
    pub serial: Option<String>,
    pub detail: Option<String>,
    pub created_unix: i64,
    pub updated_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionEvent {
    pub seq: i64,
    pub at_unix: i64,
    pub from_state: String,
    pub event: String,
    pub to_state: String,
    pub detail: Option<String>,
}

fn row(r: &Row<'_>) -> rusqlite::Result<SessionRow> {
    Ok(SessionRow {
        id: r.get(0)?,
        profile: r.get(1)?,
        avd: r.get(2)?,
        project: r.get(3)?,
        app: r.get(4)?,
        state: r.get(5)?,
        serial: r.get(6)?,
        detail: r.get(7)?,
        created_unix: r.get(8)?,
        updated_unix: r.get(9)?,
    })
}

const COLS: &str =
    "id, profile, avd, project, app, state, serial, detail, created_unix, updated_unix";

impl Db {
    pub fn insert_session(&self, s: &SessionRow) -> Result<(), StorageError> {
        self.conn().execute(
            &format!(
                "INSERT INTO sessions ({COLS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"
            ),
            params![
                s.id,
                s.profile,
                s.avd,
                s.project,
                s.app,
                s.state,
                s.serial,
                s.detail,
                s.created_unix,
                s.updated_unix
            ],
        )?;
        Ok(())
    }

    /// Records one transition and updates the session row in one transaction.
    #[allow(clippy::too_many_arguments)] // one journal row: who, from, event, to, details, when
    pub fn record_transition(
        &mut self,
        id: &str,
        from: &str,
        event: &str,
        to: &str,
        detail: Option<&str>,
        serial: Option<&str>,
        app: Option<&str>,
        at_unix: i64,
    ) -> Result<(), StorageError> {
        let tx = self.conn_mut().transaction()?;
        let n = tx.execute(
            "UPDATE sessions SET state = ?2, detail = ?3, serial = COALESCE(?4, serial), app = COALESCE(?5, app), updated_unix = ?6 WHERE id = ?1",
            params![id, to, detail, serial, app, at_unix],
        )?;
        if n != 1 {
            return Err(StorageError::Invalid(format!("no session {id}")));
        }
        tx.execute(
            "INSERT INTO session_events (session_id, at_unix, from_state, event, to_state, detail) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, at_unix, from, event, to, detail],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Binds a project folder to a session (a device session that starts running a project).
    /// Moves a session to another device profile (PRV-007 device switch).
    pub fn set_session_device(
        &self,
        id: &str,
        profile: &str,
        avd: &str,
    ) -> Result<(), StorageError> {
        let n = self.conn().execute(
            "UPDATE sessions SET profile = ?2, avd = ?3, serial = NULL, app = NULL WHERE id = ?1",
            params![id, profile, avd],
        )?;
        if n == 1 {
            Ok(())
        } else {
            Err(StorageError::Invalid(format!("no session {id}")))
        }
    }

    pub fn set_session_project(&self, id: &str, project: &str) -> Result<(), StorageError> {
        let n = self.conn().execute(
            "UPDATE sessions SET project = ?2 WHERE id = ?1",
            params![id, project],
        )?;
        if n == 1 {
            Ok(())
        } else {
            Err(StorageError::Invalid(format!("no session {id}")))
        }
    }

    pub fn session(&self, id: &str) -> Result<Option<SessionRow>, StorageError> {
        Ok(self
            .conn()
            .query_row(
                &format!("SELECT {COLS} FROM sessions WHERE id = ?1"),
                [id],
                row,
            )
            .optional()?)
    }

    /// Sessions that are not idle / stopped, newest first.
    pub fn active_sessions(&self) -> Result<Vec<SessionRow>, StorageError> {
        let mut st = self.conn().prepare(&format!(
            "SELECT {COLS} FROM sessions WHERE state NOT IN ('idle', 'stopped') ORDER BY created_unix DESC, id DESC"
        ))?;
        let v = st.query_map([], row)?.collect::<Result<Vec<_>, _>>()?;
        Ok(v)
    }

    /// Latest sessions of any state, newest first.
    pub fn recent_sessions(&self, limit: u32) -> Result<Vec<SessionRow>, StorageError> {
        let mut st = self.conn().prepare(&format!(
            "SELECT {COLS} FROM sessions ORDER BY created_unix DESC, id DESC LIMIT ?1"
        ))?;
        let v = st.query_map([limit], row)?.collect::<Result<Vec<_>, _>>()?;
        Ok(v)
    }

    /// The newest `limit` events of a session, oldest first.
    pub fn session_events(&self, id: &str, limit: u32) -> Result<Vec<SessionEvent>, StorageError> {
        let mut st = self.conn().prepare(
            "SELECT seq, at_unix, from_state, event, to_state, detail FROM session_events WHERE session_id = ?1 ORDER BY seq DESC LIMIT ?2",
        )?;
        let mut v = st
            .query_map(params![id, limit], |r| {
                Ok(SessionEvent {
                    seq: r.get(0)?,
                    at_unix: r.get(1)?,
                    from_state: r.get(2)?,
                    event: r.get(3)?,
                    to_state: r.get(4)?,
                    detail: r.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        v.reverse();
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db(tag: &str) -> (Db, std::path::PathBuf) {
        let d = std::env::temp_dir().join(format!("umr-storage-sess-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        (Db::open_in(&d).unwrap(), d)
    }

    fn new_row(id: &str, at: i64) -> SessionRow {
        SessionRow {
            id: id.into(),
            profile: "standard".into(),
            avd: "umr_standard_api36".into(),
            project: None,
            app: None,
            state: "idle".into(),
            serial: None,
            detail: None,
            created_unix: at,
            updated_unix: at,
        }
    }

    #[test]
    fn journal_records_transitions_and_finds_active_sessions() {
        let (mut db, dir) = db("j");
        db.insert_session(&new_row("s1", 10)).unwrap();
        db.insert_session(&new_row("s2", 20)).unwrap();
        db.record_transition("s1", "idle", "start", "preparing", None, None, None, 11)
            .unwrap();
        db.record_transition(
            "s1",
            "preparing",
            "env_ready",
            "booting",
            None,
            None,
            None,
            12,
        )
        .unwrap();
        db.record_transition(
            "s1",
            "booting",
            "device_online",
            "building",
            None,
            Some("emulator-5554"),
            None,
            13,
        )
        .unwrap();
        let s = db.session("s1").unwrap().unwrap();
        assert_eq!(
            (s.state.as_str(), s.serial.as_deref()),
            ("building", Some("emulator-5554"))
        );
        let active = db.active_sessions().unwrap();
        assert_eq!(
            active.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["s1"]
        );
        let ev = db.session_events("s1", 2).unwrap();
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[1].to_state, "building");
        assert!(db
            .record_transition("nope", "a", "b", "c", None, None, None, 1)
            .is_err());
        assert_eq!(db.recent_sessions(10).unwrap()[0].id, "s2");
        drop(db);
        let _ = std::fs::remove_dir_all(dir);
    }
}
