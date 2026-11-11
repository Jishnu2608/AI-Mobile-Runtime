//! Runtime Core of the AI Mobile Runtime.
//!
//! The Core is a per-user background service that owns all runtime state:
//! environment, devices, sessions, processes and diagnostics. The CLI, the
//! desktop app and the MCP server are thin clients of it.
//!
//! Implemented so far: managed paths, single-instance lock, logging, process
//! lifecycle (CORE-002) and the local JSON-RPC API over a user-only named pipe
//! (CORE-003). Tokens (CORE-004) and storage (CORE-005) follow.

pub mod api_server;
pub mod auth;
pub mod daemon;
pub mod doctor;
pub mod install_api;
pub mod instance;
pub mod logging;
pub mod paths;

#[cfg(test)]
pub(crate) mod test_support;

/// Product name shown to users.
pub const PRODUCT_NAME: &str = "AI Mobile Runtime";

/// Version of the Runtime Core, taken from the workspace manifest.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// One-line identification used by `--version` output and logs.
#[must_use]
pub fn version_line(binary: &str) -> String {
    format!("{binary} {VERSION} ({PRODUCT_NAME})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_line_contains_binary_version_and_product() {
        let line = version_line("umr-core");
        assert!(line.starts_with("umr-core "));
        assert!(line.contains(VERSION));
        assert!(line.contains(PRODUCT_NAME));
    }

    #[test]
    fn version_is_semver_like() {
        assert_eq!(VERSION.split('.').count(), 3);
    }
}
