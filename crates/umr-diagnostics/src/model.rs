//! The normalised diagnostic every collector's output becomes (DIA-002).

use serde::Serialize;
pub use umr_adapter::Severity;

/// The six categories of the PRD (FR-DIA-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// The machine or toolchain: missing/wrong SDK, JDK, licence, tool, port in use, out of memory.
    Environment,
    /// The project does not compile or package (compiler, resource, bundler, Gradle script errors).
    Build,
    /// The app runs but reported an error it survived (Flutter framework exception, JS error).
    Runtime,
    /// The app process died (uncaught exception, native signal, ANR).
    Crash,
    /// The device or emulator: offline, unauthorised, not found, out of storage, incompatible.
    Device,
    /// A host or service could not be reached (dependency download, Metro, DNS, TLS, proxy).
    Network,
}

/// Whether the user's own code is at fault (FR-DIA-04).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Located in the project's own sources (not `build/`, `node_modules/`, generated code).
    App,
    /// The toolchain, the device or the network — not something to fix in the app's code.
    Environment,
    /// Neither could be shown (e.g. a crash whose stack has only framework frames).
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    pub category: Category,
    pub origin: Origin,
    pub severity: Severity,
    /// Tool or collector that reported it: `kotlinc`, `gradle`, `dart`, `metro`, `logcat`, ...
    pub source: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stack: Vec<String>,
    /// Stable identity of "the same problem" ([`crate::fingerprint`]): equal for repeats.
    pub fingerprint: String,
}
