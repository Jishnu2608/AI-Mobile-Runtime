//! `umr` — command-line interface of the AI Mobile Runtime (CLI-001).
//!
//! Commands now: `status`, `start`, `stop`, `doctor`, `plan`, `install`, `components`, `remove`,
//! `repair`, `clean`, `admin`, `project`, `trust`, `device list|profiles|create|boot|stop|use|posture`, `session`, `app`, `screenshot`, `ui`, `input`. More (`run`, `device`, `logs`, `mcp`, …)
//! arrive with later tasks.
//!
//! Exit codes: 0 success · 1 failure / not running (status) / failing checks (doctor) · 2 usage error.

mod agent_cmd;
mod core_process;
mod device_cmd;
mod diag_cmd;
mod env_print;
mod install_cmd;
mod manage_cmd;
mod mcp_cmd;
mod plan_print;
mod preview_cmd;
mod project_cmd;
mod run_cmd;
mod session_cmd;
mod update_cmd;

use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use serde_json::Value;
use umr_api::methods::{
    self, CheckStatus, CoreInfo, DoctorReport, ProjectPlanParams, ShutdownAccepted,
};

use crate::core_process::CoreTarget;

#[derive(Debug, Parser)]
#[command(
    name = "umr",
    version,
    about = "AI Mobile Runtime — run and preview mobile apps next to your coding agent"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show whether the Runtime Core is running.
    Status {
        /// Print machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Start the Runtime Core in the background (no-op if already running).
    Start,
    /// Stop the Runtime Core.
    Stop,
    /// Check this machine (starts the Runtime Core if needed).
    Doctor {
        /// Print machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Show what a project needs and what would be installed (installs nothing, changes nothing).
    Plan {
        /// Project folder (default: current folder).
        path: Option<std::path::PathBuf>,
        /// Scan the environment again instead of using the last scan.
        #[arg(long)]
        rescan: bool,
        /// Print machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Install what `umr plan` lists into the managed folder (asks before anything is installed).
    Install {
        /// Project folder (default: current folder).
        path: Option<std::path::PathBuf>,
        /// Skip the final confirmation (licences are still shown and must be accepted).
        #[arg(long)]
        yes: bool,
    },
    /// List installed components, check that they work, and show disk usage.
    Components {
        /// Print machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Remove one component the product installed (id from `umr components`).
    Remove {
        id: String,
        /// Remove even if other components need it.
        #[arg(long)]
        force: bool,
        /// Do not ask for confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Remove broken components; with a project folder, install them again.
    Repair {
        path: Option<std::path::PathBuf>,
        /// Skip the final install confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Show what kind of project a folder is, with the evidence, and whether it is trusted.
    Project {
        path: Option<std::path::PathBuf>,
        /// Print machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Trust a project folder so its build scripts may run (or `--revoke` the trust).
    Trust {
        path: Option<std::path::PathBuf>,
        #[arg(long)]
        revoke: bool,
        /// Do not ask for confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Devices seen by the product's adb server.
    Device {
        #[command(subcommand)]
        action: DeviceAction,
    },
    /// Build, install and launch a project on the session's device (Kotlin/Java via Gradle,
    /// Flutter, React Native / Expo) and open the preview. The folder must be trusted and its
    /// toolchain installed.
    Run {
        /// Project folder (default: current folder).
        path: Option<std::path::PathBuf>,
        /// Device size class (default: standard).
        #[arg(long)]
        profile: Option<String>,
        /// Boot the device cold.
        #[arg(long)]
        cold: bool,
        /// Stop the running app and its tools instead.
        #[arg(long)]
        stop: bool,
        /// Do not reload on saved files.
        #[arg(long)]
        no_watch: bool,
        /// Do not open the preview window.
        #[arg(long)]
        no_preview: bool,
        #[arg(long)]
        json: bool,
    },
    /// The live preview window (docked beside your terminal), its status and the latency
    /// self-test.
    Preview {
        #[command(subcommand)]
        action: Option<PreviewAction>,
        /// Where the window sits: right (default), left or float.
        #[arg(long, default_value = "right")]
        dock: String,
    },
    /// Problems of the current run, classified (build, runtime, crash, environment, device,
    /// network) and deduplicated; `umr problems show <fingerprint>` for one with its stack.
    Problems {
        #[command(subcommand)]
        action: Option<ProblemsAction>,
        /// Only this category.
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Log lines of the current run: tool output, progress and the app's device log.
    Logs {
        /// `tool`, `umr` or `logcat`.
        #[arg(long)]
        source: Option<String>,
        /// Lowest level: V, D, I, W, E or F.
        #[arg(long)]
        level: Option<String>,
        /// Only lines containing this text (or tag).
        #[arg(long)]
        grep: Option<String>,
        /// How many recent lines.
        #[arg(short = 'n', long, default_value_t = 100)]
        lines: usize,
        /// Keep printing new lines.
        #[arg(short, long)]
        follow: bool,
        /// One JSON object per line.
        #[arg(long)]
        json: bool,
    },
    /// MCP server for an AI coding agent on stdin/stdout (the agent starts it; see
    /// `umr agent setup`).
    Mcp {
        /// The agent client whose token and permissions to use.
        #[arg(long, default_value = "claude-code")]
        client: String,
    },
    /// Connect AI coding agents: setup, clients, grants, audit log.
    Agent {
        #[command(subcommand)]
        action: AgentAction,
    },
    /// Requests from agents waiting for your answer (running a new project folder, switching
    /// the device).
    Approvals {
        #[command(subcommand)]
        action: Option<ApprovalAction>,
    },
    /// Apply code changes to the running app: hot (default), restart or full.
    Reload {
        kind: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Framework adapters.
    Adapter {
        #[command(subcommand)]
        action: AdapterAction,
    },
    /// The current runtime session (device and app).
    Session {
        #[command(subcommand)]
        action: Option<SessionAction>,
    },
    /// Install, launch, stop or uninstall an app on the current device.
    App {
        #[command(subcommand)]
        action: AppAction,
    },
    /// Save a screenshot of the current device (PNG).
    Screenshot {
        /// Copy the PNG here as well.
        #[arg(long)]
        out: Option<std::path::PathBuf>,
        /// Device serial (default: the current session's device).
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show the UI tree of the current screen (text, ids, bounds, clickable elements).
    Ui {
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Send input to the current device.
    Input {
        #[command(subcommand)]
        action: InputCmd,
        #[arg(long, global = true)]
        target: Option<String>,
    },
    /// Check for a newer version and install it (installed copies; portable copies get a link).
    Update {
        /// Only check, do not install.
        #[arg(long)]
        check: bool,
        /// Do not ask.
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
    /// Delete the download cache and old logs.
    Clean,
    /// Run one administrator action through a Windows UAC prompt:
    /// `enable-whpx` or `enable-long-paths`.
    Admin {
        action: String,
        /// Do not ask for confirmation (Windows still shows its UAC prompt).
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Debug, Subcommand)]
enum DeviceAction {
    /// List devices and emulators (starts the product's private adb server if needed).
    List {
        /// Print machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Show the device size classes (compact, standard, large, fold, flip, tablet).
    Profiles {
        /// Print machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Create the emulator (AVD) for a size class from the product's own hardware definition.
    Create {
        profile: String,
        /// API level of the system image (default: newest installed).
        #[arg(long)]
        api: Option<String>,
    },
    /// Boot the emulator for a size class headless (creates it if needed). Warm boots from the
    /// product's snapshot when the GPU mode still matches; checks that the screen renders.
    Boot {
        profile: String,
        #[arg(long)]
        api: Option<String>,
        /// Ignore the warm snapshot and boot cold.
        #[arg(long)]
        cold: bool,
        /// Show the emulator's own window (debugging).
        #[arg(long)]
        window: bool,
        /// GPU mode: `host` (default), or software `swiftshader_indirect` / `angle_indirect`.
        #[arg(long)]
        gpu: Option<String>,
        /// Print machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Stop an emulator the product started.
    Stop {
        profile: String,
        #[arg(long)]
        api: Option<String>,
    },
    /// Start a session on a size class: boot it (switching from another device), optionally
    /// install and launch an app.
    Use {
        profile: String,
        #[arg(long)]
        api: Option<String>,
        #[arg(long)]
        cold: bool,
        /// APK to install and launch after boot.
        #[arg(long)]
        apk: Option<std::path::PathBuf>,
        /// Package to launch (already installed, or the APK's package).
        #[arg(long)]
        package: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Rotate the emulator 90° (left or right).
    Rotate {
        #[arg(default_value = "left")]
        direction: String,
    },
    /// Fold or unfold a foldable emulator while it runs: closed, half-opened, opened, flipped, tent.
    Posture {
        posture: String,
        #[arg(long)]
        target: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum PreviewAction {
    /// Frames, windows and the time the product adds (NFR-02a).
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Touch → visible latency and frame rate on the device (NFR-02b / NFR-03); taps the home
    /// screen.
    Selftest {
        #[arg(long, default_value_t = 20)]
        taps: u32,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum AgentAction {
    /// Connect Claude Code or Codex: creates its client, writes its MCP server entry and the
    /// agent instructions (shows the changes and asks first).
    Setup {
        /// `claude-code` or `codex`.
        agent: String,
        /// Project folder (`.mcp.json`, skill / `AGENTS.md`); default: current folder.
        #[arg(long)]
        project: Option<std::path::PathBuf>,
        /// Do not ask.
        #[arg(long)]
        yes: bool,
        /// Only show what would change.
        #[arg(long)]
        dry_run: bool,
    },
    /// Create an agent client with its own token (default scopes: observe, interact, control).
    Add {
        name: String,
        /// Comma-separated: observe, interact, control.
        #[arg(long)]
        scopes: Option<String>,
    },
    /// Agent clients, their scopes and grants.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Remove an agent client (its token stops working).
    Remove { name: String },
    /// Let an agent run a project folder (or `device:switch`) without asking each time.
    Grant { name: String, what: String },
    /// Take a grant back.
    Ungrant { name: String, what: String },
    /// What agents called (newest last).
    Audit {
        /// Only this agent.
        #[arg(long)]
        client: Option<String>,
        #[arg(short = 'n', long, default_value_t = 30)]
        lines: usize,
        #[arg(long)]
        json: bool,
    },
    /// Print the agent instructions (`skill` = Claude Code SKILL.md, `agents` = AGENTS.md
    /// section, `short` = MCP instructions).
    Instructions {
        #[arg(long, default_value = "skill")]
        format: String,
    },
}

#[derive(Debug, Subcommand)]
enum ApprovalAction {
    /// Allow a request.
    Allow {
        id: String,
        /// Remember it for this agent (records a grant).
        #[arg(long)]
        always: bool,
    },
    /// Deny a request.
    Deny { id: String },
}

#[derive(Debug, Subcommand)]
enum ProblemsAction {
    /// One problem with its stack.
    Show {
        fingerprint: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum AdapterAction {
    /// Run the adapter contract test kit live: start, every reload kind, stop.
    Check {
        path: Option<std::path::PathBuf>,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum SessionAction {
    /// State, recent events, supervised processes and ports (default).
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Stop the session and its emulator.
    Stop,
}

#[derive(Debug, Subcommand)]
enum AppAction {
    Install {
        apk: std::path::PathBuf,
        #[arg(long)]
        target: Option<String>,
    },
    Launch {
        package: String,
        /// Activity (`.Main` or `pkg/.Main`); default: the launcher activity.
        #[arg(long)]
        activity: Option<String>,
        #[arg(long)]
        target: Option<String>,
    },
    Stop {
        package: String,
        #[arg(long)]
        target: Option<String>,
    },
    Uninstall {
        package: String,
        #[arg(long)]
        target: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum InputCmd {
    /// Tap at device pixels.
    Tap { x: i32, y: i32 },
    /// Swipe from (x1, y1) to (x2, y2).
    Swipe {
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        #[arg(long, default_value_t = 300)]
        ms: u64,
    },
    /// Type text into the focused field.
    Text { text: String },
    /// Press a key: home, back, recents, enter, delete, tab, escape, menu, power, `volume_up`, `volume_down`, up, down, left, right.
    Key { name: String },
}

#[allow(clippy::too_many_lines)] // one flat dispatch over all commands
fn main() -> ExitCode {
    let cli = Cli::parse();
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => return fail(&format!("cannot start async runtime: {e}")),
    };
    rt.block_on(async {
        let target = match CoreTarget::resolve() {
            Ok(t) => t,
            Err(e) => return fail(&e),
        };
        match cli.command {
            Command::Status { json } => status(&target, json).await,
            Command::Start => start(&target).await,
            Command::Stop => stop(&target).await,
            Command::Doctor { json } => doctor(&target, json).await,
            Command::Plan { path, rescan, json } => plan(&target, path, rescan, json).await,
            Command::Install { path, yes } => match project_dir(path) {
                Ok(dir) => install_cmd::install(&target, dir, yes).await,
                Err(e) => fail(&e),
            },
            Command::Components { json } => manage_cmd::components(&target, json).await,
            Command::Remove { id, force, yes } => manage_cmd::remove(&target, id, force, yes).await,
            Command::Repair { path, yes } => {
                let dir = match path.map(|p| project_dir(Some(p))).transpose() {
                    Ok(d) => d,
                    Err(e) => return fail(&e),
                };
                manage_cmd::repair(&target, dir, yes).await
            }
            Command::Clean => manage_cmd::clean(&target).await,
            Command::Device { action } => match action {
                DeviceAction::List { json } => device_cmd::list(&target, json).await,
                DeviceAction::Profiles { json } => device_cmd::profiles(&target, json).await,
                DeviceAction::Create { profile, api } => {
                    device_cmd::create(&target, &profile, api).await
                }
                DeviceAction::Boot {
                    profile,
                    api,
                    cold,
                    window,
                    gpu,
                    json,
                } => device_cmd::boot(&target, &profile, api, cold, window, gpu, json).await,
                DeviceAction::Stop { profile, api } => {
                    device_cmd::stop(&target, &profile, api).await
                }
                DeviceAction::Use {
                    profile,
                    api,
                    cold,
                    apk,
                    package,
                    json,
                } => session_cmd::use_profile(&target, &profile, api, cold, apk, package, json).await,
                DeviceAction::Rotate { direction } => preview_cmd::rotate(&target, &direction).await,
                DeviceAction::Posture { posture, target: t } => {
                    session_cmd::call_print(
                        &target,
                        methods::DEVICE_POSTURE,
                        serde_json::json!({ "posture": posture, "target": t }),
                    )
                    .await
                }
            },
            Command::Run {
                stop: true,
                ..
            } => run_cmd::stop(&target).await,
            Command::Run {
                path,
                profile,
                cold,
                json,
                no_watch,
                no_preview,
                ..
            } => {
                // AC-CLI-01: the app appears in the docked window
                if !no_preview && !json {
                    if let Err(e) = preview_cmd::open_window(&target, "right").await {
                        eprintln!("umr: preview window: {e}");
                    }
                }
                run_cmd::run(&target, path, profile, cold, !no_watch, json).await
            }
            Command::Preview { action, dock } => match action {
                None => preview_cmd::open(&target, &dock).await,
                Some(PreviewAction::Status { json }) => preview_cmd::status(&target, json).await,
                Some(PreviewAction::Selftest { taps, json }) => preview_cmd::selftest(&target, taps, json).await,
            },
            Command::Problems { action: Some(ProblemsAction::Show { fingerprint, json }), .. } => {
                diag_cmd::show(&target, &fingerprint, json).await
            }
            Command::Problems { action: None, category, json } => {
                diag_cmd::problems(&target, category, json).await
            }
            Command::Logs { source, level, grep, lines, follow, json } => {
                diag_cmd::logs(&target, source, level, grep, lines, follow, json).await
            }
            Command::Update { check, yes, json } => update_cmd::run(&target, check, yes, json).await,
            Command::Mcp { client } => mcp_cmd::run(&target, &client).await,
            Command::Agent { action } => match action {
                AgentAction::Setup { agent, project, yes, dry_run } => {
                    agent_cmd::setup(&target, &agent, project, yes, dry_run).await
                }
                AgentAction::Add { name, scopes } => agent_cmd::add(&target, &name, scopes).await,
                AgentAction::List { json } => agent_cmd::list(&target, json).await,
                AgentAction::Remove { name } => agent_cmd::remove(&target, &name).await,
                AgentAction::Grant { name, what } => agent_cmd::grant(&target, &name, &what, true).await,
                AgentAction::Ungrant { name, what } => agent_cmd::grant(&target, &name, &what, false).await,
                AgentAction::Audit { client, lines, json } => agent_cmd::audit(&target, client, lines, json).await,
                AgentAction::Instructions { format } => {
                    match format.as_str() {
                        "skill" => print!("{}", umr_mcp::instructions::skill_markdown(true)),
                        "agents" => print!("{}", umr_mcp::instructions::skill_markdown(false)),
                        "short" => println!("{}", umr_mcp::instructions::INSTRUCTIONS),
                        _ => return fail("--format is skill, agents or short"),
                    }
                    ExitCode::SUCCESS
                }
            },
            Command::Approvals { action } => match action {
                None => agent_cmd::approvals(&target, None).await,
                Some(ApprovalAction::Allow { id, always }) => agent_cmd::approvals(&target, Some((id, true, always))).await,
                Some(ApprovalAction::Deny { id }) => agent_cmd::approvals(&target, Some((id, false, false))).await,
            },
            Command::Reload { kind, json } => run_cmd::reload(&target, kind, json).await,
            Command::Adapter {
                action: AdapterAction::Check { path, profile, json },
            } => run_cmd::check(&target, path, profile, json).await,
            Command::Session { action } => match action.unwrap_or(SessionAction::Status { json: false }) {
                SessionAction::Status { json } => session_cmd::status(&target, json).await,
                SessionAction::Stop => session_cmd::stop(&target).await,
            },
            Command::App { action } => {
                let (method, params) = match action {
                    AppAction::Install { apk, target: t } => (
                        methods::APP_INSTALL,
                        serde_json::json!({ "apk": session_cmd::absolute(&apk), "target": t }),
                    ),
                    AppAction::Launch { package, activity, target: t } => (
                        methods::APP_LAUNCH,
                        serde_json::json!({ "package": package, "activity": activity, "target": t }),
                    ),
                    AppAction::Stop { package, target: t } => (
                        methods::APP_STOP,
                        serde_json::json!({ "package": package, "target": t }),
                    ),
                    AppAction::Uninstall { package, target: t } => (
                        methods::APP_UNINSTALL,
                        serde_json::json!({ "package": package, "target": t }),
                    ),
                };
                session_cmd::call_print(&target, method, params).await
            }
            Command::Screenshot { out, target: t, json } => {
                session_cmd::screenshot(&target, out, t, json).await
            }
            Command::Ui { target: t, json } => session_cmd::ui(&target, t, json).await,
            Command::Input { action, target: t } => {
                let params = match action {
                    InputCmd::Tap { x, y } => serde_json::json!({ "action": "tap", "x": x, "y": y }),
                    InputCmd::Swipe { x1, y1, x2, y2, ms } => serde_json::json!({
                        "action": "swipe", "x": x1, "y": y1, "x2": x2, "y2": y2, "ms": ms
                    }),
                    InputCmd::Text { text } => serde_json::json!({ "action": "text", "text": text }),
                    InputCmd::Key { name } => serde_json::json!({ "action": "key", "key": name }),
                };
                let mut params = params;
                params["target"] = serde_json::json!(t);
                session_cmd::call_print(&target, methods::DEVICE_INPUT, params).await
            }
            Command::Project { path, json } => match project_dir(path) {
                Ok(dir) => project_cmd::show(&target, dir, json).await,
                Err(e) => fail(&e),
            },
            Command::Trust { path, revoke, yes } => match project_dir(path) {
                Ok(dir) => project_cmd::trust(&target, dir, revoke, yes).await,
                Err(e) => fail(&e),
            },
            Command::Admin { action, yes } => manage_cmd::admin(&target, &action, yes).await,
        }
    })
}

/// Absolute project folder without the `\\?\` prefix `canonicalize` adds on Windows.
fn project_dir(path: Option<std::path::PathBuf>) -> Result<String, String> {
    let dir = path
        .map_or_else(std::env::current_dir, Ok)
        .and_then(std::fs::canonicalize)
        .map_err(|e| format!("project folder: {e}"))?;
    let dir = dir.display().to_string();
    Ok(dir
        .strip_prefix(r"\\?\")
        .map_or_else(|| dir.clone(), str::to_string))
}

pub(crate) fn fail(msg: &str) -> ExitCode {
    eprintln!("umr: {msg}");
    ExitCode::FAILURE
}

async fn status(target: &CoreTarget, json: bool) -> ExitCode {
    let client = match target.connect().await {
        Ok(c) => c,
        Err(e) => return fail(&e),
    };
    let Some(mut client) = client else {
        if json {
            println!("{}", serde_json::json!({ "running": false }));
        } else {
            println!("Runtime Core: not running");
            println!("Start it with: umr start");
        }
        return ExitCode::FAILURE;
    };
    match client.call::<CoreInfo>(methods::INFO, Value::Null).await {
        Ok(info) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "running": true, "version": info.version, "api_version": info.api_version,
                        "pid": info.pid, "uptime_s": info.uptime_s, "data_root": info.data_root, "toolchains_root": info.toolchains_root, "pipe": target.pipe
                    })
                );
            } else {
                println!("Runtime Core: running");
                println!("  version    {} (API {})", info.version, info.api_version);
                println!("  pid        {}", info.pid);
                println!("  uptime     {}", format_uptime(info.uptime_s));
                println!("  data       {}", info.data_root);
                if !info.toolchains_root.is_empty() && info.toolchains_root != info.data_root {
                    println!("  toolchains {}", info.toolchains_root);
                }
            }
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e.to_string()),
    }
}

async fn start(target: &CoreTarget) -> ExitCode {
    match target.connect_or_start().await {
        Ok((mut client, started)) => {
            let pid = client
                .call::<CoreInfo>(methods::INFO, Value::Null)
                .await
                .map(|i| i.pid)
                .ok();
            let pid = pid.map_or_else(String::new, |p| format!(" (pid {p})"));
            if started {
                println!("Runtime Core started{pid}");
            } else {
                println!("Runtime Core is already running{pid}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e),
    }
}

async fn stop(target: &CoreTarget) -> ExitCode {
    let client = match target.connect().await {
        Ok(c) => c,
        Err(e) => return fail(&e),
    };
    let Some(mut client) = client else {
        println!("Runtime Core is not running");
        return ExitCode::SUCCESS;
    };
    if let Err(e) = client
        .call::<ShutdownAccepted>(methods::SHUTDOWN, Value::Null)
        .await
    {
        return fail(&format!("stop request failed: {e}"));
    }
    drop(client);
    if target.wait_until_stopped(Duration::from_secs(10)).await {
        println!("Runtime Core stopped");
        ExitCode::SUCCESS
    } else {
        fail("the Runtime Core did not stop within 10 s")
    }
}

/// Stops the Core if it runs; errors are ignored (used before an update replaces its files).
pub(crate) async fn stop_quiet(target: &CoreTarget) {
    if let Ok(Some(mut client)) = target.connect().await {
        let _ = client
            .call::<ShutdownAccepted>(methods::SHUTDOWN, Value::Null)
            .await;
        drop(client);
        let _ = target.wait_until_stopped(Duration::from_secs(10)).await;
    }
}

async fn doctor(target: &CoreTarget, json: bool) -> ExitCode {
    let (mut client, started) = match target.connect_or_start().await {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    let report = match client
        .call::<DoctorReport>(methods::DOCTOR, Value::Null)
        .await
    {
        Ok(r) => r,
        Err(e) => return fail(&e.to_string()),
    };
    if !json {
        if started {
            println!("(started the Runtime Core)\n");
        }
        for c in &report.checks {
            let mark = match c.status {
                CheckStatus::Ok => "[ ok ]",
                CheckStatus::Warn => "[warn]",
                CheckStatus::Fail => "[FAIL]",
            };
            println!("{mark} {:<22} {}", c.title, c.detail);
        }
        println!("\nScanning the development environment…");
    }
    let env: umr_env::EnvReport = match client.call(methods::ENV_SCAN, Value::Null).await {
        Ok(r) => r,
        Err(e) => return fail(&format!("environment scan failed: {e}")),
    };
    if json {
        let out = serde_json::json!({ "checks": report.checks, "environment": env });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
    } else {
        env_print::print(&env);
        println!();
        println!(
            "{}",
            match report.overall() {
                CheckStatus::Ok => "Core checks passed.",
                CheckStatus::Warn => "Core checks passed with warnings.",
                CheckStatus::Fail => "Some core checks failed.",
            }
        );
    }
    if report.overall() == CheckStatus::Fail {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
async fn plan(
    target: &CoreTarget,
    path: Option<std::path::PathBuf>,
    rescan: bool,
    json: bool,
) -> ExitCode {
    let dir = match path
        .map_or_else(std::env::current_dir, Ok)
        .and_then(std::fs::canonicalize)
    {
        Ok(d) => d,
        Err(e) => return fail(&format!("project folder: {e}")),
    };
    // canonicalize gives a \\?\ path on Windows; send the plain form.
    let dir = dir.display().to_string();
    let dir = dir
        .strip_prefix(r"\\?\")
        .map_or_else(|| dir.clone(), str::to_string);
    let (mut client, started) = match target.connect_or_start().await {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    if started && !json {
        println!("(started the Runtime Core)\n");
    }
    let params = serde_json::to_value(ProjectPlanParams { path: dir, rescan }).unwrap_or_default();
    let out: Value = match client.call(methods::PROJECT_PLAN, params).await {
        Ok(v) => v,
        Err(e) => return fail(&e.to_string()),
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return ExitCode::SUCCESS;
    }
    let facts: Result<umr_resolver::project::ProjectFacts, _> =
        serde_json::from_value(out["project"].clone());
    let plan: Result<umr_resolver::plan::Plan, _> = serde_json::from_value(out["plan"].clone());
    match (facts, plan) {
        (Ok(f), Ok(p)) => {
            plan_print::print(&f, &p);
            if p.conflicts.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        (Err(e), _) | (_, Err(e)) => fail(&format!("unexpected plan format: {e}")),
    }
}

fn format_uptime(s: u64) -> String {
    match s {
        0..=59 => format!("{s} s"),
        60..=3599 => format!("{} min {} s", s / 60, s % 60),
        _ => format!("{} h {} min", s / 3600, (s % 3600) / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn uptime_formatting() {
        assert_eq!(format_uptime(5), "5 s");
        assert_eq!(format_uptime(125), "2 min 5 s");
        assert_eq!(format_uptime(7260), "2 h 1 min");
    }
}
