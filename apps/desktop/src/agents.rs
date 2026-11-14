//! Phase 10.5 (CON-005): the "Connect your agent" page.
//!
//! The page does not edit agent config files itself. It runs the installed `umr.exe` (the file
//! next to this program) with one of four fixed `umr agent … --json` command lines, so the
//! desktop app and the CLI share one implementation (`apps/cli/src/agent_setup.rs`). The web view
//! can only choose the action, a known client id, a scope and the project folder; every value is
//! checked before it becomes an argument, and nothing goes through a shell.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use serde_json::Value;

/// What the page asks for.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRequest {
    /// `clients`, `plan`, `apply`, `doctor` or `fix`.
    pub action: String,
    pub client: Option<String>,
    pub project: Option<String>,
    /// `project` or `user`.
    pub scope: Option<String>,
    /// For `plan` / `apply`: disconnect instead of connect.
    #[serde(default)]
    pub remove: bool,
}

const CLIENTS: [&str; 5] = ["claude-code", "codex", "cursor", "claude-desktop", "vscode"];

/// The `umr` arguments for a request, or why it is refused.
pub fn arguments(r: &AgentRequest) -> Result<Vec<String>, String> {
    let client = match r.client.as_deref() {
        Some(c) if CLIENTS.contains(&c) => Some(c.to_string()),
        Some(c) => return Err(format!("unknown agent '{c}'")),
        None => None,
    };
    let project = match r.project.as_deref() {
        Some(p) if !p.is_empty() => {
            if !Path::new(p).is_absolute() || !Path::new(p).is_dir() {
                return Err(format!("{p} is not a folder"));
            }
            Some(p.to_string())
        }
        _ => None,
    };
    let scope = match r.scope.as_deref() {
        Some(s @ ("project" | "user")) => Some(s.to_string()),
        Some(s) => return Err(format!("unknown scope '{s}'")),
        None => None,
    };
    let mut a: Vec<String> = vec!["agent".into()];
    let need_client = || client.clone().ok_or_else(|| "no agent given".to_string());
    match r.action.as_str() {
        "clients" => a.push("clients".into()),
        "plan" | "apply" => {
            a.push("setup".into());
            a.push(need_client()?);
            if let Some(s) = &scope {
                a.extend(["--scope".into(), s.clone()]);
            }
            if r.remove {
                a.push("--remove".into());
            }
            if r.action == "apply" {
                a.push("--yes".into());
            }
        }
        "doctor" | "fix" => {
            a.push("doctor".into());
            if let Some(c) = &client {
                a.push(c.clone());
            }
            if r.action == "fix" {
                a.push("--fix".into());
            }
        }
        other => return Err(format!("unknown action '{other}'")),
    }
    if let Some(p) = project {
        a.extend(["--project".into(), p]);
    }
    a.push("--json".into());
    Ok(a)
}

/// The `umr.exe` installed next to this program.
fn umr_exe() -> Result<PathBuf, String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = me.with_file_name("umr.exe");
    if exe.is_file() {
        Ok(exe)
    } else {
        Err(format!(
            "{} not found next to the desktop app",
            exe.display()
        ))
    }
}

/// Runs `umr` and returns its JSON output. Doctor answers with exit code 1 when a check fails,
/// and still prints its report, so any JSON on stdout counts.
pub fn run(r: &AgentRequest) -> Result<Value, String> {
    let args = arguments(r)?;
    let mut cmd = Command::new(umr_exe()?);
    cmd.args(&args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().map_err(|e| format!("cannot run umr: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    if let Ok(v) = serde_json::from_str::<Value>(stdout.trim()) {
        return Ok(v);
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let msg = stderr.trim().trim_start_matches("umr: ");
    Err(if msg.is_empty() {
        format!("umr {} failed", args.join(" "))
    } else {
        msg.to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(action: &str, client: Option<&str>) -> AgentRequest {
        AgentRequest {
            action: action.into(),
            client: client.map(String::from),
            project: None,
            scope: None,
            remove: false,
        }
    }

    #[test]
    fn requests_become_fixed_command_lines() {
        assert_eq!(
            arguments(&req("clients", None)).unwrap(),
            ["agent", "clients", "--json"]
        );
        let mut r = req("apply", Some("cursor"));
        r.scope = Some("user".into());
        r.remove = true;
        assert_eq!(
            arguments(&r).unwrap(),
            ["agent", "setup", "cursor", "--scope", "user", "--remove", "--yes", "--json"]
        );
        assert_eq!(
            arguments(&req("plan", Some("codex"))).unwrap(),
            ["agent", "setup", "codex", "--json"]
        );
        assert_eq!(
            arguments(&req("fix", None)).unwrap(),
            ["agent", "doctor", "--fix", "--json"]
        );
        let dir = std::env::temp_dir();
        let mut r = req("doctor", Some("vscode"));
        r.project = Some(dir.display().to_string());
        assert_eq!(arguments(&r).unwrap()[3], "--project");
    }

    #[test]
    fn anything_else_is_refused() {
        assert!(arguments(&req("plan", Some("evil; rm"))).is_err());
        assert!(arguments(&req("plan", None)).is_err());
        assert!(arguments(&req("shutdown", None)).is_err());
        let mut r = req("plan", Some("cursor"));
        r.scope = Some("global".into());
        assert!(arguments(&r).is_err());
        let mut r = req("plan", Some("cursor"));
        r.project = Some("relative\\path".into());
        assert!(arguments(&r).is_err());
    }
}
