//! Method names, parameters and results of the Core API.
//!
//! Authentication (CORE-004): a connection starts unauthenticated. Only [`PING`] and
//! [`AUTH_LOGIN`] are allowed before a successful `auth.login`. Every other method needs a
//! scope: `observe`, `interact`, `control` or `admin` (admin grants all).

use serde::{Deserialize, Serialize};

// ---- core ---------------------------------------------------------------------------------

/// `core.ping` → [`Pong`]. Liveness check. No authentication needed.
pub const PING: &str = "core.ping";
/// `core.info` → [`CoreInfo`]. Scope: observe.
pub const INFO: &str = "core.info";
/// `core.doctor` → [`DoctorReport`]. Basic health checks. Scope: observe.
pub const DOCTOR: &str = "core.doctor";
/// `env.scan` → environment report (JSON object, see `umr_env::EnvReport`). Scope: observe.
/// Takes a few seconds: runs read-only tool checks. The result is also stored by the Core.
pub const ENV_SCAN: &str = "env.scan";
/// `project.plan` [`ProjectPlanParams`] → [`ProjectPlan`]-shaped JSON
/// (`{ "project": umr_resolver::project::ProjectFacts, "plan": umr_resolver::plan::Plan }`).
/// Scope: observe. Reads the project (never changes it) and uses the latest stored environment
/// scan, scanning first if there is none or `rescan` is set. Installs nothing.
pub const PROJECT_PLAN: &str = "project.plan";
// ---- install (INS-001…004) -----------------------------------------------------------------
// Installing changes the machine, so all install calls except status need the admin scope
// (agents never get it in the MVP). Licences are accepted only through `install.licence.accept`
// for a text this Core has shown; `install.start` refuses while any is missing.

/// `install.prepare` [`ProjectPlanParams`] → `{ "plan": Plan, "prepared": umr_installer::engine::Prepared }`.
/// Reads official release metadata (network) and returns the exact downloads and the full
/// licence texts that need acceptance. Downloads nothing large, installs nothing. Scope: admin.
pub const INSTALL_PREPARE: &str = "install.prepare";
/// `install.licence.accept` [`AcceptLicenceParams`] → [`AcceptLicenceResult`]. Scope: admin.
pub const INSTALL_ACCEPT_LICENCE: &str = "install.licence.accept";
/// `install.start` [`ProjectPlanParams`] → [`InstallStarted`]. Starts a background job. Scope: admin.
pub const INSTALL_START: &str = "install.start";
/// `install.status` [`JobParams`] → `umr_installer::engine::JobStatus`. Scope: observe.
pub const INSTALL_STATUS: &str = "install.status";
/// `install.cancel` [`JobParams`] → [`CancelResult`]. Downloads are kept for resume. Scope: admin.
pub const INSTALL_CANCEL: &str = "install.cancel";

/// `core.shutdown` → [`ShutdownAccepted`]. Scope: admin.
pub const SHUTDOWN: &str = "core.shutdown";

// ---- auth ---------------------------------------------------------------------------------

/// `auth.login` [`LoginParams`] → [`LoginResult`]. No authentication needed.
pub const AUTH_LOGIN: &str = "auth.login";
/// `auth.client.create` [`CreateClientParams`] → [`CreateClientResult`]. Scope: admin.
pub const AUTH_CLIENT_CREATE: &str = "auth.client.create";
/// `auth.client.list` → `Vec<ClientInfo>`. Scope: admin.
pub const AUTH_CLIENT_LIST: &str = "auth.client.list";
/// `auth.client.revoke` [`RevokeClientParams`] → [`RevokeClientResult`]. Scope: admin.
pub const AUTH_CLIENT_REVOKE: &str = "auth.client.revoke";

/// Scope names used on the wire.
pub mod scopes {
    pub const OBSERVE: &str = "observe";
    pub const INTERACT: &str = "interact";
    pub const CONTROL: &str = "control";
    pub const ADMIN: &str = "admin";
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pong {
    pub pong: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreInfo {
    pub version: String,
    pub api_version: u32,
    pub pid: u32,
    pub uptime_s: u64,
    pub data_root: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShutdownAccepted {
    pub stopping: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectPlanParams {
    /// Absolute path of the project folder.
    pub path: String,
    /// Run a fresh environment scan instead of using the stored one.
    #[serde(default)]
    pub rescan: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptLicenceParams {
    /// Licence id, e.g. `android-sdk-license`.
    pub id: String,
    /// SHA-256 of the exact text that was shown to the user (from `install.prepare`).
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptLicenceResult {
    pub accepted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallStarted {
    pub job_id: String,
    pub steps: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct JobParams {
    /// Job id; the latest job when omitted (status only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelResult {
    pub cancelling: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginParams {
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginResult {
    pub client_id: String,
    pub name: String,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateClientParams {
    pub name: String,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateClientResult {
    pub id: String,
    /// Shown once. The Core stores only its hash.
    pub token: String,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientInfo {
    pub id: String,
    pub name: String,
    pub scopes: Vec<String>,
    pub created_unix: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevokeClientParams {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevokeClientResult {
    pub revoked: bool,
}

/// Result of one doctor check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    pub id: String,
    pub title: String,
    pub status: CheckStatus,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoctorReport {
    pub checks: Vec<Check>,
}

impl DoctorReport {
    /// Worst status across all checks.
    #[must_use]
    pub fn overall(&self) -> CheckStatus {
        if self.checks.iter().any(|c| c.status == CheckStatus::Fail) {
            CheckStatus::Fail
        } else if self.checks.iter().any(|c| c.status == CheckStatus::Warn) {
            CheckStatus::Warn
        } else {
            CheckStatus::Ok
        }
    }
}
