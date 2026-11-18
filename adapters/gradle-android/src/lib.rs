//! Gradle Android adapter (ADP-002, Phase 3 step 3.2) — Kotlin and Java projects.
//!
//! * Build: the project's own wrapper (`gradlew.bat :<app>:assembleDebug --console=plain`) with
//!   the isolated environment and the resolver's JDK, through the Supervisor in the run's group:
//!   the Gradle daemon stays warm between builds and ends with the session.
//! * Output is read while Gradle runs: `> Task` lines become progress, compiler lines become
//!   [`Problem`]s with file and line (kotlinc, javac, AAPT), and Gradle's "What went wrong" becomes
//!   the failure summary.
//! * Install `adb install -r -t` (keeps app data), launch the launcher activity.
//! * Capabilities: incremental build only. No hot reload / restart outside Android Studio, so a
//!   reload is always a full (incremental) rebuild + reinstall + relaunch.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use umr_adapter::problems::{gradle_failure, parse_line};
use umr_adapter::{
    Adapter, AdapterContext, AdapterError, Capabilities, Event, Framework, Issue, Phase, Problem,
    ReloadKind, RunningApp, Severity, Sink,
};
use umr_supervisor::{ProcessSpec, RunState};

/// Longest build the adapter waits for (cold builds download Gradle + dependencies).
pub const BUILD_TIMEOUT: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Default, Clone, Copy)]
pub struct GradleAndroid;

/// The application module: a sub-folder whose build file applies `com.android.application`.
#[must_use]
pub fn app_module(root: &Path) -> Option<String> {
    let has_app_plugin = |dir: &Path| {
        ["build.gradle.kts", "build.gradle"].iter().any(|f| {
            std::fs::read_to_string(dir.join(f)).is_ok_and(|t| {
                t.contains("com.android.application") || t.contains("android.application")
            })
        })
    };
    if has_app_plugin(&root.join("app")) {
        return Some("app".into());
    }
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs.into_iter()
        .find(|d| has_app_plugin(d))
        .and_then(|d| d.file_name().map(|n| n.to_string_lossy().into_owned()))
}

/// Newest `.apk` in `<module>/build/outputs/apk/debug`.
#[must_use]
pub fn debug_apk(root: &Path, module: &str) -> Option<PathBuf> {
    let dir = root
        .join(module)
        .join("build")
        .join("outputs")
        .join("apk")
        .join("debug");
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("apk")))
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
}

/// Folders that are output, caches or tools, never inputs of the app's build.
const NOT_INPUTS: &[&str] = &[
    "build",
    ".gradle",
    ".cxx",
    ".kotlin",
    ".idea",
    ".git",
    "node_modules",
    ".dart_tool",
];

/// Newest modification time of the files below `dir` (output and cache folders skipped).
fn newest_input(dir: &Path, depth: usize, budget: &mut usize) -> Option<std::time::SystemTime> {
    let mut best = None;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        if *budget == 0 {
            return Some(std::time::SystemTime::now()); // too many files to be sure: rebuild
        }
        *budget -= 1;
        let name = e.file_name();
        if name
            .to_str()
            .is_some_and(|n| NOT_INPUTS.contains(&n) || n == "local.properties")
        {
            continue;
        }
        let Ok(t) = e.file_type() else { continue };
        let m = if t.is_dir() {
            if depth == 0 {
                continue;
            }
            newest_input(&e.path(), depth - 1, budget)
        } else {
            e.metadata().and_then(|m| m.modified()).ok()
        };
        if m > best {
            best = m;
        }
    }
    best
}

/// AUD-P1-45: the debug APK when nothing it is built from changed since it was built: the
/// Gradle project below `root` and the `extra` files (React Native: `package.json` and the lock
/// file, whose native modules go into the APK). A device switch then installs it instead of a
/// Gradle build (30–45 s measured), as the Flutter adapter already does.
#[must_use]
pub fn up_to_date_apk(root: &Path, module: &str, extra: &[PathBuf]) -> Option<PathBuf> {
    let apk = debug_apk(root, module)?;
    // a no-change Gradle build leaves the APK's time alone: the stamp holds when the last
    // successful build started, which covers every input it saw
    let time = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let built = time(&apk).max(time(&stamp_path(&apk)))?;
    let mut budget = 60_000;
    let tree = newest_input(root, 12, &mut budget)?;
    let files = extra
        .iter()
        .filter_map(|f| std::fs::metadata(f).and_then(|m| m.modified()).ok())
        .max();
    (tree <= built && files.is_none_or(|t| t <= built)).then_some(apk)
}

/// The file next to the APK whose time is the start of the last successful build.
fn stamp_path(apk: &Path) -> PathBuf {
    apk.with_file_name(".umr-built")
}

/// Records that the build that started at `started` produced `apk` (AUD-P1-45).
fn stamp(apk: &Path, started: std::time::SystemTime) {
    let p = stamp_path(apk);
    if std::fs::write(&p, b"").is_ok() {
        if let Ok(f) = std::fs::OpenOptions::new().write(true).open(&p) {
            let _ = f.set_modified(started);
        }
    }
}

/// What a Gradle log line means for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineKind {
    Task(String),
    Problem(Problem),
    Other,
}

#[must_use]
pub fn classify(line: &str, root: &Path) -> LineKind {
    if let Some(t) = line.strip_prefix("> Task ") {
        return LineKind::Task(t.split_whitespace().next().unwrap_or(t).to_string());
    }
    parse_line(line, root).map_or(LineKind::Other, LineKind::Problem)
}

impl Adapter for GradleAndroid {
    fn id(&self) -> &'static str {
        "gradle-android"
    }
    fn display_name(&self) -> &'static str {
        "Android (Kotlin / Java, Gradle)"
    }
    fn frameworks(&self) -> &'static [Framework] {
        &[Framework::AndroidNative]
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            hot_reload: false,
            hot_restart: false,
            incremental_build: true,
            structured_build_events: false,
        }
    }

    fn validate(&self, ctx: &AdapterContext) -> Vec<Issue> {
        let mut v = Vec::new();
        let root = &ctx.project;
        if !root.join("gradlew.bat").is_file() {
            v.push(Issue {
                message: "the project has no Gradle wrapper (gradlew.bat)".into(),
                fix: "add the wrapper (`gradle wrapper`) or create the project with Android Studio / `android create`".into(),
            });
        } else if !root
            .join("gradle")
            .join("wrapper")
            .join("gradle-wrapper.jar")
            .is_file()
        {
            v.push(Issue {
                message: "gradle/wrapper/gradle-wrapper.jar is missing".into(),
                fix: "restore the wrapper files from version control".into(),
            });
        }
        if app_module(root).is_none() {
            v.push(Issue {
                message: "no module applies com.android.application".into(),
                fix: "open the folder that contains the app module".into(),
            });
        }
        match &ctx.jdk_home {
            Some(j) if j.join("bin").join("java.exe").is_file() => {}
            _ => v.push(Issue {
                message: "no JDK selected for the build".into(),
                fix: "run `umr install` for this project".into(),
            }),
        }
        if umr_device::app::find_aapt2(&ctx.sdk).is_none() {
            v.push(Issue {
                message: "Android build-tools are not installed".into(),
                fix: "run `umr install` for this project".into(),
            });
        }
        v
    }

    /// Sources, resources and build scripts need a (incremental) rebuild.
    fn change_kind(&self, rel: &str) -> Option<ReloadKind> {
        let ext = rel.rsplit('.').next().unwrap_or("");
        let source = matches!(
            ext,
            "kt" | "kts"
                | "java"
                | "xml"
                | "gradle"
                | "toml"
                | "properties"
                | "pro"
                | "json"
                | "png"
                | "webp"
                | "jpg"
                | "ttf"
        ) || rel.contains("/src/");
        source.then_some(ReloadKind::Full)
    }

    fn start(&self, ctx: &AdapterContext, sink: Sink) -> Result<Box<dyn RunningApp>, AdapterError> {
        if let Some(i) = self.validate(ctx).into_iter().next() {
            return Err(AdapterError::Missing(format!("{} ({})", i.message, i.fix)));
        }
        let module = app_module(&ctx.project).unwrap_or_else(|| "app".into());
        // AUD-P1-45: nothing changed since the last build (a device switch): install that APK
        let (apk, reused) = match up_to_date_apk(&ctx.project, &module, &[]) {
            Some(apk) => {
                sink(Event::Phase {
                    phase: Phase::Building,
                    message: format!(
                        "reusing {} (nothing changed since it was built)",
                        apk.display()
                    ),
                });
                (apk, true)
            }
            None => (build(ctx, &module, &sink)?, false),
        };
        let package = install(ctx, &apk, &sink)?;
        launch(ctx, &package, &sink)?;
        if reused {
            warm_up(ctx, &module);
        }
        Ok(Box::new(GradleApp {
            ctx: ctx.clone(),
            module,
            package,
        }))
    }
}

fn emit_line(line: &str, root: &Path, sink: &Sink, problems: &mut Vec<Problem>, tasks: &mut u32) {
    match classify(line, root) {
        LineKind::Task(t) => {
            *tasks += 1;
            sink(Event::Phase {
                phase: Phase::Building,
                message: t,
            });
        }
        LineKind::Problem(p) => {
            if !problems.contains(&p) {
                sink(Event::Problem(p.clone()));
                problems.push(p);
            }
        }
        LineKind::Other => {}
    }
}

/// `gradlew :<module>:assembleDebug` as a supervised process of the run's group.
fn gradle_spec(
    ctx: &AdapterContext,
    module: &str,
    extra: &[String],
    name: &str,
    log: &Path,
) -> ProcessSpec {
    let mut spec = ProcessSpec::new(name, ctx.project.join("gradlew.bat"), log).args(
        [
            format!(":{module}:assembleDebug"),
            "--console=plain".into(),
            // reuse the configured task graph: a no-change build skips ~10 s of configuration
            "--configuration-cache".into(),
            "--configuration-cache-problems=warn".into(),
        ]
        .into_iter()
        .chain(extra.iter().cloned()),
    );
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
    spec
}

/// Longest wait for a warm-up build before a real build starts anyway.
const WARM_UP_WAIT: Duration = Duration::from_secs(180);

fn warm_up_name(ctx: &AdapterContext) -> String {
    format!("gradle-warm:{}", ctx.group)
}

/// ADP-002b: a run that reuses an unchanged APK never starts Gradle, so the first edit paid for
/// a cold Gradle daemon (45 s instead of 7 s on the Kotlin sample, 2026-11-19). This starts a
/// no-change build in the background (nothing is rebuilt: the APK is up to date) so the daemon
/// is warm by the time the user saves.
pub fn warm_up(ctx: &AdapterContext, module: &str) {
    let name = warm_up_name(ctx);
    let log = ctx
        .log_dir
        .join(format!("gradle-warm-{}.log", ctx.group.replace(':', "-")));
    let mut spec = gradle_spec(ctx, module, &[], &name, &log);
    spec.restart = umr_supervisor::RestartPolicy::Never;
    let _ = ctx.supervisor.start(spec);
}

/// A real build waits for a running warm-up: started next to it, Gradle would find its only
/// daemon busy and start a second, cold one.
fn wait_for_warm_up(ctx: &AdapterContext, sink: &Sink) {
    let name = warm_up_name(ctx);
    let running = |ctx: &AdapterContext| {
        matches!(
            ctx.supervisor.status(&name).map(|s| s.state),
            Some(RunState::Running | RunState::Restarting)
        )
    };
    if !running(ctx) {
        return;
    }
    sink(Event::Phase {
        phase: Phase::Building,
        message: "Gradle is still starting up; the build follows".into(),
    });
    let start = Instant::now();
    while running(ctx) && start.elapsed() < WARM_UP_WAIT {
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Runs `gradlew :<module>:assembleDebug` in `ctx.project`; returns the APK.
///
/// # Errors
/// The build failed (with the problems found) or could not start.
pub fn build(ctx: &AdapterContext, module: &str, sink: &Sink) -> Result<PathBuf, AdapterError> {
    build_with(ctx, module, &[], sink)
}

/// [`build`] with extra Gradle arguments (e.g. `-PreactNativeArchitectures=x86_64`).
///
/// # Errors
/// The build failed (with the problems found) or could not start.
#[allow(clippy::too_many_lines)] // start, follow the log, judge the result
pub fn build_with(
    ctx: &AdapterContext,
    module: &str,
    extra: &[String],
    sink: &Sink,
) -> Result<PathBuf, AdapterError> {
    let root = &ctx.project;
    let name = format!("gradle:{}", ctx.group);
    let log = ctx
        .log_dir
        .join(format!("gradle-{}.log", ctx.group.replace(':', "-")));
    let spec = gradle_spec(ctx, module, extra, &name, &log);
    wait_for_warm_up(ctx, sink);
    sink(Event::Phase {
        phase: Phase::Building,
        message: format!("gradlew :{module}:assembleDebug"),
    });
    let start = Instant::now();
    let started_at = std::time::SystemTime::now();
    ctx.supervisor
        .start(spec)
        .map_err(|e| AdapterError::Failed(format!("cannot start Gradle: {e}")))?;

    // follow the log while Gradle runs
    let (mut pos, mut pending, mut problems, mut tasks) = (0u64, String::new(), Vec::new(), 0u32);
    let read_new =
        |pos: &mut u64, pending: &mut String, problems: &mut Vec<Problem>, tasks: &mut u32| {
            let Ok(mut f) = std::fs::File::open(&log) else {
                return;
            };
            if f.seek(SeekFrom::Start(*pos)).is_err() {
                return;
            }
            let mut buf = Vec::new();
            if f.read_to_end(&mut buf).is_ok() {
                *pos += buf.len() as u64;
                pending.push_str(&String::from_utf8_lossy(&buf));
                while let Some(nl) = pending.find('\n') {
                    let line: String = pending.drain(..=nl).collect();
                    emit_line(line.trim_end(), root, sink, problems, tasks);
                }
            }
        };
    let status = loop {
        read_new(&mut pos, &mut pending, &mut problems, &mut tasks);
        let st = ctx.supervisor.status(&name);
        match st.as_ref().map(|s| s.state) {
            Some(RunState::Running | RunState::Restarting) => {}
            _ => break st,
        }
        if start.elapsed() > BUILD_TIMEOUT {
            ctx.supervisor.stop(&name);
            return Err(AdapterError::BuildFailed {
                summary: format!(
                    "Gradle did not finish within {} min",
                    BUILD_TIMEOUT.as_secs() / 60
                ),
                problems,
            });
        }
        std::thread::sleep(Duration::from_millis(300));
    };
    read_new(&mut pos, &mut pending, &mut problems, &mut tasks);
    if !pending.trim().is_empty() {
        emit_line(pending.trim_end(), root, sink, &mut problems, &mut tasks);
    }
    let code = status.and_then(|s| s.exit_code);
    if code != Some(0) {
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        let summary = gradle_failure(&text)
            .unwrap_or_else(|| format!("Gradle exited with {code:?}; log: {}", log.display()));
        if problems.is_empty() {
            let p = Problem {
                severity: Severity::Error,
                message: summary.clone(),
                file: None,
                line: None,
                column: None,
                source: "gradle".into(),
            };
            sink(Event::Problem(p.clone()));
            problems.push(p);
        }
        return Err(AdapterError::BuildFailed { summary, problems });
    }
    sink(Event::Log {
        line: format!(
            "build finished in {:.1} s ({tasks} tasks)",
            start.elapsed().as_secs_f64()
        ),
    });
    let apk = debug_apk(root, module).ok_or_else(|| {
        AdapterError::Failed(format!(
            "the build succeeded but no APK is in {module}/build/outputs/apk/debug"
        ))
    })?;
    stamp(&apk, started_at);
    Ok(apk)
}

/// Installs the APK on the run's device; returns its package.
///
/// # Errors
/// aapt2 is missing or adb refused the APK.
pub fn install(ctx: &AdapterContext, apk: &Path, sink: &Sink) -> Result<String, AdapterError> {
    sink(Event::Phase {
        phase: Phase::Installing,
        message: apk.display().to_string(),
    });
    let aapt2 = umr_device::app::find_aapt2(&ctx.sdk)
        .ok_or_else(|| AdapterError::Missing("Android build-tools (aapt2)".into()))?;
    let package = umr_device::app::apk_package(&aapt2, apk).map_err(AdapterError::InstallFailed)?;
    // AUD-P1-45: after a device switch the emulator usually has this very APK already
    if umr_device::app::installed_same(ctx.device.adb.port, &ctx.device.serial, &package, apk) {
        sink(Event::Log {
            line: format!("{package}: this APK is installed already"),
        });
        return Ok(package);
    }
    umr_device::app::install(&ctx.device.adb, &ctx.device.serial, apk)
        .map_err(AdapterError::InstallFailed)?;
    Ok(package)
}

/// Starts the launcher activity and reports `AppStarted`.
///
/// # Errors
/// The activity could not be started.
pub fn launch(ctx: &AdapterContext, package: &str, sink: &Sink) -> Result<(), AdapterError> {
    sink(Event::Phase {
        phase: Phase::Launching,
        message: package.to_string(),
    });
    let l = umr_device::app::launch(ctx.device.adb.port, &ctx.device.serial, package, None)
        .map_err(AdapterError::LaunchFailed)?;
    sink(Event::AppStarted {
        package: package.to_string(),
        pid: l.pid,
    });
    Ok(())
}

#[derive(Debug)]
struct GradleApp {
    ctx: AdapterContext,
    module: String,
    package: String,
}

impl RunningApp for GradleApp {
    fn package(&self) -> &str {
        &self.package
    }

    fn reload(&mut self, kind: ReloadKind, sink: &Sink) -> Result<Duration, AdapterError> {
        if kind != ReloadKind::Full {
            return Err(AdapterError::Unsupported(
                "native Android has no hot reload outside Android Studio; use a full reload (incremental build + reinstall)".into(),
            ));
        }
        let start = Instant::now();
        sink(Event::Phase {
            phase: Phase::Reloading,
            message: "incremental build".into(),
        });
        let apk = build(&self.ctx, &self.module, sink)?;
        let pkg = install(&self.ctx, &apk, sink)?;
        launch(&self.ctx, &pkg, sink)?;
        self.package = pkg;
        let took = start.elapsed();
        sink(Event::Reloaded {
            kind,
            ms: u64::try_from(took.as_millis()).unwrap_or(u64::MAX),
        });
        Ok(took)
    }

    fn is_alive(&mut self) -> bool {
        umr_device::app::pid(
            self.ctx.device.adb.port,
            &self.ctx.device.serial,
            &self.package,
        )
        .is_some()
    }

    fn stop(self: Box<Self>) {
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
    fn an_unchanged_project_reuses_its_apk() {
        // AUD-P1-45
        let d = tmp("reuse");
        let set = |p: &Path, secs: u64| {
            let f = std::fs::OpenOptions::new().write(true).open(p).unwrap();
            f.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs))
                .unwrap();
        };
        let src = d.join("app/src/main/java/Main.kt");
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        std::fs::write(&src, "fun main() {}").unwrap();
        set(&src, 1_000);
        assert_eq!(up_to_date_apk(&d, "app", &[]), None, "nothing built yet");
        let out = d.join("app/build/outputs/apk/debug");
        std::fs::create_dir_all(&out).unwrap();
        let apk = out.join("app-debug.apk");
        std::fs::write(&apk, "apk").unwrap();
        set(&apk, 2_000);
        // build output newer than the APK does not count
        let tmp_out = d.join("app/build/tmp/x.txt");
        std::fs::create_dir_all(tmp_out.parent().unwrap()).unwrap();
        std::fs::write(&tmp_out, "x").unwrap();
        assert_eq!(
            up_to_date_apk(&d, "app", &[]).as_deref(),
            Some(apk.as_path())
        );
        // a later edit of the sources: build again
        set(&src, 3_000);
        assert_eq!(up_to_date_apk(&d, "app", &[]), None);
        // … unless a later no-change build confirmed the APK (its time stays old, the stamp is new)
        stamp(
            &apk,
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(4_000),
        );
        assert_eq!(
            up_to_date_apk(&d, "app", &[]).as_deref(),
            Some(apk.as_path())
        );
        std::fs::remove_file(stamp_path(&apk)).unwrap();
        set(&src, 1_000);
        // React Native: a newer package.json (native modules) also means a build
        let pkg = d.join("package.json");
        std::fs::write(&pkg, "{}").unwrap();
        set(&pkg, 3_000);
        assert_eq!(up_to_date_apk(&d, "app", std::slice::from_ref(&pkg)), None);
        set(&pkg, 1_500);
        assert!(up_to_date_apk(&d, "app", &[pkg]).is_some());
        let _ = std::fs::remove_dir_all(&d);
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("umr-gradle-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn finds_the_app_module_and_apk() {
        let d = tmp("mod");
        std::fs::create_dir_all(d.join("lib")).unwrap();
        std::fs::write(
            d.join("lib").join("build.gradle.kts"),
            "plugins { id(\"com.android.library\") }",
        )
        .unwrap();
        assert_eq!(app_module(&d), None);
        std::fs::create_dir_all(d.join("mobile")).unwrap();
        std::fs::write(
            d.join("mobile").join("build.gradle"),
            "plugins { alias(libs.plugins.android.application) }",
        )
        .unwrap();
        assert_eq!(app_module(&d).as_deref(), Some("mobile"));
        std::fs::create_dir_all(d.join("app")).unwrap();
        std::fs::write(
            d.join("app").join("build.gradle.kts"),
            "plugins { id(\"com.android.application\") }",
        )
        .unwrap();
        assert_eq!(app_module(&d).as_deref(), Some("app"), "`app` wins");

        let out = d.join("app/build/outputs/apk/debug");
        std::fs::create_dir_all(&out).unwrap();
        assert!(debug_apk(&d, "app").is_none());
        std::fs::write(out.join("app-debug.apk"), b"x").unwrap();
        assert_eq!(
            debug_apk(&d, "app").unwrap().file_name().unwrap(),
            "app-debug.apk"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn gradle_lines() {
        let root = Path::new(r"C:\p");
        assert_eq!(
            classify("> Task :app:compileDebugKotlin", root),
            LineKind::Task(":app:compileDebugKotlin".into())
        );
        assert_eq!(
            classify("> Task :app:preBuild UP-TO-DATE", root),
            LineKind::Task(":app:preBuild".into())
        );
        assert!(matches!(
            classify("e: file:///C:/p/app/src/main/java/x/A.kt:3:1 Syntax error", root),
            LineKind::Problem(p) if p.file.as_deref() == Some("app/src/main/java/x/A.kt")
        ));
        assert_eq!(classify("BUILD SUCCESSFUL in 4s", root), LineKind::Other);
    }

    #[test]
    fn changes_need_a_rebuild() {
        let g = GradleAndroid;
        assert_eq!(
            g.change_kind("app/src/main/kotlin/x/A.kt"),
            Some(ReloadKind::Full)
        );
        assert_eq!(
            g.change_kind("app/build.gradle.kts"),
            Some(ReloadKind::Full)
        );
        assert_eq!(g.change_kind("README.md"), None);
    }

    #[test]
    fn declares_full_reload_only() {
        let c = GradleAndroid.capabilities();
        assert!(!c.hot_reload && !c.hot_restart && c.incremental_build);
        assert_eq!(
            umr_adapter::contract::check_static(&GradleAndroid),
            Vec::<String>::new()
        );
    }
}
