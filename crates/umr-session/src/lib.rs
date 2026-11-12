//! Runtime session state machine (Phase 2 step 2.6, architecture §4.7).
//!
//! A session binds a device profile (and later a project + framework adapter) to one running
//! emulator. Every state change goes through [`next`] (invalid ones are refused) and is written
//! to the recovery journal (`umr-storage::sessions`) before it takes effect for clients.
//!
//! ```text
//! Idle ─start→ Preparing ─env_ready→ Booting ─device_online→ Building ─artifact_ready→ Launching ─app_foreground→ Running
//!              │ env_missing                  │ device_failed   │ build_failed          │ launch_failed            │ file_changed(hot) ⇄ Reloading
//!              ▼                              ▼                 ▼                       ▼                          │ process_died → Crashed ─restart→ Launching
//!          EnvBlocked                    DeviceError       BuildFailed            LaunchFailed                     │ device_failed → DeviceError ─recover→ Booting
//! Building ─nothing_to_launch→ Running (device-only session); any active state ─stop→ Stopped
//! ```
//!
//! Recovery: emulators live in the Core's Job Objects, so they end with the Core. On start the
//! Core moves every session that was still active to `DeviceError` ([`Event::CoreRestarted`]);
//! the next `session.start` for the same profile recovers it (`recover` → Booting).

use serde::{Deserialize, Serialize};
use umr_storage::sessions::SessionRow;
use umr_storage::{Db, StorageError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Idle,
    Preparing,
    EnvBlocked,
    Booting,
    Building,
    BuildFailed,
    Launching,
    LaunchFailed,
    Running,
    Reloading,
    Crashed,
    DeviceError,
    Stopped,
}

impl State {
    pub const ALL: [Self; 13] = [
        Self::Idle,
        Self::Preparing,
        Self::EnvBlocked,
        Self::Booting,
        Self::Building,
        Self::BuildFailed,
        Self::Launching,
        Self::LaunchFailed,
        Self::Running,
        Self::Reloading,
        Self::Crashed,
        Self::DeviceError,
        Self::Stopped,
    ];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Preparing => "preparing",
            Self::EnvBlocked => "env_blocked",
            Self::Booting => "booting",
            Self::Building => "building",
            Self::BuildFailed => "build_failed",
            Self::Launching => "launching",
            Self::LaunchFailed => "launch_failed",
            Self::Running => "running",
            Self::Reloading => "reloading",
            Self::Crashed => "crashed",
            Self::DeviceError => "device_error",
            Self::Stopped => "stopped",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|x| x.as_str() == s)
    }

    /// Not idle and not stopped.
    #[must_use]
    pub fn is_active(self) -> bool {
        !matches!(self, Self::Idle | Self::Stopped)
    }

    /// A device is (or should be) up: input, screenshots and app commands make sense.
    #[must_use]
    pub fn has_device(self) -> bool {
        matches!(
            self,
            Self::Building
                | Self::BuildFailed
                | Self::Launching
                | Self::LaunchFailed
                | Self::Running
                | Self::Reloading
                | Self::Crashed
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    Start,
    EnvMissing,
    EnvReady,
    DeviceOnline,
    DeviceFailed,
    BuildFailed,
    /// A changed file needs a rebuild (also leaves `BuildFailed`).
    Rebuild,
    ArtifactReady,
    /// Device-only session: nothing to build or launch.
    NothingToLaunch,
    AppForeground,
    LaunchFailed,
    /// A changed file the framework can hot-reload.
    HotChange,
    Reloaded,
    ProcessDied,
    Restart,
    Recover,
    CoreRestarted,
    Stop,
}

impl Event {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::EnvMissing => "env_missing",
            Self::EnvReady => "env_ready",
            Self::DeviceOnline => "device_online",
            Self::DeviceFailed => "device_failed",
            Self::BuildFailed => "build_failed",
            Self::Rebuild => "rebuild",
            Self::ArtifactReady => "artifact_ready",
            Self::NothingToLaunch => "nothing_to_launch",
            Self::AppForeground => "app_foreground",
            Self::LaunchFailed => "launch_failed",
            Self::HotChange => "hot_change",
            Self::Reloaded => "reloaded",
            Self::ProcessDied => "process_died",
            Self::Restart => "restart",
            Self::Recover => "recover",
            Self::CoreRestarted => "core_restarted",
            Self::Stop => "stop",
        }
    }
}

/// The transition table. `None` = the event is not valid in this state.
#[must_use]
#[allow(clippy::match_same_arms)] // one row per transition reads like the diagram
pub fn next(s: State, e: Event) -> Option<State> {
    use Event as E;
    use State as S;
    Some(match (s, e) {
        (S::Idle | S::Stopped, E::Start) => S::Preparing,
        (S::Preparing, E::EnvMissing) => S::EnvBlocked,
        (S::EnvBlocked, E::EnvReady) => S::Preparing,
        (S::Preparing, E::EnvReady) => S::Booting,
        (S::Booting, E::DeviceOnline) => S::Building,
        (S::Building, E::BuildFailed) => S::BuildFailed,
        (S::BuildFailed | S::Running, E::Rebuild) => S::Building,
        (S::Building, E::ArtifactReady) => S::Launching,
        (S::Building, E::NothingToLaunch) => S::Running,
        (S::Launching, E::AppForeground) => S::Running,
        (S::Launching, E::LaunchFailed) => S::LaunchFailed,
        (S::LaunchFailed | S::Crashed | S::Running, E::Restart) => S::Launching,
        (S::Running, E::HotChange) => S::Reloading,
        (S::Reloading, E::Reloaded) => S::Running,
        (S::Running | S::Reloading, E::ProcessDied) => S::Crashed,
        (S::DeviceError, E::Recover) => S::Booting,
        (s, E::DeviceFailed) if s.has_device() || s == S::Booting => S::DeviceError,
        (s, E::CoreRestarted) if s.is_active() => S::DeviceError,
        (s, E::Stop) if s.is_active() => S::Stopped,
        _ => return None,
    })
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0))
}

/// A new, unique session id (`s-<unix ms hex>-<counter>`).
#[must_use]
pub fn new_id() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    format!("s-{ms:x}-{}", N.fetch_add(1, Ordering::Relaxed))
}

#[derive(Debug)]
pub enum SessionError {
    Invalid { state: State, event: Event },
    Storage(StorageError),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid { state, event } => write!(
                f,
                "'{}' is not possible while the session is {}",
                event.as_str(),
                state.as_str()
            ),
            Self::Storage(e) => write!(f, "session journal: {e}"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<StorageError> for SessionError {
    fn from(e: StorageError) -> Self {
        Self::Storage(e)
    }
}

/// A session and its journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Session {
    pub id: String,
    pub profile: String,
    pub avd: String,
    pub project: Option<String>,
    pub app: Option<String>,
    pub state: State,
    pub serial: Option<String>,
    pub detail: Option<String>,
    pub created_unix: i64,
    pub updated_unix: i64,
}

impl Session {
    /// Creates an `Idle` session and stores it.
    pub fn create(
        db: &Db,
        profile: &str,
        avd: &str,
        project: Option<&str>,
    ) -> Result<Self, SessionError> {
        let at = now_unix();
        let s = Self {
            id: new_id(),
            profile: profile.into(),
            avd: avd.into(),
            project: project.map(Into::into),
            app: None,
            state: State::Idle,
            serial: None,
            detail: None,
            created_unix: at,
            updated_unix: at,
        };
        db.insert_session(&s.to_row())?;
        Ok(s)
    }

    #[must_use]
    pub fn to_row(&self) -> SessionRow {
        SessionRow {
            id: self.id.clone(),
            profile: self.profile.clone(),
            avd: self.avd.clone(),
            project: self.project.clone(),
            app: self.app.clone(),
            state: self.state.as_str().into(),
            serial: self.serial.clone(),
            detail: self.detail.clone(),
            created_unix: self.created_unix,
            updated_unix: self.updated_unix,
        }
    }

    /// From a journal row (unknown states read as `DeviceError`).
    #[must_use]
    pub fn from_row(r: SessionRow) -> Self {
        Self {
            state: State::parse(&r.state).unwrap_or(State::DeviceError),
            id: r.id,
            profile: r.profile,
            avd: r.avd,
            project: r.project,
            app: r.app,
            serial: r.serial,
            detail: r.detail,
            created_unix: r.created_unix,
            updated_unix: r.updated_unix,
        }
    }

    /// Binds the project this session runs.
    pub fn set_project(&mut self, db: &Db, project: &str) -> Result<(), SessionError> {
        db.set_session_project(&self.id, project)?;
        self.project = Some(project.to_string());
        Ok(())
    }

    /// Applies an event: validated, journaled, then applied.
    pub fn apply(
        &mut self,
        db: &mut Db,
        event: Event,
        detail: Option<&str>,
    ) -> Result<State, SessionError> {
        let to = next(self.state, event).ok_or(SessionError::Invalid {
            state: self.state,
            event,
        })?;
        let at = now_unix();
        db.record_transition(
            &self.id,
            self.state.as_str(),
            event.as_str(),
            to.as_str(),
            detail,
            self.serial.as_deref(),
            self.app.as_deref(),
            at,
        )?;
        self.state = to;
        self.detail = detail.map(Into::into);
        self.updated_unix = at;
        Ok(to)
    }
}

/// Core start: every session still active lost its device with the previous Core.
/// Returns the sessions that were settled.
pub fn recover_after_restart(db: &mut Db) -> Result<Vec<Session>, SessionError> {
    let mut out = Vec::new();
    for row in db.active_sessions()? {
        let mut s = Session::from_row(row);
        if s.state != State::DeviceError {
            let was = s.state.as_str();
            s.apply(
                db,
                Event::CoreRestarted,
                Some(&format!(
                    "the Runtime Core stopped while the session was {was}; its emulator ended with it"
                )),
            )?;
        }
        out.push(s);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use Event as E;
    use State as S;

    fn db(tag: &str) -> (Db, std::path::PathBuf) {
        let d = std::env::temp_dir().join(format!("umr-session-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        (Db::open_in(&d).unwrap(), d)
    }

    #[test]
    fn happy_paths() {
        let mut s = S::Idle;
        for (e, want) in [
            (E::Start, S::Preparing),
            (E::EnvReady, S::Booting),
            (E::DeviceOnline, S::Building),
            (E::ArtifactReady, S::Launching),
            (E::AppForeground, S::Running),
            (E::HotChange, S::Reloading),
            (E::Reloaded, S::Running),
            (E::ProcessDied, S::Crashed),
            (E::Restart, S::Launching),
            (E::AppForeground, S::Running),
            (E::Stop, S::Stopped),
            (E::Start, S::Preparing),
        ] {
            s = next(s, e).unwrap_or_else(|| panic!("{s:?} + {e:?}"));
            assert_eq!(s, want);
        }
        // device-only session
        assert_eq!(next(S::Building, E::NothingToLaunch), Some(S::Running));
    }

    #[test]
    fn errors_and_recovery() {
        assert_eq!(next(S::Preparing, E::EnvMissing), Some(S::EnvBlocked));
        assert_eq!(next(S::EnvBlocked, E::EnvReady), Some(S::Preparing));
        assert_eq!(next(S::Booting, E::DeviceFailed), Some(S::DeviceError));
        assert_eq!(next(S::Running, E::DeviceFailed), Some(S::DeviceError));
        assert_eq!(next(S::DeviceError, E::Recover), Some(S::Booting));
        assert_eq!(next(S::Building, E::BuildFailed), Some(S::BuildFailed));
        assert_eq!(next(S::BuildFailed, E::Rebuild), Some(S::Building));
        assert_eq!(next(S::Launching, E::LaunchFailed), Some(S::LaunchFailed));
    }

    #[test]
    fn invalid_transitions_are_refused() {
        assert_eq!(next(S::Idle, E::Stop), None, "nothing to stop");
        assert_eq!(next(S::Stopped, E::Stop), None);
        assert_eq!(next(S::Idle, E::DeviceOnline), None);
        assert_eq!(next(S::Preparing, E::DeviceFailed), None, "no device yet");
        assert_eq!(next(S::Running, E::Start), None);
        // every active state can be stopped
        for st in State::ALL.into_iter().filter(|s| s.is_active()) {
            assert_eq!(next(st, E::Stop), Some(S::Stopped), "{st:?}");
        }
        for st in State::ALL {
            assert_eq!(State::parse(st.as_str()), Some(st));
        }
    }

    #[test]
    fn journaled_session_and_restart_recovery() {
        let (mut db, dir) = db("journal");
        let mut a = Session::create(&db, "standard", "umr_standard_api36", None).unwrap();
        a.apply(&mut db, E::Start, None).unwrap();
        a.apply(&mut db, E::EnvReady, None).unwrap();
        a.serial = Some("emulator-5554".into());
        a.apply(&mut db, E::DeviceOnline, Some("warm boot 7 s"))
            .unwrap();
        a.apply(&mut db, E::NothingToLaunch, None).unwrap();
        let err = a.apply(&mut db, E::Start, None).unwrap_err();
        assert!(err
            .to_string()
            .contains("not possible while the session is running"));

        let mut b = Session::create(&db, "fold", "umr_fold_api36", None).unwrap();
        b.apply(&mut db, E::Start, None).unwrap();
        b.apply(&mut db, E::Stop, None).unwrap();

        // the Core restarts: only the running session is settled
        let rec = recover_after_restart(&mut db).unwrap();
        assert_eq!(rec.len(), 1);
        assert_eq!(
            (rec[0].id.as_str(), rec[0].state),
            (a.id.as_str(), S::DeviceError)
        );
        assert!(rec[0]
            .detail
            .as_deref()
            .unwrap()
            .contains("while the session was running"));
        let stored = Session::from_row(db.session(&a.id).unwrap().unwrap());
        assert_eq!(stored.state, S::DeviceError);
        assert_eq!(stored.serial.as_deref(), Some("emulator-5554"));
        let ev = db.session_events(&a.id, 10).unwrap();
        assert_eq!(
            ev.iter().map(|e| e.event.as_str()).collect::<Vec<_>>(),
            [
                "start",
                "env_ready",
                "device_online",
                "nothing_to_launch",
                "core_restarted"
            ]
        );
        // a second restart changes nothing more
        assert_eq!(
            recover_after_restart(&mut db).unwrap()[0].state,
            S::DeviceError
        );
        assert_eq!(db.session_events(&a.id, 10).unwrap().len(), 5);
        drop(db);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn ids_are_unique() {
        assert_ne!(new_id(), new_id());
    }
}
