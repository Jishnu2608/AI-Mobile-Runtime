//! Framework adapter contract (ADP-001, Phase 3 step 3.1).
//!
//! The Core holds no framework logic (`master_build` §6): every framework is an [`Adapter`] that
//! declares its [`Capabilities`] and turns a project folder into a running app on a device.
//!
//! | Item | Meaning |
//! |---|---|
//! | [`Adapter::detect`] | Confidence 0–100 that a folder is this framework's project |
//! | [`Adapter::validate`] | What is missing before a run (tools, wrapper, SDK) — with a fix |
//! | [`Adapter::start`] | Build, install, launch; returns a [`RunningApp`] |
//! | [`RunningApp::reload`] | `Hot` / `Restart` / `Full` — only what the capabilities declare; anything else is [`AdapterError::Unsupported`] |
//! | [`Event`] | Progress, log lines, structured [`Problem`]s (file and line), app started / reloaded / stopped |
//! | [`contract`] | Test kit every adapter must pass (static rules + a live scenario) |
//!
//! Long-running tools (Gradle, `flutter run`) run through the Supervisor in the run's process
//! group, so a session stop ends them and everything they started (Gradle daemon, Dart VM).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use umr_device::server::AdbServer;
use umr_supervisor::Supervisor;

pub mod contract;
pub mod problems;

pub use umr_resolver::project::Framework;

/// What an adapter can do. The Core never assumes more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[allow(clippy::struct_excessive_bools)] // independent yes/no capabilities
pub struct Capabilities {
    /// Code changes applied to the running app, state kept (Flutter hot reload, Fast Refresh).
    pub hot_reload: bool,
    /// App restarted in place without a native rebuild (Flutter hot restart).
    pub hot_restart: bool,
    /// Rebuilds reuse previous work (Gradle incremental build + daemon).
    pub incremental_build: bool,
    /// Build errors arrive as structured events, not only as console text.
    pub structured_build_events: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReloadKind {
    /// Keep state, apply changed code.
    Hot,
    /// Restart the app without a native rebuild.
    Restart,
    /// Rebuild, reinstall, relaunch.
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}

/// A build or runtime problem with its location, when the tool reports one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    pub severity: Severity,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    /// Tool that reported it: `kotlinc`, `javac`, `gradle`, `dart`, `flutter`, ...
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Building,
    Installing,
    Launching,
    Reloading,
}

/// What an adapter reports while it works.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Phase { phase: Phase, message: String },
    Log { line: String },
    Problem(Problem),
    AppStarted { package: String, pid: Option<u32> },
    Reloaded { kind: ReloadKind, ms: u64 },
    AppStopped { reason: String },
}

/// Receives events (the Core forwards them to the session and to clients).
pub type Sink = Arc<dyn Fn(Event) + Send + Sync>;

/// A sink that drops everything.
#[must_use]
pub fn null_sink() -> Sink {
    Arc::new(|_| {})
}

/// Something that must be fixed before a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Issue {
    pub message: String,
    pub fix: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterError {
    /// The folder is not this adapter's kind of project.
    NotApplicable(String),
    /// A tool or file the run needs is missing (see [`Adapter::validate`]).
    Missing(String),
    /// The build failed; `problems` holds what the tools reported.
    BuildFailed {
        summary: String,
        problems: Vec<Problem>,
    },
    InstallFailed(String),
    LaunchFailed(String),
    /// The capability is not declared.
    Unsupported(String),
    /// Anything else (process could not start, protocol error, ...).
    Failed(String),
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotApplicable(m) => write!(f, "not a project for this adapter: {m}"),
            Self::Missing(m) => write!(f, "missing: {m}"),
            Self::BuildFailed { summary, problems } => {
                write!(f, "build failed: {summary}")?;
                if let Some(p) = problems.first() {
                    write!(f, " (first problem: {}", p.message)?;
                    if let (Some(file), Some(line)) = (&p.file, p.line) {
                        write!(f, " at {file}:{line}")?;
                    }
                    write!(f, ")")?;
                }
                Ok(())
            }
            Self::InstallFailed(m) => write!(f, "install failed: {m}"),
            Self::LaunchFailed(m) => write!(f, "launch failed: {m}"),
            Self::Unsupported(m) => write!(f, "not supported: {m}"),
            Self::Failed(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for AdapterError {}

/// The device a run targets (the product's private adb server + serial).
#[derive(Debug, Clone)]
pub struct DeviceTarget {
    pub serial: String,
    pub adb: AdbServer,
}

/// Everything an adapter gets from the Core for one run.
#[derive(Debug, Clone)]
pub struct AdapterContext {
    pub project: PathBuf,
    /// Isolated child environment (umr-env recipe: `JAVA_HOME`, `ANDROID_HOME`, `FLUTTER_ROOT`, `PATH`,
    /// `ANDROID_USER_HOME`, `TEMP`, adb port, ...).
    pub env: HashMap<String, String>,
    pub sdk: PathBuf,
    pub jdk_home: Option<PathBuf>,
    pub flutter_root: Option<PathBuf>,
    pub device: DeviceTarget,
    pub supervisor: Supervisor,
    /// Supervisor group of this run (`run:<session id>`): stopped with the session.
    pub group: String,
    pub log_dir: PathBuf,
}

/// A framework adapter.
pub trait Adapter: Send + Sync {
    /// Stable id: `gradle-android`, `flutter`, ...
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn frameworks(&self) -> &'static [Framework];
    fn capabilities(&self) -> Capabilities;
    /// 0 = not this framework; 100 = certain.
    fn detect(&self, root: &Path) -> u8 {
        let d = umr_resolver::detect::detect(root);
        if self.frameworks().contains(&d.framework) {
            d.confidence
        } else {
            0
        }
    }
    /// Problems to fix before [`Adapter::start`]; empty = ready.
    fn validate(&self, ctx: &AdapterContext) -> Vec<Issue>;
    /// Builds, installs and launches the app.
    fn start(&self, ctx: &AdapterContext, sink: Sink) -> Result<Box<dyn RunningApp>, AdapterError>;
    /// ADP-006: what a saved file needs (`rel` is relative to the project, `/`-separated).
    /// `None`: nothing (not a source file, or the framework applies it by itself).
    /// Generated folders are filtered out before (see [`ignored`]).
    fn change_kind(&self, _rel: &str) -> Option<ReloadKind> {
        None
    }
}

/// Folders tools generate or own; changes there never trigger a reload.
pub const IGNORED_DIRS: &[&str] = &[
    ".git",
    ".idea",
    ".vscode",
    ".gradle",
    ".kotlin",
    ".cxx",
    ".dart_tool",
    ".expo",
    ".umr",
    "build",
    "node_modules",
    "Pods",
];

/// Whether a project-relative path is generated output or tool state.
#[must_use]
pub fn ignored(rel: &str) -> bool {
    let rel = rel.replace('\\', "/");
    let name = rel.rsplit('/').next().unwrap_or(&rel);
    rel.split('/').any(|seg| IGNORED_DIRS.contains(&seg))
        // tools rewrite these on every run (Flutter: android/local.properties)
        || name == "local.properties"
        || name.ends_with('~')
        || std::path::Path::new(name).extension().is_some_and(|e| e.eq_ignore_ascii_case("tmp"))
        || name.starts_with(".#")
}

/// The stronger of two reloads (`Full` > `Restart` > `Hot`).
#[must_use]
pub fn stronger(a: Option<ReloadKind>, b: Option<ReloadKind>) -> Option<ReloadKind> {
    let rank = |k: Option<ReloadKind>| match k {
        None => 0,
        Some(ReloadKind::Hot) => 1,
        Some(ReloadKind::Restart) => 2,
        Some(ReloadKind::Full) => 3,
    };
    if rank(b) > rank(a) {
        b
    } else {
        a
    }
}

/// A running app started by an adapter.
pub trait RunningApp: Send + std::fmt::Debug {
    /// Android package (`com.example.app`).
    fn package(&self) -> &str;
    /// Applies changes; returns how long it took. Kinds outside the capabilities return
    /// [`AdapterError::Unsupported`].
    fn reload(&mut self, kind: ReloadKind, sink: &Sink) -> Result<Duration, AdapterError>;
    /// Whether the app (and the adapter's tool, if it needs one) still runs.
    fn is_alive(&mut self) -> bool;
    /// Stops the app and the adapter's tools.
    fn stop(self: Box<Self>);
}

/// Picks the adapter with the highest detection confidence (≥ 40).
#[must_use]
pub fn choose<'a>(adapters: &'a [Box<dyn Adapter>], root: &Path) -> Option<(&'a dyn Adapter, u8)> {
    adapters
        .iter()
        .map(|a| (a.as_ref(), a.detect(root)))
        .filter(|(_, c)| *c >= 40)
        .max_by_key(|(_, c)| *c)
}

#[cfg(test)]
mod watch_tests {
    use super::*;

    #[test]
    fn generated_paths_are_ignored() {
        for p in [
            "build/app/outputs/x.apk",
            "android/app/build/tmp/a",
            "node_modules/react/index.js",
            ".dart_tool/package_config.json",
            "android/local.properties",
            "lib/main.dart~",
            ".git/HEAD",
        ] {
            assert!(ignored(p), "{p}");
        }
        for p in [
            "lib/main.dart",
            "App.js",
            "app/src/main/kotlin/A.kt",
            "android/app/build.gradle.kts",
        ] {
            assert!(!ignored(p), "{p}");
        }
        assert_eq!(
            stronger(Some(ReloadKind::Hot), Some(ReloadKind::Full)),
            Some(ReloadKind::Full)
        );
        assert_eq!(stronger(None, Some(ReloadKind::Hot)), Some(ReloadKind::Hot));
        assert_eq!(
            stronger(Some(ReloadKind::Restart), None),
            Some(ReloadKind::Restart)
        );
    }
}
