//! Integration tests that run the real `umr-core` binary as separate processes.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_umr-core");

struct TempRoot(PathBuf);

impl TempRoot {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("umr-it-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }

    fn pid_file(&self) -> PathBuf {
        self.0.join("run").join("core.pid")
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A running Core process that is always killed, even when a test panics.
struct CoreProcess(Child);

impl CoreProcess {
    fn spawn(root: &TempRoot) -> Self {
        let child = core(root, &["run"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Self(child)
    }

    fn id(&self) -> u32 {
        self.0.id()
    }

    /// Waits until the pid file names this process (ignores a stale file from a dead Core).
    fn wait_until_running(&mut self, root: &TempRoot) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            let current = std::fs::read_to_string(root.pid_file())
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok());
            if current == Some(self.id()) {
                return;
            }
            assert!(
                self.0.try_wait().unwrap().is_none(),
                "core exited before it started"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("core did not write its pid within 10 s");
    }

    /// Simulates a crash: terminate without clean shutdown.
    fn kill(mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Drop for CoreProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn core(root: &TempRoot, args: &[&str]) -> Command {
    let mut c = Command::new(BIN);
    c.args(args).env("UMR_HOME", &root.0).stdin(Stdio::null());
    c
}

#[test]
fn version_flag_prints_version() {
    let out = Command::new(BIN).arg("--version").output().unwrap();
    assert!(out.status.success());
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.starts_with("umr-core "), "{s}");
}

#[test]
fn unknown_arguments_exit_with_usage_error() {
    let out = Command::new(BIN).arg("bogus").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn second_core_exits_with_code_3_and_reports_first_pid() {
    let root = TempRoot::new("single-instance");
    let mut first = CoreProcess::spawn(&root);
    first.wait_until_running(&root);

    let second = core(&root, &["run"]).output().unwrap();
    assert_eq!(
        second.status.code(),
        Some(3),
        "second instance must refuse to start"
    );
    let err = String::from_utf8_lossy(&second.stderr);
    assert!(
        err.contains(&first.id().to_string()),
        "stderr should name the running pid: {err}"
    );
}

#[test]
fn lock_is_released_when_core_process_dies() {
    let root = TempRoot::new("crash-release");
    let mut first = CoreProcess::spawn(&root);
    first.wait_until_running(&root);
    first.kill(); // crash: the pid file stays behind, the OS releases the lock

    let mut again = CoreProcess::spawn(&root);
    again.wait_until_running(&root); // must start and overwrite the stale pid file
}

#[test]
fn first_start_creates_config_template_and_database() {
    let root = TempRoot::new("storage-files");
    let mut c = CoreProcess::spawn(&root);
    c.wait_until_running(&root);
    let config = std::fs::read_to_string(root.0.join("config.toml")).unwrap();
    assert!(config.contains("telemetry = false"), "{config}");
    // The pid file appears when the lock is taken; the database is opened just after.
    let db = root.0.join("umr.db");
    let start = Instant::now();
    while !db.is_file() && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(db.is_file(), "database not created within 10 s");
}

#[test]
fn invalid_config_stops_the_core_with_a_clear_message() {
    let root = TempRoot::new("bad-config");
    std::fs::write(root.0.join("config.toml"), "[core]\nlgo = \"debug\"\n").unwrap();
    let out = core(&root, &["run"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("config.toml") && err.contains("lgo"),
        "error must name the file and the bad key: {err}"
    );
}

#[test]
fn core_writes_a_log_file() {
    let root = TempRoot::new("log-file");
    let mut c = CoreProcess::spawn(&root);
    c.wait_until_running(&root);
    let logs = root.0.join("logs");
    let start = Instant::now();
    let mut found = false;
    while start.elapsed() < Duration::from_secs(5) && !found {
        found = std::fs::read_dir(&logs).unwrap().flatten().any(|e| {
            std::fs::read_to_string(e.path()).is_ok_and(|s| s.contains("Runtime Core started"))
        });
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        found,
        "log file with start message expected in {}",
        logs.display()
    );
}
