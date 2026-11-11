//! Basic health checks reported by `core.doctor` / `umr doctor`.
//!
//! This is the CLI-001 baseline. The full environment scan (JDK, Android SDK, Flutter, Node,
//! virtualization, conflicts) arrives with ENV-001…006 and will extend this report.

use std::path::Path;

use umr_api::methods::{Check, CheckStatus, DoctorReport};

/// Runs all basic checks. `db` is the Core's open database (absent in some unit tests).
#[must_use]
pub fn run_checks(data_root: &Path, db: Option<&umr_storage::Db>) -> DoctorReport {
    let mut report = DoctorReport {
        checks: vec![
            Check {
                id: "core.running".into(),
                title: "Runtime Core".into(),
                status: CheckStatus::Ok,
                detail: format!(
                    "running, version {}, pid {}",
                    crate::VERSION,
                    std::process::id()
                ),
            },
            windows_version_check(),
            long_paths_check(),
            data_root_check(data_root),
        ],
    };
    if let Some(db) = db {
        report.checks.push(storage_check(db));
    }
    report
}

fn storage_check(db: &umr_storage::Db) -> Check {
    let title = "Database".to_string();
    let id = "data.database".to_string();
    match (db.schema_version(), db.quick_check()) {
        (Ok(v), Ok(true)) => Check {
            id,
            title,
            status: CheckStatus::Ok,
            detail: format!("schema v{v}, integrity ok ({})", db.path().display()),
        },
        (Ok(v), Ok(false)) => Check {
            id,
            title,
            status: CheckStatus::Fail,
            detail: format!(
                "schema v{v}, integrity check FAILED ({})",
                db.path().display()
            ),
        },
        (Err(e), _) | (_, Err(e)) => Check {
            id,
            title,
            status: CheckStatus::Fail,
            detail: e.to_string(),
        },
    }
}

#[cfg(windows)]
fn windows_version_check() -> Check {
    let title = "Windows 11".to_string();
    match umr_platform::os::windows_version() {
        Ok(v) if v.is_windows_11_or_later() => Check {
            id: "os.windows11".into(),
            title,
            status: CheckStatus::Ok,
            detail: format!(
                "build {}{}",
                v.build,
                v.display_version
                    .map(|d| format!(" ({d})"))
                    .unwrap_or_default()
            ),
        },
        Ok(v) => Check {
            id: "os.windows11".into(),
            title,
            status: CheckStatus::Fail,
            detail: format!(
                "build {} is older than Windows 11 (22000); only Windows 11 is supported",
                v.build
            ),
        },
        Err(e) => Check {
            id: "os.windows11".into(),
            title,
            status: CheckStatus::Warn,
            detail: format!("cannot read Windows version: {e}"),
        },
    }
}

#[cfg(windows)]
fn long_paths_check() -> Check {
    let title = "Long path support".to_string();
    match umr_platform::os::long_paths_enabled() {
        Ok(true) => Check { id: "os.long_paths".into(), title, status: CheckStatus::Ok, detail: "enabled".into() },
        Ok(false) => Check {
            id: "os.long_paths".into(),
            title,
            status: CheckStatus::Warn,
            detail: "disabled; deep Gradle/NDK build paths may fail (can be enabled later with your consent)".into(),
        },
        Err(e) => Check { id: "os.long_paths".into(), title, status: CheckStatus::Warn, detail: format!("cannot read setting: {e}") },
    }
}

#[cfg(not(windows))]
fn windows_version_check() -> Check {
    Check {
        id: "os.windows11".into(),
        title: "Windows 11".into(),
        status: CheckStatus::Fail,
        detail: "only Windows 11 is supported".into(),
    }
}

#[cfg(not(windows))]
fn long_paths_check() -> Check {
    Check {
        id: "os.long_paths".into(),
        title: "Long path support".into(),
        status: CheckStatus::Ok,
        detail: "not applicable".into(),
    }
}

fn data_root_check(root: &Path) -> Check {
    let title = "Data folder writable".to_string();
    let probe = root.join("run").join(".write-probe");
    let result = std::fs::write(&probe, b"ok").and_then(|()| std::fs::remove_file(&probe));
    match result {
        Ok(()) => Check {
            id: "data.writable".into(),
            title,
            status: CheckStatus::Ok,
            detail: root.display().to_string(),
        },
        Err(e) => Check {
            id: "data.writable".into(),
            title,
            status: CheckStatus::Fail,
            detail: format!("{}: {e}", root.display()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempRoot;

    #[test]
    fn report_has_core_os_and_data_checks() {
        let tmp = TempRoot::new("doctor");
        std::fs::create_dir_all(tmp.path().join("run")).unwrap();
        let db = umr_storage::Db::open_in(tmp.path()).unwrap();
        let r = run_checks(tmp.path(), Some(&db));
        let ids: Vec<_> = r.checks.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "core.running",
                "os.windows11",
                "os.long_paths",
                "data.writable",
                "data.database"
            ]
        );
        assert_eq!(r.checks[3].status, CheckStatus::Ok);
        assert_eq!(
            r.checks[4].status,
            CheckStatus::Ok,
            "{}",
            r.checks[4].detail
        );
    }

    #[test]
    fn unwritable_root_fails() {
        let r = data_root_check(Path::new(r"Z:\definitely\missing\umr"));
        assert_eq!(r.status, CheckStatus::Fail);
    }
}
