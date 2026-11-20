//! End-to-end tests of the `tumr` binary against a real `tumr-core` process.
//!
//! Needs `tumr-core.exe` built in the same target folder (the workspace test run builds it).

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

const TUMR: &str = env!("CARGO_BIN_EXE_tumr");

fn core_bin() -> PathBuf {
    let p = PathBuf::from(TUMR).with_file_name("tumr-core.exe");
    assert!(
        p.is_file(),
        "build tumr-core first (cargo build --workspace): {}",
        p.display()
    );
    p
}

struct Home(PathBuf);

impl Home {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("tumr-cli-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }

    fn tumr(&self, args: &[&str]) -> Output {
        Command::new(TUMR)
            .args(args)
            .env("TUMR_HOME", &self.0)
            .env("TUMR_CORE_BIN", core_bin())
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = self.tumr(&["stop"]); // never leave a Core running
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn full_lifecycle_status_start_doctor_stop() {
    let h = Home::new("lifecycle");

    let s = h.tumr(&["status"]);
    assert_eq!(s.status.code(), Some(1), "status when not running exits 1");
    assert!(stdout(&s).contains("not running"));

    let s = h.tumr(&["start"]);
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

    let s = h.tumr(&["start"]);
    assert!(stdout(&s).contains("already running"), "{}", stdout(&s));

    let s = h.tumr(&["status", "--json"]);
    assert!(s.status.success());
    let v: serde_json::Value = serde_json::from_slice(&s.stdout).unwrap();
    assert_eq!(v["running"], true);
    assert_eq!(PathBuf::from(v["data_root"].as_str().unwrap()), h.0);

    let s = h.tumr(&["doctor"]);
    let out = stdout(&s);
    assert!(
        out.contains("Windows 11") && out.contains("Data folder writable"),
        "{out}"
    );

    let s = h.tumr(&["stop"]);
    assert!(s.status.success());
    assert!(stdout(&s).contains("Runtime Core stopped"));

    let s = h.tumr(&["status"]);
    assert_eq!(s.status.code(), Some(1));
}

#[test]
fn doctor_starts_the_core_when_needed() {
    let h = Home::new("doctor-autostart");
    let s = h.tumr(&["doctor", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&s.stdout).expect("doctor --json output");
    assert!(v["checks"].as_array().unwrap().len() >= 4);
    assert_eq!(v["environment"]["readiness"].as_array().unwrap().len(), 4);
    assert!(v["environment"]["host"]["windows_build"].as_u64().unwrap() > 10_000);
    assert!(
        h.tumr(&["status"]).status.success(),
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
    let s = h.tumr(&["plan", proj.to_str().unwrap(), "--json"]);
    assert!(s.status.success(), "{}", String::from_utf8_lossy(&s.stderr));
    let v: serde_json::Value = serde_json::from_slice(&s.stdout).expect("plan --json output");
    assert_eq!(v["project"]["framework"], "expo");
    assert_eq!(v["plan"]["profile"], "expo-go");
    let text = h.tumr(&["plan", proj.to_str().unwrap()]);
    let out = stdout(&text);
    assert!(
        out.contains("Expo") && out.contains("Nothing was installed"),
        "{out}"
    );
}

#[test]
fn stop_when_not_running_is_ok_and_usage_errors_exit_2() {
    let h = Home::new("misc");
    let s = h.tumr(&["stop"]);
    assert!(s.status.success());
    assert!(stdout(&s).contains("not running"));
    assert_eq!(h.tumr(&["frobnicate"]).status.code(), Some(2));
}
