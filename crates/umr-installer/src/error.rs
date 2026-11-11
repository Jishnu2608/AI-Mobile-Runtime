//! Installer errors.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "kebab-case")]
pub enum InstallError {
    /// Network problem (connect, TLS, reset) after all retries.
    Network(String),
    /// Unexpected HTTP status.
    Http {
        status: u16,
        url: String,
    },
    /// URL is not HTTPS or not on the official-host allowlist.
    HostNotAllowed(String),
    /// Download does not match the published checksum.
    Checksum {
        expected: String,
        actual: String,
    },
    /// Archive is unsafe or broken.
    Extract(String),
    /// Official metadata could not be read or has no matching file.
    Metadata(String),
    /// A tool (Android CLI) failed.
    Tool(String),
    /// Licences that must be accepted first.
    LicenceNotAccepted(Vec<String>),
    /// Not enough free disk space.
    Disk(String),
    /// Component cannot be installed automatically yet.
    Unsupported(String),
    Io(String),
    Storage(String),
    Cancelled,
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network(e) => write!(f, "network error: {e}"),
            Self::Http { status, url } => write!(f, "HTTP {status} for {url}"),
            Self::HostNotAllowed(u) => write!(
                f,
                "refusing to download from {u} (not an official HTTPS source)"
            ),
            Self::Checksum { expected, actual } => {
                write!(f, "checksum mismatch: expected {expected}, got {actual}")
            }
            Self::Extract(e) => write!(f, "cannot extract archive: {e}"),
            Self::Metadata(e) => write!(f, "release metadata: {e}"),
            Self::Tool(e) | Self::Disk(e) | Self::Unsupported(e) => write!(f, "{e}"),
            Self::LicenceNotAccepted(ids) => {
                write!(f, "licence not accepted yet: {}", ids.join(", "))
            }
            Self::Io(e) => write!(f, "file error: {e}"),
            Self::Storage(e) => write!(f, "database error: {e}"),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::error::Error for InstallError {}

impl From<std::io::Error> for InstallError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

impl From<umr_storage::StorageError> for InstallError {
    fn from(e: umr_storage::StorageError) -> Self {
        Self::Storage(e.to_string())
    }
}
