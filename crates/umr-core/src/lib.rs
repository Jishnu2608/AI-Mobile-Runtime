//! Runtime Core of the AI Mobile Runtime.
//!
//! The Core is a per-user background service that owns all runtime state:
//! environment, devices, sessions, processes and diagnostics. The CLI, the
//! desktop app and the MCP server are thin clients of it.
//!
//! This crate is a skeleton (CORE-001). Lifecycle, API and storage follow in
//! CORE-002 to CORE-006.

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
