//! React Native / Expo adapter (Phase 3 step 3.4: ADP-004, ADP-005, ADP-007).
//!
//! Two modes, chosen from the project:
//!
//! | Mode | When | Run |
//! |---|---|---|
//! | Expo Go (fast path) | Expo project without an `android` folder | Metro via `expo start --go --android`: Expo CLI installs Expo Go if needed and opens the project — no native build |
//! | Native | bare React Native, or Expo with `android` (dev build / prebuild) | Metro (`react-native start` / `expo start --dev-client`) + Gradle build of `android` (Gradle adapter) + install + launch |
//!
//! ADP-007 (POC-006 findings 7 and 8): Metro listens on `localhost` only (never the LAN) and the
//! device reaches it through `adb reverse tcp:<port>`; `CI` is never set, because it turns off
//! Metro's file watching (no Fast Refresh).
//!
//! Capabilities: Fast Refresh (hot reload) is Metro's own — it applies saved files by itself, so
//! a `hot` reload only confirms it; `restart` reloads the JS bundle (`R R` on the device); `full`
//! restarts Metro (Expo Go) or rebuilds and reinstalls the native app.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use umr_adapter::problems::{parse_line, LogFollower};
use umr_adapter::{
    Adapter, AdapterContext, AdapterError, Capabilities, Event, Framework, Issue, Phase, Problem,
    ReloadKind, RunningApp, Severity, Sink,
};
use umr_supervisor::{ProcessSpec, RunState};

/// Longest project path for native React Native builds: `CMake` keeps object paths under 250
/// characters, and React Native's codegen nests ~170 characters below the project.
pub const MAX_NATIVE_PATH: usize = 80;

/// Package of the Expo Go app.
pub const EXPO_GO: &str = "host.exp.exponent";
/// Longest wait for dependencies, Metro's first bundle, or a native build.
pub const SLOW: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Default, Clone, Copy)]
pub struct ReactNative;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    ExpoGo,
    Native,
}

/// Package manager from the lockfile (npm when there is none).
#[must_use]
pub fn package_manager(root: &Path) -> &'static str {
    if root.join("pnpm-lock.yaml").is_file() {
        "pnpm"
    } else if root.join("yarn.lock").is_file() {
        "yarn"
    } else if root.join("bun.lockb").is_file() || root.join("bun.lock").is_file() {
        "bun"
    } else {
        "npm"
    }
}

/// Dependencies are fully installed: the package manager's completion marker exists (a folder
/// with only some packages, e.g. after an interrupted install, does not count).
#[must_use]
pub fn dependencies_installed(root: &Path) -> bool {
    let nm = root.join("node_modules");
    if !nm.is_dir() {
        return false;
    }
    match package_manager(root) {
        "pnpm" => nm.join(".modules.yaml").is_file(),
        "yarn" => nm.join(".yarn-state.yml").is_file() || nm.join(".yarn-integrity").is_file(),
        "npm" => nm.join(".package-lock.json").is_file(),
        _ => true,
    }
}

fn package_json(root: &Path) -> serde_json::Value {
    std::fs::read_to_string(root.join("package.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(serde_json::Value::Null)
}

/// Whether `expo` is a dependency.
#[must_use]
pub fn uses_expo(root: &Path) -> bool {
    let p = package_json(root);
    ["dependencies", "devDependencies"]
        .iter()
        .any(|k| p[k].get("expo").is_some())
}

#[must_use]
pub fn mode(root: &Path) -> Mode {
    if uses_expo(root) && !root.join("android").is_dir() {
        Mode::ExpoGo
    } else {
        Mode::Native
    }
}

/// `expo start` / `react-native start` arguments (ADP-007: localhost only).
#[must_use]
/// `install_expo_go`: Expo Go is missing, so Expo CLI (`--android`) downloads and installs it.
/// Otherwise the product opens the project itself, without Expo CLI's online Expo Go check.
pub fn metro_args(root: &Path, mode: Mode, port: u16, install_expo_go: bool) -> Vec<String> {
    let port = port.to_string();
    let mut a: Vec<String> = if uses_expo(root) {
        let mut v = vec!["npx", "expo", "start", "--localhost", "--port", &port];
        match mode {
            Mode::ExpoGo if install_expo_go => v.extend(["--go", "--android"]),
            Mode::ExpoGo => v.push("--go"),
            Mode::Native => v.push("--dev-client"),
        }
        v.into_iter().map(String::from).collect()
    } else {
        [
            "npx",
            "react-native",
            "start",
            "--host",
            "127.0.0.1",
            "--port",
            &port,
        ]
        .into_iter()
        .map(String::from)
        .collect()
    };
    a.insert(0, "/c".into());
    a.insert(0, "/d".into());
    a
}

/// Something accepts connections on localhost:`port`.
fn port_open(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_millis(300),
    )
    .is_ok()
}

/// First free port from 8081 (React Native's default, which native builds expect).
fn metro_port() -> Result<u16, AdapterError> {
    (8081..=8180)
        .find(|&p| umr_supervisor::ports::bindable(p))
        .ok_or_else(|| AdapterError::Failed("no free port for Metro in 8081..8180".into()))
}

/// Expo / Metro output that marks a finished bundle.
#[must_use]
pub fn bundled(line: &str) -> bool {
    let l = line.trim();
    (l.contains("Bundled") && !l.contains("Bundling failed")) || l.starts_with("BUNDLE ")
}

fn bundling_failed(line: &str) -> bool {
    line.contains("Bundling failed") || line.contains("Unable to resolve module")
}

fn env_for(ctx: &AdapterContext) -> std::collections::HashMap<String, String> {
    let mut env = ctx.env.clone();
    env.remove("CI"); // ADP-007: CI turns off Metro's watcher
    env.insert("ANDROID_HOME".into(), ctx.sdk.display().to_string());
    env.insert("ANDROID_SDK_ROOT".into(), ctx.sdk.display().to_string());
    env.insert("EXPO_NO_TELEMETRY".into(), "1".into());
    if let Some(j) = &ctx.jdk_home {
        env.insert("JAVA_HOME".into(), j.display().to_string());
    }
    env
}

fn adb(ctx: &AdapterContext, args: &[&str]) -> Result<(), AdapterError> {
    let out = std::process::Command::new(&ctx.device.adb.adb_exe)
        .args([
            "-P",
            &ctx.device.adb.port.to_string(),
            "-s",
            &ctx.device.serial,
        ])
        .args(args)
        .envs(&ctx.device.adb.env)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| AdapterError::Failed(format!("adb: {e}")))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(AdapterError::Failed(format!(
            "adb {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

/// Runs a one-shot tool (dependency install) in the run's group; waits for it.
fn run_tool(
    ctx: &AdapterContext,
    name: &str,
    args: &[&str],
    sink: &Sink,
) -> Result<(), AdapterError> {
    let pname = format!("{name}:{}", ctx.group);
    let log = ctx
        .log_dir
        .join(format!("{name}-{}.log", ctx.group.replace(':', "-")));
    let mut spec = ProcessSpec::new(&pname, "cmd.exe", &log)
        .args(["/d", "/c"].iter().chain(args.iter()).copied());
    spec.env = env_for(ctx);
    spec.cwd = Some(ctx.project.clone());
    spec.group = Some(ctx.group.clone());
    sink(Event::Phase {
        phase: Phase::Building,
        message: args.join(" "),
    });
    ctx.supervisor
        .start(spec)
        .map_err(|e| AdapterError::Failed(format!("cannot start {name}: {e}")))?;
    let start = Instant::now();
    loop {
        let st = ctx.supervisor.status(&pname);
        if !matches!(st.as_ref().map(|s| s.state), Some(RunState::Running)) {
            let code = st.and_then(|s| s.exit_code);
            if code == Some(0) {
                return Ok(());
            }
            let text = std::fs::read_to_string(&log).unwrap_or_default();
            let tail: Vec<&str> = text.lines().rev().take(8).collect();
            return Err(AdapterError::BuildFailed {
                summary: format!(
                    "`{}` failed ({code:?}): {}",
                    args.join(" "),
                    tail.into_iter().rev().collect::<Vec<_>>().join(" | ")
                ),
                problems: Vec::new(),
            });
        }
        if start.elapsed() > SLOW {
            ctx.supervisor.stop(&pname);
            return Err(AdapterError::Failed(format!(
                "`{}` did not finish",
                args.join(" ")
            )));
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

/// Metro started for this run.
#[derive(Debug)]
struct Metro {
    name: String,
    log: PathBuf,
    follower: LogFollower,
    port: u16,
}

fn start_metro(ctx: &AdapterContext, mode: Mode, sink: &Sink) -> Result<Metro, AdapterError> {
    let port = metro_port()?;
    // ADP-007: the device reaches Metro on localhost through adb
    adb(
        ctx,
        &["reverse", &format!("tcp:{port}"), &format!("tcp:{port}")],
    )?;
    let name = format!("metro:{}", ctx.group);
    let log = ctx
        .log_dir
        .join(format!("metro-{}.log", ctx.group.replace(':', "-")));
    let _ = std::fs::write(&log, b"");
    let follower = LogFollower::from_end(&log);
    let (aport, serial) = (ctx.device.adb.port, ctx.device.serial.clone());
    let have_go = mode == Mode::ExpoGo && umr_device::app::is_installed(aport, &serial, EXPO_GO);
    let mut spec = ProcessSpec::new(&name, "cmd.exe", &log).args(metro_args(
        &ctx.project,
        mode,
        port,
        mode == Mode::ExpoGo && !have_go,
    ));
    spec.env = env_for(ctx);
    if mode == Mode::Native && !uses_expo(&ctx.project) {
        spec.env.insert("RCT_METRO_PORT".into(), port.to_string());
    }
    spec.cwd = Some(ctx.project.clone());
    spec.group = Some(ctx.group.clone());
    sink(Event::Phase {
        phase: Phase::Building,
        message: format!("Metro on localhost:{port}"),
    });
    ctx.supervisor
        .start(spec)
        .map_err(|e| AdapterError::Failed(format!("cannot start Metro: {e}")))?;
    if have_go {
        // open the project once Metro listens
        let start = Instant::now();
        while !port_open(port) {
            if !ctx.supervisor.is_running(&name) || start.elapsed() > Duration::from_secs(180) {
                break; // wait_ready reports why
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        let _ = umr_device::app::force_stop(aport, &serial, EXPO_GO);
        umr_device::adb::shell(aport, &serial, &open_expo_go(port))
            .map_err(|e| AdapterError::LaunchFailed(e.to_string()))?;
    }
    Ok(Metro {
        name,
        log,
        follower,
        port,
    })
}

/// Reads Metro output: events, problems; returns `Some(true)` on a finished bundle,
/// `Some(false)` on a failed one.
fn pump(m: &mut Metro, root: &Path, sink: &Sink, problems: &mut Vec<Problem>) -> Option<bool> {
    let mut outcome = None;
    for line in m.follower.new_lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Some(p) = parse_line(&line, root) {
            if !problems.contains(&p) {
                sink(Event::Problem(p.clone()));
                problems.push(p);
            }
        }
        if bundled(&line) {
            sink(Event::Phase {
                phase: Phase::Launching,
                message: line.trim().to_string(),
            });
            outcome = Some(true);
        } else if bundling_failed(&line) {
            outcome = Some(false);
        }
        sink(Event::Log { line });
    }
    outcome
}

/// `am start` that opens the project in Expo Go from Metro on localhost.
#[must_use]
pub fn open_expo_go(port: u16) -> String {
    format!("am start -a android.intent.action.VIEW -d exp://127.0.0.1:{port} {EXPO_GO}")
}

/// Waits for Metro's bundle and the app process.
fn wait_ready(
    ctx: &AdapterContext,
    m: &mut Metro,
    package: &str,
    sink: &Sink,
) -> Result<(), AdapterError> {
    let start = Instant::now();
    let mut problems = Vec::new();
    let mut bundled_ok = false;
    loop {
        match pump(m, &ctx.project, sink, &mut problems) {
            Some(true) => bundled_ok = true,
            Some(false) => {
                return Err(AdapterError::BuildFailed {
                    summary: "Metro could not bundle the app".into(),
                    problems,
                })
            }
            None => {}
        }
        if !ctx.supervisor.is_running(&m.name) {
            let text = std::fs::read_to_string(&m.log).unwrap_or_default();
            let tail: Vec<&str> = text.lines().rev().take(6).collect();
            return Err(AdapterError::Failed(format!(
                "Metro stopped: {}",
                tail.into_iter().rev().collect::<Vec<_>>().join(" | ")
            )));
        }
        let running =
            umr_device::app::pid(ctx.device.adb.port, &ctx.device.serial, package).is_some();
        if bundled_ok && running {
            return Ok(());
        }
        if start.elapsed() > SLOW {
            return Err(AdapterError::Failed(
                "the app did not load its bundle in time".into(),
            ));
        }
        std::thread::sleep(Duration::from_millis(400));
    }
}

/// Native app: Gradle build of `android/`, install, launch.
fn native_build(ctx: &AdapterContext, port: u16, sink: &Sink) -> Result<String, AdapterError> {
    let mut g = ctx.clone();
    g.project = ctx.project.join("android");
    g.env = env_for(ctx);
    g.env.insert("RCT_METRO_PORT".into(), port.to_string());
    let module = umr_adapter_gradle::app_module(&g.project).unwrap_or_else(|| "app".into());
    // only the device's ABI: React Native compiles C++ per ABI (4x the work for all of them)
    let abi = umr_device::adb::shell(
        ctx.device.adb.port,
        &ctx.device.serial,
        "getprop ro.product.cpu.abi",
    )
    .ok()
    .filter(|a| {
        !a.is_empty()
            && a.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    })
    .unwrap_or_else(|| "x86_64".into());
    let apk = umr_adapter_gradle::build_with(
        &g,
        &module,
        &[format!("-PreactNativeArchitectures={abi}")],
        sink,
    )?;
    let package = umr_adapter_gradle::install(&g, &apk, sink)?;
    umr_adapter_gradle::launch(&g, &package, sink)?;
    Ok(package)
}

impl Adapter for ReactNative {
    fn id(&self) -> &'static str {
        "react-native"
    }
    fn display_name(&self) -> &'static str {
        "React Native / Expo"
    }
    fn frameworks(&self) -> &'static [Framework] {
        &[Framework::ReactNative, Framework::Expo]
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            hot_reload: true,
            hot_restart: true,
            incremental_build: true,
            structured_build_events: false,
        }
    }

    fn validate(&self, ctx: &AdapterContext) -> Vec<Issue> {
        let mut v = Vec::new();
        let root = &ctx.project;
        if !root.join("package.json").is_file() {
            v.push(Issue {
                message: "no package.json in the project folder".into(),
                fix: "open the app's root folder".into(),
            });
        }
        let path = ctx
            .env
            .get("PATH")
            .or_else(|| ctx.env.get("Path"))
            .cloned()
            .unwrap_or_default();
        if !path
            .split(';')
            .any(|d| Path::new(d).join("node.exe").is_file())
        {
            v.push(Issue {
                message: "Node.js is not available".into(),
                fix: "run `umr install` for this project".into(),
            });
        }
        if mode(root) == Mode::Native {
            let len = root.display().to_string().len();
            if len > MAX_NATIVE_PATH {
                v.push(Issue {
                    message: format!(
                        "the project folder path has {len} characters; React Native's C++ build (CMake / ninja) fails beyond the Windows path limit with folders deeper than {MAX_NATIVE_PATH}"
                    ),
                    fix: r"move the project to a short folder such as C:\dev\app".into(),
                });
            }
            if !root.join("android").join("gradlew.bat").is_file() {
                v.push(Issue {
                    message: "android/gradlew.bat is missing".into(),
                    fix: if uses_expo(root) {
                        "run `npx expo prebuild --platform android` once, or remove the android folder to use Expo Go".into()
                    } else {
                        "restore the android folder of the React Native project".into()
                    },
                });
            }
            if !ctx
                .jdk_home
                .as_ref()
                .is_some_and(|j| j.join("bin").join("java.exe").is_file())
            {
                v.push(Issue {
                    message: "no JDK selected for the native build".into(),
                    fix: "run `umr install` for this project".into(),
                });
            }
        }
        v
    }

    /// JS / TS: nothing — Metro's Fast Refresh applies it. Dependencies, Metro / Babel / app
    /// configuration and native code: full (Metro restart, native rebuild).
    fn change_kind(&self, rel: &str) -> Option<ReloadKind> {
        let name = rel.rsplit('/').next().unwrap_or(rel);
        let config = matches!(
            name,
            "package.json"
                | "app.json"
                | "app.config.js"
                | "app.config.ts"
                | "babel.config.js"
                | "metro.config.js"
                | "package-lock.json"
                | "yarn.lock"
                | "pnpm-lock.yaml"
        ) && !rel.contains('/');
        (config || rel.starts_with("android/")).then_some(ReloadKind::Full)
    }

    fn start(&self, ctx: &AdapterContext, sink: Sink) -> Result<Box<dyn RunningApp>, AdapterError> {
        if let Some(i) = self.validate(ctx).into_iter().next() {
            return Err(AdapterError::Missing(format!("{} ({})", i.message, i.fix)));
        }
        let root = &ctx.project;
        if !dependencies_installed(root) {
            let pm = package_manager(root);
            run_tool(ctx, pm, &[pm, "install"], &sink)?;
        }
        let mode = mode(root);
        let mut metro = start_metro(ctx, mode, &sink)?;
        let package = match mode {
            // Expo CLI installs / opens Expo Go itself (`--android`)
            Mode::ExpoGo => EXPO_GO.to_string(),
            Mode::Native => native_build(ctx, metro.port, &sink)?,
        };
        if let Err(e) = wait_ready(ctx, &mut metro, &package, &sink) {
            ctx.supervisor.stop(&metro.name);
            return Err(e);
        }
        if mode == Mode::ExpoGo {
            sink(Event::AppStarted {
                package: package.clone(),
                pid: umr_device::app::pid(ctx.device.adb.port, &ctx.device.serial, &package),
            });
        }
        Ok(Box::new(RnApp {
            ctx: ctx.clone(),
            mode,
            package,
            metro: Some(metro),
        }))
    }
}

#[derive(Debug)]
struct RnApp {
    ctx: AdapterContext,
    mode: Mode,
    package: String,
    metro: Option<Metro>,
}

impl RnApp {
    fn metro_running(&self) -> bool {
        self.metro
            .as_ref()
            .is_some_and(|m| self.ctx.supervisor.is_running(&m.name))
    }

    fn stop_metro(&mut self) {
        if let Some(m) = self.metro.take() {
            self.ctx.supervisor.stop(&m.name);
            let _ = self
                .ctx
                .supervisor
                .wait_exit(&m.name, Duration::from_secs(5));
            let _ = adb(
                &self.ctx,
                &["reverse", "--remove", &format!("tcp:{}", m.port)],
            );
        }
    }
}

impl RunningApp for RnApp {
    fn package(&self) -> &str {
        &self.package
    }

    fn reload(&mut self, kind: ReloadKind, sink: &Sink) -> Result<Duration, AdapterError> {
        let start = Instant::now();
        match kind {
            ReloadKind::Hot => {
                // Fast Refresh: Metro already applied saved files; report what it did since
                if let Some(m) = self.metro.as_mut() {
                    let mut problems = Vec::new();
                    if pump(m, &self.ctx.project, sink, &mut problems) == Some(false) {
                        return Err(AdapterError::BuildFailed {
                            summary: "Metro could not bundle the change".into(),
                            problems,
                        });
                    }
                }
                if !self.metro_running() {
                    return Err(AdapterError::Failed("Metro is not running".into()));
                }
                sink(Event::Log {
                    line: "Fast Refresh applies saved files automatically".into(),
                });
            }
            ReloadKind::Restart => {
                sink(Event::Phase {
                    phase: Phase::Reloading,
                    message: "reload the JS bundle".into(),
                });
                // restart the app: it loads a fresh bundle from Metro (`R R` key presses are
                // not reliable: Expo Go ignores them)
                let (port, serial) = (self.ctx.device.adb.port, self.ctx.device.serial.clone());
                let _ = umr_device::app::force_stop(port, &serial, &self.package);
                let mport = self.metro.as_ref().map_or(8081, |m| m.port);
                match self.mode {
                    Mode::ExpoGo => {
                        umr_device::adb::shell(port, &serial, &open_expo_go(mport))
                            .map_err(|e| AdapterError::Failed(e.to_string()))?;
                    }
                    Mode::Native => {
                        umr_device::app::launch(port, &serial, &self.package, None)
                            .map_err(AdapterError::LaunchFailed)?;
                    }
                }
                let pkg = self.package.clone();
                let mut m = self
                    .metro
                    .take()
                    .ok_or_else(|| AdapterError::Failed("Metro is not running".into()))?;
                let r = wait_ready(&self.ctx, &mut m, &pkg, sink);
                self.metro = Some(m);
                r?;
            }
            ReloadKind::Full => {
                sink(Event::Phase {
                    phase: Phase::Reloading,
                    message: "restart Metro and the app".into(),
                });
                let _ = umr_device::app::force_stop(
                    self.ctx.device.adb.port,
                    &self.ctx.device.serial,
                    &self.package,
                );
                self.stop_metro();
                let mut m = start_metro(&self.ctx, self.mode, sink)?;
                if self.mode == Mode::Native {
                    self.package = native_build(&self.ctx, m.port, sink)?;
                }
                let pkg = self.package.clone();
                wait_ready(&self.ctx, &mut m, &pkg, sink)?;
                self.metro = Some(m);
            }
        }
        let took = start.elapsed();
        sink(Event::Reloaded {
            kind,
            ms: u64::try_from(took.as_millis()).unwrap_or(u64::MAX),
        });
        Ok(took)
    }

    fn is_alive(&mut self) -> bool {
        self.metro_running()
            && umr_device::app::pid(
                self.ctx.device.adb.port,
                &self.ctx.device.serial,
                &self.package,
            )
            .is_some()
    }

    fn stop(mut self: Box<Self>) {
        let _ = umr_device::app::force_stop(
            self.ctx.device.adb.port,
            &self.ctx.device.serial,
            &self.package,
        );
        self.stop_metro();
        self.ctx.supervisor.stop_group(&self.ctx.group);
    }
}

/// A runtime problem without a location still counts as a problem.
#[must_use]
pub fn runtime_problem(message: &str) -> Problem {
    Problem {
        severity: Severity::Error,
        message: message.to_string(),
        file: None,
        line: None,
        column: None,
        source: "react-native".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("umr-rn-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn modes_and_package_managers() {
        let d = tmp("mode");
        std::fs::write(
            d.join("package.json"),
            r#"{"dependencies":{"expo":"~58.0.25","react-native":"0.88.1"}}"#,
        )
        .unwrap();
        assert_eq!(mode(&d), Mode::ExpoGo);
        assert_eq!(package_manager(&d), "npm");
        std::fs::write(d.join("yarn.lock"), "").unwrap();
        assert_eq!(package_manager(&d), "yarn");
        std::fs::create_dir_all(d.join("android")).unwrap();
        assert_eq!(mode(&d), Mode::Native, "Expo with android = dev build");
        std::fs::write(
            d.join("package.json"),
            r#"{"dependencies":{"react-native":"0.88.1"}}"#,
        )
        .unwrap();
        assert!(!uses_expo(&d));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn partial_node_modules_is_not_installed() {
        let d = tmp("deps");
        std::fs::write(d.join("package.json"), "{}").unwrap();
        assert!(!dependencies_installed(&d));
        std::fs::create_dir_all(d.join("node_modules").join("react-native")).unwrap();
        assert!(!dependencies_installed(&d), "interrupted npm install");
        std::fs::write(d.join("node_modules").join(".package-lock.json"), "{}").unwrap();
        assert!(dependencies_installed(&d));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn metro_is_localhost_only() {
        let d = tmp("args");
        std::fs::write(
            d.join("package.json"),
            r#"{"dependencies":{"expo":"~58.0.25"}}"#,
        )
        .unwrap();
        let go = metro_args(&d, Mode::ExpoGo, 8081, true).join(" ");
        assert_eq!(
            go,
            "/d /c npx expo start --localhost --port 8081 --go --android"
        );
        let dev = metro_args(&d, Mode::Native, 8082, false).join(" ");
        assert!(dev.ends_with("--localhost --port 8082 --dev-client"));
        std::fs::write(
            d.join("package.json"),
            r#"{"dependencies":{"react-native":"0.88.1"}}"#,
        )
        .unwrap();
        let bare = metro_args(&d, Mode::Native, 8081, false).join(" ");
        assert!(bare.contains("react-native start --host 127.0.0.1 --port 8081"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn change_kinds() {
        let r = ReactNative;
        assert_eq!(r.change_kind("App.js"), None, "Fast Refresh");
        assert_eq!(r.change_kind("src/screens/Home.tsx"), None);
        assert_eq!(r.change_kind("package.json"), Some(ReloadKind::Full));
        assert_eq!(
            r.change_kind("android/app/src/main/AndroidManifest.xml"),
            Some(ReloadKind::Full)
        );
        assert_eq!(r.change_kind("node_modules/x/package.json"), None);
    }

    #[test]
    fn expo_go_intent() {
        assert_eq!(
            open_expo_go(8081),
            "am start -a android.intent.action.VIEW -d exp://127.0.0.1:8081 host.exp.exponent"
        );
    }

    #[test]
    fn bundle_lines() {
        assert!(bundled("Android Bundled 812ms index.js (650 modules)"));
        assert!(bundled("BUNDLE  ./index.js"));
        assert!(!bundled(
            "Android Bundling failed 120ms index.js (1 module)"
        ));
        assert!(bundling_failed(
            "Android Bundling failed 120ms index.js (1 module)"
        ));
    }

    #[test]
    fn static_contract() {
        assert_eq!(
            umr_adapter::contract::check_static(&ReactNative),
            Vec::<String>::new()
        );
        assert_eq!(runtime_problem("x").source, "react-native");
    }
}
