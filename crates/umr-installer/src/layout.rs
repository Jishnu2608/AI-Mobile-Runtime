//! Folders below the managed root. The environment scan (ENV-002…004) looks in the same places.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Layout {
    root: PathBuf,
}

impl Layout {
    #[must_use]
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Download cache (`*.part` while incomplete; kept for resume).
    #[must_use]
    pub fn downloads(&self) -> PathBuf {
        self.root.join("downloads")
    }

    /// Per-transaction extraction folders; empty when nothing is in progress.
    #[must_use]
    pub fn staging(&self) -> PathBuf {
        self.root.join("staging")
    }

    /// `jdks\<name>\bin\java.exe`
    #[must_use]
    pub fn jdks(&self) -> PathBuf {
        self.root.join("jdks")
    }

    /// `node\<version>\node.exe`
    #[must_use]
    pub fn node(&self) -> PathBuf {
        self.root.join("node")
    }

    /// `flutter\<version>\bin\flutter.bat`
    #[must_use]
    pub fn flutter(&self) -> PathBuf {
        self.root.join("flutter")
    }

    /// Managed Android SDK.
    #[must_use]
    pub fn android_sdk(&self) -> PathBuf {
        self.root.join("android-sdk")
    }

    /// Install logs (Android CLI output).
    #[must_use]
    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// Is `p` inside the managed root? (NFR-08: no writes elsewhere.)
    #[must_use]
    pub fn contains(&self, p: &Path) -> bool {
        p.starts_with(&self.root)
    }
}
