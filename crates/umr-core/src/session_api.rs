//! Session API (step 2.6): `session.start` / `session.status` / `session.stop`, recovery at
//! Core start and settling at Core stop. One session (one device) at a time in the MVP.

use std::sync::atomic::Ordering;
use std::sync::PoisonError;

use umr_api::methods::StartSessionParams;
use umr_api::rpc::{codes, Id, Response, RpcError};
use umr_session::{Event, Session, State};

use crate::api_server::{ok, ApiState};
use crate::{app_api, device_api};

fn journal_error(e: &umr_session::SessionError) -> RpcError {
    RpcError::new(codes::INTERNAL_ERROR, e.to_string())
}

/// Applies an event, journals it and publishes the new state.
pub(crate) fn apply(
    state: &ApiState,
    s: &mut Session,
    event: Event,
    detail: Option<&str>,
) -> Result<(), RpcError> {
    {
        let mut db = state.db.lock().unwrap_or_else(PoisonError::into_inner);
        s.apply(&mut db, event, detail)
            .map_err(|e| journal_error(&e))?;
    }
    tracing::info!(session = %s.id, event = event.as_str(), state = s.state.as_str(), detail, "session");
    publish(state, Some(s.clone()));
    Ok(())
}

/// Stores the current session and wakes every `session.wait` (SES-002).
pub(crate) fn publish(state: &ApiState, s: Option<Session>) {
    *state.session.lock().unwrap_or_else(PoisonError::into_inner) = s;
    *state
        .session_version
        .lock()
        .unwrap_or_else(PoisonError::into_inner) += 1;
    state.session_changed.notify_all();
}

fn version(state: &ApiState) -> u64 {
    *state
        .session_version
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// `session.wait`: answers when the session changed after `since`, or at the timeout. While it
/// waits it checks every 2 s whether the device or the app died (what a polling client used to
/// notice), so a crash wakes the waiters too.
pub fn wait(state: &ApiState, id: Id, p: umr_api::methods::SessionWaitParams) -> Response {
    let timeout = std::time::Duration::from_millis(p.timeout_ms.clamp(0, 60_000));
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let v = version(state);
        if p.since == 0 || v > p.since {
            return ok(
                id,
                &serde_json::json!({ "version": v, "changed": p.since != 0, "session": current(state) }),
            );
        }
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return ok(
                id,
                &serde_json::json!({ "version": v, "changed": false, "session": current(state) }),
            );
        }
        let slice = left.min(std::time::Duration::from_secs(2));
        {
            let guard = state
                .session_version
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let _ = state
                .session_changed
                .wait_timeout_while(guard, slice, |ver| *ver <= p.since)
                .unwrap_or_else(PoisonError::into_inner);
        }
        if version(state) <= p.since {
            // nothing published: look for a dead device or app (moves the session on)
            let _ = refresh(state);
        }
    }
}

pub(crate) fn current(state: &ApiState) -> Option<Session> {
    state
        .session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// Notices a dead emulator or app and moves the session on (`device_error` / `crashed`).
fn refresh(state: &ApiState) -> Option<Session> {
    let mut s = current(state)?;
    if s.state.has_device() {
        let alive = device_api::running(state).iter().any(|r| r.avd == s.avd);
        if !alive {
            let _ = apply(
                state,
                &mut s,
                Event::DeviceFailed,
                Some("the emulator stopped"),
            );
            return Some(s);
        }
    }
    if s.state == State::Running {
        if let (Some(app), Some(serial)) = (s.app.clone(), s.serial.clone()) {
            if umr_device::app::pid(state.adb_port, &serial, &app).is_none() {
                let _ = apply(
                    state,
                    &mut s,
                    Event::ProcessDied,
                    Some(&format!("{app} is no longer running")),
                );
            }
        }
    }
    Some(s)
}

struct Busy<'a>(&'a ApiState);

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.session_busy.store(false, Ordering::SeqCst);
    }
}

fn view(s: &Session, boot: Option<&umr_device::emulator::BootReport>) -> serde_json::Value {
    serde_json::json!({ "session": s, "boot": boot })
}

/// The session's device for a profile is up (booted, switched or recovered); used by
/// `project.run`.
pub(crate) fn ensure_device(state: &ApiState, p: &StartSessionParams) -> Result<(), RpcError> {
    if state
        .session_busy
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(RpcError::new(codes::BUSY, "a session is already starting"));
    }
    let _busy = Busy(state);
    start_inner(state, p).map(|_| ())
}

pub fn start(state: &ApiState, id: Id, p: &StartSessionParams) -> Response {
    if state
        .session_busy
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Response::err(
            Some(id),
            RpcError::new(codes::BUSY, "a session is already starting"),
        );
    }
    // a project running on the old device follows the switch (AC-CLI-03)
    let follow = state
        .run_info
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let busy = Busy(state);
    let r = start_inner(state, p);
    drop(busy);
    crate::preview_api::refresh_target(state);
    match r {
        Ok(v) => {
            // a screen-size change on the same emulator keeps the app running: no re-run
            let switched = follow
                .as_ref()
                .filter(|_| p.apk.is_none() && p.package.is_none() && v["resized"] != true);
            if let Some(run) = switched {
                let rerun =
                    crate::run_api::run_inner(state, &run.project, Some(&p.profile), false, true);
                return ok(
                    id,
                    &serde_json::json!({ "session": v["session"], "boot": v["boot"], "run": rerun.unwrap_or_else(|e| serde_json::json!({ "status": "failed", "error": e.message })) }),
                );
            }
            ok(id, &v)
        }
        Err(e) => Response::err(Some(id), e),
    }
}

#[allow(clippy::too_many_lines)] // the session lifecycle in order, one step per block
fn start_inner(state: &ApiState, p: &StartSessionParams) -> Result<serde_json::Value, RpcError> {
    if let Some(pkg) = &p.package {
        if !umr_device::app::valid_package(pkg) {
            return Err(RpcError::new(
                codes::INVALID_PARAMS,
                format!("'{pkg}' is not a valid Android package name"),
            ));
        }
    }
    let avd = device_api::resolve_avd(state, &p.profile, p.api.as_deref())?;
    let mut s = match refresh(state) {
        // the same device is already up and nothing new to launch (whatever the app's state:
        // a crashed or failed app does not mean the device has to go)
        Some(mut s)
            if s.avd == avd.name
                && s.state.has_device()
                && device_api::running(state).iter().any(|r| r.avd == s.avd)
                && p.apk.is_none()
                && p.package.is_none() =>
        {
            let boot = device_api::running(state)
                .into_iter()
                .find(|r| r.avd == avd.name);
            // SNAP-003: another size on the same emulator (compact / standard / large): change
            // the screen in place; the app keeps running
            if s.profile != p.profile {
                if let Some(serial) = s.serial.clone() {
                    if let Some(Err(e)) = device_api::apply_display(state, &serial, &p.profile) {
                        return Err(RpcError::new(
                            codes::INTERNAL_ERROR,
                            format!("could not switch the screen to {}: {e}", p.profile),
                        ));
                    }
                }
                {
                    let db = state.db.lock().unwrap_or_else(PoisonError::into_inner);
                    s.set_device(&db, &p.profile, &avd.name)
                        .map_err(|e| journal_error(&e))?;
                }
                publish(state, Some(s.clone()));
                let mut v = view(&s, boot.as_ref());
                v["resized"] = serde_json::json!(true);
                return Ok(v);
            }
            return Ok(view(&s, boot.as_ref()));
        }
        // recover the session the previous Core (or a crash) left behind
        Some(mut s) if s.avd == avd.name && s.state == State::DeviceError => {
            apply(state, &mut s, Event::Recover, Some("recovering"))?;
            s
        }
        // PRV-007: a device switch keeps the session (same id, journal continues)
        Some(mut old)
            if old.avd != avd.name
                && (old.state.has_device() || old.state == State::DeviceError) =>
        {
            // the app stops; the run's Gradle daemon stays warm for the re-run on the new device,
            // and the old emulator shuts down while the new one boots
            crate::run_api::stop_app(state);
            device_api::stop_avd_background(state, &old.avd);
            apply(
                state,
                &mut old,
                Event::SwitchDevice,
                Some(&format!("switch to {}", p.profile)),
            )?;
            {
                let db = state.db.lock().unwrap_or_else(PoisonError::into_inner);
                old.set_device(&db, &p.profile, &avd.name)
                    .map_err(|e| journal_error(&e))?;
            }
            old.serial = None;
            old.app = None; // the app is not on the new device (yet)
            publish(state, Some(old.clone()));
            old
        }
        other => {
            if let Some(mut old) = other.filter(|o| o.state.is_active()) {
                device_api::stop_avd(state, &old.avd);
                apply(
                    state,
                    &mut old,
                    Event::Stop,
                    Some(&format!("switched to {}", p.profile)),
                )?;
            }
            let mut s = {
                let db = state.db.lock().unwrap_or_else(PoisonError::into_inner);
                Session::create(&db, &p.profile, &avd.name, None).map_err(|e| journal_error(&e))?
            };
            apply(state, &mut s, Event::Start, None)?;
            let emulator = state
                .toolchains
                .join("android-sdk")
                .join("emulator")
                .join("emulator.exe");
            if !emulator.is_file() {
                let msg = "the Android emulator is not installed yet; run `umr install` for a project first";
                apply(state, &mut s, Event::EnvMissing, Some(msg))?;
                return Err(RpcError::new(codes::UNAVAILABLE, msg));
            }
            apply(state, &mut s, Event::EnvReady, None)?;
            s
        }
    };

    // Booting
    let boot = match device_api::boot_avd(state, &avd, p.cold, false, None) {
        Ok(b) => b,
        Err(e) => {
            apply(state, &mut s, Event::DeviceFailed, Some(&e.message))?;
            return Err(e);
        }
    };
    s.serial = Some(boot.serial.clone());
    // SNAP-003: the shared emulator keeps the last size it was set to (also in its snapshot):
    // set it for this profile on every boot
    if let Some(Err(e)) = device_api::apply_display(state, &boot.serial, &p.profile) {
        tracing::warn!(profile = %p.profile, error = %e, "setting the screen size failed");
    }
    // AUD-P1-41: a foldable starts open flat on its inner screen, whatever its snapshot held
    if let Some(r) = device_api::running(state)
        .into_iter()
        .find(|r| r.serial == boot.serial)
    {
        let g = r
            .grpc_port
            .map(|port| umr_device::grpc::Emulator::new(port, r.grpc_token.clone()));
        crate::fold_api::on_boot(state, &boot.serial, &r.avd, g.as_ref());
    }
    let how = format!(
        "{:?} boot in {} s on {}",
        boot.mode, boot.seconds, boot.serial
    )
    .to_lowercase();
    apply(state, &mut s, Event::DeviceOnline, Some(&how))?;
    // AUD-P1-9: in the first minute and a half after a boot Android's System UI is often slow
    // and shows "isn't responding" over the app: choose Wait for the user
    if let Some(weak) = state.self_ref.get().cloned() {
        let serial = boot.serial.clone();
        let _ = std::thread::Builder::new()
            .name("umr-anr-guard".into())
            .spawn(move || {
                for _ in 0..30 {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    let Some(st) = weak.upgrade() else { return };
                    crate::diag_api::clear_system_anr(&st.diag, st.adb_port, &serial);
                }
            });
    }

    // Building → Launching / Running
    let package = match (&p.apk, &p.package) {
        (None, None) => {
            apply(state, &mut s, Event::NothingToLaunch, Some("device only"))?;
            return Ok(view(&s, Some(&boot)));
        }
        (Some(apk), _) => {
            apply(state, &mut s, Event::ArtifactReady, Some(apk))?;
            match app_api::install_apk(state, &boot.serial, apk) {
                Ok(pkg) => pkg,
                Err(e) => {
                    apply(state, &mut s, Event::LaunchFailed, Some(&e.message))?;
                    return Err(e);
                }
            }
        }
        (None, Some(pkg)) => {
            apply(
                state,
                &mut s,
                Event::ArtifactReady,
                Some("already installed"),
            )?;
            pkg.clone()
        }
    };
    let pkg = p.package.clone().unwrap_or(package);
    s.app = Some(pkg.clone());
    match umr_device::app::launch(state.adb_port, &boot.serial, &pkg, None) {
        Ok(l) => {
            apply(
                state,
                &mut s,
                Event::AppForeground,
                Some(&format!(
                    "{} started in {} ms",
                    l.component,
                    l.total_ms.unwrap_or(0)
                )),
            )?;
            Ok(view(&s, Some(&boot)))
        }
        Err(e) => {
            apply(state, &mut s, Event::LaunchFailed, Some(&e))?;
            Err(RpcError::new(codes::INTERNAL_ERROR, e))
        }
    }
}

pub fn status(state: &ApiState, id: Id) -> Response {
    let s = refresh(state);
    let events = s.as_ref().and_then(|s| {
        state
            .db
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .session_events(&s.id, 20)
            .ok()
    });
    let boot = s.as_ref().and_then(|s| {
        device_api::running(state)
            .into_iter()
            .find(|r| r.avd == s.avd)
    });
    ok(
        id,
        &serde_json::json!({
            "session": s,
            "boot": boot,
            "events": events,
            "processes": state.supervisor.list(),
            "ports": state.ports.leases(),
        }),
    )
}

pub fn stop(state: &ApiState, id: Id) -> Response {
    let Some(mut s) = current(state) else {
        return ok(id, &serde_json::json!({ "session": null }));
    };
    if s.state.is_active() {
        crate::run_api::stop_run(state);
        device_api::stop_avd(state, &s.avd);
        if let Err(e) = apply(state, &mut s, Event::Stop, Some("stopped by the user")) {
            return Response::err(Some(id), e);
        }
    }
    ok(id, &serde_json::json!({ "session": s }))
}

/// Core start: settles sessions the previous Core left active; keeps the newest as current.
pub fn recover(state: &ApiState) {
    let recovered = {
        let mut db = state.db.lock().unwrap_or_else(PoisonError::into_inner);
        umr_session::recover_after_restart(&mut db)
    };
    match recovered {
        Ok(list) if !list.is_empty() => {
            for s in &list {
                tracing::warn!(session = %s.id, avd = %s.avd, "session lost its device with the previous Core; `session.start` recovers it");
            }
            publish(state, list.into_iter().next());
        }
        Ok(_) => {}
        Err(e) => tracing::error!(error = %e, "session recovery failed"),
    }
}

/// Core stop: the active session ends with it.
pub fn settle_on_shutdown(state: &ApiState) {
    crate::run_api::stop_run(state);
    if let Some(mut s) = current(state).filter(|s| s.state.is_active()) {
        let _ = apply(state, &mut s, Event::Stop, Some("the Runtime Core stopped"));
    }
}

/// `app.launch` on the session's device: a crashed / failed session runs again.
pub(crate) fn app_launched(state: &ApiState, serial: &str, package: &str, detail: &str) {
    let Some(mut s) = current(state) else { return };
    if s.serial.as_deref() != Some(serial) {
        return;
    }
    let other_app = s.app.as_deref() != Some(package);
    let relaunch = matches!(s.state, State::Crashed | State::LaunchFailed)
        || (s.state == State::Running && other_app);
    if relaunch {
        s.app = Some(package.to_string());
        let _ = apply(state, &mut s, Event::Restart, Some(package));
        let _ = apply(state, &mut s, Event::AppForeground, Some(detail));
    }
}

/// `app.stop` of the session's app: recorded as stopped on purpose, not as a crash.
pub(crate) fn app_stopped(state: &ApiState, serial: &str, package: &str) {
    let Some(mut s) = current(state) else { return };
    if s.serial.as_deref() == Some(serial)
        && s.app.as_deref() == Some(package)
        && s.state == State::Running
    {
        let _ = apply(
            state,
            &mut s,
            Event::ProcessDied,
            Some(&format!("{package} stopped with app.stop")),
        );
    }
}
