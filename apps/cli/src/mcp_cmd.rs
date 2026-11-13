//! `umr mcp` (MCP-001 / MCP-006): the MCP server on stdin/stdout for one agent, connected to
//! the Core's named pipe with that agent's own client token (`umr agent setup` writes it).
//! Only MCP messages go to stdout; diagnostics go to stderr. Ending this process (the agent
//! quits or is killed) leaves the Core, the device and the running app untouched.

use std::path::PathBuf;
use std::process::ExitCode;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use umr_api::client::Client;
use umr_mcp::{Core, Server};

use crate::core_process::CoreTarget;
use crate::fail;

/// Token file of an agent client.
pub fn token_path(target: &CoreTarget, name: &str) -> PathBuf {
    target
        .paths
        .agent_tokens_dir()
        .join(format!("{name}.token"))
}

/// The Core through the pipe, reconnecting once if the Core restarted.
struct PipeCore {
    target: CoreTarget,
    token: String,
    client: Option<Client>,
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
        for attempt in 0..2 {
            let c = self.client().await?;
            match c.call_raw(method, params.clone()).await {
                Ok(v) => return Ok(v),
                Err(umr_api::client::ClientError::Rpc(e)) => return Err(e.message),
                Err(e) if attempt == 0 => {
                    eprintln!("umr mcp: connection lost ({e}); reconnecting");
                    self.client = None;
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        Err("the Runtime Core is not reachable".into())
    }
}

pub async fn run(target: &CoreTarget, client: &str) -> ExitCode {
    let path = token_path(target, client);
    let token = match std::fs::read_to_string(&path) {
        Ok(t) => t.trim().to_string(),
        Err(_) => {
            return fail(&format!(
                "no agent client '{client}' ({} missing); create it with `umr agent setup claude-code|codex` or `umr agent add {client}`",
                path.display()
            ))
        }
    };
    let mut server = Server::new(PipeCore {
        target: target.clone(),
        token,
        client: None,
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
