//! `umr agent …` and `umr approvals` (Phase 6: MCP-003, MCP-004, MCP-005).
//!
//! * `agent add|list|remove|grant|ungrant|audit`: agent clients (own token, scopes, grants).
//! * `agent setup claude-code|codex`: connects an agent to the runtime — writes its MCP server
//!   entry and the agent instructions, after showing exactly what changes and asking.
//! * `approvals [allow|deny <id>]`: requests from agents waiting for the user.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde_json::{json, Value};
use umr_api::methods;

use crate::core_process::CoreTarget;
use crate::fail;
use crate::mcp_cmd::token_path;

/// Scopes an agent gets by default: everything except `admin`.
pub const AGENT_SCOPES: [&str; 3] = ["observe", "interact", "control"];

async fn call(target: &CoreTarget, method: &str, params: Value) -> Result<Value, String> {
    let mut c = target.connect_or_start().await.map(|(c, _)| c)?;
    c.call_raw(method, params).await.map_err(|e| e.to_string())
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 40
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

async fn find_client(target: &CoreTarget, name: &str) -> Result<Option<Value>, String> {
    let v = call(target, methods::AUTH_CLIENT_LIST, Value::Null).await?;
    Ok(v.as_array()
        .into_iter()
        .flatten()
        .find(|c| c["name"] == name)
        .cloned())
}

/// Creates the client and its token file. `Ok(false)` if it already exists.
async fn ensure_client(target: &CoreTarget, name: &str, scopes: &[String]) -> Result<bool, String> {
    if !valid_name(name) {
        return Err(format!("'{name}': use letters, digits, - and _ (≤ 40)"));
    }
    if scopes.iter().any(|s| s == "admin") {
        return Err("agents never get the admin scope".into());
    }
    let file = token_path(target, name);
    if find_client(target, name).await?.is_some() && file.is_file() {
        return Ok(false);
    }
    if let Some(old) = find_client(target, name).await? {
        // a client without its token file is unusable: replace it
        call(
            target,
            methods::AUTH_CLIENT_REVOKE,
            json!({ "id": old["id"] }),
        )
        .await?;
    }
    let v = call(
        target,
        methods::AUTH_CLIENT_CREATE,
        json!({ "name": name, "scopes": scopes }),
    )
    .await?;
    let token = v["token"].as_str().ok_or("the Core returned no token")?;
    std::fs::create_dir_all(target.paths.agent_tokens_dir()).map_err(|e| e.to_string())?;
    umr_core::auth::write_secret(&file, token.as_bytes())
        .map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(true)
}

pub async fn add(target: &CoreTarget, name: &str, scopes: Option<String>) -> ExitCode {
    let scopes: Vec<String> = scopes.map_or_else(
        || AGENT_SCOPES.iter().map(ToString::to_string).collect(),
        |s| {
            s.split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect()
        },
    );
    match ensure_client(target, name, &scopes).await {
        Ok(true) => {
            println!(
                "Agent client '{name}' created ({}). Its MCP server: `umr mcp --client {name}`.",
                scopes.join(", ")
            );
            ExitCode::SUCCESS
        }
        Ok(false) => {
            println!(
                "Agent client '{name}' exists already (remove it first to change its scopes)."
            );
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e),
    }
}

pub async fn list(target: &CoreTarget, json_out: bool) -> ExitCode {
    let v = match call(target, methods::AUTH_CLIENT_LIST, Value::Null).await {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    if json_out {
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return ExitCode::SUCCESS;
    }
    let clients = v.as_array().cloned().unwrap_or_default();
    if clients.is_empty() {
        println!("No agent clients. Connect one with `umr agent setup claude-code` or `umr agent setup codex`.");
    }
    for c in clients {
        let name = c["name"].as_str().unwrap_or("?");
        let scopes: Vec<&str> = c["scopes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let grants: Vec<&str> = c["grants"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        println!(
            "{name}  [{}]  scopes: {}  grants: {}{}",
            c["id"].as_str().unwrap_or("?"),
            scopes.join(", "),
            if grants.is_empty() {
                "none".to_string()
            } else {
                grants.join(", ")
            },
            if token_path(target, name).is_file() {
                ""
            } else {
                "  (no token file: run `umr agent add` again)"
            }
        );
    }
    ExitCode::SUCCESS
}

pub async fn remove(target: &CoreTarget, name: &str) -> ExitCode {
    match find_client(target, name).await {
        Ok(Some(c)) => {
            if let Err(e) = call(
                target,
                methods::AUTH_CLIENT_REVOKE,
                json!({ "id": c["id"] }),
            )
            .await
            {
                return fail(&e);
            }
            let _ = std::fs::remove_file(token_path(target, name));
            println!("Agent client '{name}' removed; its token no longer works.");
            ExitCode::SUCCESS
        }
        Ok(None) => fail(&format!("no agent client '{name}'")),
        Err(e) => fail(&e),
    }
}

/// A grant as typed: a folder (made absolute) or `device:switch`.
fn grant_of(arg: &str) -> String {
    if arg == "device:switch" || arg.starts_with("project:") {
        arg.to_string()
    } else {
        format!("project:{}", crate::session_cmd::absolute(Path::new(arg)))
    }
}

pub async fn grant(target: &CoreTarget, name: &str, what: &str, add: bool) -> ExitCode {
    let c = match find_client(target, name).await {
        Ok(Some(c)) => c,
        Ok(None) => return fail(&format!("no agent client '{name}'")),
        Err(e) => return fail(&e),
    };
    let mut grants: Vec<String> = c["grants"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| g.as_str().map(String::from))
        .collect();
    let g = grant_of(what);
    if add {
        grants.push(g);
    } else {
        let key = g.replace('\\', "/").to_lowercase();
        grants.retain(|x| {
            x.replace('\\', "/").to_lowercase().trim_end_matches('/') != key.trim_end_matches('/')
        });
    }
    match call(
        target,
        methods::AUTH_CLIENT_GRANTS,
        json!({ "id": c["id"], "grants": grants }),
    )
    .await
    {
        Ok(v) => {
            println!("{name}: grants {}", v["grants"]);
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e),
    }
}

pub async fn audit(
    target: &CoreTarget,
    name: Option<String>,
    n: usize,
    json_out: bool,
) -> ExitCode {
    let client = match &name {
        Some(nm) => match find_client(target, nm).await {
            Ok(Some(c)) => c["id"].as_str().map(String::from),
            Ok(None) => return fail(&format!("no agent client '{nm}'")),
            Err(e) => return fail(&e),
        },
        None => None,
    };
    let v = match call(
        target,
        methods::AUDIT_LIST,
        json!({ "client": client, "limit": n }),
    )
    .await
    {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    if json_out {
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return ExitCode::SUCCESS;
    }
    let names = call(target, methods::AUTH_CLIENT_LIST, Value::Null)
        .await
        .unwrap_or_default();
    let name_of = |id: &str| {
        names
            .as_array()
            .into_iter()
            .flatten()
            .find(|c| c["id"] == id)
            .and_then(|c| c["name"].as_str())
            .unwrap_or(id)
            .to_string()
    };
    let entries = v["entries"].as_array().cloned().unwrap_or_default();
    if entries.is_empty() {
        println!("No agent calls recorded.");
    }
    for e in entries.iter().rev() {
        println!(
            "{}  {:<12} {:<20} {}",
            e["at_unix"],
            name_of(e["client_id"].as_str().unwrap_or("?")),
            e["action"].as_str().unwrap_or("?"),
            e["detail"].as_str().unwrap_or("")
        );
    }
    ExitCode::SUCCESS
}

pub async fn approvals(target: &CoreTarget, decide: Option<(String, bool, bool)>) -> ExitCode {
    if let Some((id, allow, always)) = decide {
        let p =
            json!({ "id": id, "decision": if allow { "allow" } else { "deny" }, "always": always });
        return match call(target, methods::APPROVALS_DECIDE, p).await {
            Ok(_) => {
                println!(
                    "{} {id}{}",
                    if allow { "Allowed" } else { "Denied" },
                    if always { " (remembered)" } else { "" }
                );
                ExitCode::SUCCESS
            }
            Err(e) => fail(&e),
        };
    }
    let v = match call(target, methods::APPROVALS_LIST, Value::Null).await {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    let list = v["pending"].as_array().cloned().unwrap_or_default();
    if list.is_empty() {
        println!("No requests waiting.");
    }
    for p in list {
        println!(
            "{}  {} wants to {}: {}",
            p["id"].as_str().unwrap_or("?"),
            p["client"].as_str().unwrap_or("?"),
            p["action"].as_str().unwrap_or("?"),
            p["detail"].as_str().unwrap_or("")
        );
        println!(
            "    `umr approvals allow {0}` [--always] · `umr approvals deny {0}`",
            p["id"].as_str().unwrap_or("?")
        );
    }
    ExitCode::SUCCESS
}

// ---- setup (MCP-004 / MCP-005) -----------------------------------------------------------------

/// A planned file change.
#[derive(Debug, PartialEq, Eq)]
pub struct Change {
    pub path: PathBuf,
    pub old: Option<String>,
    pub new: String,
}

/// Lines that differ between `old` and `new` (common start and end left out), `-` / `+`.
#[must_use]
pub fn diff(old: &str, new: &str) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let pre = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suf = a[pre..]
        .iter()
        .rev()
        .zip(b[pre..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let removed = a[pre..a.len() - suf].iter().map(|l| format!("- {l}\n"));
    let added = b[pre..b.len() - suf].iter().map(|l| format!("+ {l}\n"));
    removed.chain(added).collect()
}

/// `.mcp.json` with the `umr` server set (other servers and keys kept).
pub fn claude_mcp_json(old: Option<&str>, exe: &str, client: &str) -> Result<String, String> {
    let mut v: Value = match old {
        Some(t) if !t.trim().is_empty() => serde_json::from_str(t)
            .map_err(|e| format!(".mcp.json is not valid JSON ({e}); fix it first"))?,
        _ => json!({}),
    };
    if !v.is_object() {
        return Err(".mcp.json is not a JSON object".into());
    }
    if !v["mcpServers"].is_object() {
        v["mcpServers"] = json!({});
    }
    v["mcpServers"]["umr"] =
        json!({ "type": "stdio", "command": exe, "args": ["mcp", "--client", client], "env": {} });
    Ok(serde_json::to_string_pretty(&v).unwrap_or_default() + "\n")
}

/// Codex `config.toml` with the `[mcp_servers.umr]` table set (the rest kept as it is).
#[must_use]
pub fn codex_toml(old: Option<&str>, exe: &str, client: &str) -> String {
    let table = format!(
        "[mcp_servers.umr]\ncommand = '{exe}'\nargs = [\"mcp\", \"--client\", \"{client}\"]\nstartup_timeout_sec = 30\ntool_timeout_sec = 600\n"
    );
    let old = old.unwrap_or("");
    let lines: Vec<&str> = old.lines().collect();
    let start = lines.iter().position(|l| l.trim() == "[mcp_servers.umr]");
    match start {
        Some(i) => {
            let end = lines[i + 1..]
                .iter()
                .position(|l| l.trim_start().starts_with('['))
                .map_or(lines.len(), |j| i + 1 + j);
            let mut out: Vec<String> = lines[..i].iter().map(ToString::to_string).collect();
            out.extend(table.lines().map(String::from));
            if end < lines.len() {
                out.push(String::new());
            }
            out.extend(lines[end..].iter().map(ToString::to_string));
            out.join("\n") + "\n"
        }
        None if old.trim().is_empty() => table,
        None => format!("{}\n\n{table}", old.trim_end()),
    }
}

const BEGIN: &str = "<!-- umr:begin (written by `umr agent setup`; replaced on the next setup) -->";
const END: &str = "<!-- umr:end -->";

/// `AGENTS.md` with the runtime section set (marked; the rest kept).
#[must_use]
pub fn agents_md(old: Option<&str>) -> String {
    let section = format!(
        "{BEGIN}\n{}{END}\n",
        umr_mcp::instructions::skill_markdown(false)
    );
    let old = old.unwrap_or("");
    match (old.find(BEGIN), old.find(END)) {
        (Some(a), Some(b)) if b > a => format!(
            "{}{section}{}",
            &old[..a],
            old[b + END.len()..].trim_start_matches('\n')
        ),
        _ if old.trim().is_empty() => section,
        _ => format!("{}\n\n{section}", old.trim_end()),
    }
}

fn read(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn codex_home() -> Result<PathBuf, String> {
    if let Some(h) = std::env::var_os("CODEX_HOME").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(h));
    }
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|h| PathBuf::from(h).join(".codex"))
        .ok_or_else(|| "cannot find the home folder (set CODEX_HOME)".into())
}

/// What `setup` would write.
pub fn plan_setup(agent: &str, project: &Path, exe: &str) -> Result<Vec<Change>, String> {
    let mut out = Vec::new();
    match agent {
        "claude-code" => {
            let p = project.join(".mcp.json");
            let old = read(&p);
            out.push(Change {
                new: claude_mcp_json(old.as_deref(), exe, agent)?,
                path: p,
                old,
            });
            let p = project
                .join(".claude")
                .join("skills")
                .join("umr-runtime")
                .join("SKILL.md");
            let old = read(&p);
            out.push(Change {
                new: umr_mcp::instructions::skill_markdown(true),
                path: p,
                old,
            });
        }
        "codex" => {
            let p = codex_home()?.join("config.toml");
            let old = read(&p);
            out.push(Change {
                new: codex_toml(old.as_deref(), exe, agent),
                path: p,
                old,
            });
            let p = project.join("AGENTS.md");
            let old = read(&p);
            out.push(Change {
                new: agents_md(old.as_deref()),
                path: p,
                old,
            });
        }
        other => return Err(format!("unknown agent '{other}': claude-code or codex")),
    }
    out.retain(|c| c.old.as_deref() != Some(c.new.as_str()));
    Ok(out)
}

pub async fn setup(
    target: &CoreTarget,
    agent: &str,
    project: Option<PathBuf>,
    yes: bool,
    dry_run: bool,
) -> ExitCode {
    let project = project.unwrap_or_else(|| PathBuf::from("."));
    let project = PathBuf::from(crate::session_cmd::absolute(&project));
    if !project.is_dir() {
        return fail(&format!("{} is not a folder", project.display()));
    }
    let exe = match std::env::current_exe() {
        Ok(p) => p.display().to_string(),
        Err(e) => return fail(&format!("cannot locate umr.exe: {e}")),
    };
    let changes = match plan_setup(agent, &project, &exe) {
        Ok(c) => c,
        Err(e) => return fail(&e),
    };
    let has_client = token_path(target, agent).is_file();
    println!("Connecting {agent} to AI Mobile Runtime:");
    if !has_client {
        println!("  • create agent client '{agent}' (scopes: {}; new project folders and device switches ask you first)", AGENT_SCOPES.join(", "));
    }
    if changes.is_empty() {
        println!("  • its files are already up to date");
    }
    for c in &changes {
        println!(
            "\n{} {}",
            if c.old.is_some() { "change" } else { "create" },
            c.path.display()
        );
        print!("{}", diff(c.old.as_deref().unwrap_or(""), &c.new));
    }
    if dry_run {
        println!("\n(dry run: nothing written)");
        return ExitCode::SUCCESS;
    }
    if !has_client || !changes.is_empty() {
        if !yes {
            print!("\nApply? [y/N] ");
            let _ = std::io::stdout().flush();
            let mut answer = String::new();
            let _ = std::io::stdin().read_line(&mut answer);
            if !matches!(answer.trim(), "y" | "Y" | "yes") {
                println!("Nothing changed.");
                return ExitCode::SUCCESS;
            }
        }
        if let Err(e) = ensure_client(target, agent, &AGENT_SCOPES.map(String::from)).await {
            return fail(&e);
        }
        for c in &changes {
            if let Some(dir) = c.path.parent() {
                if let Err(e) = std::fs::create_dir_all(dir) {
                    return fail(&format!("{}: {e}", dir.display()));
                }
            }
            if let Err(e) = std::fs::write(&c.path, &c.new) {
                return fail(&format!("{}: {e}", c.path.display()));
            }
        }
    }
    match agent {
        "claude-code" => println!("\nDone. Start Claude Code in {} and approve the project MCP server `umr` when it asks (`/mcp` shows it).", project.display()),
        _ => println!("\nDone. Start Codex in {}; `/mcp` lists the `umr` server.", project.display()),
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_json_is_merged() {
        let old = r#"{"mcpServers": {"other": {"command": "x"}}, "keep": 1}"#;
        let v: Value = serde_json::from_str(
            &claude_mcp_json(Some(old), r"C:\u\umr.exe", "claude-code").unwrap(),
        )
        .unwrap();
        assert_eq!(v["mcpServers"]["other"]["command"], "x");
        assert_eq!(v["keep"], 1);
        assert_eq!(
            v["mcpServers"]["umr"]["args"],
            json!(["mcp", "--client", "claude-code"])
        );
        assert!(claude_mcp_json(Some("not json"), "x", "y").is_err());
    }

    #[test]
    fn codex_table_is_added_or_replaced() {
        let fresh = codex_toml(None, r"C:\u\umr.exe", "codex");
        assert!(fresh.starts_with("[mcp_servers.umr]\ncommand = 'C:\\u\\umr.exe'\n"));
        let old = "model = \"o3\"\n\n[mcp_servers.umr]\ncommand = 'old'\nargs = []\n\n[mcp_servers.other]\ncommand = 'x'\n";
        let new = codex_toml(Some(old), "new", "codex");
        assert!(new.starts_with("model = \"o3\"\n\n[mcp_servers.umr]\ncommand = 'new'\n"));
        assert!(new.contains("[mcp_servers.other]\ncommand = 'x'"));
        assert_eq!(new.matches("[mcp_servers.umr]").count(), 1);
        assert_eq!(codex_toml(Some(&new), "new", "codex"), new, "idempotent");
        let appended = codex_toml(Some("model = \"o3\"\n"), "e", "codex");
        assert!(appended.starts_with("model = \"o3\"\n\n[mcp_servers.umr]"));
    }

    #[test]
    fn agents_section_is_replaced_not_duplicated() {
        let once = agents_md(Some("# My project\nRules.\n"));
        assert!(once.starts_with("# My project\nRules.\n\n<!-- umr:begin"));
        let twice = agents_md(Some(&once));
        assert_eq!(once, twice);
        assert_eq!(twice.matches("umr:begin").count(), 1);
    }

    #[test]
    fn diff_shows_only_changed_lines() {
        assert_eq!(diff("a\nb\nc\n", "a\nx\nc\n"), "- b\n+ x\n");
        assert_eq!(diff("", "new\n"), "+ new\n");
    }

    #[test]
    fn plan_writes_into_the_project_and_is_empty_when_current() {
        let dir = std::env::temp_dir().join(format!("umr-agent-setup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let plan = plan_setup("claude-code", &dir, "umr.exe").unwrap();
        assert_eq!(plan.len(), 2);
        for c in &plan {
            std::fs::create_dir_all(c.path.parent().unwrap()).unwrap();
            std::fs::write(&c.path, &c.new).unwrap();
        }
        assert_eq!(
            plan_setup("claude-code", &dir, "umr.exe").unwrap(),
            Vec::new()
        );
        assert!(plan_setup("cursor", &dir, "umr.exe").is_err());
        let _ = std::fs::remove_dir_all(&dir);
        assert!(valid_name("claude-code") && !valid_name("a b") && !valid_name(""));
    }
}
