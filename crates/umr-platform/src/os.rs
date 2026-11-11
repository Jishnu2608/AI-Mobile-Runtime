//! Operating-system facts needed by environment detection.

use std::io;

use crate::registry::{read_dword, read_string, Hive};

/// First Windows build number that is Windows 11.
pub const WINDOWS_11_MIN_BUILD: u32 = 22_000;

/// Windows version facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsVersion {
    /// Build number, e.g. 26100.
    pub build: u32,
    /// Display version such as `24H2`, if present.
    pub display_version: Option<String>,
    /// Edition name from the registry (may still say "Windows 10" on Windows 11; use [`Self::is_windows_11`]).
    pub product_name: Option<String>,
}

impl WindowsVersion {
    /// Windows 11 is identified by build number, not by product name.
    #[must_use]
    pub fn is_windows_11_or_later(&self) -> bool {
        self.build >= WINDOWS_11_MIN_BUILD
    }
}

const NT: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";

/// Reads the Windows version from the registry.
pub fn windows_version() -> io::Result<WindowsVersion> {
    let build = read_string(Hive::LocalMachine, NT, "CurrentBuildNumber")?
        .and_then(|s| s.trim().parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "CurrentBuildNumber missing"))?;
    Ok(WindowsVersion {
        build,
        display_version: read_string(Hive::LocalMachine, NT, "DisplayVersion")?,
        product_name: read_string(Hive::LocalMachine, NT, "ProductName")?,
    })
}

/// Whether Win32 long paths (> 260 chars) are enabled machine-wide.
pub fn long_paths_enabled() -> io::Result<bool> {
    Ok(read_dword(
        Hive::LocalMachine,
        r"SYSTEM\CurrentControlSet\Control\FileSystem",
        "LongPathsEnabled",
    )?
    .unwrap_or(0)
        == 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_classification() {
        let v = |build| WindowsVersion {
            build,
            display_version: None,
            product_name: None,
        };
        assert!(!v(19_045).is_windows_11_or_later()); // Windows 10 22H2
        assert!(v(22_000).is_windows_11_or_later());
        assert!(v(26_100).is_windows_11_or_later());
    }

    #[test]
    fn reads_this_machine() {
        let v = windows_version().unwrap();
        assert!(v.build > 10_000);
        // The development and CI machines run Windows 11 / Server 2022+.
        let _ = long_paths_enabled().unwrap();
    }
}
