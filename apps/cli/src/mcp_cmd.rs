//! `tumr mcp` (MCP-001 / MCP-006): the MCP server on stdin/stdout for one agent, connected to
//! the Core's named pipe with that agent's own client token (`tumr agent setup` writes it).
//!
//! PLUG-002 (owner decision 2026-11-20, "auto-connect + notice"): an agent plugin starts this
//! server before `tumr agent setup` ever ran. Without a token it creates the agent's client
//! itself (default scopes, never admin) and puts "<agent> connected" on the home screen. A client
//! the user disconnected in the app ([`off_path`]) is not recreated.
//!
//! PLUG-004 (owner decision, "snap left, once"): the first `run_project` of an agent opens the
//! phone window docked to the left edge; a window that is already open stays where it is.
//! Only MCP messages go to stdout; diagnostics go to stderr. Ending this process (the agent
//! quits or is killed) leaves the Core, the device and the running app untouched.

use std::path::PathBuf;
use std::process::ExitCode;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tumr_api::client::Client;
use tumr_api::methods;
use tumr_mcp::{Core, Server};

use crate::core_process::CoreTarget;
use crate::fail;

/// Token file of an agent client.
pub fn token_path(target: &CoreTarget, name: &str) -> PathBuf {
    target
        .paths
        .agent_tokens_dir()
        .join(format!("{name}.token"))
}

/// Marker written when the user disconnects an agent in the app: `tumr mcp` then leaves it
/// disconnected instead of recreating its client.
pub fn off_path(target: &CoreTarget, name: &str) -> PathBuf {
    target.paths.agent_tokens_dir().join(format!("{name}.off"))
}

/// The token of `client`, created on first use (PLUG-002). `Err` explains what the user can do.
async fn token_or_connect(target: &CoreTarget, client: &str) -> Result<String, String> {
    let path = token_path(target, client);
    if let Ok(t) = std::fs::read_to_string(&path) {
        return Ok(t.trim().to_string());
    }
    if off_path(target, client).is_file() {
        return Err(format!(
            "'{client}' was disconnected in Toolchain Universal Mobile Runtime; connect it again on its Agents page (or `tumr agent add {client}`)"
        ));
    }
    let scopes: Vec<String> = crate::agent_cmd::AGENT_SCOPES
        .iter()
        .map(ToString::to_string)
        .collect();
    crate::agent_cmd::ensure_client(target, client, &scopes)
        .await
        .map_err(|e| format!("cannot connect '{client}' to the runtime: {e}"))?;
    let name = crate::agent_clients::find(client).map_or(client, |c| c.name);
    let _ = crate::agent_cmd::call(
        target,
        methods::AGENT_NOTE,
        json!({ "kind": "connected", "message": format!("{name} connected itself through its plugin") }),
    )
    .await;
    std::fs::read_to_string(&path)
        .map(|t| t.trim().to_string())
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// The Core through the pipe, reconnecting once if the Core restarted.
struct PipeCore {
    target: CoreTarget,
    token: String,
    client: Option<Client>,
    /// The phone window was opened (or found open) for this agent session.
    window: bool,
}

impl PipeCore {
    async fn client(&mut self) -> Result<&mut Client, String> {
        if self.client.is_none() {
            self.client = Some(self.target.connect_or_start_as(&self.token).await?);
        }
        self.client
            .as_mut()
            .ok_or_else(|| "not connected".to_string())
    }
}

impl Core for PipeCore {
    async fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        if method == methods::PROJECT_RUN && !self.window {
            self.window = true;
            if let Err(e) = crate::preview_cmd::open_window(&self.target, "left").await {
                eprintln!("tumr mcp: phone window: {e}");
            }
        }
        for attempt in 0..2 {
            let c = self.client().await?;
            match c.call_raw(method, params.clone()).await {
                Ok(v) => return Ok(v),
                Err(tumr_api::client::ClientError::Rpc(e)) => return Err(e.message),
                Err(e) if attempt == 0 => {
                    eprintln!("tumr mcp: connection lost ({e}); reconnecting");
                    self.client = None;
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        Err("the Runtime Core is not reachable".into())
    }
}

pub async fn run(target: &CoreTarget, client: &str) -> ExitCode {
    let token = match token_or_connect(target, client).await {
        Ok(t) => t,
        Err(e) => return fail(&e),
    };
    let mut server = Server::new(PipeCore {
        target: target.clone(),
        token,
        client: None,
        window: false,
    });
    // log in now so a bad token is reported at once (the agent shows the server as failed)
    if let Err(e) = server.core.client().await {
        return fail(&format!("cannot reach the Runtime Core as '{client}': {e}"));
    }
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut out = tokio::io::stdout();
    loop {
        let line = match lines.next_line().await {
            Ok(Some(l)) => l,
            Ok(None) => return ExitCode::SUCCESS,
            Err(e) => return fail(&format!("stdin: {e}")),
        };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => server.handle(&msg).await,
            Err(e) => Some(json!({ "jsonrpc": "2.0", "id": null,
                "error": { "code": -32700, "message": format!("parse error: {e}") } })),
        };
        if let Some(r) = reply {
            let mut text = serde_json::to_string(&r).unwrap_or_default();
            text.push('\n');
            if out.write_all(text.as_bytes()).await.is_err() || out.flush().await.is_err() {
                return ExitCode::SUCCESS; // the agent went away
            }
        }
    }
}
