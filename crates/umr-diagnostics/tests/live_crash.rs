//! Live check (DIA-006): `collect_crash_buffer` end-to-end against a real device, through the
//! product's own private adb server. Ignored by default — run manually with an emulator booted
//! on port 5137 (`umr device boot <profile>`) and a package to crash:
//!
//! ```text
//! umr_crash_pkg=com.umr.flutter_demo umr_crash_serial=emulator-5554 \
//!   cargo test -p umr-diagnostics --test live_crash -- --ignored --nocapture
//! ```

#[test]
#[ignore = "needs a booted emulator on the product's private adb port"]
fn collects_a_real_crash_from_the_device() {
    let port: u16 = std::env::var("umr_crash_port")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5137);
    let serial = std::env::var("umr_crash_serial").unwrap_or_else(|_| "emulator-5554".into());
    let package =
        std::env::var("umr_crash_pkg").expect("set umr_crash_pkg to an installed, running package");

    // force a real, on-device crash the same way a developer would reproduce one
    let status = std::process::Command::new(
        std::env::var("umr_adb_exe")
            .unwrap_or_else(|_| r"D:\UMR\android-sdk\platform-tools\adb.exe".into()),
    )
    .args([
        "-P",
        &port.to_string(),
        "-s",
        &serial,
        "logcat",
        "-c",
        "-b",
        "crash",
    ])
    .status()
    .expect("clear the crash buffer");
    assert!(status.success());

    let status = std::process::Command::new(
        std::env::var("umr_adb_exe")
            .unwrap_or_else(|_| r"D:\UMR\android-sdk\platform-tools\adb.exe".into()),
    )
    .args([
        "-P",
        &port.to_string(),
        "-s",
        &serial,
        "shell",
        "am",
        "crash",
        &package,
    ])
    .status()
    .expect("trigger the crash");
    assert!(status.success());

    std::thread::sleep(std::time::Duration::from_millis(500));

    let events =
        umr_diagnostics::collect_crash_buffer(port, &serial).expect("read the crash buffer");
    println!(
        "collected {} crash event(s) from {package} on {serial}:",
        events.len()
    );
    for e in &events {
        println!(
            "  {:?} pid={:?} message={:?} frames={} file={:?} line={:?}",
            e.kind,
            e.pid,
            e.message,
            e.stack.len(),
            e.file,
            e.line
        );
    }
    assert_eq!(events.len(), 1, "exactly the one crash we just triggered");
    assert_eq!(events[0].package.as_deref(), Some(package.as_str()));
    assert_ne!(events[0].message, String::new());
    assert!(
        !events[0].stack.is_empty(),
        "a Java/Kotlin crash always has a stack"
    );
}
