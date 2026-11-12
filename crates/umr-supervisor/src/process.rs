//! Supervised processes.
//!
//! * Each process tree runs in its own kill-on-close Job Object; stopping a process ends its
//!   children too (emulator → qemu, Gradle wrapper → daemon, npx → Metro).
//! * stdout and stderr go to one log file per process (appended across restarts).
//! * Restart policies: `Never` (emulator: a crash becomes a device error the session handles)
//!   or `OnFailure` with a budget (Metro / Flutter tool in Phase 3: max 3 restarts in 5 min).
//!   A clean exit (code 0) is never restarted.
//! * A monitor thread notices exits within ~200 ms.
//! * Groups: processes with the same `group` share one job that outlives each process (a Gradle
//!   build leaves its daemon running for the next build); [`Supervisor::stop_group`] ends it.
//! * Piped processes ([`Supervisor::start_piped`]) hand stdin / stdout to the caller (machine
//!   protocols such as `flutter run --machine`); stderr still goes to the log.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use umr_platform::job::KillOnCloseJob;

/// What happens when a process ends on its own with a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartPolicy {
    Never,
    /// Restart after `delay`, at most `max` times within `window`; then the process is failed.
    OnFailure {
        max: u32,
        window: Duration,
        delay: Duration,
    },
}

impl RestartPolicy {
    /// Dev servers (Metro, Flutter tool): 3 restarts within 5 minutes, 1 s apart.
    pub const DEV_SERVER: Self = Self::OnFailure {
        max: 3,
        window: Duration::from_secs(300),
        delay: Duration::from_secs(1),
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Running,
    /// Ended with a failure; a restart is scheduled.
    Restarting,
    /// Ended on its own with exit code 0.
    Exited,
    /// Ended with a failure and is not restarted (policy `Never` or budget used up).
    Failed,
    /// Stopped by the Supervisor.
    Stopped,
}

/// How to start a process.
#[derive(Debug, Clone)]
pub struct ProcessSpec {
    /// Unique name, e.g. `emulator:umr_standard_api36`.
    pub name: String,
    pub program: PathBuf,
    pub args: Vec<String>,
    /// Added to the inherited environment (umr-env recipe output).
    pub env: HashMap<String, String>,
    pub cwd: Option<PathBuf>,
    pub log: PathBuf,
    pub restart: RestartPolicy,
    /// Shared job (see the module docs); `None` = a job of its own.
    pub group: Option<String>,
}

impl ProcessSpec {
    #[must_use]
    pub fn new(name: &str, program: impl Into<PathBuf>, log: impl Into<PathBuf>) -> Self {
        Self {
            name: name.to_string(),
            program: program.into(),
            args: Vec::new(),
            env: HashMap::new(),
            cwd: None,
            log: log.into(),
            restart: RestartPolicy::Never,
            group: None,
        }
    }

    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }
}

/// Public view of a supervised process.
#[derive(Debug, Clone, Serialize)]
pub struct ProcessStatus {
    pub name: String,
    pub state: RunState,
    pub pid: Option<u32>,
    pub restarts: u32,
    pub exit_code: Option<i32>,
    pub started_unix: u64,
    pub log: String,
    pub last_error: Option<String>,
}

#[derive(Debug)]
struct Entry {
    spec: ProcessSpec,
    child: Option<Child>,
    job: Option<KillOnCloseJob>,
    state: RunState,
    pid: Option<u32>,
    started_unix: u64,
    recent_restarts: Vec<Instant>,
    total_restarts: u32,
    exit_code: Option<i32>,
    restart_at: Option<Instant>,
    last_error: Option<String>,
}

impl Entry {
    fn status(&self) -> ProcessStatus {
        ProcessStatus {
            name: self.spec.name.clone(),
            state: self.state,
            pid: self.pid,
            restarts: self.total_restarts,
            exit_code: self.exit_code,
            started_unix: self.started_unix,
            log: self.spec.log.display().to_string(),
            last_error: self.last_error.clone(),
        }
    }

    /// Ends the tree (job) and forgets the child.
    fn kill(&mut self) {
        if let Some(c) = self.child.as_mut() {
            let _ = c.kill();
            let _ = c.wait();
        }
        self.child = None;
        self.job = None; // closing the job ends every remaining member
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Starts `spec`; output into its log file (stdout piped instead when `piped`).
fn launch(spec: &ProcessSpec, append: bool, piped: bool) -> Result<Child, String> {
    if let Some(dir) = spec.log.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let mut out: File = if append {
        OpenOptions::new().create(true).append(true).open(&spec.log)
    } else {
        File::create(&spec.log)
    }
    .map_err(|e| format!("{}: {e}", spec.log.display()))?;
    if append {
        let _ = writeln!(out, "\n--- restarted by the Supervisor ---");
    }
    let err = out.try_clone().map_err(|e| e.to_string())?;
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args).envs(&spec.env).stderr(err);
    if piped {
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null()).stdout(out);
    }
    if let Some(d) = &spec.cwd {
        cmd.current_dir(d);
    }
    cmd.spawn()
        .map_err(|e| format!("cannot start {}: {e}", spec.program.display()))
}

#[derive(Debug, Default)]
struct Inner {
    procs: Mutex<HashMap<String, Entry>>,
    /// Shared jobs by group name.
    groups: Mutex<HashMap<String, KillOnCloseJob>>,
    shutdown: AtomicBool,
}

impl Inner {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        self.procs.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Puts the child into its group's job (returns `None`) or into a job of its own. A job
    /// failure (e.g. nested-job limits) is not fatal: the process is still killed directly.
    fn jobify(&self, spec: &ProcessSpec, child: &Child) -> Option<KillOnCloseJob> {
        if let Some(g) = &spec.group {
            let mut groups = self.groups.lock().unwrap_or_else(PoisonError::into_inner);
            if !groups.contains_key(g) {
                if let Ok(j) = KillOnCloseJob::new() {
                    groups.insert(g.clone(), j);
                }
            }
            if let Some(j) = groups.get(g) {
                let _ = j.assign(child);
            }
            return None;
        }
        KillOnCloseJob::new()
            .and_then(|j| j.assign(child).map(|()| j))
            .ok()
    }

    /// One monitor pass: notice exits, apply restart policies, run due restarts.
    fn tick(&self) {
        let now = Instant::now();
        for e in self.lock().values_mut() {
            match e.state {
                RunState::Running => {
                    let exited = e.child.as_mut().and_then(|c| c.try_wait().ok().flatten());
                    let Some(status) = exited else { continue };
                    e.exit_code = status.code();
                    e.kill();
                    if status.success() {
                        e.state = RunState::Exited;
                        continue;
                    }
                    match e.spec.restart {
                        RestartPolicy::Never => {
                            e.state = RunState::Failed;
                            e.last_error = Some(format!("exited with {status}"));
                        }
                        RestartPolicy::OnFailure { max, window, delay } => {
                            e.recent_restarts
                                .retain(|t| now.duration_since(*t) < window);
                            if e.recent_restarts.len() < max as usize {
                                e.state = RunState::Restarting;
                                e.restart_at = Some(now + delay);
                                e.last_error = Some(format!("exited with {status}; restarting"));
                            } else {
                                e.state = RunState::Failed;
                                e.last_error = Some(format!(
                                    "exited with {status}; {max} restarts within {} s used up, not restarted",
                                    window.as_secs()
                                ));
                            }
                        }
                    }
                }
                RunState::Restarting if e.restart_at.is_some_and(|t| now >= t) => {
                    e.restart_at = None;
                    match launch(&e.spec, true, false) {
                        Ok(child) => {
                            e.pid = Some(child.id());
                            e.job = self.jobify(&e.spec, &child);
                            e.child = Some(child);
                            e.state = RunState::Running;
                            e.started_unix = now_unix();
                            e.recent_restarts.push(now);
                            e.total_restarts += 1;
                        }
                        Err(err) => {
                            e.state = RunState::Failed;
                            e.last_error = Some(err);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

fn monitor(inner: &Weak<Inner>) {
    loop {
        std::thread::sleep(Duration::from_millis(200));
        let Some(i) = inner.upgrade() else { return };
        if i.shutdown.load(Ordering::SeqCst) {
            return;
        }
        i.tick();
    }
}

/// The Supervisor. Cloning shares the same process table.
#[derive(Debug, Clone)]
pub struct Supervisor {
    inner: Arc<Inner>,
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl Supervisor {
    /// Creates a Supervisor and its monitor thread (which ends with the last clone).
    #[must_use]
    pub fn new() -> Self {
        let inner = Arc::new(Inner::default());
        let weak = Arc::downgrade(&inner);
        std::thread::Builder::new()
            .name("umr-supervisor".into())
            .spawn(move || monitor(&weak))
            .ok();
        Self { inner }
    }

    /// Starts a process; returns its PID. A name that is still running (or restarting) is refused.
    pub fn start(&self, spec: ProcessSpec) -> Result<u32, String> {
        self.start_inner(spec, false).map(|(pid, _)| pid)
    }

    /// Starts a process whose stdin / stdout the caller drives (restart policy forced to
    /// `Never`: a restarted process could not be handed back).
    pub fn start_piped(
        &self,
        mut spec: ProcessSpec,
    ) -> Result<(u32, std::process::ChildStdin, std::process::ChildStdout), String> {
        spec.restart = RestartPolicy::Never;
        let (pid, io) = self.start_inner(spec, true)?;
        io.ok_or_else(|| "no pipes".to_string())
            .map(|(i, o)| (pid, i, o))
    }

    #[allow(clippy::type_complexity)]
    fn start_inner(
        &self,
        spec: ProcessSpec,
        piped: bool,
    ) -> Result<
        (
            u32,
            Option<(std::process::ChildStdin, std::process::ChildStdout)>,
        ),
        String,
    > {
        let mut procs = self.inner.lock();
        if let Some(e) = procs.get(&spec.name) {
            if matches!(e.state, RunState::Running | RunState::Restarting) {
                return Err(format!("{} is already running", spec.name));
            }
        }
        let mut child = launch(&spec, false, piped)?;
        let job = self.inner.jobify(&spec, &child);
        let io = if piped {
            child.stdin.take().zip(child.stdout.take())
        } else {
            None
        };
        let pid = child.id();
        procs.insert(
            spec.name.clone(),
            Entry {
                spec,
                child: Some(child),
                job,
                state: RunState::Running,
                pid: Some(pid),
                started_unix: now_unix(),
                recent_restarts: Vec::new(),
                total_restarts: 0,
                exit_code: None,
                restart_at: None,
                last_error: None,
            },
        );
        Ok((pid, io))
    }

    /// Ends every process of a group and whatever they left behind (e.g. a Gradle daemon).
    pub fn stop_group(&self, group: &str) {
        for e in self.inner.lock().values_mut() {
            if e.spec.group.as_deref() == Some(group) {
                e.kill();
                if matches!(e.state, RunState::Running | RunState::Restarting) {
                    e.state = RunState::Stopped;
                }
            }
        }
        self.inner
            .groups
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(group);
    }

    #[must_use]
    pub fn status(&self, name: &str) -> Option<ProcessStatus> {
        self.inner.lock().get(name).map(Entry::status)
    }

    /// All processes, sorted by name.
    #[must_use]
    pub fn list(&self) -> Vec<ProcessStatus> {
        let mut v: Vec<ProcessStatus> = self.inner.lock().values().map(Entry::status).collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    /// Checks the process directly (does not wait for the monitor).
    #[must_use]
    pub fn is_running(&self, name: &str) -> bool {
        let mut procs = self.inner.lock();
        let Some(e) = procs.get_mut(name) else {
            return false;
        };
        e.state == RunState::Running
            && e.child
                .as_mut()
                .is_some_and(|c| matches!(c.try_wait(), Ok(None)))
    }

    /// Waits until the process is no longer running; false on timeout.
    #[must_use]
    pub fn wait_exit(&self, name: &str, timeout: Duration) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if !self.is_running(name) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }

    /// Ends the process tree now (no restart). Returns whether it was running.
    #[allow(clippy::must_use_candidate)] // callers may only want the side effect
    pub fn stop(&self, name: &str) -> bool {
        let mut procs = self.inner.lock();
        let Some(e) = procs.get_mut(name) else {
            return false;
        };
        let was = matches!(e.state, RunState::Running | RunState::Restarting);
        e.kill();
        e.restart_at = None;
        if was {
            e.state = RunState::Stopped;
        }
        was
    }

    /// Forgets a process that is not running.
    pub fn remove(&self, name: &str) {
        let mut procs = self.inner.lock();
        if let Some(e) = procs.get_mut(name) {
            e.kill();
        }
        procs.remove(name);
    }

    /// Stops everything and ends the monitor (Core shutdown).
    pub fn shutdown(&self) {
        self.inner.shutdown.store(true, Ordering::SeqCst);
        self.inner
            .groups
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        for e in self.inner.lock().values_mut() {
            e.kill();
            if matches!(e.state, RunState::Running | RunState::Restarting) {
                e.state = RunState::Stopped;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use umr_platform::process::{descendants, is_running};

    fn log(tag: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("umr-sup-{}", std::process::id()))
            .join(format!("{tag}.log"))
    }

    fn cmd(name: &str, script: &str) -> ProcessSpec {
        ProcessSpec::new(name, "cmd.exe", log(name)).args(["/d", "/c", script])
    }

    fn wait_for(sup: &Supervisor, name: &str, state: RunState, secs: u64) -> ProcessStatus {
        let start = Instant::now();
        loop {
            let s = sup.status(name).unwrap();
            if s.state == state || start.elapsed() > Duration::from_secs(secs) {
                return s;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn clean_exit_is_not_restarted_and_output_is_logged() {
        let sup = Supervisor::new();
        let mut spec = cmd("ok", "echo hello-from-tool");
        spec.restart = RestartPolicy::DEV_SERVER;
        sup.start(spec).unwrap();
        let s = wait_for(&sup, "ok", RunState::Exited, 10);
        assert_eq!(s.state, RunState::Exited);
        assert_eq!((s.exit_code, s.restarts), (Some(0), 0));
        assert!(std::fs::read_to_string(log("ok"))
            .unwrap()
            .contains("hello-from-tool"));
    }

    #[test]
    fn failures_restart_within_budget_then_fail() {
        let sup = Supervisor::new();
        let mut spec = cmd("crashy", "exit 3");
        spec.restart = RestartPolicy::OnFailure {
            max: 2,
            window: Duration::from_secs(60),
            delay: Duration::from_millis(50),
        };
        sup.start(spec).unwrap();
        let s = wait_for(&sup, "crashy", RunState::Failed, 15);
        assert_eq!(s.state, RunState::Failed, "{s:?}");
        assert_eq!(s.restarts, 2);
        assert_eq!(s.exit_code, Some(3));
        assert!(s.last_error.unwrap().contains("used up"));
        let text = std::fs::read_to_string(log("crashy")).unwrap();
        assert_eq!(text.matches("restarted by the Supervisor").count(), 2);
    }

    #[test]
    fn group_outlives_its_process_until_stopped() {
        let sup = Supervisor::new();
        // the process starts a background child (like a Gradle daemon) and exits at once
        let mut spec = cmd("build", "start /b ping -n 60 127.0.0.1 >nul");
        spec.group = Some("project:x".into());
        let pid = sup.start(spec).unwrap();
        let start = Instant::now();
        let daemon = loop {
            if let Some(p) = descendants(pid)
                .unwrap_or_default()
                .into_iter()
                .find(|p| p.exe_name.eq_ignore_ascii_case("PING.EXE"))
            {
                break p.pid;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "no background child"
            );
            std::thread::sleep(Duration::from_millis(50));
        };
        let _ = wait_for(&sup, "build", RunState::Exited, 10);
        assert!(
            is_running(daemon).unwrap(),
            "the background child survives the build"
        );
        sup.stop_group("project:x");
        let start = Instant::now();
        while is_running(daemon).unwrap_or(false) && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!is_running(daemon).unwrap_or(false), "stop_group ends it");
    }

    #[test]
    fn piped_process_talks_to_the_caller() {
        use std::io::{BufRead, BufReader, Write as _};
        let sup = Supervisor::new();
        let spec = ProcessSpec::new("echo", "findstr.exe", log("echo")).args(["x"]);
        let (_pid, mut stdin, stdout) = sup.start_piped(spec).unwrap();
        writeln!(stdin, "abc").unwrap();
        writeln!(stdin, "xyz").unwrap();
        drop(stdin);
        let mut line = String::new();
        BufReader::new(stdout).read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "xyz");
        let _ = wait_for(&sup, "echo", RunState::Exited, 10);
    }

    #[test]
    fn never_policy_reports_failure() {
        let sup = Supervisor::new();
        sup.start(cmd("emu-like", "exit 1")).unwrap();
        let s = wait_for(&sup, "emu-like", RunState::Failed, 10);
        assert_eq!((s.state, s.restarts), (RunState::Failed, 0));
    }

    #[test]
    fn stop_ends_the_whole_tree_and_names_are_unique() {
        let sup = Supervisor::new();
        let pid = sup.start(cmd("tree", "ping -n 60 127.0.0.1 >nul")).unwrap();
        assert!(sup
            .start(cmd("tree", "exit 0"))
            .unwrap_err()
            .contains("already running"));
        // wait for the grandchild (ping) to appear
        let start = Instant::now();
        let ping = loop {
            if let Some(p) = descendants(pid).unwrap_or_default().into_iter().next() {
                break p.pid;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "child never started"
            );
            std::thread::sleep(Duration::from_millis(50));
        };
        assert!(sup.is_running("tree"));
        assert!(sup.stop("tree"));
        assert!(sup.wait_exit("tree", Duration::from_secs(5)));
        let start = Instant::now();
        while is_running(ping).unwrap_or(false) && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            !is_running(ping).unwrap_or(false),
            "grandchild ended with the job"
        );
        assert_eq!(sup.status("tree").unwrap().state, RunState::Stopped);
        // a stopped name can be started again
        sup.start(cmd("tree", "exit 0")).unwrap();
        sup.shutdown();
    }
}
