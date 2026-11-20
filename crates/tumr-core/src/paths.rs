//! Locations of the Runtime Core's own files.
//!
//! The Core's own files live in one per-user home folder:
//!
//! ```text
//! <home>/            %USERPROFILE%\.tumr on Windows (override: TUMR_HOME)
//!   config.toml      settings, incl. [storage] toolchains_dir
//!   tumr.db           database
//!   logs/            rotating daily log files of the Core
//!   run/             lock, pid and admin token of the running Core
//! ```
//!
//! Toolchains (JDK, Android SDK, images, AVDs, Node, Flutter, download cache) go to the
//! toolchains folder: `toolchains_dir` from `config.toml`, else the home folder itself.
//!
//! The home is deliberately **not** in `AppData` (CORE-007): processes started by a packaged
//! (MSIX) app such as Claude Desktop get their `AppData` writes redirected into a private copy,
//! so an agent inside Claude Desktop and a normal terminal would see different data.

use std::io;
use std::path::{Path, PathBuf};

/// Environment variable that overrides the home folder (used by tests and power users).
pub const HOME_ENV: &str = "TUMR_HOME";

/// Name of the default home folder inside the user's profile.
pub const DEFAULT_DIR: &str = ".tumr";

/// RENAME-001 (2026-11-20): the product was called "umr" before; its home folder and override
/// variable are taken over once (see [`migrate_home`]).
pub const LEGACY_HOME_ENV: &str = "UMR_HOME";
/// The home folder name before RENAME-001.
pub const LEGACY_DIR: &str = ".umr";

/// RENAME-001: moves `<profile>\.umr` to `<profile>\.tumr` the first time the renamed product
/// runs (only when `.tumr` does not exist yet). If the move fails (the old Core still has files
/// open), the old folder is used as it is, and the move is tried again next time.
#[must_use]
pub fn migrate_home(profile: &Path) -> PathBuf {
    let new = profile.join(DEFAULT_DIR);
    let old = profile.join(LEGACY_DIR);
    if !new.exists() && old.is_dir() && std::fs::rename(&old, &new).is_err() {
        rename_legacy_files(&old);
        return old;
    }
    rename_legacy_files(&new);
    new
}

/// RENAME-001: files inside the home that carried the old name (the database).
pub fn rename_legacy_files(root: &Path) {
    for (old, new) in [
        ("umr.db", "tumr.db"),
        ("umr.db-wal", "tumr.db-wal"),
        ("umr.db-shm", "tumr.db-shm"),
    ] {
        let (o, n) = (root.join(old), root.join(new));
        if o.is_file() && !n.exists() {
            let _ = std::fs::rename(&o, &n);
        }
    }
}

/// Resolved directories of the Runtime Core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorePaths {
    root: PathBuf,
    /// True when the root was chosen explicitly (`TUMR_HOME` or tests), not the default location.
    custom: bool,
}

impl CorePaths {
    /// Uses an explicit root directory.
    #[must_use]
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            custom: true,
        }
    }

    fn default_root(root: PathBuf) -> Self {
        Self {
            root,
            custom: false,
        }
    }

    /// Resolves the home: `TUMR_HOME` if set, else the user's profile known folder (asked from
    /// the shell, not the possibly-stale environment) + `.tumr`, else `%USERPROFILE%\.tumr`.
    pub fn resolve() -> io::Result<Self> {
        let set = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
        if let Some(home) = set(HOME_ENV).or_else(|| set(LEGACY_HOME_ENV)) {
            rename_legacy_files(Path::new(&home));
            return Ok(Self::with_root(home));
        }
        #[cfg(windows)]
        if let Ok(profile) = tumr_platform::folders::profile() {
            return Ok(Self::default_root(migrate_home(&profile)));
        }
        Self::resolve_from(|key| std::env::var_os(key))
    }

    /// Resolution with an injectable environment lookup (testable without touching the process env).
    pub fn resolve_from(env: impl Fn(&str) -> Option<std::ffi::OsString>) -> io::Result<Self> {
        if let Some(home) = env(HOME_ENV).filter(|v| !v.is_empty()) {
            return Ok(Self::with_root(home));
        }
        if let Some(profile) = env("USERPROFILE").filter(|v| !v.is_empty()) {
            return Ok(Self::default_root(PathBuf::from(profile).join(DEFAULT_DIR)));
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("cannot determine data folder: set {HOME_ENV} or USERPROFILE"),
        ))
    }

    /// Managed root folder.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Folder for the Core's log files.
    #[must_use]
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// Folder for runtime state of the running Core (lock, pid).
    #[must_use]
    pub fn run_dir(&self) -> PathBuf {
        self.root.join("run")
    }

    /// Lock file that guarantees a single Core per user.
    #[must_use]
    pub fn lock_file(&self) -> PathBuf {
        self.run_dir().join("core.lock")
    }

    /// File holding the process id of the running Core.
    #[must_use]
    pub fn pid_file(&self) -> PathBuf {
        self.run_dir().join("core.pid")
    }

    /// Admin token of the running Core (owner-only file, recreated at each start).
    #[must_use]
    pub fn admin_token_file(&self) -> PathBuf {
        self.run_dir().join("admin.token")
    }

    /// Registry of API clients (token hashes and scopes, owner-only file).
    #[must_use]
    pub fn clients_file(&self) -> PathBuf {
        self.root.join("clients.json")
    }

    /// Agent client tokens written by `tumr agent add` (`<name>.token`, owner-only files).
    #[must_use]
    pub fn agent_tokens_dir(&self) -> PathBuf {
        self.root.join("agents")
    }

    /// Whether the root was set explicitly rather than the per-user default.
    #[must_use]
    pub fn is_custom_root(&self) -> bool {
        self.custom
    }

    /// Named pipe of the Core's API for this user (and root, if custom).
    #[cfg(windows)]
    pub fn pipe_name(&self) -> io::Result<String> {
        let sid = tumr_platform::identity::current_user_sid()?;
        Ok(tumr_api::pipe_name::pipe_name(
            &sid,
            self.custom.then_some(self.root.as_path()),
        ))
    }

    /// Creates the folders the Core needs.
    pub fn ensure_dirs(&self) -> io::Result<()> {
        std::fs::create_dir_all(self.logs_dir())?;
        std::fs::create_dir_all(self.run_dir())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn env_of<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |k| {
            pairs
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| OsString::from(v))
        }
    }

    #[test]
    fn tumr_home_overrides_profile() {
        let p =
            CorePaths::resolve_from(env_of(&[("TUMR_HOME", r"D:\x"), ("USERPROFILE", r"C:\U")]))
                .unwrap();
        assert_eq!(p.root(), Path::new(r"D:\x"));
    }

    #[test]
    fn the_old_home_moves_once_with_its_database() {
        let base = std::env::temp_dir().join(format!("tumr-migrate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join(LEGACY_DIR).join("agents")).unwrap();
        std::fs::write(base.join(LEGACY_DIR).join("umr.db"), "db").unwrap();
        std::fs::write(
            base.join(LEGACY_DIR).join("agents").join("codex.token"),
            "t",
        )
        .unwrap();
        let home = migrate_home(&base);
        assert_eq!(home, base.join(DEFAULT_DIR));
        assert!(!base.join(LEGACY_DIR).exists(), "moved, not copied");
        assert!(home.join("tumr.db").is_file() && !home.join("umr.db").exists());
        assert!(home.join("agents").join("codex.token").is_file());
        // a second run with both names around keeps the new one
        std::fs::create_dir_all(base.join(LEGACY_DIR)).unwrap();
        assert_eq!(migrate_home(&base), base.join(DEFAULT_DIR));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn falls_back_to_profile_dot_tumr() {
        let p = CorePaths::resolve_from(env_of(&[
            ("USERPROFILE", r"C:\U"),
            ("LOCALAPPDATA", r"C:\U\AppData\Local"),
        ]))
        .unwrap();
        assert_eq!(
            p.root(),
            Path::new(r"C:\U\.tumr"),
            "never AppData (CORE-007)"
        );
    }

    #[cfg(windows)]
    #[test]
    fn default_home_is_outside_appdata() {
        let profile = tumr_platform::folders::profile().unwrap();
        let home = profile.join(DEFAULT_DIR);
        assert!(!home.starts_with(tumr_platform::folders::local_app_data().unwrap()));
        assert!(!home.starts_with(tumr_platform::folders::roaming_app_data().unwrap()));
    }

    #[test]
    fn empty_values_are_ignored() {
        let p = CorePaths::resolve_from(env_of(&[("TUMR_HOME", ""), ("USERPROFILE", r"C:\U")]))
            .unwrap();
        assert_eq!(p.root(), Path::new(r"C:\U\.tumr"));
    }

    #[test]
    fn errors_without_any_location() {
        assert!(CorePaths::resolve_from(env_of(&[])).is_err());
    }

    #[test]
    fn subpaths_are_under_root() {
        let p = CorePaths::with_root(r"D:\r");
        assert_eq!(p.logs_dir(), Path::new(r"D:\r\logs"));
        assert_eq!(p.lock_file(), Path::new(r"D:\r\run\core.lock"));
        assert_eq!(p.pid_file(), Path::new(r"D:\r\run\core.pid"));
    }
}
