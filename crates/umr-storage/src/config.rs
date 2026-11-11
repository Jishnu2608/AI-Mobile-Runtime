//! User configuration: `<root>/config.toml`.
//!
//! Missing file or missing keys mean defaults. Unknown keys are rejected so typos are not
//! silently ignored. Privacy-relevant settings default to off.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// File name inside the data root.
pub const CONFIG_FILE: &str = "config.toml";

/// Whole configuration file.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub core: CoreConfig,
    pub privacy: PrivacyConfig,
}

/// `[core]` section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CoreConfig {
    /// Log filter, e.g. `info`, `debug`, `umr_core=trace`. The `UMR_LOG` environment variable wins.
    pub log: String,
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self { log: "info".into() }
    }
}

/// `[privacy]` section.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PrivacyConfig {
    /// Anonymous usage statistics. Off unless the user turns it on (master spec §11).
    pub telemetry: bool,
}

/// Config loading errors.
#[derive(Debug)]
pub enum ConfigError {
    Io(PathBuf, std::io::Error),
    Parse(PathBuf, String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(p, e) => write!(f, "cannot read {}: {e}", p.display()),
            Self::Parse(p, e) => write!(f, "invalid {}: {e}", p.display()),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Commented template written on first start, so users can see the available settings.
pub const DEFAULT_TEMPLATE: &str = r#"# AI Mobile Runtime configuration.
# Delete a line to go back to its default.

[core]
# Log level: error | warn | info | debug | trace (the UMR_LOG environment variable overrides this)
log = "info"

[privacy]
# Anonymous usage statistics. Off by default.
telemetry = false
"#;

impl Config {
    /// Parses TOML text.
    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }

    /// Loads `<root>/config.toml`; a missing file yields the defaults.
    pub fn load_in(root: &Path) -> Result<Self, ConfigError> {
        let path = root.join(CONFIG_FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text).map_err(|e| ConfigError::Parse(path, e)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(ConfigError::Io(path, e)),
        }
    }

    /// Writes the commented default template if no config file exists yet. Returns whether it wrote one.
    pub fn write_template_if_missing(root: &Path) -> std::io::Result<bool> {
        let path = root.join(CONFIG_FILE);
        if path.exists() {
            return Ok(false);
        }
        std::fs::create_dir_all(root)?;
        std::fs::write(&path, DEFAULT_TEMPLATE)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_safe() {
        let c = Config::default();
        assert_eq!(c.core.log, "info");
        assert!(!c.privacy.telemetry, "telemetry must default to off");
    }

    #[test]
    fn template_parses_to_defaults() {
        assert_eq!(Config::parse(DEFAULT_TEMPLATE).unwrap(), Config::default());
    }

    #[test]
    fn partial_file_keeps_other_defaults() {
        let c = Config::parse("[core]\nlog = \"debug\"\n").unwrap();
        assert_eq!(c.core.log, "debug");
        assert!(!c.privacy.telemetry);
    }

    #[test]
    fn unknown_keys_and_wrong_types_are_errors() {
        assert!(Config::parse("[core]\nlgo = \"debug\"\n").is_err());
        assert!(Config::parse("[privacy]\ntelemetry = \"yes\"\n").is_err());
        assert!(Config::parse("[nonsense]\n").is_err());
    }

    #[test]
    fn load_missing_file_gives_defaults_and_template_is_written_once() {
        let dir = std::env::temp_dir().join(format!("umr-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(Config::load_in(&dir).unwrap(), Config::default());
        assert!(Config::write_template_if_missing(&dir).unwrap());
        assert!(!Config::write_template_if_missing(&dir).unwrap());
        std::fs::write(dir.join(CONFIG_FILE), "[core]\nlog = \"warn\"\n").unwrap();
        assert_eq!(Config::load_in(&dir).unwrap().core.log, "warn");
        std::fs::write(dir.join(CONFIG_FILE), "not = [toml").unwrap();
        assert!(matches!(Config::load_in(&dir), Err(ConfigError::Parse(..))));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
