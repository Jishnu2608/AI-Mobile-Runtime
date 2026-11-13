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
/// `project.detect` [`ProjectPathParams`] → `{ "detection": umr_resolver::detect::Detection,
/// "trusted": bool }`. Reads files only. Scope: observe.
pub const PROJECT_DETECT: &str = "project.detect";
/// `project.trust` [`TrustParams`] → [`TrustResult`]. Workspace trust: the product only runs a
/// project's build scripts after the user trusted the folder. Scope: admin.
pub const PROJECT_TRUST: &str = "project.trust";

/// `device.list` → `{ "server": {port, protocol, connected, last_error}, "devices": [Device],
/// "coexistence": Coexistence }`. Starts the product's private adb server on first use
/// (never touches port 5037). Scope: observe.
pub const DEVICE_LIST: &str = "device.list";
/// `device.profiles` [`ProfilesParams`] → `{ "api": "36", "profiles": [{profile, avd}] }`:
/// the size-class catalogue and which AVDs exist. Scope: observe.
pub const DEVICE_PROFILES: &str = "device.profiles";
/// `device.create` [`CreateDeviceParams`] → AVD info. Creates the AVD for a profile from the
/// product's own hardware definition (lazy; keeps user data). Scope: control.
pub const DEVICE_CREATE: &str = "device.create";
/// `device.boot` [`BootDeviceParams`] → boot report `{avd, serial, console_port, grpc_port, gpu,
/// mode: cold|warm, cold_reason, seconds, boot_completed_seconds, health, snapshot_saved,
/// attempts, log}`. Creates the AVD if needed, boots it headless (`-gpu host`, gRPC with token),
/// waits for `sys.boot_completed`, runs the render health check and keeps a warm snapshot whose
/// GPU mode is recorded (DEV-003/009). Returns the running report if already booted.
/// Scope: control.
pub const DEVICE_BOOT: &str = "device.boot";
/// `device.stop` [`DeviceRefParams`] → `{ "stopped": bool }`. Stops an emulator the product
/// started. Scope: control.
pub const DEVICE_STOP: &str = "device.stop";

// ---- device interaction (DEV-005/006, step 2.5) ---------------------------------------------
// `target` (optional) is a device serial; without it the Core uses the current session's
// device, else the only emulator it started, else the only online device.

/// `device.screenshot` [`TargetParams`] → `{ serial, path, width, height, bytes, source }`
/// (PNG saved under `<home>/screenshots`; gRPC on emulators, `screencap` otherwise). Scope: observe.
pub const DEVICE_SCREENSHOT: &str = "device.screenshot";
/// `device.ui` [`TargetParams`] → `{ serial, count, nodes: [UiNode] }`: compact UI tree. Scope: observe.
pub const DEVICE_UI: &str = "device.ui";
/// `device.input` [`InputParams`] → `{ serial, via }`: tap, swipe, text or key. Scope: interact.
pub const DEVICE_INPUT: &str = "device.input";
/// `device.posture` [`PostureParams`] → `{ serial, posture }`: fold / half-open / unfold a
/// foldable emulator while it runs (DEV-008b). Scope: interact.
pub const DEVICE_POSTURE: &str = "device.posture";
/// `app.install` [`InstallAppParams`] → `{ serial, package }`. Scope: control.
pub const APP_INSTALL: &str = "app.install";
/// `app.launch` [`AppParams`] → `{ serial, component, total_ms, pid }`. Scope: control.
pub const APP_LAUNCH: &str = "app.launch";
/// `app.stop` [`AppParams`] → `{ serial, package }` (force-stop). Scope: control.
pub const APP_STOP: &str = "app.stop";
/// `app.uninstall` [`AppParams`] → `{ serial, package }`. Scope: control.
pub const APP_UNINSTALL: &str = "app.uninstall";

// ---- sessions (step 2.6) ---------------------------------------------------------------------

/// `session.start` [`StartSessionParams`] → session. Boots the profile's emulator (switching away
/// from another session's device), optionally installs + launches an app; recovers a session
/// the previous Core left in `device_error`. Scope: control.
pub const SESSION_START: &str = "session.start";
/// `session.status` → `{ session, events, processes, ports }` (checks the device and app are
/// still alive first). Scope: observe.
pub const SESSION_STATUS: &str = "session.status";
/// `session.stop` → session (emulator stopped). Scope: control.
pub const SESSION_STOP: &str = "session.stop";

// ---- running projects (Phase 3: ADP-001..003) -------------------------------------------------

/// `project.run` [`RunParams`] → `{ status: running|build_failed|launch_failed|failed, adapter,
/// package, seconds, problems, session }`. Trusted folders only; the toolchain must be installed.
/// Boots (or reuses) the session's device, then the adapter builds, installs and launches.
/// Scope: control.
pub const PROJECT_RUN: &str = "project.run";
/// `project.reload` [`ReloadParams`] → `{ status: reloaded|unsupported|build_failed|failed, kind,
/// ms, problems }`. Scope: control.
pub const PROJECT_RELOAD: &str = "project.reload";
/// `project.stop` → `{ stopped }`: stops the app and its tools (Gradle daemon, Flutter tool).
/// Scope: control.
pub const PROJECT_STOP: &str = "project.stop";
/// `project.status` [`RunStatusParams`] → `{ run, alive, events, cursor }`: adapter events
/// (progress, problems with file and line, reloads) after `since`. Scope: observe.
pub const PROJECT_STATUS: &str = "project.status";
/// `adapter.check` [`AdapterCheckParams`] → `{ passed, report }`: the adapter contract test kit
/// run live on the session's device (start, each reload kind, stop). Scope: control.
pub const ADAPTER_CHECK: &str = "adapter.check";

// ---- diagnostics (Phase 5) --------------------------------------------------------------------
// Everything returned is passed through the log sanitiser (DIA-005) first.

/// `diagnostics.list` [`DiagnosticsParams`] → `{ entries, counts, total, cursor, run }`: the
/// current run's problems, one entry per fingerprint (`category`, `origin`, `severity`, `source`,
/// `message`, `file`, `line`, `column`, `fingerprint`, `count`, `first_seen_ms`, `last_seen_ms`,
/// `seq`, `stack_frames`), new or repeated after `since`; `counts` per category. Scope: observe.
pub const DIAGNOSTICS_LIST: &str = "diagnostics.list";
/// `diagnostics.get` [`DiagnosticGetParams`] → one entry with its stack. Scope: observe.
pub const DIAGNOSTICS_GET: &str = "diagnostics.get";
/// `logs.tail` [`LogsParams`] → `{ lines: [{seq, at_ms, source, level, tag, pid, text}], cursor }`:
/// the run's tool output (`tool`), progress (`umr`) and the app's device log (`logcat`).
/// Scope: observe.
pub const LOGS_TAIL: &str = "logs.tail";

// ---- preview (Phase 4) ------------------------------------------------------------------------

/// `preview.open` [`PreviewOpenParams`] → `{ url, device, profile, state }`: a single-use
/// `ws://127.0.0.1:<port>/preview?token=…` for a preview window (frames + input, the current
/// session's device). Scope: interact.
pub const PREVIEW_OPEN: &str = "preview.open";
/// `preview.status` → `{ running, port, windows, device, stats }` (NFR-02a pipeline timing).
/// Scope: observe.
pub const PREVIEW_STATUS: &str = "preview.status";
/// `preview.selftest` [`SelfTestParams`] → `{ selftest, pipeline, verdict }`: touch → visible
/// latency and frame rate on the device, with the NFR-02a/02b/03 verdict (NFR-02c). Taps the
/// home screen. Scope: interact.
pub const PREVIEW_SELFTEST: &str = "preview.selftest";
/// `device.rotate` [`RotateParams`] → `{ serial, rotated }`. Scope: interact.
pub const DEVICE_ROTATE: &str = "device.rotate";

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

// ---- components and administrator actions (INS-005/006) ------------------------------------

/// `components.list` → `{ "components": [umr_installer::manage::ComponentHealth], "disk": DiskUsage }`.
/// Verifies every recorded component (starts JDK/Node once). Scope: observe.
pub const COMPONENTS_LIST: &str = "components.list";
/// `components.remove` [`RemoveComponentParams`] → `umr_installer::manage::Removed`. Managed
/// components only. Scope: admin.
pub const COMPONENTS_REMOVE: &str = "components.remove";
/// `components.repair` → `{ "removed": [Removed] }`: removes broken managed components so the
/// next `install.start` restores them. Scope: admin.
pub const COMPONENTS_REPAIR: &str = "components.repair";
/// `components.clean` → `{ "freed_bytes": u64 }`: download cache and old logs. Scope: admin.
pub const COMPONENTS_CLEAN: &str = "components.clean";
/// `admin.run` [`AdminRunParams`] → [`AdminRunResult`]. Starts the elevated helper for one
/// fixed action; Windows shows its UAC prompt. Scope: admin.
pub const ADMIN_RUN: &str = "admin.run";

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

// ---- agents (Phase 6) ---------------------------------------------------------------------------

/// `runtime.digest` [`DigestParams`] → [`Digest`]-shaped JSON (≤ 1 KB): session, device, app,
/// run state, problem counts, up to 3 new problems since `since`, the next cursor. The first
/// call an agent makes after each change (MCP `runtime_status`). Scope: observe.
pub const RUNTIME_DIGEST: &str = "runtime.digest";
/// `approvals.list` → `{ pending: [{id, client, action, detail, created_ms}] }`: requests from
/// agents waiting for the user (running an ungranted project, switching the device). Scope: admin.
pub const APPROVALS_LIST: &str = "approvals.list";
/// `approvals.decide` [`DecideParams`] → `{ id, decision }`. `always` also records a grant so
/// the same request from that client no longer asks. Scope: admin.
pub const APPROVALS_DECIDE: &str = "approvals.decide";
/// `audit.list` [`AuditParams`] → `{ entries: [{at_unix, client_id, action, detail}] }`: what
/// non-admin clients (agents) called, newest first. Scope: admin.
pub const AUDIT_LIST: &str = "audit.list";
/// `auth.client.grants` [`GrantsParams`] → `{ id, grants }`: set a client's grants
/// (`project:<folder>`, `device:switch`); an empty list removes them. Scope: admin.
pub const AUTH_CLIENT_GRANTS: &str = "auth.client.grants";

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
    /// Home folder (config, database, logs).
    pub data_root: String,
    /// Toolchains folder (SDKs, images, caches); same as `data_root` unless configured.
    #[serde(default)]
    pub toolchains_root: String,
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

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProfilesParams {
    /// API level of the system image; newest installed when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateDeviceParams {
    /// Profile id: `compact`, `standard`, `large`, `fold`, `flip`, `tablet`.
    pub profile: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BootDeviceParams {
    /// Profile id (see [`CreateDeviceParams`]).
    pub profile: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
    /// Ignore the warm snapshot and boot cold (a fresh snapshot is saved afterwards).
    #[serde(default)]
    pub cold: bool,
    /// Show the emulator's own window (debugging only; the product preview is headless).
    #[serde(default)]
    pub window: bool,
    /// GPU mode: `host` (default, DEC-006), or the software fallbacks `swiftshader_indirect` /
    /// `angle_indirect` for machines whose GPU driver fails.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRefParams {
    pub profile: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// `device.screenshot` only: scale the image down to at most this width (emulators scale
    /// on their side; physical devices return full size).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_width: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputAction {
    Tap,
    Swipe,
    Text,
    Key,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub action: InputAction,
    /// tap / swipe start (device pixels)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<i32>,
    /// swipe end
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x2: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y2: Option<i32>,
    /// swipe duration (default 300 ms)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// `home`, `back`, `recents`, `enter`, `delete`, `tab`, `escape`, `menu`, `power`,
    /// `volume_up`, `volume_down`, `up`, `down`, `left`, `right`, `wakeup`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostureParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// `closed`, `half-opened`, `opened`, `flipped`, `tent`
    pub posture: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallAppParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Absolute path of the APK.
    pub apk: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub package: String,
    /// `app.launch` only: activity (`.Main` or `pkg/.Main`); default the launcher activity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartSessionParams {
    pub profile: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
    /// Boot cold (ignore the warm snapshot).
    #[serde(default)]
    pub cold: bool,
    /// APK to install and launch after boot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apk: Option<String>,
    /// Package to launch (already installed, or the APK's package).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewOpenParams {
    /// Allow input from the window (default true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interact: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfTestParams {
    /// Number of timed taps (default 20).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taps: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RotateParams {
    /// `left` (default) or `right`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunParams {
    /// Absolute path of the project folder.
    pub path: String,
    /// Device size class (default `standard`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default)]
    pub cold: bool,
    /// ADP-006: reload on saved files (default on).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watch: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReloadParams {
    /// `hot` (default), `restart` or `full`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunStatusParams {
    /// Only events after this cursor (from the previous call).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticsParams {
    /// Only entries new or repeated after this cursor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<u64>,
    /// `environment`, `build`, `runtime`, `crash`, `device` or `network`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    /// Include stack frames (default: only their number).
    #[serde(default)]
    pub stack: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticGetParams {
    pub fingerprint: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogsParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    /// `tool`, `umr` or `logcat`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Lowest level shown: `V` `D` `I` `W` `E` `F`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    /// Case-insensitive text (or tag) filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grep: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterCheckParams {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectPathParams {
    /// Absolute path of the project folder.
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustParams {
    pub path: String,
    /// `false` removes trust.
    #[serde(default = "yes")]
    pub trusted: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustResult {
    pub path: String,
    pub trusted: bool,
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
pub struct RemoveComponentParams {
    pub id: String,
    /// Remove even if other components depend on it.
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminRunParams {
    /// `enable-whpx` | `enable-long-paths`
    pub action: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminRunResult {
    pub action: String,
    /// `already-on` | `done` | `restart-required` | `declined` | `failed` | `needs-firmware`
    pub status: String,
    pub message: String,
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DigestParams {
    /// Cursor from the previous digest (`"<problems>.<events>"`); only changes after it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecideParams {
    pub id: String,
    /// `allow` or `deny`.
    pub decision: String,
    /// With `allow`: remember it (records a grant for the client).
    #[serde(default)]
    pub always: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantsParams {
    pub id: String,
    pub grants: Vec<String>,
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
    /// MCP-003 grants (`project:<folder>`, `device:switch`).
    #[serde(default)]
    pub grants: Vec<String>,
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
