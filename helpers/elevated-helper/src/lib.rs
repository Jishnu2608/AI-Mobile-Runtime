//! INS-005: the administrator actions the product may ever perform, and nothing else.
//!
//! The helper binary (`umr-elevate.exe`) accepts exactly one argument: an action name from
//! [`Action`]. It takes no paths, values or commands from its caller, so even a compromised
//! caller cannot make it do anything outside this list. Windows' UAC prompt is the user's
//! consent for each single run.
//!
//! | Action | What it does |
//! |---|---|
//! | `enable-whpx` | `dism /Online /Enable-Feature /FeatureName:HypervisorPlatform /All /NoRestart` (restart needed) |
//! | `enable-long-paths` | `HKLM\SYSTEM\CurrentControlSet\Control\FileSystem\LongPathsEnabled = 1` |

use std::path::{Path, PathBuf};

/// Exit codes of `umr-elevate.exe`.
pub mod exit {
    /// Done; nothing else needed.
    pub const DONE: u32 = 0;
    /// The action failed (details on stderr; the caller cannot read it through UAC).
    pub const FAILED: u32 = 1;
    /// Unknown or missing action.
    pub const USAGE: u32 = 2;
    /// Not started with administrator rights.
    pub const NOT_ELEVATED: u32 = 3;
    /// Done; Windows must restart before it takes effect.
    pub const RESTART_REQUIRED: u32 = 10;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    EnableWhpx,
    EnableLongPaths,
}

impl Action {
    pub const ALL: [Self; 2] = [Self::EnableWhpx, Self::EnableLongPaths];

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::EnableWhpx => "enable-whpx",
            Self::EnableLongPaths => "enable-long-paths",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.name() == s)
    }

    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::EnableWhpx => "Turn on Windows Hypervisor Platform",
            Self::EnableLongPaths => "Enable Win32 long paths",
        }
    }

    #[must_use]
    pub fn explanation(self) -> &'static str {
        match self {
            Self::EnableWhpx => "The Android emulator needs hardware acceleration. This turns on the Windows optional feature \"Windows Hypervisor Platform\". Windows must restart afterwards.",
            Self::EnableLongPaths => "Lets programs use file paths longer than 260 characters (node_modules and native builds often need it). Sets one Windows setting; no restart needed for new programs.",
        }
    }

    /// The exact DISM command for `enable-whpx` (absolute path from the system directory).
    #[must_use]
    pub fn dism_command(system_dir: &Path) -> (PathBuf, Vec<&'static str>) {
        (
            system_dir.join("dism.exe"),
            vec![
                "/Online",
                "/Enable-Feature",
                "/FeatureName:HypervisorPlatform",
                "/All",
                "/NoRestart",
            ],
        )
    }
}

pub const LONG_PATHS_KEY: &str = r"SYSTEM\CurrentControlSet\Control\FileSystem";
pub const LONG_PATHS_VALUE: &str = "LongPathsEnabled";

/// DISM: 0 = done, 3010 = done but restart required.
#[must_use]
pub fn map_dism_exit(code: Option<i32>) -> u32 {
    match code {
        Some(0) => exit::DONE,
        Some(3010) => exit::RESTART_REQUIRED,
        _ => exit::FAILED,
    }
}

/// What the caller tells the user about an exit code.
#[must_use]
pub fn describe_exit(code: u32) -> &'static str {
    match code {
        exit::DONE => "done",
        exit::RESTART_REQUIRED => "done; restart Windows to finish",
        exit::USAGE => "the helper did not recognise the action",
        exit::NOT_ELEVATED => "the helper did not get administrator rights",
        _ => "failed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_known_actions_parse() {
        for a in Action::ALL {
            assert_eq!(Action::parse(a.name()), Some(a));
        }
        for bad in [
            "",
            "enable-WHPX",
            "enable-whpx ",
            "cmd",
            "--help",
            "enable-whpx;calc",
            "../x",
        ] {
            assert_eq!(Action::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn dism_command_is_fixed() {
        let (exe, args) = Action::dism_command(Path::new(r"C:\Windows\System32"));
        assert_eq!(exe, PathBuf::from(r"C:\Windows\System32\dism.exe"));
        assert_eq!(
            args,
            [
                "/Online",
                "/Enable-Feature",
                "/FeatureName:HypervisorPlatform",
                "/All",
                "/NoRestart"
            ]
        );
        assert_eq!(map_dism_exit(Some(0)), exit::DONE);
        assert_eq!(map_dism_exit(Some(3010)), exit::RESTART_REQUIRED);
        assert_eq!(map_dism_exit(Some(5)), exit::FAILED);
        assert_eq!(map_dism_exit(None), exit::FAILED);
    }
}
