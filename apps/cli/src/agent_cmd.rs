//! `umr agent …` and `umr approvals` (Phase 6: MCP-003, MCP-004, MCP-005).
//!
//! * `agent add|list|remove|grant|ungrant|audit`: agent clients (own token, scopes, grants).
//! * `agent setup` / `agent doctor`: see `agent_setup.rs`.
//! * `approvals [allow|deny <id>]`: requests from agents waiting for the user.

use std::path::Path;
use std::process::ExitCode;

use serde_json::{json, Value};
use umr_api::methods;

use crate::core_process::CoreTarget;
use crate::fail;
use crate::mcp_cmd::token_path;

/// Scopes an agent gets by default: everything except `admin`.
pub const AGENT_SCOPES: [&str; 3] = ["observe", "interact", "control"];

pub(crate) async fn call(
    target: &CoreTarget,
    method: &str,
    params: Value,
) -> Result<Value, String> {
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

pub(crate) async fn find_client(target: &CoreTarget, name: &str) -> Result<Option<Value>, String> {
    let v = call(target, methods::AUTH_CLIENT_LIST, Value::Null).await?;
    Ok(v.as_array()
        .into_iter()
        .flatten()
        .find(|c| c["name"] == name)
        .cloned())
}

/// Creates the client and its token file. `Ok(false)` if it already exists.
pub(crate) async fn ensure_client(
    target: &CoreTarget,
    name: &str,
    scopes: &[String],
) -> Result<bool, String> {
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
        println!("No agent clients. See the agents on this PC with `umr agent clients`, then `umr agent setup <agent>`.");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_names_are_checked() {
        assert!(valid_name("claude-code") && !valid_name("a b") && !valid_name(""));
    }
}
