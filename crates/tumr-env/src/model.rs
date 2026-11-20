//! Data types of the environment report (serialised into `env.scan` results and stored).

use serde::{Deserialize, Serialize};

/// Where a detected item came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Origin {
    /// Installed by the product in its managed folder.
    Managed,
    /// Named by an environment variable (`JAVA_HOME`, `ANDROID_HOME`, …).
    EnvVar,
    /// Found on PATH.
    Path,
    /// Found through the Windows registry.
    Registry,
    /// Found in a well-known install folder.
    CommonDir,
    /// Shipped inside Android Studio.
    AndroidStudio,
}

/// Result of a functional check (does the tool actually run?).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "detail", rename_all = "kebab-case")]
pub enum Health {
    /// Ran successfully.
    Ok,
    /// Present but not working (with the reason).
    Broken(String),
    /// Not checked (e.g. would be slow or have side effects).
    NotChecked(String),
}

impl Health {
    #[must_use]
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Virtualization {
    /// VT-x/AMD-V enabled in firmware (reads false while a hypervisor already owns it).
    pub firmware_enabled: bool,
    /// Windows runs on top of a hypervisor.
    pub hypervisor_present: bool,
    /// `usable` | `no-hypervisor` | `not-installed` | `error: …`
    pub whpx: String,
    pub aehd_installed: bool,
    /// Output of `emulator -accel-check`, when an emulator was found.
    pub emulator_accel_check: Option<String>,
    /// Overall: can the Android emulator use hardware acceleration?
    pub acceleration_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Proxy {
    pub http_proxy: Option<String>,
    pub https_proxy: Option<String>,
    pub no_proxy: Option<String>,
    /// `WinINet` ("Internet Options") proxy, if enabled.
    pub system_proxy: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostInfo {
    pub windows_build: u32,
    pub windows_display_version: Option<String>,
    pub windows_11: bool,
    pub arch: String,
    pub ram_total_bytes: u64,
    pub ram_available_bytes: u64,
    /// Free / total space on the volume of the data folder.
    pub disk_free_bytes: u64,
    pub disk_total_bytes: u64,
    pub long_paths_enabled: bool,
    pub virtualization: Virtualization,
    pub proxy: Proxy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Jdk {
    pub home: String,
    /// Feature version, e.g. 17, 21; 8 for 1.8.
    pub major: Option<u32>,
    /// Full version string, e.g. `17.0.21`.
    pub version: Option<String>,
    pub vendor: Option<String>,
    /// Has `javac` (a JDK, not just a JRE). Gradle needs a JDK.
    pub is_jdk: bool,
    pub origins: Vec<Origin>,
    pub health: Health,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SdkPackage {
    /// Package path as Android CLI writes it, e.g. `platforms/android-36`.
    pub path: String,
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AndroidSdk {
    pub root: String,
    pub origins: Vec<Origin>,
    pub packages: Vec<SdkPackage>,
    /// Accepted licence ids (files in `licenses/`).
    pub licenses: Vec<String>,
    pub adb: Health,
    pub emulator: Health,
    pub cmdline_tools: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub path: String,
    pub version: Option<String>,
    pub origins: Vec<Origin>,
    pub health: Health,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warn,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflict {
    pub id: String,
    pub severity: Severity,
    pub message: String,
    pub hint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileReadiness {
    /// `android-native` | `flutter-android` | `react-native-android` | `expo-go`
    pub profile: String,
    pub ready: bool,
    /// Human-readable list of what is missing.
    pub missing: Vec<String>,
}

/// The full environment report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvReport {
    pub scanned_unix: u64,
    pub duration_ms: u64,
    pub host: HostInfo,
    pub jdks: Vec<Jdk>,
    pub android_sdks: Vec<AndroidSdk>,
    pub tools: Vec<Tool>,
    pub conflicts: Vec<Conflict>,
    pub readiness: Vec<ProfileReadiness>,
}

impl EnvReport {
    /// First tool with this name.
    #[must_use]
    pub fn tool(&self, name: &str) -> Option<&Tool> {
        self.tools.iter().find(|t| t.name == name)
    }
}
