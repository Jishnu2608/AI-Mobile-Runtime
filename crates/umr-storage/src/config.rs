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
    pub storage: StorageConfig,
    pub device: DeviceConfig,
    pub update: UpdateConfig,
}

/// `[update]` section (REL-002).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UpdateConfig {
    /// Release manifest (`releases.json`). Empty = no update checks. HTTPS only (plain HTTP
    /// is accepted for 127.0.0.1 / localhost, for testing).
    pub manifest_url: String,
    /// `stable`, or `beta` (also offers stable releases).
    pub channel: String,
    /// Refuse an installer without a valid Authenticode signature.
    pub require_signed: bool,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            manifest_url: String::new(),
            channel: "stable".into(),
            require_signed: false,
        }
    }
}

/// `[device]` section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DeviceConfig {
    /// Port of the product's own adb server (never 5037, which belongs to Android Studio and the
    /// user's `adb`). If taken by another program, the next free port is used.
    pub adb_port: u16,
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self { adb_port: 5137 }
    }
}

/// `[storage]` section.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StorageConfig {
    /// Folder for toolchains (JDK, Android SDK, images, AVDs, Node, Flutter, downloads).
    /// Empty = inside the home folder. Must be an absolute path.
    pub toolchains_dir: String,
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

[storage]
# Where toolchains go (JDK, Android SDK, emulator images, AVDs, Node, Flutter, downloads).
# Several GB. Empty = inside this folder. Example: toolchains_dir = 'D:\UMR'
toolchains_dir = ""

[device]
# Port of the product's own adb server. 5037 (Android Studio / your own adb) is never used.
adb_port = 5137

[update]
# Release manifest checked by `umr update`. Empty = no update checks.
manifest_url = ""
# stable | beta
channel = "stable"
# Only install signed updates.
require_signed = false
"#;

impl Config {
    /// Toolchains folder for a home folder (`[storage] toolchains_dir`, else the home itself).
    /// A relative `toolchains_dir` is rejected (returns an error message).
    pub fn toolchains_root(&self, home: &Path) -> Result<PathBuf, String> {
        let t = self.storage.toolchains_dir.trim();
        if t.is_empty() {
            return Ok(home.to_path_buf());
        }
        let p = PathBuf::from(t);
        if p.is_absolute() {
            Ok(p)
        } else {
            Err(format!(
                "[storage] toolchains_dir must be an absolute path, got '{t}'"
            ))
        }
    }

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
    fn toolchains_dir_setting() {
        let home = Path::new(r"C:\Users\u\.umr");
        let c = Config::default();
        assert_eq!(c.toolchains_root(home).unwrap(), home);
        let c = Config::parse("[storage]\ntoolchains_dir = 'D:\\UMR'\n").unwrap();
        assert_eq!(c.toolchains_root(home).unwrap(), Path::new(r"D:\UMR"));
        let c = Config::parse("[storage]\ntoolchains_dir = 'relative'\n").unwrap();
        assert!(c.toolchains_root(home).is_err());
        assert!(
            Config::parse("[storage]\ntoolchain_dir = 'x'\n").is_err(),
            "typos rejected"
        );
        assert_eq!(Config::parse(DEFAULT_TEMPLATE).unwrap(), Config::default());
    }

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
