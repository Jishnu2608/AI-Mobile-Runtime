//! Core side of the installer API (INS-001…004): `install.prepare`, `install.licence.accept`,
//! `install.start`, `install.status`, `install.cancel`.
//!
//! One install job at a time runs on its own thread; its status is kept in [`ApiState`].
//! Licences can only be accepted for a text this Core showed in `install.prepare`, and the
//! engine re-checks acceptance before the first step (defence in depth).

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use umr_api::methods::{
    AcceptLicenceParams, AcceptLicenceResult, CancelResult, InstallStarted, JobParams,
    ProjectPlanParams,
};
use umr_api::rpc::{codes, Id, Response, RpcError};
use umr_installer::engine::{self, Endpoints, Job, Prepared, RunContext};
use umr_installer::http::{Http, Policy};
use umr_installer::layout::Layout;
use umr_installer::InstallError;

use crate::api_server::{build_plan, internal, ok, ApiState};

/// Android CLI gets this long per SDK package (the system image took 830 s in POC-001).
const SDK_PACKAGE_TIMEOUT: Duration = Duration::from_secs(2 * 60 * 60);

fn err(id: Id, code: i64, msg: impl Into<String>) -> Response {
    Response::err(Some(id), RpcError::new(code, msg))
}

fn install_error(id: Id, e: &InstallError) -> Response {
    let code = match e {
        InstallError::LicenceNotAccepted(_) => codes::LICENCE_REQUIRED,
        InstallError::Disk(_) => codes::DISK_SPACE,
        InstallError::Network(_)
        | InstallError::Http { .. }
        | InstallError::Metadata(_)
        | InstallError::HostNotAllowed(_) => codes::UNAVAILABLE,
        _ => codes::INTERNAL_ERROR,
    };
    err(id, code, e.to_string())
}

/// Plan + prepared steps for a project.
fn plan_and_prepare(
    state: &ApiState,
    p: &ProjectPlanParams,
) -> Result<(umr_resolver::plan::Plan, Prepared), Result<RpcError, InstallError>> {
    let (_, plan) = build_plan(state, p).map_err(Ok)?;
    let http = Http::new(Policy::official()).map_err(Err)?;
    let layout = Layout::new(&state.toolchains);
    let prepared =
        engine::prepare(&http, &Endpoints::default(), &layout, &plan, &state.db).map_err(Err)?;
    let mut offered = state
        .offered_licences
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for l in &prepared.licences {
        offered.insert(l.id.clone(), l.sha256.clone());
    }
    Ok((plan, prepared))
}

pub fn prepare(state: &ApiState, id: Id, p: &ProjectPlanParams) -> Response {
    match plan_and_prepare(state, p) {
        Ok((plan, prepared)) => ok(
            id,
            &serde_json::json!({ "plan": plan, "prepared": prepared }),
        ),
        Err(Ok(rpc)) => Response::err(Some(id), rpc),
        Err(Err(e)) => install_error(id, &e),
    }
}

pub fn accept_licence(
    state: &ApiState,
    id: Id,
    client: Option<&str>,
    p: &AcceptLicenceParams,
) -> Response {
    let shown = state
        .offered_licences
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&p.id)
        .is_some_and(|sha| sha.eq_ignore_ascii_case(&p.sha256));
    if !shown {
        return err(id, codes::INVALID_PARAMS, format!("licence '{}' with this text was not shown by install.prepare; show the full text first", p.id));
    }
    let db = state
        .db
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let now = umr_installer::now_unix();
    if let Err(e) = db.accept_licence(&p.id, &p.sha256.to_lowercase(), client, now) {
        return internal(id, &e);
    }
    let _ = db.audit(
        now,
        client,
        "licence.accept",
        Some(&format!("{} sha256={}", p.id, p.sha256)),
    );
    tracing::info!(licence = %p.id, client = ?client, "licence accepted by the user");
    ok(id, &AcceptLicenceResult { accepted: true })
}

pub fn start(state: &ApiState, id: Id, p: &ProjectPlanParams) -> Response {
    if state
        .install_job
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .is_some_and(|j| j.is_running())
    {
        return err(
            id,
            codes::BUSY,
            "another install is running; see install.status",
        );
    }
    let (plan, prepared) = match plan_and_prepare(state, p) {
        Ok(v) => v,
        Err(Ok(rpc)) => return Response::err(Some(id), rpc),
        Err(Err(e)) => return install_error(id, &e),
    };
    if !plan.disk_ok {
        return install_error(
            id,
            &InstallError::Disk(format!(
                "not enough free disk space: about {} MB needed, {} MB free",
                plan.total_disk_bytes / 1_048_576,
                plan.disk_free_bytes / 1_048_576
            )),
        );
    }
    let missing: Vec<String> = prepared.unaccepted().iter().map(|l| l.id.clone()).collect();
    if !missing.is_empty() {
        return install_error(id, &InstallError::LicenceNotAccepted(missing));
    }

    let job_id = umr_installer::random_id();
    let layout = Layout::new(&state.toolchains);
    let log = layout.logs().join(format!("install-{job_id}.log"));
    let job = Arc::new(Job::new(&job_id, &prepared, &log));
    {
        let mut slot = state
            .install_job
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if slot.as_ref().is_some_and(|j| j.is_running()) {
            return err(
                id,
                codes::BUSY,
                "another install is running; see install.status",
            );
        }
        *slot = Some(job.clone());
    }

    // Isolated environment for Android CLI (ENV-008/009): managed SDK, Android user home,
    // temp and JVM socket folders under the managed root.
    let ctx = umr_env::ScanContext::from_system(state.toolchains.clone());
    let dirs = umr_env::recipe::IsolationDirs::under(&state.toolchains);
    if let Err(e) = dirs.create() {
        return internal(id, &e);
    }
    let sel = umr_env::recipe::Selection {
        adb_port: Some(state.adb_port),
        android_sdk: Some(layout.android_sdk()),
        ..Default::default()
    };
    let android_env = umr_env::recipe::child_env(&ctx, &sel);

    let steps = prepared.steps.len();
    tracing::info!(job = %job_id, steps, "install job started");
    {
        let db = state
            .db
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = db.audit(
            umr_installer::now_unix(),
            None,
            "install.start",
            Some(&format!("job {job_id}: {} ({steps} steps)", p.path)),
        );
    }
    let (db, scan_stale) = (state.db.clone(), state.scan_stale.clone());
    std::thread::Builder::new()
        .name(format!("install-{job_id}"))
        .spawn(move || {
            let http = match Http::new(Policy::official()) {
                Ok(h) => h,
                Err(e) => {
                    tracing::error!(error = %e, "install job could not start");
                    return;
                }
            };
            let run_ctx = RunContext { http: &http, layout: &layout, db: &db, android_env, sdk_timeout: SDK_PACKAGE_TIMEOUT };
            engine::run(&job, &prepared, &run_ctx);
            scan_stale.store(true, Ordering::Relaxed);
            let st = job.status();
            tracing::info!(job = %st.id, state = ?st.state, message = ?st.message, "install job finished");
        })
        .map_or_else(|e| internal(id.clone(), &e), |_| ok(id.clone(), &InstallStarted { job_id: job_id.clone(), steps }))
}

pub fn status(state: &ApiState, id: Id, p: &JobParams) -> Response {
    let slot = state
        .install_job
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match slot.as_ref() {
        Some(j) if p.job_id.as_ref().is_none_or(|want| *want == j.status().id) => {
            ok(id, &j.status())
        }
        _ => err(id, codes::INVALID_PARAMS, "no such install job"),
    }
}

pub fn cancel(state: &ApiState, id: Id, p: &JobParams) -> Response {
    let slot = state
        .install_job
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match slot.as_ref() {
        Some(j) if p.job_id.as_ref().is_none_or(|want| *want == j.status().id) => {
            let running = j.is_running();
            j.cancel();
            ok(
                id,
                &CancelResult {
                    cancelling: running,
                },
            )
        }
        _ => err(id, codes::INVALID_PARAMS, "no such install job"),
    }
}
