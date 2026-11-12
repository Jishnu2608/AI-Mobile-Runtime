//! Contract test kit (ADP-001). Every adapter must pass:
//!
//! * [`check_static`]: id, name, frameworks; full reload is always declared supported.
//! * [`run`]: the live scenario on a real device — validate → start (app runs, `AppStarted`
//!   reported) → each reload kind (declared ones must work and keep the app running, the others
//!   must answer `Unsupported`) → stop (app gone).
//!
//! `app_running(package)` tells the kit whether the app runs on the device; the live tests use
//! `pidof` over the product's adb, unit tests a fake.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::{Adapter, AdapterContext, AdapterError, Event, ReloadKind, Sink};

/// Static rules; returns the violations.
#[must_use]
pub fn check_static(a: &dyn Adapter) -> Vec<String> {
    let mut v = Vec::new();
    let id = a.id();
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
        v.push(format!("id '{id}' must be lower-case words joined by '-'"));
    }
    if a.display_name().trim().is_empty() {
        v.push("display name is empty".into());
    }
    if a.frameworks().is_empty() {
        v.push("no frameworks declared".into());
    }
    let c = a.capabilities();
    if c.hot_reload && !c.hot_restart {
        v.push(
            "hot reload without hot restart: a failed hot reload needs a restart to fall back to"
                .into(),
        );
    }
    v
}

#[derive(Debug, Clone, Serialize)]
pub struct Step {
    pub name: String,
    pub ok: bool,
    pub detail: String,
    pub ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub adapter: String,
    pub steps: Vec<Step>,
}

impl Report {
    #[must_use]
    pub fn passed(&self) -> bool {
        !self.steps.is_empty() && self.steps.iter().all(|s| s.ok)
    }
}

fn ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

fn wait(timeout: Duration, mut f: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    loop {
        if f() {
            return true;
        }
        if start.elapsed() > timeout {
            return false;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Runs the live scenario. `app_running(package)` checks the device.
#[allow(clippy::too_many_lines)] // the scenario reads top to bottom
pub fn run(a: &dyn Adapter, ctx: &AdapterContext, app_running: &dyn Fn(&str) -> bool) -> Report {
    let mut steps = Vec::new();
    let mut step = |name: &str, ok: bool, detail: String, d: Duration| {
        steps.push(Step {
            name: name.into(),
            ok,
            detail,
            ms: ms(d),
        });
        ok
    };
    let events: Arc<Mutex<Vec<Event>>> = Arc::default();
    let ev = events.clone();
    let sink: Sink = Arc::new(move |e| {
        ev.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(e);
    });

    for v in check_static(a) {
        step("static", false, v, Duration::ZERO);
    }
    let t = Instant::now();
    let issues = a.validate(ctx);
    if !step(
        "validate",
        issues.is_empty(),
        issues
            .iter()
            .map(|i| format!("{} (fix: {})", i.message, i.fix))
            .collect::<Vec<_>>()
            .join("; "),
        t.elapsed(),
    ) {
        return Report {
            adapter: a.id().into(),
            steps,
        };
    }

    let t = Instant::now();
    let mut app = match a.start(ctx, sink.clone()) {
        Ok(app) => app,
        Err(e) => {
            step("start", false, e.to_string(), t.elapsed());
            return Report {
                adapter: a.id().into(),
                steps,
            };
        }
    };
    let pkg = app.package().to_string();
    let started = wait(Duration::from_secs(15), || app_running(&pkg));
    let announced = events
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .any(|e| matches!(e, Event::AppStarted { package, .. } if *package == pkg));
    step(
        "start",
        started && announced,
        format!("{pkg} running: {started}, AppStarted reported: {announced}"),
        t.elapsed(),
    );

    let caps = a.capabilities();
    for (kind, declared) in [
        (ReloadKind::Hot, caps.hot_reload),
        (ReloadKind::Restart, caps.hot_restart),
        (ReloadKind::Full, true),
    ] {
        let t = Instant::now();
        let r = app.reload(kind, &sink);
        let name = format!("reload {kind:?}").to_lowercase();
        match (declared, r) {
            (true, Ok(d)) => {
                let alive = wait(Duration::from_secs(15), || app_running(&pkg)) && app.is_alive();
                step(
                    &name,
                    alive,
                    format!("{} ms, app running afterwards: {alive}", d.as_millis()),
                    t.elapsed(),
                );
            }
            (true, Err(e)) => {
                step(&name, false, e.to_string(), t.elapsed());
            }
            (false, Err(AdapterError::Unsupported(_))) => {
                step(
                    &name,
                    true,
                    "not declared; answered Unsupported".into(),
                    t.elapsed(),
                );
            }
            (false, other) => {
                step(
                    &name,
                    false,
                    format!("not declared but answered {other:?}"),
                    t.elapsed(),
                );
            }
        }
    }

    let t = Instant::now();
    app.stop();
    let gone = wait(Duration::from_secs(10), || !app_running(&pkg));
    step("stop", gone, format!("{pkg} stopped: {gone}"), t.elapsed());
    Report {
        adapter: a.id().into(),
        steps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Capabilities, Framework, Issue, RunningApp};
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// A fake adapter whose "device" is a flag.
    struct Fake {
        caps: Capabilities,
        running: Arc<AtomicBool>,
        lie_about_hot: bool,
    }

    #[derive(Debug)]
    struct FakeApp {
        running: Arc<AtomicBool>,
        caps: Capabilities,
        lie_about_hot: bool,
    }

    impl RunningApp for FakeApp {
        fn package(&self) -> &'static str {
            "com.umr.fake"
        }
        fn reload(&mut self, kind: ReloadKind, sink: &Sink) -> Result<Duration, AdapterError> {
            let ok = match kind {
                ReloadKind::Hot => self.caps.hot_reload || self.lie_about_hot,
                ReloadKind::Restart => self.caps.hot_restart,
                ReloadKind::Full => true,
            };
            if !ok {
                return Err(AdapterError::Unsupported(format!("{kind:?}")));
            }
            sink(Event::Reloaded { kind, ms: 5 });
            Ok(Duration::from_millis(5))
        }
        fn is_alive(&mut self) -> bool {
            self.running.load(Ordering::SeqCst)
        }
        fn stop(self: Box<Self>) {
            self.running.store(false, Ordering::SeqCst);
        }
    }

    impl Adapter for Fake {
        fn id(&self) -> &'static str {
            "fake"
        }
        fn display_name(&self) -> &'static str {
            "Fake"
        }
        fn frameworks(&self) -> &'static [Framework] {
            &[Framework::AndroidNative]
        }
        fn capabilities(&self) -> Capabilities {
            self.caps
        }
        fn detect(&self, _: &Path) -> u8 {
            100
        }
        fn validate(&self, _: &AdapterContext) -> Vec<Issue> {
            Vec::new()
        }
        fn start(
            &self,
            _: &AdapterContext,
            sink: Sink,
        ) -> Result<Box<dyn RunningApp>, AdapterError> {
            self.running.store(true, Ordering::SeqCst);
            sink(Event::AppStarted {
                package: "com.umr.fake".into(),
                pid: Some(1),
            });
            Ok(Box::new(FakeApp {
                running: self.running.clone(),
                caps: self.caps,
                lie_about_hot: self.lie_about_hot,
            }))
        }
    }

    fn ctx() -> AdapterContext {
        AdapterContext {
            project: "C:\\p".into(),
            env: std::collections::HashMap::new(),
            sdk: "C:\\sdk".into(),
            jdk_home: None,
            flutter_root: None,
            device: crate::DeviceTarget {
                serial: "emulator-5554".into(),
                adb: umr_device::server::AdbServer::new(
                    Path::new("adb.exe"),
                    5137,
                    std::collections::HashMap::new(),
                ),
            },
            supervisor: umr_supervisor::Supervisor::new(),
            group: "run:test".into(),
            log_dir: std::env::temp_dir(),
        }
    }

    fn caps(hot: bool) -> Capabilities {
        Capabilities {
            hot_reload: hot,
            hot_restart: hot,
            incremental_build: true,
            structured_build_events: false,
        }
    }

    #[test]
    fn honest_adapters_pass() {
        for hot in [false, true] {
            let running = Arc::new(AtomicBool::new(false));
            let a = Fake {
                caps: caps(hot),
                running: running.clone(),
                lie_about_hot: false,
            };
            let r = run(&a, &ctx(), &|_| running.load(Ordering::SeqCst));
            assert!(r.passed(), "{r:?}");
            assert_eq!(r.steps.len(), 6, "validate, start, 3 reloads, stop");
        }
    }

    #[test]
    fn undeclared_capability_that_works_fails_the_contract() {
        let running = Arc::new(AtomicBool::new(false));
        let a = Fake {
            caps: caps(false),
            running: running.clone(),
            lie_about_hot: true,
        };
        let r = run(&a, &ctx(), &|_| running.load(Ordering::SeqCst));
        assert!(!r.passed());
        assert!(r.steps.iter().any(|s| s.name == "reload hot" && !s.ok));
    }

    #[test]
    fn static_rules() {
        let a = Fake {
            caps: Capabilities {
                hot_reload: true,
                hot_restart: false,
                incremental_build: false,
                structured_build_events: false,
            },
            running: Arc::default(),
            lie_about_hot: false,
        };
        let v = check_static(&a);
        assert_eq!(v.len(), 1, "{v:?}");
    }
}
