//! `umr` — command-line interface of the AI Mobile Runtime (CLI-001).
//!
//! Commands now: `status`, `start`, `stop`, `doctor`, `plan`, `install`, `components`, `remove`,
//! `repair`, `clean`, `admin`, `project`, `trust`. More (`run`, `device`, `logs`, `mcp`, …)
//! arrive with later tasks.
//!
//! Exit codes: 0 success · 1 failure / not running (status) / failing checks (doctor) · 2 usage error.

mod core_process;
mod env_print;
mod install_cmd;
mod manage_cmd;
mod plan_print;
mod project_cmd;

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
