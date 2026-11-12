//! DEV-005: app lifecycle on a device through the product's adb server (DEC-017: adb instead
//! of Android CLI for speed; Android CLI stays the SDK package backend).
//!
//! install (`adb install -r -t`), launch (launcher activity via `cmd package resolve-activity`,
//! `am start -W`), stop (`am force-stop`), uninstall (`pm uninstall`), state (`pidof`).

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::Serialize;

use crate::adb;
use crate::server::AdbServer;

/// Package names are `a.b_c` style; anything else is refused before it reaches a shell.
#[must_use]
pub fn valid_package(p: &str) -> bool {
    !p.is_empty()
        && p.len() <= 255
        && p.contains('.')
        && p.split('.').all(|seg| {
            !seg.is_empty() && seg.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

/// Component names `pkg/.Activity` or `pkg/pkg.Activity`.
#[must_use]
pub fn valid_component(c: &str) -> bool {
    c.split_once('/').is_some_and(|(p, a)| {
        valid_package(p)
            && !a.is_empty()
            && a.chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' || ch == '$')
    })
}

fn check_package(p: &str) -> Result<(), String> {
    if valid_package(p) {
        Ok(())
    } else {
        Err(format!("'{p}' is not a valid Android package name"))
    }
}

/// `package: name='…'` from `aapt2 dump badging`.
#[must_use]
pub fn parse_badging_package(text: &str) -> Option<String> {
    let line = text.lines().find(|l| l.starts_with("package:"))?;
    let at = line.find("name='")? + 6;
    let end = line[at..].find('\'')? + at;
    Some(line[at..end].to_string())
}

/// Package name of an APK (`<build-tools>/aapt2 dump badging`).
pub fn apk_package(aapt2: &Path, apk: &Path) -> Result<String, String> {
    let out = Command::new(aapt2)
        .args(["dump", "badging"])
        .arg(apk)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run aapt2: {e}"))?;
    parse_badging_package(&String::from_utf8_lossy(&out.stdout)).ok_or_else(|| {
        format!(
            "cannot read the package name of {}: {}",
            apk.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        )
    })
}

/// Newest `aapt2.exe` under `<sdk>/build-tools/*`.
#[must_use]
pub fn find_aapt2(sdk: &Path) -> Option<std::path::PathBuf> {
    let mut dirs: Vec<_> = std::fs::read_dir(sdk.join("build-tools"))
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("aapt2.exe").is_file())
        .collect();
    dirs.sort();
    dirs.pop().map(|d| d.join("aapt2.exe"))
}

/// Installs (or updates) an APK; debuggable test builds allowed (`-t`).
pub fn install(server: &AdbServer, serial: &str, apk: &Path) -> Result<(), String> {
    if !apk.is_file() {
        return Err(format!("{} does not exist", apk.display()));
    }
    let out = Command::new(&server.adb_exe)
        .args([
            "-P",
            &server.port.to_string(),
            "-s",
            serial,
            "install",
            "-r",
            "-t",
        ])
        .arg(apk)
        .envs(&server.env)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run adb: {e}"))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if out.status.success() && text.contains("Success") {
        Ok(())
    } else {
        let reason = text
            .lines()
            .find(|l| l.contains("Failure") || l.contains("error"))
            .unwrap_or(text.trim());
        Err(format!("install failed: {}", reason.trim()))
    }
}

/// The launcher activity (`pkg/.MainActivity`) of an installed package.
pub fn launcher_activity(port: u16, serial: &str, package: &str) -> Result<String, String> {
    check_package(package)?;
    let out = adb::shell(
        port,
        serial,
        &format!("cmd package resolve-activity --brief -a android.intent.action.MAIN -c android.intent.category.LAUNCHER {package}"),
    )
    .map_err(|e| e.to_string())?;
    out.lines()
        .map(str::trim)
        .rfind(|l| valid_component(l))
        .map(ToString::to_string)
        .ok_or_else(|| format!("{package} has no launcher activity (is it installed?)"))
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Launched {
    pub component: String,
    /// `TotalTime` reported by `am start -W` (ms).
    pub total_ms: Option<u64>,
    pub pid: Option<u32>,
}

/// Values from `am start -W` output.
#[must_use]
pub fn parse_am_start(text: &str) -> (bool, Option<u64>, Option<String>) {
    let ok = text.lines().any(|l| l.trim() == "Status: ok");
    let total = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("TotalTime:"))
        .and_then(|v| v.trim().parse().ok());
    let error = text
        .lines()
        .find(|l| l.starts_with("Error") || l.contains("Exception"))
        .map(|l| l.trim().to_string());
    (ok, total, error)
}

/// Starts the app (its launcher activity unless `activity` is given) and waits until it is drawn.
pub fn launch(
    port: u16,
    serial: &str,
    package: &str,
    activity: Option<&str>,
) -> Result<Launched, String> {
    check_package(package)?;
    let component = match activity {
        Some(a) if a.contains('/') => a.to_string(),
        Some(a) => format!("{package}/{a}"),
        None => launcher_activity(port, serial, package)?,
    };
    if !valid_component(&component) {
        return Err(format!("'{component}' is not a valid activity"));
    }
    let out = adb::shell_bytes(
        port,
        serial,
        &format!("am start -W -n {component}"),
        Duration::from_secs(60),
    )
    .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out);
    let (ok, total_ms, error) = parse_am_start(&text);
    if !ok {
        return Err(error.unwrap_or_else(|| format!("launch failed: {}", text.trim())));
    }
    Ok(Launched {
        component,
        total_ms,
        pid: pid(port, serial, package),
    })
}

/// PID of the app's main process, if it runs.
#[must_use]
pub fn pid(port: u16, serial: &str, package: &str) -> Option<u32> {
    if !valid_package(package) {
        return None;
    }
    adb::shell(port, serial, &format!("pidof {package}"))
        .ok()?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

pub fn force_stop(port: u16, serial: &str, package: &str) -> Result<(), String> {
    check_package(package)?;
    adb::shell(port, serial, &format!("am force-stop {package}"))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Whether the package is installed (`pm path`).
#[must_use]
pub fn is_installed(port: u16, serial: &str, package: &str) -> bool {
    valid_package(package)
        && adb::shell(port, serial, &format!("pm path {package}"))
            .is_ok_and(|o| o.starts_with("package:"))
}

pub fn uninstall(port: u16, serial: &str, package: &str) -> Result<(), String> {
    check_package(package)?;
    let out =
        adb::shell(port, serial, &format!("pm uninstall {package}")).map_err(|e| e.to_string())?;
    if out.contains("Success") {
        Ok(())
    } else {
        Err(format!("uninstall failed: {out}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adb::fake::FakeAdb;

    #[test]
    fn names_are_validated_before_any_shell() {
        assert!(valid_package("com.example.app_1"));
        for bad in [
            "",
            "noDot",
            "com..x",
            "com.x;reboot",
            "com.x y",
            "com.x&&rm",
        ] {
            assert!(!valid_package(bad), "{bad}");
        }
        assert!(valid_component("com.example/.MainActivity"));
        assert!(valid_component("com.example/com.example.Main$Inner"));
        assert!(!valid_component("com.example/.Main;reboot"));
        assert!(!valid_component("com.example"));
    }

    #[test]
    fn parses_tool_output() {
        assert_eq!(
            parse_badging_package(
                "package: name='com.umr.probe' versionCode='1' versionName='1.0'\nsdkVersion:'24'"
            )
            .as_deref(),
            Some("com.umr.probe")
        );
        let ok = "Starting: Intent { cmp=com.x/.A }\nStatus: ok\nLaunchState: COLD\nActivity: com.x/.A\nTotalTime: 412\nWaitTime: 415\nComplete\n";
        assert_eq!(parse_am_start(ok), (true, Some(412), None));
        let bad = "Starting: Intent { cmp=com.x/.B }\nError type 3\nError: Activity class {com.x/com.x.B} does not exist.\n";
        let (o, _, e) = parse_am_start(bad);
        assert!(!o && e.unwrap().starts_with("Error"));
    }

    #[test]
    fn lifecycle_against_fake_adb() {
        let f = FakeAdb::start(41, "");
        let s = "emulator-5554";
        f.set_shell(
            "cmd package resolve-activity --brief -a android.intent.action.MAIN -c android.intent.category.LAUNCHER com.x",
            b"priority=0 preferredOrder=0 match=0x108000\ncom.x/.MainActivity\n",
        );
        f.set_shell(
            "am start -W -n com.x/.MainActivity",
            b"Status: ok\nTotalTime: 300\n",
        );
        f.set_shell("pidof com.x", b"4242\n");
        f.set_shell("pm path com.x", b"package:/data/app/com.x/base.apk\n");
        f.set_shell("am force-stop com.x", b"");
        f.set_shell("pm uninstall com.x", b"Success\n");
        let l = launch(f.port, s, "com.x", None).unwrap();
        assert_eq!(
            l,
            Launched {
                component: "com.x/.MainActivity".into(),
                total_ms: Some(300),
                pid: Some(4242)
            }
        );
        assert!(is_installed(f.port, s, "com.x"));
        force_stop(f.port, s, "com.x").unwrap();
        uninstall(f.port, s, "com.x").unwrap();
        assert!(launch(f.port, s, "com.x;reboot", None).is_err());
    }
}
