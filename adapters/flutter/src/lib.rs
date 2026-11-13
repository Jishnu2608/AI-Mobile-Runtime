//! Flutter adapter (ADP-003, Phase 3 step 3.3).
//!
//! Drives `flutter run --machine -d <serial>` (the Flutter tool's JSON protocol, POC-006) through
//! the Supervisor with stdin / stdout piped:
//!
//! * stdout: one JSON array per line, e.g. `[{"event":"app.started","params":{"appId":"…"}}]`;
//!   `app.progress` → progress, `app.log` / `daemon.logMessage` → log + problems,
//!   `app.stop` → the app ended. Plain lines (Gradle output) are parsed for problems.
//! * stdin: `[{"id":1,"method":"app.restart","params":{"appId":"…","fullRestart":false}}]`
//!   → hot reload (`fullRestart:false`) or hot restart (`true`); the reply carries
//!   `{"code":0,"message":"Reloaded 1 of 640 libraries in 312ms."}` (non-zero = compile error).
//! * Full reload = stop the tool and run again (native changes, new plugins).
//!
//! Fast start (PRV-012 / AC-CLI-01): when nothing native changed since the last debug APK was
//! built, `flutter run --use-application-binary` reuses it (no Gradle). When the device even has
//! that exact APK installed (same size and SHA-256), the adapter skips the install too: it starts
//! `flutter attach`, launches the app and lets the tool connect and sync the current Dart code.
//! If attaching fails, it falls back to `flutter run`.
//!
//! Environment: the isolated recipe with `FLUTTER_ROOT`, `PUB_CACHE` and `APPDATA` in the managed
//! folder (Flutter keeps `.flutter_settings` in `APPDATA`, POC-006 finding 10).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, ChildStdout};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use umr_adapter::problems::{flutter_exception, parse_line};
use umr_adapter::{
    Adapter, AdapterContext, AdapterError, Capabilities, Event, Framework, Issue, Phase, Problem,
    ReloadKind, RunningApp, Severity, Sink,
};
use umr_supervisor::ProcessSpec;

/// Longest first run (Gradle build of the Android shell, pub get).
pub const START_TIMEOUT: Duration = Duration::from_secs(30 * 60);
pub const RELOAD_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Default, Clone, Copy)]
pub struct Flutter;

/// One decoded line of `flutter run --machine` output.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Event {
        name: String,
        params: Value,
    },
    Reply {
        id: u64,
        result: Option<Value>,
        error: Option<Value>,
    },
    Text(String),
}

/// Decodes a stdout line (`[{...}]` JSON, anything else is text).
#[must_use]
pub fn decode(line: &str) -> Vec<Message> {
    let t = line.trim();
    if t.starts_with('[') && t.ends_with(']') {
        if let Ok(Value::Array(items)) = serde_json::from_str::<Value>(t) {
            return items
                .into_iter()
                .map(|v| {
                    if let Some(name) = v.get("event").and_then(Value::as_str) {
                        Message::Event {
                            name: name.to_string(),
                            params: v.get("params").cloned().unwrap_or(Value::Null),
                        }
                    } else if let Some(id) = v.get("id").and_then(Value::as_u64) {
                        Message::Reply {
                            id,
                            result: v.get("result").cloned(),
                            error: v.get("error").cloned(),
                        }
                    } else {
                        Message::Text(v.to_string())
                    }
                })
                .collect();
        }
    }
    vec![Message::Text(line.to_string())]
}

/// `app.restart` request line.
#[must_use]
pub fn restart_request(id: u64, app_id: &str, full: bool) -> String {
    json!([{ "id": id, "method": "app.restart",
             "params": { "appId": app_id, "fullRestart": full, "pause": false, "reason": "manual" } }])
    .to_string()
}

#[must_use]
pub fn stop_request(id: u64, app_id: &str) -> String {
    json!([{ "id": id, "method": "app.stop", "params": { "appId": app_id } }]).to_string()
}

/// Newest modification time under `dir` (skipping build output and tool state).
fn newest(dir: &Path, depth: usize) -> Option<std::time::SystemTime> {
    let mut best = std::fs::metadata(dir).and_then(|m| m.modified()).ok();
    if depth == 0 {
        return best;
    }
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if matches!(
            name.as_str(),
            "build" | ".gradle" | ".cxx" | ".kotlin" | "local.properties"
        ) {
            continue;
        }
        let p = e.path();
        let t = if p.is_dir() {
            newest(&p, depth - 1)
        } else {
            e.metadata().and_then(|m| m.modified()).ok()
        };
        if t > best {
            best = t;
        }
    }
    best
}

/// The last debug APK, if nothing native changed after it was built (pubspec, `android/`).
#[must_use]
pub fn reusable_apk(root: &Path) -> Option<PathBuf> {
    let apk = root.join("build/app/outputs/flutter-apk/app-debug.apk");
    let built = std::fs::metadata(&apk).and_then(|m| m.modified()).ok()?;
    let inputs = [
        std::fs::metadata(root.join("pubspec.yaml"))
            .and_then(|m| m.modified())
            .ok(),
        std::fs::metadata(root.join("pubspec.lock"))
            .and_then(|m| m.modified())
            .ok(),
        newest(&root.join("android"), 6),
    ];
    inputs.iter().flatten().all(|t| *t <= built).then_some(apk)
}

/// `applicationId` from `android/app/build.gradle(.kts)`.
#[must_use]
pub fn application_id(root: &Path) -> Option<String> {
    for f in ["build.gradle.kts", "build.gradle"] {
        let Ok(t) = std::fs::read_to_string(root.join("android").join("app").join(f)) else {
            continue;
        };
        for line in t.lines() {
            let l = line.trim();
            if let Some(rest) = l.strip_prefix("applicationId") {
                let v = rest
                    .trim_start_matches([' ', '='])
                    .trim()
                    .trim_matches(['"', '\'']);
                if umr_device::app::valid_package(v) {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

#[derive(Debug, Default)]
struct Shared {
    /// Any output from the tool yet (attach: the tool is listening).
    seen: bool,
    app_id: Option<String>,
    started: bool,
    stopped: Option<String>,
    replies: HashMap<u64, (Option<Value>, Option<Value>)>,
    problems: Vec<Problem>,
    eof: bool,
    /// Lines of a Flutter exception being printed (one `app.log` event per line).
    exc: Vec<String>,
}

type State = Arc<(Mutex<Shared>, Condvar)>;

fn add_problem(p: Problem, sink: &Sink, st: &mut Shared) {
    if !st.problems.contains(&p) {
        sink(Event::Problem(p.clone()));
        st.problems.push(p);
    }
}

/// Reports the buffered exception block (if any) as one problem.
fn flush_exception(root: &Path, sink: &Sink, st: &mut Shared) {
    if st.exc.is_empty() {
        return;
    }
    let text = std::mem::take(&mut st.exc).join(
        "
",
    );
    if let Some(p) = flutter_exception(&text, root) {
        add_problem(p, sink, st);
    }
}

/// `E/flutter ( 4321): x` → `x`.
fn log_text(line: &str) -> &str {
    match line
        .find("/flutter (")
        .and_then(|a| line[a..].find("): ").map(|b| a + b + 3))
    {
        Some(at) => &line[at..],
        None => line,
    }
}

/// Compiler errors line by line; Flutter exceptions as whole blocks: a framework error runs to
/// its closing `════` line, an unhandled exception to its first app frame (or the first line
/// after its stack).
fn problem_from_log(text: &str, root: &Path, sink: &Sink, st: &mut Shared) {
    for line in text.lines() {
        if let Some(p) = parse_line(line, root) {
            add_problem(p, sink, st);
        }
        let t = log_text(line).trim();
        if t.contains("EXCEPTION CAUGHT BY") || t.contains("Unhandled Exception:") {
            flush_exception(root, sink, st);
            st.exc.push(line.to_string());
            continue;
        }
        if st.exc.is_empty() {
            continue;
        }
        let framework_block = st.exc[0].contains("EXCEPTION CAUGHT BY");
        let frame = t.starts_with('#');
        let had_frames = st.exc.iter().any(|l| log_text(l).trim().starts_with('#'));
        if !framework_block && had_frames && !frame {
            flush_exception(root, sink, st);
            continue;
        }
        st.exc.push(line.to_string());
        let located = !framework_block
            && frame
            && flutter_exception(
                &st.exc.join(
                    "
",
                ),
                root,
            )
            .is_some_and(|p| p.file.is_some());
        if (framework_block && t.starts_with("════")) || located || st.exc.len() >= 80 {
            flush_exception(root, sink, st);
        }
    }
}

/// Handles one decoded message (reader thread).
fn handle(msg: Message, root: &Path, sink: &Sink, st: &mut Shared) {
    match msg {
        Message::Event { name, params } => match name.as_str() {
            "app.start" => st.app_id = params["appId"].as_str().map(String::from),
            "app.started" => st.started = true,
            "app.progress" => {
                if let Some(m) = params["message"].as_str() {
                    let phase = if m.starts_with("Installing") {
                        Phase::Installing
                    } else {
                        Phase::Building
                    };
                    sink(Event::Phase {
                        phase,
                        message: m.to_string(),
                    });
                }
            }
            "app.log" | "daemon.logMessage" => {
                let text = params["log"]
                    .as_str()
                    .or_else(|| params["message"].as_str())
                    .unwrap_or_default();
                for l in text.lines() {
                    sink(Event::Log {
                        line: l.to_string(),
                    });
                }
                problem_from_log(text, root, sink, st);
            }
            "app.stop" => {
                flush_exception(root, sink, st);
                let why = params["error"]
                    .as_str()
                    .unwrap_or("the app stopped")
                    .to_string();
                st.stopped = Some(why.clone());
                sink(Event::AppStopped { reason: why });
            }
            _ => {}
        },
        Message::Reply { id, result, error } => {
            st.replies.insert(id, (result, error));
        }
        Message::Text(t) => {
            if !t.trim().is_empty() {
                sink(Event::Log { line: t.clone() });
                problem_from_log(&t, root, sink, st);
            }
        }
    }
}

#[allow(clippy::needless_pass_by_value)] // owned by the reader thread
fn reader(out: ChildStdout, root: PathBuf, sink: Sink, state: State) {
    let r = BufReader::new(out);
    for line in r.lines() {
        let Ok(line) = line else { break };
        let (lock, cv) = &*state;
        let mut st = lock.lock().unwrap_or_else(PoisonError::into_inner);
        st.seen = true;
        for m in decode(&line) {
            handle(m, &root, &sink, &mut st);
        }
        cv.notify_all();
    }
    let (lock, cv) = &*state;
    lock.lock().unwrap_or_else(PoisonError::into_inner).eof = true;
    cv.notify_all();
}

/// Waits until `done(state)` returns `Some`, the tool ends, or the timeout passes.
fn wait<T>(
    state: &State,
    timeout: Duration,
    mut done: impl FnMut(&mut Shared) -> Option<T>,
) -> Result<T, String> {
    let (lock, cv) = &**state;
    let start = Instant::now();
    let mut st = lock.lock().unwrap_or_else(PoisonError::into_inner);
    loop {
        if let Some(v) = done(&mut st) {
            return Ok(v);
        }
        if st.eof {
            return Err(st
                .stopped
                .clone()
                .unwrap_or_else(|| "the Flutter tool exited".into()));
        }
        let left = timeout.checked_sub(start.elapsed()).ok_or("timed out")?;
        st = cv
            .wait_timeout(st, left.min(Duration::from_millis(500)))
            .unwrap_or_else(PoisonError::into_inner)
            .0;
    }
}

#[derive(Debug)]
struct Session {
    name: String,
    log: PathBuf,
    stdin: ChildStdin,
    state: State,
    next_id: u64,
}

/// SHA-256 of a file as lower-case hex.
fn sha256_file(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Some(
        h.finalize()
            .iter()
            .fold(String::with_capacity(64), |mut s, b| {
                use std::fmt::Write as _;
                let _ = write!(s, "{b:02x}");
                s
            }),
    )
}

/// The local APK's hash, cached next to it (keyed by size and modification time).
fn apk_hash(apk: &Path) -> Option<String> {
    let m = std::fs::metadata(apk).ok()?;
    let mtime = m
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis();
    let key = format!("{} {}", m.len(), mtime);
    let cache = apk.with_extension("apk.umr-sha256");
    if let Ok(t) = std::fs::read_to_string(&cache) {
        if let Some((k, h)) = t.trim().rsplit_once(' ') {
            if k == key {
                return Some(h.to_string());
            }
        }
    }
    let h = sha256_file(apk)?;
    let _ = std::fs::write(&cache, format!("{key} {h}"));
    Some(h)
}

/// Whether the device has exactly this APK installed for `package`: one adb round trip instead
/// of three (pm path / stat / sha256sum), since each costs seconds under host load (PRV-012),
/// so the fast path stays fast only if it asks the device once.
fn installed_same(ctx: &AdapterContext, package: &str, apk: &Path) -> bool {
    let (port, serial) = (ctx.device.adb.port, ctx.device.serial.as_str());
    let local = std::fs::metadata(apk).map_or(0, |m| m.len());
    let Some(hash) = apk_hash(apk) else {
        return false;
    };
    let cmd = format!(
        "p=$(pm path {package} 2>/dev/null | sed -n 's/^package://p' | grep base.apk | head -n1); \
         [ -n \"$p\" ] && stat -c %s \"$p\" && sha256sum \"$p\""
    );
    let Ok(out) = umr_device::adb::shell(port, serial, &cmd) else {
        return false;
    };
    let mut lines = out.lines();
    let Some(size) = lines.next().and_then(|v| v.trim().parse::<u64>().ok()) else {
        return false;
    };
    if size != local {
        return false;
    }
    let remote = lines
        .next()
        .and_then(|v| v.split_whitespace().next())
        .map(str::to_lowercase);
    remote.as_deref() == Some(hash.as_str())
}

/// Caches the launcher component resolved for an APK (`pkg/.MainActivity`), keyed like
/// [`apk_hash`]: avoids a `cmd package resolve-activity` round trip on every attach.
fn cached_component(apk: &Path, resolve: impl FnOnce() -> Option<String>) -> Option<String> {
    let m = std::fs::metadata(apk).ok()?;
    let mtime = m
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis();
    let key = format!("{} {}", m.len(), mtime);
    let cache = apk.with_extension("apk.umr-activity");
    if let Ok(t) = std::fs::read_to_string(&cache) {
        if let Some((k, c)) = t.trim().rsplit_once(' ') {
            if k == key {
                return Some(c.to_string());
            }
        }
    }
    let c = resolve()?;
    let _ = std::fs::write(&cache, format!("{key} {c}"));
    Some(c)
}

/// Dependencies are resolved (`.dart_tool/package_config.json` newer than the pubspec files):
/// `flutter run --no-pub` skips `pub get`.
fn pub_resolved(root: &Path) -> bool {
    let t = |p: &str| {
        std::fs::metadata(root.join(p))
            .and_then(|m| m.modified())
            .ok()
    };
    match t(".dart_tool/package_config.json") {
        Some(cfg) => [t("pubspec.yaml"), t("pubspec.lock")]
            .iter()
            .flatten()
            .all(|x| *x <= cfg),
        None => false,
    }
}

/// `dart.exe` + arguments that run the Flutter tool snapshot directly, when the SDK has prepared
/// it (`bin/cache/flutter_tools.stamp` is written after the snapshot is built).
#[must_use]
pub fn direct_tool(root: &Path) -> Option<(PathBuf, Vec<String>)> {
    let cache = root.join("bin").join("cache");
    let dart = cache.join("dart-sdk").join("bin").join("dart.exe");
    let snapshot = cache.join("flutter_tools.snapshot");
    let packages = root
        .join("packages")
        .join("flutter_tools")
        .join(".dart_tool")
        .join("package_config.json");
    let stamp = cache.join("flutter_tools.stamp");
    (dart.is_file() && packages.is_file() && stamp.is_file() && snapshot.is_file()).then(|| {
        (
            dart,
            vec![
                format!("--packages={}", packages.display()),
                snapshot.display().to_string(),
            ],
        )
    })
}

fn launch_tool(
    ctx: &AdapterContext,
    sink: &Sink,
    attach: Option<(&str, &str)>,
) -> Result<Session, AdapterError> {
    let flutter = ctx
        .flutter_root
        .as_ref()
        .map(|r| r.join("bin").join("flutter.bat"))
        .filter(|p| p.is_file())
        .ok_or_else(|| {
            AdapterError::Missing("Flutter SDK (run `umr install` for this project)".into())
        })?;
    let name = format!("flutter:{}", ctx.group);
    let log = ctx
        .log_dir
        .join(format!("flutter-{}.log", ctx.group.replace(':', "-")));
    let mut args: Vec<String> = match attach {
        // (package, VM service URL on the host)
        Some((pkg, url)) => [
            "attach",
            "--machine",
            "-d",
            ctx.device.serial.as_str(),
            "--app-id",
            pkg,
            "--debug-url",
            url,
        ]
        .iter()
        .map(ToString::to_string)
        .collect(),
        None => [
            "run",
            "--machine",
            "--debug",
            "-d",
            ctx.device.serial.as_str(),
        ]
        .iter()
        .map(ToString::to_string)
        .collect(),
    };
    if attach.is_none() && pub_resolved(&ctx.project) {
        args.push("--no-pub".into());
    }
    // AC-CLI-01: no native change since the last build → install that APK and sync the current
    // Dart code into it, instead of a Gradle build on every start
    if let Some(apk) = reusable_apk(&ctx.project).filter(|_| attach.is_none()) {
        sink(Event::Phase {
            phase: Phase::Building,
            message: format!(
                "reusing {} (no native changes since it was built)",
                apk.display()
            ),
        });
        args.push("--use-application-binary".into());
        args.push(apk.display().to_string());
    }
    // the prepared tool snapshot starts ~1.5 s faster than flutter.bat (which first checks
    // whether the snapshot is stale); the batch file stays the path when it is not prepared
    let (program, args) = match ctx.flutter_root.as_deref().and_then(direct_tool) {
        Some((dart, mut pre)) => {
            pre.extend(args);
            (dart, pre)
        }
        None => (flutter, args),
    };
    let mut spec = ProcessSpec::new(&name, program, &log).args(args);
    spec.env.clone_from(&ctx.env);
    if let Some(j) = &ctx.jdk_home {
        spec.env.insert("JAVA_HOME".into(), j.display().to_string());
    }
    spec.env
        .insert("ANDROID_HOME".into(), ctx.sdk.display().to_string());
    spec.env
        .insert("ANDROID_SDK_ROOT".into(), ctx.sdk.display().to_string());
    spec.cwd = Some(ctx.project.clone());
    spec.group = Some(ctx.group.clone());
    sink(Event::Phase {
        phase: Phase::Building,
        message: format!(
            "flutter {} --machine -d {}",
            if attach.is_some() { "attach" } else { "run" },
            ctx.device.serial
        ),
    });
    let (_pid, stdin, stdout) = ctx
        .supervisor
        .start_piped(spec)
        .map_err(|e| AdapterError::Failed(format!("cannot start Flutter: {e}")))?;
    let state: State = Arc::default();
    let (root, s2, st2) = (ctx.project.clone(), sink.clone(), state.clone());
    std::thread::Builder::new()
        .name("umr-flutter-reader".into())
        .spawn(move || reader(stdout, root, s2, st2))
        .map_err(|e| AdapterError::Failed(e.to_string()))?;
    Ok(Session {
        name,
        log,
        stdin,
        state,
        next_id: 1,
    })
}

/// The app's VM service URL from its log line
/// `The Dart VM service is listening on http://127.0.0.1:41931/MnsHt2taLio=/` → (port, path).
#[must_use]
pub fn vm_service(line: &str) -> Option<(u16, String)> {
    let at = line.find("http://127.0.0.1:")? + "http://127.0.0.1:".len();
    let rest = &line[at..];
    let end = rest.find('/')?;
    let port = rest[..end].parse().ok()?;
    let path = rest[end..].split_whitespace().next()?.to_string();
    path.chars()
        .all(|c| c.is_ascii_alphanumeric() || "/=_-".contains(c))
        .then_some((port, path))
}

/// Fast path: the device has the exact APK → launch it, find its VM service in the log and attach
/// the tool to that address (no install, no discovery race).
fn attach_app(ctx: &AdapterContext, sink: &Sink, package: &str) -> Option<Session> {
    let apk = reusable_apk(&ctx.project)?;
    if !installed_same(ctx, package, &apk) {
        return None;
    }
    sink(Event::Phase {
        phase: Phase::Launching,
        message: format!("{package} on the device matches the last build — attaching (no install)"),
    });
    let (port, serial) = (ctx.device.adb.port, ctx.device.serial.as_str());
    let component = cached_component(&apk, || {
        umr_device::app::launcher_activity(port, serial, package).ok()
    })?;
    sink(Event::Phase {
        phase: Phase::Launching,
        message: package.to_string(),
    });
    // one round trip for force-stop + the launch instead of three (resolve-activity, force-stop,
    // am start): PRV-012, each round trip costs seconds under host load
    let out = umr_device::adb::shell(
        port,
        serial,
        &format!("am force-stop {package}; am start -W -n {component}"),
    )
    .ok()?;
    let (ok, ..) = umr_device::app::parse_am_start(&out);
    if !ok {
        return None;
    }
    // the app logs its VM service address right after start
    let start = Instant::now();
    let (dev_port, path) = loop {
        if start.elapsed() > Duration::from_secs(15) {
            sink(Event::Phase {
                phase: Phase::Building,
                message: "the app did not announce its VM service; running it instead".into(),
            });
            return None;
        }
        std::thread::sleep(Duration::from_millis(250));
        let Some(pid) = umr_device::app::pid(port, serial, package) else {
            continue;
        };
        let log =
            umr_device::adb::shell(port, serial, &format!("logcat -d --pid={pid} -s flutter:I"))
                .unwrap_or_default();
        if let Some(found) = log.lines().rev().find_map(vm_service) {
            break found;
        }
    };
    // the device-side address: `flutter attach` forwards the port itself
    let url = format!("http://127.0.0.1:{dev_port}{path}");
    let s = launch_tool(ctx, sink, Some((package, &url))).ok()?;
    if wait(&s.state, Duration::from_secs(60), |st| {
        st.started.then_some(())
    })
    .is_err()
    {
        sink(Event::Phase {
            phase: Phase::Building,
            message: "attach did not connect; running the app instead".into(),
        });
        ctx.supervisor.stop(&s.name);
        let _ = ctx.supervisor.wait_exit(&s.name, Duration::from_secs(5));
        return None;
    }
    Some(s)
}

/// Starts the tool and waits for `app.started` ([`attach_app`] first, [`launch_tool`] otherwise).
fn run_app(ctx: &AdapterContext, sink: &Sink, package: &str) -> Result<Session, AdapterError> {
    if let Some(s) = attach_app(ctx, sink, package) {
        return Ok(s);
    }
    let s = launch_tool(ctx, sink, None)?;
    let r = wait(&s.state, START_TIMEOUT, |st| st.started.then_some(()));
    if let Err(why) = r {
        let mut problems = s
            .state
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .problems
            .clone();
        ctx.supervisor.stop(&s.name);
        // the Android build writes to stderr (the tool's log): its summary and compiler lines
        let _ = ctx.supervisor.wait_exit(&s.name, Duration::from_secs(5));
        let text = std::fs::read_to_string(&s.log).unwrap_or_default();
        for p in text.lines().filter_map(|l| parse_line(l, &ctx.project)) {
            if !problems.contains(&p) {
                problems.push(p);
            }
        }
        let summary = umr_adapter::problems::gradle_failure(&text).unwrap_or(why);
        if problems.is_empty() {
            problems.push(Problem {
                severity: Severity::Error,
                message: summary.clone(),
                file: None,
                line: None,
                column: None,
                source: "gradle".into(),
            });
        }
        return Err(AdapterError::BuildFailed { summary, problems });
    }
    Ok(s)
}

impl Adapter for Flutter {
    fn id(&self) -> &'static str {
        "flutter"
    }
    fn display_name(&self) -> &'static str {
        "Flutter"
    }
    fn frameworks(&self) -> &'static [Framework] {
        &[Framework::Flutter]
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            hot_reload: true,
            hot_restart: true,
            incremental_build: true,
            structured_build_events: true,
        }
    }

    fn validate(&self, ctx: &AdapterContext) -> Vec<Issue> {
        let mut v = Vec::new();
        if !ctx.project.join("pubspec.yaml").is_file() {
            v.push(Issue {
                message: "no pubspec.yaml in the project folder".into(),
                fix: "open the Flutter project's root folder".into(),
            });
        }
        if !ctx.project.join("android").is_dir() {
            v.push(Issue {
                message: "the project has no android folder".into(),
                fix: "run `flutter create --platforms android .` in the project".into(),
            });
        }
        if !ctx
            .flutter_root
            .as_ref()
            .is_some_and(|r| r.join("bin").join("flutter.bat").is_file())
        {
            v.push(Issue {
                message: "Flutter SDK is not installed".into(),
                fix: "run `umr install` for this project".into(),
            });
        }
        if !ctx
            .jdk_home
            .as_ref()
            .is_some_and(|j| j.join("bin").join("java.exe").is_file())
        {
            v.push(Issue {
                message: "no JDK selected for the Android build".into(),
                fix: "run `umr install` for this project".into(),
            });
        }
        v
    }

    /// Dart code → hot reload; assets → hot restart; dependencies and native code → full.
    fn change_kind(&self, rel: &str) -> Option<ReloadKind> {
        if rel.starts_with("lib/")
            && std::path::Path::new(rel)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("dart"))
        {
            Some(ReloadKind::Hot)
        } else if rel.starts_with("assets/") || rel.starts_with("fonts/") {
            Some(ReloadKind::Restart)
        } else if rel == "pubspec.yaml" || rel == "pubspec.lock" || rel.starts_with("android/") {
            Some(ReloadKind::Full)
        } else {
            None
        }
    }

    fn start(&self, ctx: &AdapterContext, sink: Sink) -> Result<Box<dyn RunningApp>, AdapterError> {
        if let Some(i) = self.validate(ctx).into_iter().next() {
            return Err(AdapterError::Missing(format!("{} ({})", i.message, i.fix)));
        }
        let package = application_id(&ctx.project).ok_or_else(|| {
            AdapterError::Missing("applicationId in android/app/build.gradle(.kts)".into())
        })?;
        let s = run_app(ctx, &sink, &package)?;
        let pid = umr_device::app::pid(ctx.device.adb.port, &ctx.device.serial, &package);
        sink(Event::AppStarted {
            package: package.clone(),
            pid,
        });
        Ok(Box::new(FlutterApp {
            ctx: ctx.clone(),
            package,
            session: Some(s),
        }))
    }
}

#[derive(Debug)]
struct FlutterApp {
    ctx: AdapterContext,
    package: String,
    session: Option<Session>,
}

impl FlutterApp {
    fn send(&mut self, line: &str) -> Result<(), AdapterError> {
        let s = self
            .session
            .as_mut()
            .ok_or_else(|| AdapterError::Failed("the Flutter tool is not running".into()))?;
        writeln!(s.stdin, "{line}")
            .and_then(|()| s.stdin.flush())
            .map_err(|e| AdapterError::Failed(format!("Flutter tool: {e}")))
    }

    fn end_tool(&mut self) {
        if let Some(mut s) = self.session.take() {
            let app_id = s
                .state
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .app_id
                .clone();
            if let Some(a) = app_id {
                let id = s.next_id;
                let _ = writeln!(s.stdin, "{}", stop_request(id, &a));
                let _ = s.stdin.flush();
                let _ = wait(&s.state, Duration::from_secs(10), |st| {
                    st.replies.contains_key(&id).then_some(())
                });
            }
            drop(s.stdin);
            let _ = self
                .ctx
                .supervisor
                .wait_exit(&s.name, Duration::from_secs(10));
            self.ctx.supervisor.stop(&s.name);
        }
    }
}

impl RunningApp for FlutterApp {
    fn package(&self) -> &str {
        &self.package
    }

    fn reload(&mut self, kind: ReloadKind, sink: &Sink) -> Result<Duration, AdapterError> {
        let start = Instant::now();
        if kind == ReloadKind::Full {
            sink(Event::Phase {
                phase: Phase::Reloading,
                message: "full rebuild (flutter run again)".into(),
            });
            self.end_tool();
            self.session = Some(run_app(&self.ctx, sink, &self.package)?);
        } else {
            let full = kind == ReloadKind::Restart;
            let (id, app_id, state) = {
                let s = self.session.as_mut().ok_or_else(|| {
                    AdapterError::Failed("the Flutter tool is not running".into())
                })?;
                let app_id = s
                    .state
                    .0
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .app_id
                    .clone()
                    .ok_or_else(|| AdapterError::Failed("no Flutter app id yet".into()))?;
                let id = s.next_id;
                s.next_id += 1;
                (id, app_id, s.state.clone())
            };
            // compile errors of a reload go to the tool's stderr (its log): remember where it ends
            let log = self.session.as_ref().map(|s| s.log.clone());
            let log_pos = log
                .as_ref()
                .and_then(|l| std::fs::metadata(l).ok())
                .map_or(0, |m| m.len());
            sink(Event::Phase {
                phase: Phase::Reloading,
                message: if full { "hot restart" } else { "hot reload" }.into(),
            });
            self.send(&restart_request(id, &app_id, full))?;
            let (result, error) = wait(&state, RELOAD_TIMEOUT, |st| st.replies.remove(&id))
                .map_err(AdapterError::Failed)?;
            if let Some(e) = error {
                return Err(AdapterError::Failed(format!("Flutter: {e}")));
            }
            let r = result.unwrap_or(Value::Null);
            let code = r["code"].as_i64().unwrap_or(0);
            let message = r["message"].as_str().unwrap_or_default().to_string();
            if code != 0 {
                // give the tool a moment to flush stderr, then read what the reload added
                std::thread::sleep(Duration::from_millis(200));
                let added = log
                    .as_ref()
                    .and_then(|l| std::fs::read(l).ok())
                    .map(|b| {
                        let from = usize::try_from(log_pos).unwrap_or(0).min(b.len());
                        String::from_utf8_lossy(&b[from..]).into_owned()
                    })
                    .unwrap_or_default();
                let mut problems: Vec<Problem> = Vec::new();
                for p in message
                    .lines()
                    .chain(added.lines())
                    .filter_map(|l| parse_line(l, &self.ctx.project))
                {
                    if !problems.contains(&p) {
                        problems.push(p);
                    }
                }
                if problems.is_empty() {
                    problems.push(Problem {
                        severity: Severity::Error,
                        message: message.clone(),
                        file: None,
                        line: None,
                        column: None,
                        source: "flutter".into(),
                    });
                }
                for p in &problems {
                    sink(Event::Problem(p.clone()));
                }
                return Err(AdapterError::BuildFailed {
                    summary: message,
                    problems,
                });
            }
            sink(Event::Log { line: message });
        }
        let took = start.elapsed();
        sink(Event::Reloaded {
            kind,
            ms: u64::try_from(took.as_millis()).unwrap_or(u64::MAX),
        });
        Ok(took)
    }

    fn is_alive(&mut self) -> bool {
        let tool = self
            .session
            .as_ref()
            .is_some_and(|s| self.ctx.supervisor.is_running(&s.name));
        tool && umr_device::app::pid(
            self.ctx.device.adb.port,
            &self.ctx.device.serial,
            &self.package,
        )
        .is_some()
    }

    fn stop(mut self: Box<Self>) {
        self.end_tool();
        let _ = umr_device::app::force_stop(
            self.ctx.device.adb.port,
            &self.ctx.device.serial,
            &self.package,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_machine_protocol_lines() {
        let m = decode(
            r#"[{"event":"app.start","params":{"appId":"a1","deviceId":"emulator-5554","supportsRestart":true}}]"#,
        );
        assert!(
            matches!(&m[0], Message::Event { name, params } if name == "app.start" && params["appId"] == "a1")
        );
        let r = decode(
            r#"[{"id":3,"result":{"code":0,"message":"Reloaded 1 of 640 libraries in 312ms."}}]"#,
        );
        assert!(matches!(&r[0], Message::Reply { id: 3, result: Some(v), .. } if v["code"] == 0));
        assert_eq!(
            decode("Running Gradle task 'assembleDebug'..."),
            vec![Message::Text(
                "Running Gradle task 'assembleDebug'...".into()
            )]
        );
        assert!(matches!(&decode("[not json")[0], Message::Text(_)));
    }

    #[test]
    fn requests_have_the_protocol_shape() {
        let r: Value = serde_json::from_str(&restart_request(7, "a1", false)).unwrap();
        assert_eq!(r[0]["method"], "app.restart");
        assert_eq!(r[0]["params"]["fullRestart"], false);
        assert_eq!(r[0]["id"], 7);
        let s: Value = serde_json::from_str(&stop_request(8, "a1")).unwrap();
        assert_eq!(s[0]["method"], "app.stop");
    }

    #[test]
    fn events_drive_state_and_problems() {
        let root = Path::new(r"C:\p");
        let seen: Arc<Mutex<Vec<Event>>> = Arc::default();
        let s2 = seen.clone();
        let sink: Sink = Arc::new(move |e| s2.lock().unwrap().push(e));
        let mut st = Shared::default();
        for line in [
            r#"[{"event":"app.start","params":{"appId":"a1"}}]"#,
            r#"[{"event":"app.progress","params":{"message":"Running Gradle task 'assembleDebug'..."}}]"#,
            "lib/main.dart:12:5: Error: Expected ';' after this.",
            r#"[{"event":"app.started","params":{"appId":"a1"}}]"#,
            r#"[{"event":"app.log","params":{"appId":"a1","log":"══╡ EXCEPTION CAUGHT BY WIDGETS LIBRARY ╞══"}}]"#,
            r#"[{"event":"app.log","params":{"appId":"a1","log":"The following assertion was thrown building Home:"}}]"#,
            r#"[{"event":"app.log","params":{"appId":"a1","log":"boom"}}]"#,
            r#"[{"event":"app.log","params":{"appId":"a1","log":"The relevant error-causing widget was:"}}]"#,
            r#"[{"event":"app.log","params":{"appId":"a1","log":"  Home Home:file:///C:/p/lib/home.dart:9:3"}}]"#,
            r#"[{"event":"app.log","params":{"appId":"a1","log":"════════════════"}}]"#,
            r#"[{"event":"app.log","params":{"appId":"a1","log":"[ERROR:flutter/runtime/dart_vm_initializer.cc(40)] Unhandled Exception: Bad state: No element"}}]"#,
            r##"[{"event":"app.log","params":{"appId":"a1","log":"#0      List.first (dart:core-patch/growable_array.dart:343:5)"}}]"##,
            r##"[{"event":"app.log","params":{"appId":"a1","log":"#1      load (package:demo/data.dart:12:9)"}}]"##,
            r##"[{"event":"app.log","params":{"appId":"a1","log":"#2      main (package:demo/main.dart:5:3)"}}]"##,
            r#"[{"id":1,"result":{"code":0,"message":"ok"}}]"#,
            r#"[{"event":"app.stop","params":{"appId":"a1"}}]"#,
        ] {
            for m in decode(line) {
                handle(m, root, &sink, &mut st);
            }
        }
        assert_eq!(st.app_id.as_deref(), Some("a1"));
        assert!(st.started);
        assert_eq!(st.problems.len(), 3, "{:?}", st.problems);
        assert_eq!(st.problems[0].file.as_deref(), Some("lib/main.dart"));
        assert_eq!(
            st.problems[1].message, "EXCEPTION CAUGHT BY WIDGETS LIBRARY: boom",
            "one problem per block, with what was thrown"
        );
        assert_eq!(
            (st.problems[1].file.as_deref(), st.problems[1].line),
            (Some("lib/home.dart"), Some(9)),
            "the error-causing widget, relative to the project"
        );
        assert_eq!(
            st.problems[2].message,
            "Unhandled Exception: Bad state: No element"
        );
        assert_eq!(
            (st.problems[2].file.as_deref(), st.problems[2].line),
            (Some("lib/data.dart"), Some(12)),
            "the first app frame, not dart:core"
        );
        assert!(st.replies.contains_key(&1));
        assert!(st.stopped.is_some());
        let ev = seen.lock().unwrap();
        assert!(ev.iter().any(|e| matches!(
            e,
            Event::Phase {
                phase: Phase::Building,
                ..
            }
        )));
        assert!(ev.iter().any(|e| matches!(e, Event::AppStopped { .. })));
    }

    #[test]
    fn apk_is_reused_only_without_native_changes() {
        let d = std::env::temp_dir().join(format!("umr-flutter-apk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("android/app")).unwrap();
        std::fs::write(d.join("pubspec.yaml"), "name: x").unwrap();
        std::fs::write(d.join("android/app/build.gradle.kts"), "").unwrap();
        assert!(reusable_apk(&d).is_none(), "nothing built yet");
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::create_dir_all(d.join("build/app/outputs/flutter-apk")).unwrap();
        std::fs::write(d.join("build/app/outputs/flutter-apk/app-debug.apk"), "apk").unwrap();
        assert!(reusable_apk(&d).is_some());
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(d.join("android/app/build.gradle.kts"), "changed").unwrap();
        assert!(reusable_apk(&d).is_none(), "native change → rebuild");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn component_is_cached_and_invalidated_with_the_apk() {
        let d = std::env::temp_dir().join(format!("umr-flutter-activity-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let apk = d.join("app.apk");
        std::fs::write(&apk, "v1").unwrap();
        let calls = std::sync::atomic::AtomicU32::new(0);
        let resolve = || {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Some("com.x/.MainActivity".to_string())
        };
        assert_eq!(
            cached_component(&apk, resolve),
            Some("com.x/.MainActivity".into())
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        // same apk (size + mtime unchanged): no device round trip
        assert_eq!(
            cached_component(&apk, resolve),
            Some("com.x/.MainActivity".into())
        );
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "cache hit"
        );
        // a rebuilt apk invalidates the cache
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&apk, "v2, longer").unwrap();
        cached_component(&apk, resolve);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn direct_tool_only_when_prepared() {
        let d = std::env::temp_dir().join(format!("umr-flutter-direct-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let cache = d.join("bin/cache");
        std::fs::create_dir_all(cache.join("dart-sdk/bin")).unwrap();
        std::fs::create_dir_all(d.join("packages/flutter_tools/.dart_tool")).unwrap();
        std::fs::write(cache.join("dart-sdk/bin/dart.exe"), "").unwrap();
        std::fs::write(cache.join("flutter_tools.snapshot"), "").unwrap();
        std::fs::write(
            d.join("packages/flutter_tools/.dart_tool/package_config.json"),
            "{}",
        )
        .unwrap();
        assert!(
            direct_tool(&d).is_none(),
            "no stamp: the snapshot may be stale"
        );
        std::fs::write(cache.join("flutter_tools.stamp"), "rev").unwrap();
        let (dart, pre) = direct_tool(&d).unwrap();
        assert!(dart.ends_with("dart.exe"));
        assert!(pre[0].starts_with("--packages=") && pre[1].ends_with("flutter_tools.snapshot"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn vm_service_from_log_line() {
        let l = "11-13 15:15:21.844  6760  6806 I flutter : The Dart VM service is listening on http://127.0.0.1:41931/MnsHt2taLio=/";
        assert_eq!(vm_service(l), Some((41931, "/MnsHt2taLio=/".into())));
        assert_eq!(vm_service("I flutter : hello"), None);
        assert_eq!(
            vm_service("listening on http://127.0.0.1:41931/a;rm=/"),
            None
        );
    }

    #[test]
    fn change_kinds() {
        let f = Flutter;
        assert_eq!(f.change_kind("lib/src/page.dart"), Some(ReloadKind::Hot));
        assert_eq!(f.change_kind("assets/logo.png"), Some(ReloadKind::Restart));
        assert_eq!(f.change_kind("pubspec.yaml"), Some(ReloadKind::Full));
        assert_eq!(
            f.change_kind("android/app/build.gradle.kts"),
            Some(ReloadKind::Full)
        );
        assert_eq!(f.change_kind("test/widget_test.dart"), None);
    }

    #[test]
    fn application_id_from_gradle() {
        let d = std::env::temp_dir().join(format!("umr-flutter-appid-{}", std::process::id()));
        std::fs::create_dir_all(d.join("android").join("app")).unwrap();
        std::fs::write(
            d.join("android").join("app").join("build.gradle.kts"),
            "android {\n    defaultConfig {\n        applicationId = \"com.umr.flutterdemo\"\n    }\n}\n",
        )
        .unwrap();
        assert_eq!(application_id(&d).as_deref(), Some("com.umr.flutterdemo"));
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(
            umr_adapter::contract::check_static(&Flutter),
            Vec::<String>::new()
        );
    }
}
