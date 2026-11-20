//! AUD-P1-9: Android's "System UI isn't responding" dialog. On a cold or freshly restored
//! emulator, System UI is often slow for a moment and Android shows this dialog over the app;
//! a new user sees a broken phone before their app had a chance. The runtime notices it, chooses
//! "Wait" (the same as a person would; nothing is closed) and records a device warning.
//! The app's own ANR is not touched: that one is a real problem for the developer to see.

use std::time::Duration;

use crate::{adb, ui};

/// The package whose "isn't responding" dialog has the focus, from `dumpsys window`
/// (`mCurrentFocus=Window{… u0 Application Not Responding: com.android.systemui}`).
#[must_use]
pub fn focused_anr(dumpsys_window: &str) -> Option<String> {
    let line = dumpsys_window
        .lines()
        .find(|l| l.trim_start().starts_with("mCurrentFocus="))?;
    let at = line.find("Application Not Responding: ")? + "Application Not Responding: ".len();
    let pkg: String = line[at..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_')
        .collect();
    (!pkg.is_empty()).then_some(pkg)
}

/// The packages whose ANR dialog the runtime dismisses: Android's own, never the app's.
#[must_use]
pub fn is_system(pkg: &str) -> bool {
    matches!(
        pkg,
        "com.android.systemui" | "com.google.android.apps.nexuslauncher" | "com.android.launcher3"
    )
}

/// Whether a system ANR dialog shows now; returns its package.
#[must_use]
pub fn system_anr(port: u16, serial: &str) -> Option<String> {
    let out = adb::shell_bytes(
        port,
        serial,
        "dumpsys window | grep mCurrentFocus",
        Duration::from_secs(5),
    )
    .ok()?;
    focused_anr(&String::from_utf8_lossy(&out)).filter(|p| is_system(p))
}

/// Chooses "Wait" in the dialog (found in the UI tree); Back closes it the same way when the
/// button cannot be found. Returns whether the dialog is gone afterwards.
#[must_use = "whether the dialog is gone"]
pub fn dismiss(port: u16, serial: &str) -> bool {
    let tapped = ui::dump(port, serial).ok().and_then(|nodes| {
        nodes
            .iter()
            .find(|n| n.text.eq_ignore_ascii_case("Wait"))
            .map(|n| ui::tap(port, serial, n.center[0], n.center[1]).is_ok())
    });
    if tapped != Some(true) {
        let _ = adb::shell(port, serial, "input keyevent KEYCODE_BACK");
    }
    std::thread::sleep(Duration::from_millis(600));
    system_anr(port, serial).is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_focused_anr_dialog_is_recognised() {
        // captured on the API 36 emulator (2026-11-17)
        let live = "  mCurrentFocus=Window{40d2a90 u0 Application Not Responding: com.android.systemui}\n  mFocusedApp=ActivityRecord{…}";
        assert_eq!(focused_anr(live).as_deref(), Some("com.android.systemui"));
        assert!(is_system("com.android.systemui"));
        let app =
            "  mCurrentFocus=Window{1 u0 Application Not Responding: com.tumr.samples.rncalc}";
        assert_eq!(focused_anr(app).as_deref(), Some("com.tumr.samples.rncalc"));
        assert!(
            !is_system("com.tumr.samples.rncalc"),
            "the app's own ANR stays for the developer"
        );
        let normal = "  mCurrentFocus=Window{2 u0 com.tumr.samples.rncalc/com.tumr.samples.rncalc.MainActivity}";
        assert_eq!(focused_anr(normal), None);
        assert_eq!(focused_anr(""), None);
    }
}
