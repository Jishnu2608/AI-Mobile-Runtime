//! The Core's local API server: JSON-RPC 2.0 over a user-only named pipe.
//!
//! One task accepts connections; each connection gets its own task and may send any
//! number of requests (one per line). The server stops accepting when `stop` fires.
//!
//! Every connection starts unauthenticated. `core.ping` and `auth.login` are always allowed;
//! every other method needs a scope (see [`required_scope`]).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::io::BufReader;
use tokio::net::windows::named_pipe::NamedPipeServer;
use tokio::sync::{watch, Notify};

use umr_api::frame::{read_frame, write_frame, ReadFrame};
use umr_api::methods::{
    self, AcceptLicenceParams, AdminRunParams, ClientInfo, CoreInfo, CreateClientParams,
    CreateClientResult, JobParams, LoginParams, LoginResult, Pong, ProjectPathParams,
    ProjectPlanParams, RemoveComponentParams, RevokeClientParams, RevokeClientResult,
    ShutdownAccepted, TrustParams,
};
use umr_api::rpc::{codes, parse_request, Id, Request, Response, RpcError};
use umr_api::{API_VERSION, MAX_MESSAGE_BYTES};

use crate::auth::{AuthStore, Principal, Scope};

/// Shared state visible to request handlers.
#[derive(Debug)]
pub struct ApiState {
    pub started: Instant,
    /// Home folder: config, database, logs, tokens.
    pub data_root: PathBuf,
    /// Toolchains folder (`[storage] toolchains_dir`); the home unless configured.
    pub toolchains: PathBuf,
    pub auth: Mutex<AuthStore>,
    pub db: Arc<Mutex<umr_storage::Db>>,
    /// Fired by `core.shutdown`; the daemon waits on it.
    pub shutdown_requested: Notify,
    /// The latest install job (INS-003).
    pub install_job: Mutex<Option<Arc<umr_installer::engine::Job>>>,
    /// Licence texts this Core has shown (id → SHA-256); only these can be accepted.
    pub offered_licences: Mutex<std::collections::HashMap<String, String>>,
    /// Set when an install changed the machine; the next plan rescans.
    pub scan_stale: Arc<std::sync::atomic::AtomicBool>,
}

impl ApiState {
    #[must_use]
    pub fn new(data_root: PathBuf, auth: AuthStore, db: umr_storage::Db) -> Self {
        Self::with_toolchains(data_root.clone(), data_root, auth, db)
    }

    /// Like [`Self::new`] with a separate toolchains folder.
    #[must_use]
    pub fn with_toolchains(
        data_root: PathBuf,
        toolchains: PathBuf,
        auth: AuthStore,
        db: umr_storage::Db,
    ) -> Self {
        let mut s = Self::bare(data_root, auth, db);
        s.toolchains = toolchains;
        s
    }

    fn bare(data_root: PathBuf, auth: AuthStore, db: umr_storage::Db) -> Self {
        Self {
            started: Instant::now(),
            toolchains: data_root.clone(),
            data_root,
            auth: Mutex::new(auth),
            db: Arc::new(Mutex::new(db)),
            shutdown_requested: Notify::new(),
            install_job: Mutex::new(None),
            offered_licences: Mutex::new(std::collections::HashMap::new()),
            scan_stale: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
}

/// Per-connection state.
#[derive(Debug, Default)]
pub struct Connection {
    pub principal: Option<Principal>,
}

/// Scope needed for a method; `None` = allowed without login.
///
/// Unknown methods also require a login (observe), so probing without a token reveals
/// nothing; after login they are rejected as "method not found".
#[must_use]
pub fn required_scope(method: &str) -> Option<Scope> {
    match method {
        methods::PING | methods::AUTH_LOGIN => None,
        methods::SHUTDOWN
        | methods::PROJECT_TRUST
        | methods::COMPONENTS_REMOVE
        | methods::COMPONENTS_REPAIR
        | methods::COMPONENTS_CLEAN
        | methods::ADMIN_RUN
        | methods::INSTALL_PREPARE
        | methods::INSTALL_ACCEPT_LICENCE
        | methods::INSTALL_START
        | methods::INSTALL_CANCEL
        | methods::AUTH_CLIENT_CREATE
        | methods::AUTH_CLIENT_LIST
        | methods::AUTH_CLIENT_REVOKE => Some(Scope::Admin),
        // core.info, core.doctor and unknown methods
        _ => Some(Scope::Observe),
    }
}

/// Checks login and scope for a request; Some(error response) when not allowed.
#[must_use]
pub fn authorize(conn: &Connection, req: &Request) -> Option<Response> {
    let id = req.id.clone();
    if let Some(scope) = required_scope(&req.method) {
        match &conn.principal {
            None => {
                return Some(Response::err(
                    Some(id),
                    RpcError::new(
                        codes::UNAUTHENTICATED,
                        "call auth.login with a valid token first",
                    ),
                ));
            }
            Some(p) if !p.allows(scope) => {
                tracing::warn!(client = %p.client_id, method = %req.method, ?scope, "API call forbidden");
                return Some(Response::err(
                    Some(id),
                    RpcError::new(
                        codes::FORBIDDEN,
                        format!(
                            "client '{}' lacks scope '{scope:?}' for {}",
                            p.name, req.method
                        ),
                    ),
                ));
            }
            Some(_) => {}
        }
    }
    None
}

/// Methods that run tools or use the network: handled on a blocking thread.
fn is_slow(method: &str) -> bool {
    matches!(
        method,
        methods::ENV_SCAN
            | methods::PROJECT_PLAN
            | methods::INSTALL_PREPARE
            | methods::INSTALL_START
    )
}

/// Handles one request on a connection (blocking; env.scan takes seconds — the server runs it
/// on a blocking thread).
#[allow(clippy::too_many_lines)] // one flat match over all API methods
pub fn dispatch(state: &ApiState, conn: &mut Connection, req: &Request) -> Response {
    if let Some(denied) = authorize(conn, req) {
        return denied;
    }
    let id = req.id.clone();
    match req.method.as_str() {
        methods::ENV_SCAN => env_scan(state, id),
        methods::PROJECT_PLAN => with_params(id, &req.params, |id, p: ProjectPlanParams| {
            project_plan(state, id, &p)
        }),
        methods::PROJECT_DETECT => with_params(id, &req.params, |id, p: ProjectPathParams| {
            crate::project_api::detect(state, id, &p)
        }),
        methods::PROJECT_TRUST => {
            let client = conn.principal.as_ref().map(|p| p.client_id.clone());
            with_params(id, &req.params, |id, p: TrustParams| {
                crate::project_api::trust(state, id, client.as_deref(), &p)
            })
        }
        methods::COMPONENTS_LIST => crate::manage_api::list(state, id),
        methods::COMPONENTS_REMOVE => {
            with_params(id, &req.params, |id, p: RemoveComponentParams| {
                crate::manage_api::remove(state, id, &p)
            })
        }
        methods::COMPONENTS_REPAIR => crate::manage_api::repair(state, id),
        methods::COMPONENTS_CLEAN => crate::manage_api::clean(state, id),
        methods::ADMIN_RUN => {
            let client = conn.principal.as_ref().map(|p| p.client_id.clone());
            with_params(id, &req.params, |id, p: AdminRunParams| {
                crate::manage_api::admin_run(state, id, client.as_deref(), &p)
            })
        }
        methods::INSTALL_PREPARE => with_params(id, &req.params, |id, p: ProjectPlanParams| {
            crate::install_api::prepare(state, id, &p)
        }),
        methods::INSTALL_ACCEPT_LICENCE => {
            let client = conn.principal.as_ref().map(|p| p.client_id.clone());
            with_params(id, &req.params, |id, p: AcceptLicenceParams| {
                crate::install_api::accept_licence(state, id, client.as_deref(), &p)
            })
        }
        methods::INSTALL_START => with_params(id, &req.params, |id, p: ProjectPlanParams| {
            crate::install_api::start(state, id, &p)
        }),
        methods::INSTALL_STATUS => {
            let params = if req.params.is_null() {
                serde_json::json!({})
            } else {
                req.params.clone()
            };
            with_params(id, &params, |id, p: JobParams| {
                crate::install_api::status(state, id, &p)
            })
        }
        methods::INSTALL_CANCEL => with_params(id, &req.params, |id, p: JobParams| {
            crate::install_api::cancel(state, id, &p)
        }),
        methods::PING => ok(id, &Pong { pong: true }),
        methods::AUTH_LOGIN => login(state, conn, id, &req.params),
        methods::INFO => ok(
            id,
            &CoreInfo {
                version: crate::VERSION.to_string(),
                api_version: API_VERSION,
                pid: std::process::id(),
                uptime_s: state.started.elapsed().as_secs(),
                data_root: state.data_root.display().to_string(),
                toolchains_root: state.toolchains.display().to_string(),
            },
        ),
        methods::DOCTOR => {
            let db = state
                .db
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            ok(
                id,
                &crate::doctor::run_checks(&state.data_root, &state.toolchains, Some(&db)),
            )
        }
        methods::SHUTDOWN => {
            tracing::info!(client = ?conn.principal.as_ref().map(|p| &p.client_id), "shutdown requested through the API");
            state.shutdown_requested.notify_one();
            ok(id, &ShutdownAccepted { stopping: true })
        }
        methods::AUTH_CLIENT_CREATE => with_params(id, &req.params, |id, p: CreateClientParams| {
            create_client(state, id, &p)
        }),
        methods::AUTH_CLIENT_LIST => {
            let list: Vec<ClientInfo> = lock(state)
                .list_clients()
                .into_iter()
                .map(|c| ClientInfo {
                    id: c.id,
                    name: c.name,
                    scopes: scope_names(&c.scopes),
                    created_unix: c.created_unix,
                })
                .collect();
            ok(id, &list)
        }
        methods::AUTH_CLIENT_REVOKE => with_params(id, &req.params, |id, p: RevokeClientParams| {
            match lock(state).revoke_client(&p.id) {
                Ok(revoked) => {
                    tracing::info!(client = %p.id, revoked, "client revoked");
                    ok(id, &RevokeClientResult { revoked })
                }
                Err(e) => internal(id, &e),
            }
        }),
        other => Response::err(
            Some(id),
            RpcError::new(codes::METHOD_NOT_FOUND, format!("unknown method '{other}'")),
        ),
    }
}

fn env_scan(state: &ApiState, id: Id) -> Response {
    match scan_and_store(state) {
        Ok(report) => ok(id, &report),
        Err(e) => internal(id, &e),
    }
}

pub(crate) fn scan_and_store(state: &ApiState) -> Result<umr_env::EnvReport, serde_json::Error> {
    let report = umr_env::scan_system(state.toolchains.clone());
    tracing::info!(
        duration_ms = report.duration_ms,
        jdks = report.jdks.len(),
        sdks = report.android_sdks.len(),
        tools = report.tools.len(),
        conflicts = report.conflicts.len(),
        "environment scan finished"
    );
    let json = serde_json::to_string(&report)?;
    let db = state
        .db
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    #[allow(clippy::cast_possible_wrap)]
    if let Err(e) = db.record_env_scan(report.scanned_unix as i64, &json) {
        tracing::warn!(error = %e, "cannot store environment scan");
    }
    drop(db);
    Ok(report)
}

/// RES-003…005: read the project, resolve versions, build the install plan (read-only).
/// Uses the stored scan unless `rescan` is set, no scan exists, or an install changed the
/// machine since.
pub(crate) fn build_plan(
    state: &ApiState,
    p: &ProjectPlanParams,
) -> Result<
    (
        umr_resolver::project::ProjectFacts,
        umr_resolver::plan::Plan,
    ),
    RpcError,
> {
    let root = std::path::Path::new(&p.path);
    if !root.is_absolute() || !root.is_dir() {
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            format!("'{}' is not an absolute path to a folder", p.path),
        ));
    }
    let pack = umr_resolver::pack::Pack::bundled()
        .map_err(|e| RpcError::new(codes::INTERNAL_ERROR, e.to_string()))?;
    let needs_rescan = state
        .scan_stale
        .swap(false, std::sync::atomic::Ordering::Relaxed);
    let stored = if p.rescan || needs_rescan {
        None
    } else {
        let db = state
            .db
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        db.latest_env_scan()
            .ok()
            .flatten()
            .and_then(|(_, json)| serde_json::from_str::<umr_env::EnvReport>(&json).ok())
    };
    let env = match stored {
        Some(env) => env,
        None => scan_and_store(state)
            .map_err(|e| RpcError::new(codes::INTERNAL_ERROR, e.to_string()))?,
    };
    let facts = umr_resolver::project::read(root);
    let resolution = umr_resolver::resolve::resolve(&facts, &pack);
    let managed_sdk = state.toolchains.join("android-sdk").display().to_string();
    let plan = umr_resolver::plan::build(&p.path, &resolution, &env, &pack, &managed_sdk);
    tracing::info!(
        framework = ?facts.framework,
        items = plan.items.len(),
        conflicts = plan.conflicts.len(),
        "project plan built"
    );
    Ok((facts, plan))
}

fn project_plan(state: &ApiState, id: Id, p: &ProjectPlanParams) -> Response {
    match build_plan(state, p) {
        Ok((facts, plan)) => ok(id, &serde_json::json!({ "project": facts, "plan": plan })),
        Err(e) => Response::err(Some(id), e),
    }
}

fn login(state: &ApiState, conn: &mut Connection, id: Id, params: &Value) -> Response {
    with_params(id, params, |id, p: LoginParams| {
        let Some(principal) = lock(state).verify(&p.token) else {
            tracing::warn!("API login with an invalid token");
            conn.principal = None;
            return Response::err(
                Some(id),
                RpcError::new(codes::UNAUTHENTICATED, "invalid token"),
            );
        };
        tracing::info!(client = %principal.client_id, "API client logged in");
        let result = LoginResult {
            client_id: principal.client_id.clone(),
            name: principal.name.clone(),
            scopes: scope_names(&principal.scopes),
        };
        conn.principal = Some(principal);
        ok(id, &result)
    })
}

fn create_client(state: &ApiState, id: Id, p: &CreateClientParams) -> Response {
    let mut scopes = Vec::new();
    for s in &p.scopes {
        match Scope::parse(s) {
            Some(scope) => scopes.push(scope),
            None => {
                return Response::err(
                    Some(id),
                    RpcError::new(codes::INVALID_PARAMS, format!("unknown scope '{s}'")),
                )
            }
        }
    }
    if p.name.trim().is_empty() || scopes.is_empty() {
        return Response::err(
            Some(id),
            RpcError::new(
                codes::INVALID_PARAMS,
                "name and at least one scope are required",
            ),
        );
    }
    match lock(state).create_client(p.name.trim(), &scopes) {
        Ok((client_id, token)) => {
            tracing::info!(client = %client_id, name = %p.name, ?scopes, "client created");
            ok(
                id,
                &CreateClientResult {
                    id: client_id,
                    token,
                    scopes: scope_names(&scopes),
                },
            )
        }
        Err(e) => internal(id, &e),
    }
}

fn scope_names(scopes: &[Scope]) -> Vec<String> {
    scopes
        .iter()
        .map(|s| match s {
            Scope::Observe => methods::scopes::OBSERVE,
            Scope::Interact => methods::scopes::INTERACT,
            Scope::Control => methods::scopes::CONTROL,
            Scope::Admin => methods::scopes::ADMIN,
        })
        .map(String::from)
        .collect()
}

fn lock(state: &ApiState) -> std::sync::MutexGuard<'_, AuthStore> {
    state
        .auth
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn with_params<P: DeserializeOwned>(
    id: Id,
    params: &Value,
    f: impl FnOnce(Id, P) -> Response,
) -> Response {
    match serde_json::from_value::<P>(params.clone()) {
        Ok(p) => f(id, p),
        Err(e) => Response::err(
            Some(id),
            RpcError::new(codes::INVALID_PARAMS, format!("invalid params: {e}")),
        ),
    }
}

pub(crate) fn internal(id: Id, e: &dyn std::fmt::Display) -> Response {
    Response::err(
        Some(id),
        RpcError::new(codes::INTERNAL_ERROR, e.to_string()),
    )
}

pub(crate) fn ok(id: Id, value: &impl serde::Serialize) -> Response {
    match serde_json::to_value(value) {
        Ok(v) => Response::ok(id, v),
        Err(e) => internal(id, &e),
    }
}

fn encode(resp: &Response) -> Vec<u8> {
    serde_json::to_vec(resp).unwrap_or_else(|_| {
        br#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"encoding failed"}}"#
            .to_vec()
    })
}

async fn serve_connection(pipe: NamedPipeServer, state: Arc<ApiState>) {
    let (r, mut w) = tokio::io::split(pipe);
    let mut reader = BufReader::new(r);
    let mut conn = Connection::default();
    loop {
        let frame = match read_frame(&mut reader, MAX_MESSAGE_BYTES).await {
            Ok(f) => f,
            Err(e) => {
                tracing::debug!(error = %e, "API connection read error");
                return;
            }
        };
        let response = match frame {
            ReadFrame::Closed => return,
            ReadFrame::TooLarge => {
                let resp = Response::err(
                    None,
                    RpcError::new(
                        codes::MESSAGE_TOO_LARGE,
                        format!("message exceeds {MAX_MESSAGE_BYTES} bytes"),
                    ),
                );
                let _ = write_frame(&mut w, &encode(&resp)).await;
                tracing::warn!("API message too large; connection closed");
                return;
            }
            ReadFrame::Message(m) => match parse_request(&m) {
                Ok(req) if is_slow(&req.method) => {
                    // Slow (runs tools): keep it off the async worker threads.
                    let st = state.clone();
                    let mut c = Connection {
                        principal: conn.principal.clone(),
                    };
                    tokio::task::spawn_blocking(move || dispatch(&st, &mut c, &req))
                        .await
                        .unwrap_or_else(|e| {
                            Response::err(None, RpcError::new(codes::INTERNAL_ERROR, e.to_string()))
                        })
                }
                Ok(req) => {
                    tracing::debug!(method = %req.method, "API request");
                    dispatch(&state, &mut conn, &req)
                }
                Err(err_resp) => *err_resp,
            },
        };
        if write_frame(&mut w, &encode(&response)).await.is_err() {
            return;
        }
    }
}

/// Accepts connections until `stop` changes to `true`.
///
/// The first pipe instance is created by the caller (so creation errors surface before the
/// Core reports "started"); further instances are created here as clients connect.
pub async fn serve(
    pipe_name: String,
    first: NamedPipeServer,
    state: Arc<ApiState>,
    mut stop: watch::Receiver<bool>,
) {
    let mut server = first;
    loop {
        tokio::select! {
            _ = stop.changed() => break,
            res = server.connect() => {
                if let Err(e) = res {
                    tracing::warn!(error = %e, "API accept failed");
                    continue;
                }
                let next = match umr_platform::secure_pipe::create_user_only_server(&pipe_name, false) {
                    Ok(n) => n,
                    Err(e) => {
                        tracing::error!(error = %e, "cannot create next API pipe instance; API stopped");
                        tokio::spawn(serve_connection(server, state.clone()));
                        break;
                    }
                };
                let connected = std::mem::replace(&mut server, next);
                tokio::spawn(serve_connection(connected, state.clone()));
            }
        }
    }
    tracing::info!("API server stopped");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempRoot;
    use serde_json::json;

    #[test]
    fn install_methods_need_admin_except_status() {
        for m in [
            methods::COMPONENTS_REMOVE,
            methods::COMPONENTS_REPAIR,
            methods::COMPONENTS_CLEAN,
            methods::ADMIN_RUN,
            methods::INSTALL_PREPARE,
            methods::INSTALL_ACCEPT_LICENCE,
            methods::INSTALL_START,
            methods::INSTALL_CANCEL,
        ] {
            assert_eq!(required_scope(m), Some(Scope::Admin), "{m}");
        }
        assert_eq!(
            required_scope(methods::INSTALL_STATUS),
            Some(Scope::Observe)
        );
        assert!(is_slow(methods::INSTALL_PREPARE) && is_slow(methods::INSTALL_START));
        assert!(!is_slow(methods::INSTALL_STATUS));
    }

    fn state(tmp: &TempRoot) -> ApiState {
        let auth = AuthStore::load(&tmp.path().join("clients.json"), "umr_admin").unwrap();
        let db = umr_storage::Db::open_in(tmp.path()).unwrap();
        ApiState::new(tmp.path().to_path_buf(), auth, db)
    }

    fn call(
        s: &ApiState,
        c: &mut Connection,
        method: &str,
        params: Value,
    ) -> Result<Value, RpcError> {
        dispatch(s, c, &Request::new(1, method, params)).into_result()
    }

    #[test]
    fn only_ping_and_login_work_without_login() {
        let tmp = TempRoot::new("api-unauth");
        let s = state(&tmp);
        let mut c = Connection::default();
        assert_eq!(
            call(&s, &mut c, methods::PING, Value::Null).unwrap(),
            json!({"pong": true})
        );
        for m in [
            methods::INFO,
            methods::DOCTOR,
            methods::SHUTDOWN,
            methods::AUTH_CLIENT_LIST,
            "no.such",
        ] {
            assert_eq!(
                call(&s, &mut c, m, Value::Null).unwrap_err().code,
                codes::UNAUTHENTICATED,
                "{m}"
            );
        }
    }

    #[test]
    fn wrong_token_is_rejected_and_admin_token_unlocks_everything() {
        let tmp = TempRoot::new("api-login");
        let s = state(&tmp);
        let mut c = Connection::default();
        let e = call(&s, &mut c, methods::AUTH_LOGIN, json!({"token": "umr_bad"})).unwrap_err();
        assert_eq!(e.code, codes::UNAUTHENTICATED);
        let r: LoginResult = serde_json::from_value(
            call(
                &s,
                &mut c,
                methods::AUTH_LOGIN,
                json!({"token": "umr_admin"}),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(r.scopes.contains(&"admin".to_string()));
        let info: CoreInfo =
            serde_json::from_value(call(&s, &mut c, methods::INFO, Value::Null).unwrap()).unwrap();
        assert_eq!(info.api_version, API_VERSION);
        assert_eq!(
            call(&s, &mut c, "no.such", Value::Null).unwrap_err().code,
            codes::METHOD_NOT_FOUND
        );
    }

    #[test]
    fn scoped_client_is_limited() {
        let tmp = TempRoot::new("api-scopes");
        let s = state(&tmp);
        let mut admin = Connection::default();
        call(
            &s,
            &mut admin,
            methods::AUTH_LOGIN,
            json!({"token": "umr_admin"}),
        )
        .unwrap();
        let created: CreateClientResult = serde_json::from_value(
            call(
                &s,
                &mut admin,
                methods::AUTH_CLIENT_CREATE,
                json!({"name": "agent", "scopes": ["observe"]}),
            )
            .unwrap(),
        )
        .unwrap();

        let mut agent = Connection::default();
        call(
            &s,
            &mut agent,
            methods::AUTH_LOGIN,
            json!({"token": created.token}),
        )
        .unwrap();
        assert!(call(&s, &mut agent, methods::INFO, Value::Null).is_ok());
        assert_eq!(
            call(&s, &mut agent, methods::SHUTDOWN, Value::Null)
                .unwrap_err()
                .code,
            codes::FORBIDDEN
        );
        assert_eq!(
            call(&s, &mut agent, methods::AUTH_CLIENT_LIST, Value::Null)
                .unwrap_err()
                .code,
            codes::FORBIDDEN
        );

        let list: Vec<ClientInfo> = serde_json::from_value(
            call(&s, &mut admin, methods::AUTH_CLIENT_LIST, Value::Null).unwrap(),
        )
        .unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].scopes, vec!["observe"]);

        call(
            &s,
            &mut admin,
            methods::AUTH_CLIENT_REVOKE,
            json!({"id": created.id}),
        )
        .unwrap();
        let mut again = Connection::default();
        assert_eq!(
            call(
                &s,
                &mut again,
                methods::AUTH_LOGIN,
                json!({"token": created.token})
            )
            .unwrap_err()
            .code,
            codes::UNAUTHENTICATED
        );
    }

    #[test]
    fn create_client_validates_input() {
        let tmp = TempRoot::new("api-validate");
        let s = state(&tmp);
        let mut admin = Connection::default();
        call(
            &s,
            &mut admin,
            methods::AUTH_LOGIN,
            json!({"token": "umr_admin"}),
        )
        .unwrap();
        for params in [
            json!({"name": "x", "scopes": ["root"]}),
            json!({"name": " ", "scopes": ["observe"]}),
            json!({"name": "x", "scopes": []}),
            json!({"oops": 1}),
        ] {
            assert_eq!(
                call(&s, &mut admin, methods::AUTH_CLIENT_CREATE, params.clone())
                    .unwrap_err()
                    .code,
                codes::INVALID_PARAMS,
                "{params}"
            );
        }
    }
}
