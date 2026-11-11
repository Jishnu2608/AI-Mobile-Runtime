//! Local API of the Runtime Core.
//!
//! - Transport: a Windows named pipe that only the current user can open
//!   (`\\.\pipe\umr-<SID>`; a hash suffix is added when `UMR_HOME` points elsewhere).
//! - Framing: one JSON message per line (`\n`-terminated), UTF-8, at most [`MAX_MESSAGE_BYTES`].
//! - Protocol: JSON-RPC 2.0 requests and responses (no batches, no notifications yet).
//!
//! Shared by the Core (server), the `umr` CLI, the desktop app and the MCP shim (clients).

pub mod client;
pub mod frame;
pub mod methods;
pub mod pipe_name;
pub mod rpc;

/// Version of this API contract; bumped on breaking changes.
pub const API_VERSION: u32 = 1;

/// Largest accepted message (one line) in bytes. Larger requests are rejected.
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
