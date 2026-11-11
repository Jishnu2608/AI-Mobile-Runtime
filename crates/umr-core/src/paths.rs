//! Locations of the Runtime Core's own files.
//!
//! Everything the product writes lives under one managed root:
//!
//! ```text
//! <root>/            %LOCALAPPDATA%\UMR on Windows (override: UMR_HOME)
//!   logs/            rotating daily log files of the Core
//!   run/             lock and pid files of the running Core
//! ```
//!
//! SDKs, caches and devices will get their own subfolders in later tasks.

use std::io;
use std::path::{Path, PathBuf};

/// Environment variable that overrides the managed root (used by tests and power users).
pub const HOME_ENV: &str = "UMR_HOME";

/// Resolved directories of the Runtime Core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorePaths {
    root: PathBuf,
    /// True when the root was chosen explicitly (`UMR_HOME` or tests), not the default location.
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

    /// Resolves the root: `UMR_HOME` if set, else the user's local application-data known folder
    /// (asked from the shell, not the possibly-stale environment) + `UMR`, else `%LOCALAPPDATA%\UMR`.
    pub fn resolve() -> io::Result<Self> {
        if let Some(home) = std::env::var_os(HOME_ENV).filter(|v| !v.is_empty()) {
            return Ok(Self::with_root(home));
        }
        #[cfg(windows)]
        if let Ok(local) = umr_platform::folders::local_app_data() {
            return Ok(Self::default_root(local.join("UMR")));
        }
        Self::resolve_from(|key| std::env::var_os(key))
    }

    /// Resolution with an injectable environment lookup (testable without touching the process env).
    pub fn resolve_from(env: impl Fn(&str) -> Option<std::ffi::OsString>) -> io::Result<Self> {
        if let Some(home) = env(HOME_ENV).filter(|v| !v.is_empty()) {
            return Ok(Self::with_root(home));
        }
        if let Some(local) = env("LOCALAPPDATA").filter(|v| !v.is_empty()) {
            return Ok(Self::default_root(PathBuf::from(local).join("UMR")));
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("cannot determine data folder: set {HOME_ENV} or LOCALAPPDATA"),
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

    /// Whether the root was set explicitly rather than the per-user default.
    #[must_use]
    pub fn is_custom_root(&self) -> bool {
        self.custom
    }

    /// Named pipe of the Core's API for this user (and root, if custom).
    #[cfg(windows)]
    pub fn pipe_name(&self) -> io::Result<String> {
        let sid = umr_platform::identity::current_user_sid()?;
        Ok(umr_api::pipe_name::pipe_name(
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
    fn umr_home_overrides_localappdata() {
        let p =
            CorePaths::resolve_from(env_of(&[("UMR_HOME", r"D:\x"), ("LOCALAPPDATA", r"C:\L")]))
                .unwrap();
        assert_eq!(p.root(), Path::new(r"D:\x"));
    }

    #[test]
    fn falls_back_to_localappdata_umr() {
        let p = CorePaths::resolve_from(env_of(&[("LOCALAPPDATA", r"C:\L")])).unwrap();
        assert_eq!(p.root(), Path::new(r"C:\L\UMR"));
    }

    #[test]
    fn empty_values_are_ignored() {
        let p = CorePaths::resolve_from(env_of(&[("UMR_HOME", ""), ("LOCALAPPDATA", r"C:\L")]))
            .unwrap();
        assert_eq!(p.root(), Path::new(r"C:\L\UMR"));
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
