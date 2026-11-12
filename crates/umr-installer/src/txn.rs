//! INS-003: transactional installs with a crash-safe journal.
//!
//! Archive components (JDK, Node, Flutter, Android cmdline-tools):
//!
//! ```text
//! started ─download+verify→ downloaded ─extract to staging + check→ extracted
//!         ─record component, move staging into place→ committing ─→ committed
//! any error → staging deleted → failed
//! ```
//! The target folder is replaced by one directory rename on the same volume, so a reader
//! never sees a half-extracted component. A crash at any point is resolved by [`recover`] on
//! the next Core start: unfinished transactions are rolled back, a transaction that was
//! committing is completed. Downloads stay in the cache, so a retry resumes them.
//!
//! Android SDK packages are installed by Android CLI (`android --sdk=<root> sdk install <pkg>
//! --no-metrics`) in the isolated environment; afterwards the package folder is checked and
//! the component recorded.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use umr_storage::components::{Component, Source};
use umr_storage::install::{JournalEntry, TxnState};
use umr_storage::Db;

use crate::catalog::Artifact;
use crate::http::Http;
use crate::layout::Layout;
use crate::{now_unix, random_id, InstallError};

/// Progress stages reported to the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Downloading,
    Extracting,
    Committing,
    Installing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Installed,
    /// Target already present and complete; only the record was refreshed.
    AlreadyInstalled,
}

/// An archive component to install.
#[derive(Debug, Clone)]
pub struct ArchiveInstall {
    /// Component id, e.g. `jdk/temurin@17.0.21+9`.
    pub component_id: String,
    pub kind: String,
    pub artifact: Artifact,
    /// Final folder (inside the managed root).
    pub target: PathBuf,
    pub strip_top: bool,
    /// File that must exist below the target afterwards (`bin\java.exe`).
    pub check: PathBuf,
}

pub type StageProgress<'a> = &'a mut dyn FnMut(Stage, u64, Option<u64>);

fn lock(db: &Mutex<Db>) -> std::sync::MutexGuard<'_, Db> {
    db.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn set(db: &Mutex<Db>, id: &str, state: TxnState, error: Option<&str>) -> Result<(), InstallError> {
    lock(db).journal_set(id, state, error, now_unix())?;
    Ok(())
}

/// Windows can briefly lock fresh files (antivirus, indexer): retry renames a few times.
fn rename_retry(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut last = None;
    for i in 0..10 {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(Duration::from_millis(100 * (i + 1)));
            }
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("rename failed")))
}

fn old_path(target: &Path, txn: &str) -> PathBuf {
    let mut s = target.as_os_str().to_os_string();
    s.push(format!(".old-{txn}"));
    PathBuf::from(s)
}

fn dir_size(p: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![p.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(e.path()),
                Ok(_) => total += e.metadata().map_or(0, |m| m.len()),
                Err(_) => {}
            }
        }
    }
    total
}

/// Installs one archive component transactionally.
pub fn install_archive(
    http: &Http,
    layout: &Layout,
    db: &Mutex<Db>,
    job: &ArchiveInstall,
    progress: StageProgress<'_>,
    cancel: &AtomicBool,
) -> Result<Outcome, InstallError> {
    if !layout.contains(&job.target) {
        return Err(InstallError::Io(format!(
            "{} is outside the managed folder",
            job.target.display()
        )));
    }
    let record = |size: u64| Component {
        id: job.component_id.clone(),
        kind: job.kind.clone(),
        version: job.artifact.version.clone(),
        path: job.target.display().to_string(),
        source: Source::Managed,
        sha256: job.artifact.checksum.sha256().map(String::from),
        size_bytes: i64::try_from(size).ok(),
        installed_unix: now_unix(),
        verified_unix: Some(now_unix()),
    };
    if job.target.join(&job.check).is_file() {
        lock(db).upsert_component(&record(dir_size(&job.target)))?;
        return Ok(Outcome::AlreadyInstalled);
    }

    let txn = random_id();
    let staging = layout.staging().join(&txn);
    lock(db).journal_begin(&JournalEntry {
        id: txn.clone(),
        component: job.component_id.clone(),
        target: job.target.display().to_string(),
        staging: Some(staging.display().to_string()),
        state: TxnState::Started,
        error: None,
        started_unix: now_unix(),
        updated_unix: now_unix(),
    })?;

    let result = (|| -> Result<(), InstallError> {
        // 1. download + verify (INS-001/002)
        let archive = layout.downloads().join(&job.artifact.file_name);
        http.download(
            &job.artifact.url,
            &archive,
            &job.artifact.checksum,
            job.artifact.size,
            &mut |d, t| progress(Stage::Downloading, d, t),
            cancel,
        )?;
        set(db, &txn, TxnState::Downloaded, None)?;

        // 2. extract into staging and check
        progress(Stage::Extracting, 0, None);
        let _ = std::fs::remove_dir_all(&staging);
        let size = crate::verify::extract_zip(&archive, &staging, job.strip_top, cancel)?;
        if !staging.join(&job.check).is_file() {
            return Err(InstallError::Extract(format!(
                "{} missing after extraction",
                job.check.display()
            )));
        }
        set(db, &txn, TxnState::Extracted, None)?;
        if cancel.load(Ordering::Relaxed) {
            return Err(InstallError::Cancelled);
        }

        // 3. commit: record, then move into place (recover() completes this after a crash)
        progress(Stage::Committing, 0, None);
        set(db, &txn, TxnState::Committing, None)?;
        lock(db).upsert_component(&record(size))?;
        if let Some(parent) = job.target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let old = old_path(&job.target, &txn);
        if job.target.exists() {
            rename_retry(&job.target, &old)?; // incomplete leftover without the check file
        }
        rename_retry(&staging, &job.target)?;
        let _ = std::fs::remove_dir_all(&old);
        set(db, &txn, TxnState::Committed, None)?;
        let _ = std::fs::remove_file(&archive); // installed: free the disk space
        Ok(())
    })();

    match result {
        Ok(()) => {
            let _ = lock(db).audit(
                now_unix(),
                None,
                "install.component",
                Some(&job.component_id),
            );
            Ok(Outcome::Installed)
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            // If the record was already written but the move failed, remove it again.
            if !job.target.join(&job.check).is_file() {
                let _ = lock(db).remove_component(&job.component_id);
            }
            let _ = set(db, &txn, TxnState::Failed, Some(&e.to_string()));
            Err(e)
        }
    }
}

/// What [`recover`] did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Recovery {
    pub completed: Vec<String>,
    pub rolled_back: Vec<String>,
}

/// Finishes or rolls back transactions left by a crash. Call once at Core start, before any
/// new install runs.
pub fn recover(layout: &Layout, db: &Mutex<Db>) -> Result<Recovery, InstallError> {
    let mut out = Recovery::default();
    let open = lock(db).journal_unfinished()?;
    for e in open {
        let staging = e.staging.as_ref().map(PathBuf::from);
        let target = PathBuf::from(&e.target);
        let old = old_path(&target, &e.id);
        let completed = if e.state == TxnState::Committing {
            match &staging {
                Some(s) if s.is_dir() => {
                    if target.exists() && !old.exists() {
                        let _ = rename_retry(&target, &old);
                    }
                    if let Some(parent) = target.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    !target.exists() && rename_retry(s, &target).is_ok()
                }
                _ => target.is_dir(),
            }
        } else {
            false
        };
        let _ = std::fs::remove_dir_all(&old);
        if completed {
            set(db, &e.id, TxnState::Committed, None)?;
            out.completed.push(e.component);
        } else {
            if let Some(s) = &staging {
                let _ = std::fs::remove_dir_all(s);
            }
            if e.state == TxnState::Committing {
                lock(db).remove_component(&e.component)?;
            }
            set(
                db,
                &e.id,
                TxnState::RolledBack,
                Some("interrupted; rolled back at start"),
            )?;
            out.rolled_back.push(e.component);
        }
    }
    // Nothing is in progress now: clear any leftover staging folders.
    for d in std::fs::read_dir(layout.staging())
        .into_iter()
        .flatten()
        .flatten()
    {
        let _ = std::fs::remove_dir_all(d.path());
    }
    Ok(out)
}

/// An Android SDK package installed through Android CLI.
#[derive(Debug, Clone)]
pub struct SdkPackageInstall {
    /// `platforms/android-36`
    pub package: String,
    pub sdk_root: PathBuf,
    /// `<sdk>\cmdline-tools\latest\bin\android.exe`
    pub android_exe: PathBuf,
    /// Isolated child environment (umr-env recipe).
    pub env: HashMap<String, String>,
    pub log: PathBuf,
    pub timeout: Duration,
}

/// Arguments for Android CLI (`--no-metrics` always).
#[must_use]
pub fn android_cli_args(sdk_root: &Path, package: &str) -> Vec<String> {
    vec![
        format!("--sdk={}", sdk_root.display()),
        "sdk".into(),
        "install".into(),
        package.into(),
        "--no-metrics".into(),
    ]
}

/// Package folder looks installed (Android CLI / sdkmanager write `package.xml`).
fn package_present(dir: &Path) -> bool {
    dir.join("package.xml").is_file() || dir.join("source.properties").is_file()
}

/// Installs one SDK package with Android CLI. The licence gate must have passed before.
pub fn install_sdk_package(
    db: &Mutex<Db>,
    job: &SdkPackageInstall,
    progress: StageProgress<'_>,
    cancel: &AtomicBool,
) -> Result<Outcome, InstallError> {
    let dir = job.sdk_root.join(job.package.replace('/', "\\"));
    let component_id = format!("android/{}", job.package);
    let record = |version: String| Component {
        id: component_id.clone(),
        kind: "android-sdk-package".into(),
        version,
        path: dir.display().to_string(),
        source: Source::Managed,
        sha256: None,
        size_bytes: i64::try_from(dir_size(&dir)).ok(),
        installed_unix: now_unix(),
        verified_unix: Some(now_unix()),
    };
    let version_of = || {
        umr_env::android::list_packages(&job.sdk_root)
            .into_iter()
            .find(|p| p.path == job.package)
            .and_then(|p| p.version)
            .unwrap_or_default()
    };
    if package_present(&dir) {
        lock(db).upsert_component(&record(version_of()))?;
        return Ok(Outcome::AlreadyInstalled);
    }
    if !job.android_exe.is_file() {
        return Err(InstallError::Tool(format!(
            "Android CLI not found at {}; install the Android SDK command-line tools first",
            job.android_exe.display()
        )));
    }
    let txn = random_id();
    lock(db).journal_begin(&JournalEntry {
        id: txn.clone(),
        component: component_id.clone(),
        target: dir.display().to_string(),
        staging: None,
        state: TxnState::Started,
        error: None,
        started_unix: now_unix(),
        updated_unix: now_unix(),
    })?;
    progress(Stage::Installing, 0, None);
    let result = run_android_cli(job, cancel);
    // INS-007: note the runtime Android CLI downloaded for itself, and drop any metrics spool.
    if let Some(home) = job.env.get("ANDROID_USER_HOME").map(PathBuf::from) {
        let _ = crate::manage::record_cli_runtime(db, &home);
        let purged = crate::manage::purge_cli_metrics(&home);
        if purged > 0 {
            let _ = lock(db).audit(
                now_unix(),
                None,
                "android-cli.metrics-purged",
                Some(&purged.to_string()),
            );
        }
    }
    let result = result.and_then(|()| {
        if package_present(&dir) {
            Ok(())
        } else {
            Err(InstallError::Tool(format!(
                "Android CLI finished but {} is missing (see {})",
                dir.display(),
                job.log.display()
            )))
        }
    });
    match result {
        Ok(()) => {
            lock(db).upsert_component(&record(version_of()))?;
            set(db, &txn, TxnState::Committed, None)?;
            let _ = lock(db).audit(now_unix(), None, "install.component", Some(&component_id));
            Ok(Outcome::Installed)
        }
        Err(e) => {
            let _ = set(db, &txn, TxnState::Failed, Some(&e.to_string()));
            Err(e)
        }
    }
}

fn run_android_cli(job: &SdkPackageInstall, cancel: &AtomicBool) -> Result<(), InstallError> {
    if let Some(d) = job.log.parent() {
        std::fs::create_dir_all(d)?;
    }
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&job.log)?;
    let args = android_cli_args(&job.sdk_root, &job.package);
    {
        use std::io::Write as _;
        let mut l = &log;
        let _ = writeln!(l, "\n> {} {}", job.android_exe.display(), args.join(" "));
    }
    let mut cmd = Command::new(&job.android_exe);
    cmd.args(&args)
        .envs(&job.env)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    let mut child = cmd
        .spawn()
        .map_err(|e| InstallError::Tool(format!("cannot start Android CLI: {e}")))?;
    let kill_job = umr_platform::job::KillOnCloseJob::new().ok();
    if let Some(j) = &kill_job {
        let _ = j.assign(&child);
    }
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err(InstallError::Tool(format!(
                    "Android CLI failed installing {} ({status}); see {}",
                    job.package,
                    job.log.display()
                )))
            };
        }
        let stop = if cancel.load(Ordering::Relaxed) {
            Some(InstallError::Cancelled)
        } else if started.elapsed() > job.timeout {
            Some(InstallError::Tool(format!(
                "Android CLI did not finish installing {} within {} min",
                job.package,
                job.timeout.as_secs() / 60
            )))
        } else {
            None
        };
        if let Some(e) = stop {
            if let Some(j) = &kill_job {
                let _ = j.terminate(1);
            }
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap as Map;

    use super::*;
    use crate::http::Policy;
    use crate::testkit::{make_zip, tmp_dir, Route, TestServer};
    use crate::verify::{sha256_hex, Checksum};

    struct Fx {
        dir: PathBuf,
        layout: Layout,
        db: Mutex<Db>,
        http: Http,
    }

    fn fx(tag: &str) -> Fx {
        let dir = tmp_dir(tag);
        let layout = Layout::new(&dir.join("root"));
        std::fs::create_dir_all(layout.root()).unwrap();
        let db = Mutex::new(Db::open_in(layout.root()).unwrap());
        let http = Http::new(Policy {
            hosts: vec![],
            allow_loopback_http: true,
            retries: 2,
            backoff: Duration::from_millis(5),
        })
        .unwrap();
        Fx {
            dir,
            layout,
            db,
            http,
        }
    }

    fn jdk_zip(dir: &Path) -> Vec<u8> {
        let z = make_zip(
            &dir.join("src.zip"),
            &[
                ("jdk-17.0.21+9/bin/java.exe", b"MZ".as_slice()),
                ("jdk-17.0.21+9/release", b"JAVA_VERSION=\"17.0.21\""),
            ],
        );
        std::fs::read(z).unwrap()
    }

    fn job(f: &Fx, url: String, sum: Checksum) -> ArchiveInstall {
        ArchiveInstall {
            component_id: "jdk/temurin@17.0.21+9".into(),
            kind: "jdk".into(),
            artifact: Artifact {
                url,
                file_name: "jdk.zip".into(),
                checksum: sum,
                size: None,
                version: "17.0.21+9".into(),
            },
            target: f.layout.jdks().join("temurin-17.0.21_9"),
            strip_top: true,
            check: PathBuf::from("bin").join("java.exe"),
        }
    }

    #[test]
    fn archive_install_commits_and_records() {
        let f = fx("txn-ok");
        let body = jdk_zip(&f.dir);
        let s = TestServer::start(Map::from([("/jdk.zip".to_string(), Route::body(&body))]));
        let j = job(&f, s.url("/jdk.zip"), Checksum::Sha256(sha256_hex(&body)));
        let mut stages = Vec::new();
        let out = install_archive(
            &f.http,
            &f.layout,
            &f.db,
            &j,
            &mut |st, _, _| stages.push(st),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(out, Outcome::Installed);
        assert!(j.target.join("bin").join("java.exe").is_file());
        assert!(stages.contains(&Stage::Downloading) && stages.contains(&Stage::Committing));
        let db = lock(&f.db);
        let c = db.component("jdk/temurin@17.0.21+9").unwrap().unwrap();
        assert_eq!(
            (c.source, c.sha256.as_deref()),
            (Source::Managed, Some(sha256_hex(&body).as_str()))
        );
        assert_eq!(db.journal_unfinished().unwrap(), Vec::new());
        assert_eq!(db.audit_count("install.component").unwrap(), 1);
        drop(db);
        assert_eq!(
            std::fs::read_dir(f.layout.staging()).unwrap().count(),
            0,
            "staging empty"
        );
        assert!(
            !f.layout.downloads().join("jdk.zip").exists(),
            "archive removed after install"
        );
        // second run: nothing downloaded again
        let before = s.requests.load(Ordering::SeqCst);
        let again = install_archive(
            &f.http,
            &f.layout,
            &f.db,
            &j,
            &mut |_, _, _| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(again, Outcome::AlreadyInstalled);
        assert_eq!(s.requests.load(Ordering::SeqCst), before);
    }

    #[test]
    fn failed_install_leaves_no_trace() {
        let f = fx("txn-bad");
        let body = jdk_zip(&f.dir);
        let s = TestServer::start(Map::from([("/jdk.zip".to_string(), Route::body(&body))]));
        let j = job(&f, s.url("/jdk.zip"), Checksum::Sha256("00".repeat(32)));
        let r = install_archive(
            &f.http,
            &f.layout,
            &f.db,
            &j,
            &mut |_, _, _| {},
            &AtomicBool::new(false),
        );
        assert!(matches!(r, Err(InstallError::Checksum { .. })));
        assert!(!j.target.exists());
        let db = lock(&f.db);
        assert!(db.component(&j.component_id).unwrap().is_none());
        assert_eq!(
            db.journal_unfinished().unwrap(),
            Vec::new(),
            "journal says failed"
        );
        // archive missing the check file
        drop(db);
        let bad = std::fs::read(make_zip(
            &f.dir.join("x.zip"),
            &[("top/readme.txt", b"no java".as_slice())],
        ))
        .unwrap();
        let s2 = TestServer::start(Map::from([("/x.zip".to_string(), Route::body(&bad))]));
        let mut j2 = job(&f, s2.url("/x.zip"), Checksum::Sha256(sha256_hex(&bad)));
        j2.artifact.file_name = "x.zip".into();
        let r = install_archive(
            &f.http,
            &f.layout,
            &f.db,
            &j2,
            &mut |_, _, _| {},
            &AtomicBool::new(false),
        );
        assert!(matches!(r, Err(InstallError::Extract(_))), "{r:?}");
        assert!(!j2.target.exists());
        assert_eq!(std::fs::read_dir(f.layout.staging()).unwrap().count(), 0);
    }

    #[test]
    fn refuses_targets_outside_the_managed_root() {
        let f = fx("txn-outside");
        let mut j = job(
            &f,
            "http://127.0.0.1:1/x".into(),
            Checksum::Sha1("0".into()),
        );
        j.target = f.dir.join("elsewhere");
        assert!(matches!(
            install_archive(
                &f.http,
                &f.layout,
                &f.db,
                &j,
                &mut |_, _, _| {},
                &AtomicBool::new(false)
            ),
            Err(InstallError::Io(_))
        ));
    }

    fn begin(f: &Fx, id: &str, comp: &str, target: &Path, staging: &Path, state: TxnState) {
        lock(&f.db)
            .journal_begin(&JournalEntry {
                id: id.into(),
                component: comp.into(),
                target: target.display().to_string(),
                staging: Some(staging.display().to_string()),
                state,
                error: None,
                started_unix: 1,
                updated_unix: 1,
            })
            .unwrap();
    }

    #[test]
    fn recovery_rolls_back_or_completes() {
        let f = fx("txn-recover");
        let st = f.layout.staging();
        // a) crashed while extracting → rolled back
        let sa = st.join("a");
        std::fs::create_dir_all(sa.join("bin")).unwrap();
        let ta = f.layout.node().join("24.15.0");
        begin(&f, "a", "node@24.15.0", &ta, &sa, TxnState::Extracted);
        // b) crashed after the record, before the move → completed
        let sb = st.join("b");
        std::fs::create_dir_all(sb.join("bin")).unwrap();
        std::fs::write(sb.join("bin").join("java.exe"), b"MZ").unwrap();
        let tb = f.layout.jdks().join("temurin-17");
        begin(&f, "b", "jdk/temurin@17", &tb, &sb, TxnState::Committing);
        // c) crashed after the move → completed
        let tc = f.layout.flutter().join("3.50.0");
        std::fs::create_dir_all(&tc).unwrap();
        begin(
            &f,
            "c",
            "flutter@3.50.0",
            &tc,
            &st.join("c"),
            TxnState::Committing,
        );
        // d) orphan staging folder without journal entry
        std::fs::create_dir_all(st.join("orphan")).unwrap();

        let r = recover(&f.layout, &f.db).unwrap();
        assert_eq!(r.rolled_back, vec!["node@24.15.0".to_string()]);
        assert_eq!(
            r.completed,
            vec!["jdk/temurin@17".to_string(), "flutter@3.50.0".to_string()]
        );
        assert!(!ta.exists() && tb.join("bin").join("java.exe").is_file() && tc.is_dir());
        assert_eq!(std::fs::read_dir(&st).unwrap().count(), 0);
        assert_eq!(lock(&f.db).journal_unfinished().unwrap(), Vec::new());
        // idempotent
        assert_eq!(recover(&f.layout, &f.db).unwrap(), Recovery::default());
    }

    #[test]
    fn android_cli_arguments_and_missing_cli() {
        let a = android_cli_args(Path::new(r"C:\UMR\android-sdk"), "platforms/android-36");
        assert_eq!(
            a,
            vec![
                r"--sdk=C:\UMR\android-sdk",
                "sdk",
                "install",
                "platforms/android-36",
                "--no-metrics"
            ]
        );
        let f = fx("txn-sdk");
        let j = SdkPackageInstall {
            package: "platform-tools".into(),
            sdk_root: f.layout.android_sdk(),
            android_exe: f.layout.android_sdk().join("missing.exe"),
            env: HashMap::new(),
            log: f.layout.logs().join("x.log"),
            timeout: Duration::from_secs(5),
        };
        assert!(matches!(
            install_sdk_package(&f.db, &j, &mut |_, _, _| {}, &AtomicBool::new(false)),
            Err(InstallError::Tool(_))
        ));
        // already present: recorded without running anything
        let dir = f.layout.android_sdk().join("platform-tools");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("package.xml"), "<x/>").unwrap();
        assert_eq!(
            install_sdk_package(&f.db, &j, &mut |_, _, _| {}, &AtomicBool::new(false)).unwrap(),
            Outcome::AlreadyInstalled
        );
        assert!(lock(&f.db)
            .component("android/platform-tools")
            .unwrap()
            .is_some());
    }

    #[test]
    fn tool_runner_reports_failure_and_cancel() {
        // A stand-in "Android CLI": cmd.exe exits with the code given.
        let f = fx("txn-tool");
        let cmd = PathBuf::from(
            std::env::var("ComSpec").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".into()),
        );
        let mut j = SdkPackageInstall {
            package: "x".into(),
            sdk_root: f.layout.android_sdk(),
            android_exe: cmd,
            env: HashMap::new(),
            log: f.layout.logs().join("t.log"),
            timeout: Duration::from_secs(30),
        };
        // cmd.exe treats our arguments as unknown → it starts an interactive shell reading stdin
        // (null) and exits 0; the package folder is missing → reported.
        let r = install_sdk_package(&f.db, &j, &mut |_, _, _| {}, &AtomicBool::new(false));
        assert!(
            matches!(&r, Err(InstallError::Tool(m)) if m.contains("missing")),
            "{r:?}"
        );
        j.timeout = Duration::from_millis(1);
        let cancel = AtomicBool::new(true);
        let r = install_sdk_package(&f.db, &j, &mut |_, _, _| {}, &cancel);
        assert!(
            matches!(r, Err(InstallError::Cancelled | InstallError::Tool(_))),
            "{r:?}"
        );
        assert!(j.log.is_file());
    }
}
