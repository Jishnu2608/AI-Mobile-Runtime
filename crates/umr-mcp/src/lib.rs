//! MCP server of the AI Mobile Runtime (Phase 6).
//!
//! * MCP-001 / MCP-006: the Model Context Protocol over stdio (newline-delimited JSON-RPC 2.0):
//!   `initialize`, `ping`, `tools/list`, `tools/call`. `umr mcp` connects it to the Core's named
//!   pipe with the agent's own client token (its scopes and grants apply).
//! * MCP-002: the tool catalogue ([`tools`]) mapped onto Core API methods; `runtime_status`
//!   keeps a cursor so each call returns only what changed (digest ≤ 1 KB).
//! * MCP-005: [`instructions`] — sent to the agent at `initialize`, and written as a skill /
//!   `AGENTS.md` section by `umr agent setup`.
//!
//! Generic over [`Core`] so the protocol and the tools are tested without a running Core.

pub mod instructions;
pub mod tools;

use std::future::Future;

use serde_json::{json, Value};

/// Protocol versions this server speaks (newest first).
pub const PROTOCOL_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// What the server needs from the Runtime Core.
pub trait Core {
    /// One Core API call; errors as text (shown to the agent).
    fn call(&mut self, method: &str, params: Value) -> impl Future<Output = Result<Value, String>>;

    /// Reads a file the Core wrote (screenshots).
    fn read_file(&self, path: &str) -> Result<Vec<u8>, String> {
        std::fs::read(path).map_err(|e| format!("{path}: {e}"))
    }
}

/// One MCP session (one agent process).
#[derive(Debug)]
pub struct Server<C> {
    pub core: C,
    /// The last `runtime_status` cursor.
    pub cursor: Option<String>,
    version: &'static str,
}

fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[allow(clippy::needless_pass_by_value)] // moved into the json! value
fn result(id: &Value, r: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": r })
}

impl<C: Core> Server<C> {
    pub fn new(core: C) -> Self {
        Self {
            core,
            cursor: None,
            version: PROTOCOL_VERSIONS[0],
        }
    }

    /// Handles one message; `None` for notifications (nothing to send back).
    pub async fn handle(&mut self, msg: &Value) -> Option<Value> {
        let id = msg.get("id")?.clone();
        let Some(method) = msg["method"].as_str() else {
            return Some(error(&id, -32600, "not a request"));
        };
        let params = &msg["params"];
        Some(match method {
            "initialize" => {
                let asked = params["protocolVersion"].as_str().unwrap_or("");
                self.version = PROTOCOL_VERSIONS
                    .iter()
                    .find(|v| **v == asked)
                    .copied()
                    .unwrap_or(PROTOCOL_VERSIONS[0]);
                result(
                    &id,
                    json!({
                        "protocolVersion": self.version,
                        "capabilities": { "tools": { "listChanged": false } },
                        "serverInfo": { "name": "umr", "title": "AI Mobile Runtime", "version": env!("CARGO_PKG_VERSION") },
                        "instructions": instructions::INSTRUCTIONS,
                    }),
                )
            }
            "ping" => result(&id, json!({})),
            "tools/list" => result(&id, json!({ "tools": tools::catalogue() })),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or("");
                let args = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                if !tools::exists(name) {
                    return Some(error(&id, -32602, &format!("unknown tool '{name}'")));
                }
                let r = match tools::run(self, name, &args).await {
                    Ok(content) => json!({ "content": content, "isError": false }),
                    Err(text) => json!({ "content": [tools::text(&text)], "isError": true }),
                };
                result(&id, r)
            }
            other => error(&id, -32601, &format!("method not found: {other}")),
        })
    }
}

/// Standard base64 (RFC 4648) for image content.
#[must_use]
pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= c.len() {
                out.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
