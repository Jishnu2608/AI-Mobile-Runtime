//! ENV-006: readiness per environment profile (what is missing to build and preview).
//!
//! Minimum requirements are deliberately coarse here; exact versions per project come from the
//! resolver and Knowledge Pack (RES-001…005). A profile is "ready" when a working toolchain of
//! each required kind exists, regardless of where it came from.

use crate::model::{EnvReport, ProfileReadiness};

/// Minimum JDK feature version for current Android Gradle Plugin releases.
pub const MIN_JDK_MAJOR: u32 = 17;
/// Minimum free disk space recommended for a first emulator setup (system image + build caches).
pub const MIN_FREE_DISK_BYTES: u64 = 20 * 1024 * 1024 * 1024;

#[derive(Debug, Default)]
struct Facts {
    windows11: bool,
    accel: bool,
    disk_ok: bool,
    jdk: bool,
    sdk_platform_tools: bool,
    sdk_platform: bool,
    sdk_build_tools: bool,
    emulator: bool,
    system_image: bool,
    flutter: bool,
    node: bool,
    git: bool,
}

fn facts(r: &EnvReport) -> Facts {
    let has_pkg = |prefix: &str| {
        r.android_sdks
            .iter()
            .any(|s| s.packages.iter().any(|p| p.path.starts_with(prefix)))
    };
    Facts {
        windows11: r.host.windows_11,
        accel: r.host.virtualization.acceleration_available,
        disk_ok: r.host.disk_free_bytes >= MIN_FREE_DISK_BYTES,
        jdk: r
            .jdks
            .iter()
            .any(|j| j.is_jdk && j.health.is_ok() && j.major.is_some_and(|m| m >= MIN_JDK_MAJOR)),
        sdk_platform_tools: r.android_sdks.iter().any(|s| s.adb.is_ok()),
        sdk_platform: has_pkg("platforms/"),
        sdk_build_tools: has_pkg("build-tools/"),
        emulator: r.android_sdks.iter().any(|s| s.emulator.is_ok()),
        system_image: has_pkg("system-images/"),
        flutter: r.tools.iter().any(|t| t.name == "flutter"),
        node: r.tools.iter().any(|t| t.name == "node" && t.health.is_ok()),
        git: r.tools.iter().any(|t| t.name == "git" && t.health.is_ok()),
    }
}

/// Computes readiness for all MVP profiles.
#[must_use]
pub fn evaluate(r: &EnvReport) -> Vec<ProfileReadiness> {
    let f = facts(r);
    let base: Vec<(bool, &str)> = vec![
        (f.windows11, "Windows 11"),
        (
            f.accel,
            "hardware acceleration for the emulator (WHPX or AEHD)",
        ),
        (f.disk_ok, "at least 20 GB free disk space"),
        (f.sdk_platform_tools, "Android SDK platform-tools (adb)"),
        (f.emulator, "Android Emulator"),
        (f.system_image, "an Android system image"),
    ];
    let native_build: Vec<(bool, &str)> = vec![
        (f.jdk, "JDK 17 or newer"),
        (f.sdk_platform, "an Android SDK platform"),
        (f.sdk_build_tools, "Android SDK build-tools"),
    ];
    let profile = |name: &str, extra: &[(bool, &str)], include_native: bool| {
        let missing: Vec<String> = base
            .iter()
            .chain(if include_native {
                native_build.iter()
            } else {
                [].iter()
            })
            .chain(extra.iter())
            .filter(|(ok, _)| !ok)
            .map(|(_, what)| (*what).to_string())
            .collect();
        ProfileReadiness {
            profile: name.into(),
            ready: missing.is_empty(),
            missing,
        }
    };
    vec![
        profile("android-native", &[], true),
        profile(
            "flutter-android",
            &[
                (f.flutter, "Flutter SDK"),
                (f.git, "Git (Flutter needs it)"),
            ],
            true,
        ),
        profile("react-native-android", &[(f.node, "Node.js")], true),
        profile("expo-go", &[(f.node, "Node.js")], false),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    pub(crate) fn empty_report() -> EnvReport {
        EnvReport {
            scanned_unix: 0,
            duration_ms: 0,
            host: HostInfo {
                windows_build: 26100,
                windows_display_version: None,
                windows_11: true,
                arch: "x86_64".into(),
                ram_total_bytes: 32 << 30,
                ram_available_bytes: 16 << 30,
                disk_free_bytes: 100 << 30,
                disk_total_bytes: 500 << 30,
                long_paths_enabled: true,
                virtualization: Virtualization {
                    firmware_enabled: false,
                    hypervisor_present: true,
                    whpx: "usable".into(),
                    aehd_installed: false,
                    emulator_accel_check: None,
                    acceleration_available: true,
                },
                proxy: Proxy::default(),
            },
            jdks: vec![],
            android_sdks: vec![],
            tools: vec![],
            conflicts: vec![],
            readiness: vec![],
        }
    }

    fn get<'a>(v: &'a [ProfileReadiness], name: &str) -> &'a ProfileReadiness {
        v.iter().find(|p| p.profile == name).unwrap()
    }

    #[test]
    fn empty_machine_misses_sdk_and_jdk() {
        let r = evaluate(&empty_report());
        let native = get(&r, "android-native");
        assert!(!native.ready);
        assert!(native.missing.iter().any(|m| m.contains("JDK 17")));
        assert!(native.missing.iter().any(|m| m.contains("platform-tools")));
        assert!(
            !get(&r, "expo-go").missing.iter().any(|m| m.contains("JDK")),
            "Expo Go needs no JDK"
        );
    }

    #[test]
    fn complete_machine_is_ready_for_all_profiles() {
        let mut rep = empty_report();
        rep.jdks.push(Jdk {
            home: "j".into(),
            major: Some(17),
            version: Some("17.0.21".into()),
            vendor: None,
            is_jdk: true,
            origins: vec![Origin::Managed],
            health: Health::Ok,
        });
        rep.android_sdks.push(AndroidSdk {
            root: "s".into(),
            origins: vec![Origin::Managed],
            packages: [
                "platforms/android-36",
                "build-tools/36.0.0",
                "system-images/android-36/google_apis/x86_64",
            ]
            .iter()
            .map(|p| SdkPackage {
                path: (*p).into(),
                version: None,
            })
            .collect(),
            licenses: vec![],
            adb: Health::Ok,
            emulator: Health::Ok,
            cmdline_tools: true,
        });
        for name in ["flutter", "node", "git"] {
            rep.tools.push(Tool {
                name: name.into(),
                path: "p".into(),
                version: None,
                origins: vec![Origin::Path],
                health: Health::Ok,
            });
        }
        assert!(
            evaluate(&rep).iter().all(|p| p.ready),
            "{:#?}",
            evaluate(&rep)
        );
    }

    #[test]
    fn jre_or_old_jdk_does_not_count() {
        let mut rep = empty_report();
        rep.jdks.push(Jdk {
            home: "j".into(),
            major: Some(11),
            version: None,
            vendor: None,
            is_jdk: true,
            origins: vec![Origin::Path],
            health: Health::Ok,
        });
        assert!(get(&evaluate(&rep), "android-native")
            .missing
            .iter()
            .any(|m| m.contains("JDK 17")));
    }
}
