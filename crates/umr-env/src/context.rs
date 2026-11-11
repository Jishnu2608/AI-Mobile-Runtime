//! Inputs of a scan: environment variables and folders, injectable for tests.

use std::collections::HashMap;
use std::path::PathBuf;

/// Everything probes need to know about the machine's configuration.
#[derive(Debug, Clone)]
pub struct ScanContext {
    /// Snapshot of environment variables (keys upper-cased; Windows env vars are case-insensitive).
    pub env: HashMap<String, String>,
    /// The product's data/managed root (`%LOCALAPPDATA%\UMR`); managed SDKs live below it.
    pub managed_root: PathBuf,
    /// `%LOCALAPPDATA%`.
    pub local_app_data: Option<PathBuf>,
    /// `C:\Program Files` and `C:\Program Files (x86)`.
    pub program_files: Vec<PathBuf>,
    /// Whether to run tools for functional checks (false in fast unit tests).
    pub run_checks: bool,
    /// Whether to consult the real registry (false in unit tests).
    pub use_registry: bool,
}

impl ScanContext {
    /// Context for the real machine.
    #[must_use]
    pub fn from_system(managed_root: PathBuf) -> Self {
        let env: HashMap<String, String> = std::env::vars()
            .map(|(k, v)| (k.to_uppercase(), v))
            .collect();
        let local_app_data = umr_platform::folders::local_app_data()
            .ok()
            .or_else(|| env.get("LOCALAPPDATA").map(PathBuf::from));
        let program_files = ["PROGRAMFILES", "PROGRAMFILES(X86)", "PROGRAMW6432"]
            .iter()
            .filter_map(|k| env.get(*k).map(PathBuf::from))
            .fold(Vec::new(), |mut acc, p| {
                if !acc.contains(&p) {
                    acc.push(p);
                }
                acc
            });
        Self {
            env,
            managed_root,
            local_app_data,
            program_files,
            run_checks: true,
            use_registry: true,
        }
    }

    /// Value of an environment variable (case-insensitive), ignoring empty values.
    #[must_use]
    pub fn var(&self, name: &str) -> Option<&str> {
        self.env
            .get(&name.to_uppercase())
            .map(String::as_str)
            .filter(|v| !v.trim().is_empty())
    }

    /// Entries of PATH, in order, without empty entries or surrounding quotes.
    #[must_use]
    pub fn path_entries(&self) -> Vec<PathBuf> {
        self.var("PATH")
            .map(|p| {
                p.split(';')
                    .map(|s| s.trim().trim_matches('"'))
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Test helper: an empty context with the given variables.
    #[must_use]
    pub fn for_test(managed_root: PathBuf, vars: &[(&str, &str)]) -> Self {
        Self {
            env: vars
                .iter()
                .map(|(k, v)| (k.to_uppercase(), (*v).to_string()))
                .collect(),
            managed_root,
            local_app_data: None,
            program_files: Vec::new(),
            run_checks: false,
            use_registry: false,
        }
    }
}
