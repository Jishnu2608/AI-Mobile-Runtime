//! End-to-end tests of the `umr` binary against a real `umr-core` process.
//!
//! Needs `umr-core.exe` built in the same target folder (the workspace test run builds it).

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

const UMR: &str = env!("CARGO_BIN_EXE_umr");

fn core_bin() -> PathBuf {
    let p = PathBuf::from(UMR).with_file_name("umr-core.exe");
    assert!(
        p.is_file(),
        "build umr-core first (cargo build --workspace): {}",
        p.display()
    );
    p
}

struct Home(PathBuf);

impl Home {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("umr-cli-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }

    fn umr(&self, args: &[&str]) -> Output {
        Command::new(UMR)
            .args(args)
            .env("UMR_HOME", &self.0)
            .env("UMR_CORE_BIN", core_bin())
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = self.umr(&["stop"]); // never leave a Core running
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn full_lifecycle_status_start_doctor_stop() {
    let h = Home::new("lifecycle");

    let s = h.umr(&["status"]);
    assert_eq!(s.status.code(), Some(1), "status when not running exits 1");
    assert!(stdout(&s).contains("not running"));

    let s = h.umr(&["start"]);
    assert!(
        s.status.success(),
        "start: {}",
        String::from_utf8_lossy(&s.stderr)
    );
    assert!(
        stdout(&s).contains("Runtime Core started"),
        "{}",
        stdout(&s)
    );

    let s = h.umr(&["start"]);
    assert!(stdout(&s).contains("already running"), "{}", stdout(&s));

    let s = h.umr(&["status", "--json"]);
    assert!(s.status.success());
    let v: serde_json::Value = serde_json::from_slice(&s.stdout).unwrap();
    assert_eq!(v["running"], true);
    assert_eq!(PathBuf::from(v["data_root"].as_str().unwrap()), h.0);

    let s = h.umr(&["doctor"]);
    let out = stdout(&s);
    assert!(
        out.contains("Windows 11") && out.contains("Data folder writable"),
        "{out}"
    );

    let s = h.umr(&["stop"]);
    assert!(s.status.success());
    assert!(stdout(&s).contains("Runtime Core stopped"));

    let s = h.umr(&["status"]);
    assert_eq!(s.status.code(), Some(1));
}

#[test]
fn doctor_starts_the_core_when_needed() {
    let h = Home::new("doctor-autostart");
    let s = h.umr(&["doctor", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&s.stdout).expect("doctor --json output");
    assert!(v["checks"].as_array().unwrap().len() >= 4);
    assert_eq!(v["environment"]["readiness"].as_array().unwrap().len(), 4);
    assert!(v["environment"]["host"]["windows_build"].as_u64().unwrap() > 10_000);
    assert!(
        h.umr(&["status"]).status.success(),
        "Core keeps running after doctor"
    );
}

#[test]
fn plan_for_an_expo_project() {
    let h = Home::new("plan");
    let proj = h.0.join("app");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(
        proj.join("package.json"),
        r#"{"dependencies":{"expo":"~58.0.0","react-native":"0.88.1"}}"#,
    )
    .unwrap();
    let s = h.umr(&["plan", proj.to_str().unwrap(), "--json"]);
    assert!(s.status.success(), "{}", String::from_utf8_lossy(&s.stderr));
    let v: serde_json::Value = serde_json::from_slice(&s.stdout).expect("plan --json output");
    assert_eq!(v["project"]["framework"], "expo");
    assert_eq!(v["plan"]["profile"], "expo-go");
    let text = h.umr(&["plan", proj.to_str().unwrap()]);
    let out = stdout(&text);
    assert!(
        out.contains("Expo") && out.contains("Nothing was installed"),
        "{out}"
    );
}

#[test]
fn stop_when_not_running_is_ok_and_usage_errors_exit_2() {
    let h = Home::new("misc");
    let s = h.umr(&["stop"]);
    assert!(s.status.success());
    assert!(stdout(&s).contains("not running"));
    assert_eq!(h.umr(&["frobnicate"]).status.code(), Some(2));
}
