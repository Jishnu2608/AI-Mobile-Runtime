//! INS-006: verify, repair, remove, disk usage. INS-007: Android CLI runtime fingerprint.
//!
//! - **Verify**: every recorded component must still exist and its key file be present; JDK
//!   and Node are also started once (`-version`) to prove they work.
//! - **Remove**: only components the product installed (source `managed`) and only inside
//!   the managed root. The folder is first moved into `staging` (one rename) and then
//!   deleted, so a half-deleted component is never left in place. Removing the Android
//!   command-line tools while other SDK packages exist needs `force` (they are needed to
//!   change the SDK later).
//! - **Repair**: broken managed components are removed so the next `install` puts them back.
//! - **Disk usage**: per component plus the download cache, logs, temp and Android user home.
//! - **Clean**: deletes the download cache and logs older than 14 days.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use umr_storage::components::{Component, Source};
use umr_storage::Db;

use crate::layout::Layout;
use crate::verify::{hash_file, Checksum};
use crate::{now_unix, random_id, InstallError};

/// Component id of the Android CLI runtime that `android.exe` downloads on first use.
pub const CLI_RUNTIME_ID: &str = "android/cli-runtime";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentHealth {
    pub id: String,
    pub kind: String,
    pub version: String,
    pub path: String,
    /// `managed` | `user` | `bundled`
    pub source: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
    /// Works, but something changed that the user should know about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskUsage {
    pub components_bytes: u64,
    pub download_cache_bytes: u64,
    pub logs_bytes: u64,
    pub temp_bytes: u64,
    /// AVDs, adb keys, Android CLI runtime.
    pub android_user_home_bytes: u64,
    pub staging_bytes: u64,
    pub total_bytes: u64,
}

fn lock(db: &Mutex<Db>) -> std::sync::MutexGuard<'_, Db> {
    db.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Size of a file or folder tree (links are not followed).
#[must_use]
pub fn size_of(p: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(p) else {
        return 0;
    };
    if meta.is_file() {
        return meta.len();
    }
    let mut total = 0;
    let mut stack = vec![p.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(e.path()),
                Ok(t) if t.is_file() => total += e.metadata().map_or(0, |m| m.len()),
                _ => {}
            }
        }
    }
    total
}

fn source_name(s: Source) -> &'static str {
    match s {
        Source::Managed => "managed",
        Source::User => "user",
        Source::Bundled => "bundled",
    }
}

/// The file that proves a component is installed.
fn key_file(c: &Component) -> Option<PathBuf> {
    let p = PathBuf::from(&c.path);
    match c.kind.as_str() {
        "jdk" => Some(p.join("bin").join("java.exe")),
        "node" => Some(p.join("node.exe")),
        "flutter" => Some(p.join("bin").join("flutter.bat")),
        "android-cli-runtime" => Some(p),
        "android-sdk-package" if c.id == "android/cmdline-tools/latest" => {
            Some(p.join("bin").join("android.exe"))
        }
        "android-sdk-package" => Some(if p.join("package.xml").is_file() {
            p.join("package.xml")
        } else {
            p.join("source.properties")
        }),
        _ => None,
    }
}

/// Runs `exe args` with a timeout; true when it exits 0.
fn runs(exe: &Path, args: &[&str], timeout: Duration) -> bool {
    let Ok(mut child) = Command::new(exe)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(s)) => return s.success(),
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

fn check(c: &Component) -> (Option<String>, Option<String>) {
    let p = PathBuf::from(&c.path);
    if !p.exists() {
        return (Some(format!("{} is missing", p.display())), None);
    }
    let Some(key) = key_file(c) else {
        return (None, None);
    };
    if !key.is_file() {
        return (Some(format!("{} is missing", key.display())), None);
    }
    let functional = match c.kind.as_str() {
        "jdk" => runs(&key, &["-version"], Duration::from_secs(20)),
        "node" => runs(&key, &["--version"], Duration::from_secs(20)),
        _ => true,
    };
    if !functional {
        return (Some(format!("{} does not start", key.display())), None);
    }
    if c.kind == "android-cli-runtime" {
        if let Some(recorded) = &c.sha256 {
            if hash_file(&key, &Checksum::Sha256(String::new()))
                .ok()
                .as_deref()
                != Some(recorded.as_str())
            {
                return (None, Some("Android CLI replaced its runtime since installation (self-update); the new files were not verified by us".into()));
            }
        }
    }
    (None, None)
}

/// Verifies all recorded components (and marks the good ones as verified).
pub fn verify(db: &Mutex<Db>) -> Result<Vec<ComponentHealth>, InstallError> {
    let list = lock(db).components(None)?;
    let mut out = Vec::with_capacity(list.len());
    for c in list {
        let (problem, note) = check(&c);
        if problem.is_none() {
            let _ = lock(db).mark_component_verified(&c.id, now_unix());
        }
        out.push(ComponentHealth {
            size_bytes: size_of(Path::new(&c.path)),
            id: c.id,
            kind: c.kind,
            version: c.version,
            path: c.path,
            source: source_name(c.source).into(),
            ok: problem.is_none(),
            problem,
            note,
        });
    }
    Ok(out)
}

/// Disk usage of the managed root.
#[must_use]
pub fn disk_usage(layout: &Layout, components: &[ComponentHealth]) -> DiskUsage {
    let mut u = DiskUsage {
        components_bytes: components
            .iter()
            .filter(|c| c.source == "managed" && c.kind != "android-cli-runtime")
            .map(|c| c.size_bytes)
            .sum(),
        download_cache_bytes: size_of(&layout.downloads()),
        logs_bytes: size_of(&layout.logs()),
        temp_bytes: size_of(&layout.root().join("tmp")),
        android_user_home_bytes: size_of(&layout.root().join("android-user-home")),
        staging_bytes: size_of(&layout.staging()),
        total_bytes: 0,
    };
    u.total_bytes = u.components_bytes
        + u.download_cache_bytes
        + u.logs_bytes
        + u.temp_bytes
        + u.android_user_home_bytes
        + u.staging_bytes;
    u
}

/// Moves a folder into staging and deletes it there.
fn delete_tree(layout: &Layout, path: &Path) -> Result<(), InstallError> {
    if !path.exists() {
        return Ok(());
    }
    let trash = layout.staging().join(format!("trash-{}", random_id()));
    std::fs::create_dir_all(layout.staging())?;
    std::fs::rename(path, &trash).map_err(|e| {
        InstallError::Io(format!(
            "{} is in use or locked ({e}); close programs using it and retry",
            path.display()
        ))
    })?;
    let _ = std::fs::remove_dir_all(&trash).or_else(|_| std::fs::remove_file(&trash));
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Removed {
    pub id: String,
    pub path: String,
    pub freed_bytes: u64,
}

/// Removes one managed component.
pub fn remove(
    db: &Mutex<Db>,
    layout: &Layout,
    id: &str,
    force: bool,
) -> Result<Removed, InstallError> {
    let c = lock(db)
        .component(id)?
        .ok_or_else(|| InstallError::Unsupported(format!("no component '{id}'")))?;
    if c.source != Source::Managed {
        return Err(InstallError::Unsupported(format!("'{id}' was not installed by this product ({}); it is only used read-only and is never removed", source_name(c.source))));
    }
    let path = PathBuf::from(&c.path);
    let root = layout.root();
    if !path.starts_with(root)
        || path == root
        || path == layout.android_sdk()
        || path.parent() == Some(root)
    {
        return Err(InstallError::Unsupported(format!(
            "refusing to remove {}: not a component folder inside the managed root",
            path.display()
        )));
    }
    if id == "android/cmdline-tools/latest" && !force {
        let others: Vec<String> = lock(db)
            .components(Some("android-sdk-package"))?
            .into_iter()
            .map(|c| c.id)
            .filter(|i| i != id)
            .collect();
        if !others.is_empty() {
            return Err(InstallError::Unsupported(format!(
                "the Android command-line tools are needed to change these SDK packages later: {}; use force to remove anyway",
                others.join(", ")
            )));
        }
    }
    let freed = size_of(&path);
    delete_tree(layout, &path)?;
    let db = lock(db);
    db.remove_component(id)?;
    let _ = db.audit(now_unix(), None, "component.remove", Some(id));
    Ok(Removed {
        id: id.into(),
        path: c.path,
        freed_bytes: freed,
    })
}

/// Removes managed components that fail verification so the next install restores them.
pub fn repair(db: &Mutex<Db>, layout: &Layout) -> Result<Vec<Removed>, InstallError> {
    let mut out = Vec::new();
    for h in verify(db)? {
        if !h.ok && h.source == "managed" {
            match remove(db, layout, &h.id, true) {
                Ok(r) => out.push(r),
                Err(InstallError::Unsupported(_)) => {
                    // record without a removable folder (e.g. folder already gone)
                    lock(db).remove_component(&h.id)?;
                    out.push(Removed {
                        id: h.id,
                        path: h.path,
                        freed_bytes: 0,
                    });
                }
                Err(e) => return Err(e),
            }
        }
    }
    Ok(out)
}

/// Deletes the download cache and logs older than 14 days. Returns freed bytes.
pub fn clean(layout: &Layout) -> Result<u64, InstallError> {
    let mut freed = 0;
    for e in std::fs::read_dir(layout.downloads())
        .into_iter()
        .flatten()
        .flatten()
    {
        let p = e.path();
        let n = size_of(&p);
        if std::fs::remove_file(&p)
            .or_else(|_| std::fs::remove_dir_all(&p))
            .is_ok()
        {
            freed += n;
        }
    }
    let cutoff = SystemTime::now() - Duration::from_secs(14 * 24 * 3600);
    for e in std::fs::read_dir(layout.logs())
        .into_iter()
        .flatten()
        .flatten()
    {
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| t < cutoff);
        if old {
            let n = size_of(&e.path());
            if std::fs::remove_file(e.path()).is_ok() {
                freed += n;
            }
        }
    }
    Ok(freed)
}

// ---- INS-007: Android CLI runtime ------------------------------------------------------------

/// `<android user home>\bin\android-cli.exe`, written by the `android.exe` launcher.
#[must_use]
pub fn cli_runtime_exe(android_user_home: &Path) -> PathBuf {
    android_user_home.join("bin").join("android-cli.exe")
}

/// Records the Android CLI runtime the launcher downloaded (SHA-256 + bundle id), so a later
/// self-update is noticed by `verify`.
pub fn record_cli_runtime(db: &Mutex<Db>, android_user_home: &Path) -> Result<bool, InstallError> {
    let exe = cli_runtime_exe(android_user_home);
    if !exe.is_file() {
        return Ok(false);
    }
    let sha = hash_file(&exe, &Checksum::Sha256(String::new()))?;
    let db = lock(db);
    if db
        .component(CLI_RUNTIME_ID)?
        .is_some_and(|c| c.sha256.as_deref() == Some(sha.as_str()))
    {
        return Ok(false);
    }
    let bundle = std::fs::read_dir(android_user_home.join("cli").join("bundles"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .next()
        .unwrap_or_default();
    db.upsert_component(&Component {
        id: CLI_RUNTIME_ID.into(),
        kind: "android-cli-runtime".into(),
        version: bundle,
        path: exe.display().to_string(),
        source: Source::Bundled,
        sha256: Some(sha),
        size_bytes: i64::try_from(size_of(&exe)).ok(),
        installed_unix: now_unix(),
        verified_unix: None,
    })?;
    Ok(true)
}

/// Android CLI keeps usage metrics in `cli\analytics\metrics\spool` when metrics are on.
/// The product always passes `--no-metrics`; if spool files appear anyway they are deleted
/// before they can be uploaded. Returns how many were removed.
#[must_use = "report purged metrics files"]
pub fn purge_cli_metrics(android_user_home: &Path) -> usize {
    let spool = android_user_home
        .join("cli")
        .join("analytics")
        .join("metrics")
        .join("spool");
    let mut n = 0;
    for e in std::fs::read_dir(&spool).into_iter().flatten().flatten() {
        if std::fs::remove_file(e.path()).is_ok() {
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::tmp_dir;

    struct Fx {
        layout: Layout,
        db: Mutex<Db>,
    }

    fn fx(tag: &str) -> Fx {
        let d = tmp_dir(tag);
        let layout = Layout::new(&d.join("root"));
        std::fs::create_dir_all(layout.root()).unwrap();
        let db = Mutex::new(Db::open_in(layout.root()).unwrap());
        Fx { layout, db }
    }

    fn add(f: &Fx, id: &str, kind: &str, path: &Path, files: &[&str], source: Source) {
        for file in files {
            let p = path.join(file);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"0123456789").unwrap();
        }
        lock(&f.db)
            .upsert_component(&Component {
                id: id.into(),
                kind: kind.into(),
                version: "1".into(),
                path: path.display().to_string(),
                source,
                sha256: None,
                size_bytes: None,
                installed_unix: 1,
                verified_unix: None,
            })
            .unwrap();
    }

    #[test]
    fn verify_finds_broken_components_and_sizes() {
        let f = fx("mg-verify");
        let sdk = f.layout.android_sdk();
        add(
            &f,
            "android/platforms/android-36",
            "android-sdk-package",
            &sdk.join("platforms").join("android-36"),
            &["package.xml", "android.jar"],
            Source::Managed,
        );
        add(
            &f,
            "android/emulator",
            "android-sdk-package",
            &sdk.join("emulator"),
            &["readme.txt"],
            Source::Managed,
        );
        add(
            &f,
            "node@24",
            "node",
            &f.layout.node().join("24"),
            &[],
            Source::Managed,
        );
        let v = verify(&f.db).unwrap();
        let by = |id: &str| v.iter().find(|h| h.id == id).unwrap();
        assert!(by("android/platforms/android-36").ok);
        assert_eq!(by("android/platforms/android-36").size_bytes, 20);
        assert!(!by("android/emulator").ok, "no package.xml");
        assert!(by("node@24")
            .problem
            .as_deref()
            .unwrap()
            .contains("missing"));
        assert!(lock(&f.db)
            .component("android/platforms/android-36")
            .unwrap()
            .unwrap()
            .verified_unix
            .is_some());
        let u = disk_usage(&f.layout, &v);
        assert_eq!(u.components_bytes, 30);
        assert!(u.total_bytes >= 30);
    }

    #[test]
    fn remove_rules() {
        let f = fx("mg-remove");
        let sdk = f.layout.android_sdk();
        let clt = sdk.join("cmdline-tools").join("latest");
        add(
            &f,
            "android/cmdline-tools/latest",
            "android-sdk-package",
            &clt,
            &["bin/android.exe"],
            Source::Managed,
        );
        add(
            &f,
            "android/platform-tools",
            "android-sdk-package",
            &sdk.join("platform-tools"),
            &["package.xml"],
            Source::Managed,
        );
        let user = tmp_dir("mg-user-jdk");
        add(
            &f,
            "jdk/user",
            "jdk",
            &user,
            &["bin/java.exe"],
            Source::User,
        );

        assert!(
            matches!(remove(&f.db, &f.layout, "jdk/user", true), Err(InstallError::Unsupported(m)) if m.contains("read-only"))
        );
        assert!(
            user.join("bin").join("java.exe").is_file(),
            "user installs are never touched"
        );
        assert!(
            matches!(remove(&f.db, &f.layout, "android/cmdline-tools/latest", false), Err(InstallError::Unsupported(m)) if m.contains("platform-tools"))
        );
        assert!(clt.exists());
        let r = remove(&f.db, &f.layout, "android/platform-tools", false).unwrap();
        assert_eq!(r.freed_bytes, 10);
        assert!(!sdk.join("platform-tools").exists());
        assert!(lock(&f.db)
            .component("android/platform-tools")
            .unwrap()
            .is_none());
        remove(&f.db, &f.layout, "android/cmdline-tools/latest", false).unwrap();
        assert_eq!(
            std::fs::read_dir(f.layout.staging()).unwrap().count(),
            0,
            "trash deleted"
        );
        assert!(remove(&f.db, &f.layout, "nope", true).is_err());
        assert_eq!(lock(&f.db).audit_count("component.remove").unwrap(), 2);

        // a record pointing at the managed root itself is refused
        add(&f, "bad", "node", f.layout.root(), &[], Source::Managed);
        assert!(remove(&f.db, &f.layout, "bad", true).is_err());
        assert!(f.layout.root().join("umr.db").exists());
    }

    #[test]
    fn repair_removes_only_broken_managed_components() {
        let f = fx("mg-repair");
        let sdk = f.layout.android_sdk();
        add(
            &f,
            "android/emulator",
            "android-sdk-package",
            &sdk.join("emulator"),
            &["half-installed.txt"],
            Source::Managed,
        );
        add(
            &f,
            "android/platform-tools",
            "android-sdk-package",
            &sdk.join("platform-tools"),
            &["package.xml"],
            Source::Managed,
        );
        let r = repair(&f.db, &f.layout).unwrap();
        assert_eq!(
            r.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(),
            ["android/emulator"]
        );
        assert!(!sdk.join("emulator").exists() && sdk.join("platform-tools").exists());
        assert_eq!(repair(&f.db, &f.layout).unwrap(), Vec::<Removed>::new());
    }

    #[test]
    fn clean_removes_download_cache() {
        let f = fx("mg-clean");
        std::fs::create_dir_all(f.layout.downloads()).unwrap();
        std::fs::write(f.layout.downloads().join("a.zip.part"), vec![0u8; 100]).unwrap();
        std::fs::create_dir_all(f.layout.logs()).unwrap();
        std::fs::write(f.layout.logs().join("fresh.log"), b"x").unwrap();
        assert_eq!(clean(&f.layout).unwrap(), 100);
        assert!(
            f.layout.logs().join("fresh.log").exists(),
            "recent logs kept"
        );
    }

    #[test]
    fn cli_runtime_fingerprint_and_metrics_guard() {
        let f = fx("mg-cli");
        let home = f.layout.root().join("android-user-home");
        assert!(!record_cli_runtime(&f.db, &home).unwrap());
        std::fs::create_dir_all(home.join("bin")).unwrap();
        std::fs::create_dir_all(home.join("cli").join("bundles").join("43dc5c")).unwrap();
        std::fs::write(cli_runtime_exe(&home), b"v1").unwrap();
        assert!(record_cli_runtime(&f.db, &home).unwrap());
        assert!(!record_cli_runtime(&f.db, &home).unwrap(), "unchanged");
        let c = lock(&f.db).component(CLI_RUNTIME_ID).unwrap().unwrap();
        assert_eq!(c.version, "43dc5c");
        let v = verify(&f.db).unwrap();
        assert!(v[0].ok && v[0].note.is_none());
        std::fs::write(cli_runtime_exe(&home), b"v2").unwrap();
        let v = verify(&f.db).unwrap();
        assert!(v[0].ok && v[0].note.as_deref().unwrap().contains("self-update"));

        let spool = home
            .join("cli")
            .join("analytics")
            .join("metrics")
            .join("spool");
        std::fs::create_dir_all(&spool).unwrap();
        std::fs::write(spool.join("x.bproto"), b"m").unwrap();
        assert_eq!(purge_cli_metrics(&home), 1);
        assert_eq!(purge_cli_metrics(&home), 0);
    }
}
