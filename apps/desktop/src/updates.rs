//! UPD-001: in-app updates. The wizard asks at start whether a newer version is published on the
//! configured channel (`[update] manifest_url`; nothing is contacted when none is set) and offers
//! **Update now**: `umr update --yes --relaunch` downloads the installer, checks its size and
//! SHA-256 (and signature when required), closes this app, installs over the existing copy
//! (settings, toolchains and emulators stay) and opens the app again.

use std::process::Command;

use serde_json::Value;

use crate::agents::umr_exe;

#[cfg(windows)]
fn no_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt as _;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

/// `umr update --check --json`: `{ current, channel, available: Release | null }`, or
/// `{ current, configured: false }` when no channel is set.
pub fn check() -> Result<Value, String> {
    let mut cmd = Command::new(umr_exe()?);
    cmd.args(["update", "--check", "--json"]);
    #[cfg(windows)]
    no_window(&mut cmd);
    let out = cmd.output().map_err(|e| format!("cannot run umr: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(text.trim()).map_err(|_| {
        let err = String::from_utf8_lossy(&out.stderr);
        err.trim().trim_start_matches("umr: ").to_string()
    })
}

/// Starts `umr update --yes --relaunch` on its own: it closes this app, so it must not be a child
/// that dies with it.
pub fn apply() -> Result<(), String> {
    let mut cmd = Command::new(umr_exe()?);
    cmd.args(["update", "--yes", "--relaunch"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("cannot start the update: {e}"))
}
