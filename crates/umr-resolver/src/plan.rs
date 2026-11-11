//! RES-004: the install plan — for each requirement, reuse an existing working install or
//! install a managed one; with approximate sizes, licences that need acceptance and admin
//! actions (Windows Hypervisor Platform, long paths). Read-only: nothing is installed here.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use umr_env::model::{AndroidSdk, EnvReport};

use crate::pack::Pack;
use crate::resolve::{Requirement, Resolution, ResolveConflict};
use crate::version::Version;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Action {
    /// A working, compatible install exists; it is used read-only.
    Reuse {
        path: String,
        version: Option<String>,
    },
    /// Download into the managed folder.
    Install,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanItem {
    pub component: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Lowest acceptable version (e.g. Flutter from pubspec), when the project gives one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<String>,
    pub action: Action,
    /// Approximate; 0 when reused.
    pub approx_download_bytes: u64,
    pub approx_disk_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub licence: Option<String>,
    pub reason: String,
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanLicence {
    pub id: String,
    pub title: String,
    pub url: String,
    pub requires_acceptance: bool,
}

/// A step that needs administrator rights (done only with explicit consent, never silently).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminAction {
    pub id: String,
    pub title: String,
    pub reason: String,
    pub reboot: bool,
    /// Without it the plan cannot reach a working preview.
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub project: String,
    pub profile: String,
    pub pack_version: String,
    pub items: Vec<PlanItem>,
    pub licences: Vec<PlanLicence>,
    pub admin_actions: Vec<AdminAction>,
    pub conflicts: Vec<ResolveConflict>,
    pub notes: Vec<String>,
    pub total_download_bytes: u64,
    pub total_disk_bytes: u64,
    pub disk_free_bytes: u64,
    /// Enough free space for the approximate total plus 10 %.
    pub disk_ok: bool,
    /// The one Android SDK folder all SDK packages come from (`None` without Android needs).
    /// A user SDK is reused read-only only when it already has every package; otherwise the
    /// managed SDK is used, so the emulator, platform and image always live together.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub android_sdk: Option<String>,
}

fn is_sdk_component(component: &str) -> bool {
    component.starts_with("android-")
}

fn sdk_has(s: &AndroidSdk, r: &Requirement) -> Option<Action> {
    if r.component == "android-cmdline-tools" {
        return s.cmdline_tools.then(|| Action::Reuse {
            path: format!("{}\\cmdline-tools", s.root),
            version: None,
        });
    }
    let pkg = r.package.as_ref()?;
    let p = s.packages.iter().find(|p| &p.path == pkg)?;
    let healthy = match pkg.as_str() {
        "platform-tools" => s.adb.is_ok(),
        "emulator" => s.emulator.is_ok(),
        _ => true,
    };
    healthy.then(|| Action::Reuse {
        path: format!("{}\\{}", s.root, pkg.replace('/', "\\")),
        version: p.version.clone(),
    })
}

fn same_dir(a: &str, b: &str) -> bool {
    let n = |s: &str| {
        s.trim_end_matches(['\\', '/'])
            .replace('/', "\\")
            .to_lowercase()
    };
    n(a) == n(b)
}

/// Picks the SDK root: a complete existing SDK, else the managed one.
fn choose_sdk<'e>(
    res: &Resolution,
    env: &'e EnvReport,
    managed_sdk: &str,
) -> Option<(String, Option<&'e AndroidSdk>)> {
    let needed: Vec<&Requirement> = res
        .requirements
        .iter()
        .filter(|r| is_sdk_component(&r.component))
        .collect();
    if needed.is_empty() {
        return None;
    }
    if let Some(s) = env
        .android_sdks
        .iter()
        .filter(|s| !same_dir(&s.root, managed_sdk))
        .find(|s| needed.iter().all(|r| sdk_has(s, r).is_some()))
    {
        return Some((s.root.clone(), Some(s)));
    }
    Some((
        managed_sdk.to_string(),
        env.android_sdks
            .iter()
            .find(|s| same_dir(&s.root, managed_sdk)),
    ))
}

/// Builds the plan for a resolution against the scanned environment.
/// `managed_sdk` is the managed Android SDK folder (`<root>\android-sdk`).
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn build(
    project: &str,
    res: &Resolution,
    env: &EnvReport,
    pack: &Pack,
    managed_sdk: &str,
) -> Plan {
    let sdk = choose_sdk(res, env, managed_sdk);
    let mut items = Vec::new();
    for r in &res.requirements {
        let comp = pack.components.get(&r.component);
        let action = if is_sdk_component(&r.component) {
            sdk.as_ref()
                .and_then(|(_, s)| s.and_then(|s| sdk_has(s, r)))
                .unwrap_or(Action::Install)
        } else {
            find_reusable(r, env)
        };
        let install = matches!(action, Action::Install);
        items.push(PlanItem {
            component: r.component.clone(),
            title: comp.map_or_else(|| r.component.clone(), |c| c.title.clone()),
            package: r.package.clone(),
            min: r.min.clone(),
            version: match &action {
                Action::Reuse {
                    version: Some(v), ..
                } => Some(v.clone()),
                _ => r.version.clone(),
            },
            approx_download_bytes: if install {
                comp.map_or(0, |c| c.approx_download_bytes)
            } else {
                0
            },
            approx_disk_bytes: if install {
                comp.map_or(0, |c| c.approx_disk_bytes)
            } else {
                0
            },
            licence: if install {
                comp.and_then(|c| c.licence.clone())
            } else {
                None
            },
            action,
            reason: r.reason.clone(),
            sources: r.sources.clone(),
        });
    }

    let ids: BTreeSet<&String> = items.iter().filter_map(|i| i.licence.as_ref()).collect();
    let licences = ids
        .into_iter()
        .filter_map(|id| {
            pack.licences.get(id).map(|l| PlanLicence {
                id: id.clone(),
                title: l.title.clone(),
                url: l.url.clone(),
                requires_acceptance: l.requires_acceptance,
            })
        })
        .collect();

    let mut admin_actions = Vec::new();
    let v = &env.host.virtualization;
    if !v.acceleration_available {
        let (title, reason) = if v.whpx == "not-installed" || v.whpx == "no-hypervisor" {
            ("Turn on Windows Hypervisor Platform", "the Android emulator needs hardware acceleration (WHPX); this is a Windows optional feature")
        } else {
            ("Enable CPU virtualization", "the Android emulator needs hardware acceleration; enable VT-x / AMD-V in the firmware (BIOS/UEFI) settings")
        };
        admin_actions.push(AdminAction {
            id: "enable-whpx".into(),
            title: title.into(),
            reason: reason.into(),
            reboot: true,
            required: true,
        });
    }
    if !env.host.long_paths_enabled
        && matches!(
            res.profile.as_str(),
            "react-native-android" | "flutter-android" | "expo-go"
        )
    {
        admin_actions.push(AdminAction {
            id: "enable-long-paths".into(),
            title: "Enable Win32 long paths".into(),
            reason: "node_modules and native builds can exceed 260 characters".into(),
            reboot: false,
            required: false,
        });
    }

    let total_download_bytes = items.iter().map(|i| i.approx_download_bytes).sum();
    let total_disk_bytes: u64 = items.iter().map(|i| i.approx_disk_bytes).sum();
    let disk_free_bytes = env.host.disk_free_bytes;
    Plan {
        project: project.into(),
        profile: res.profile.clone(),
        pack_version: res.pack_version.clone(),
        items,
        licences,
        admin_actions,
        conflicts: res.conflicts.clone(),
        notes: res.notes.clone(),
        total_download_bytes,
        total_disk_bytes,
        disk_free_bytes,
        disk_ok: disk_free_bytes >= total_disk_bytes + total_disk_bytes / 10,
        android_sdk: sdk.map(|(root, _)| root),
    }
}

fn find_reusable(r: &Requirement, env: &EnvReport) -> Action {
    let min = r.min.as_deref().and_then(Version::parse);
    let max = r.max.as_deref().and_then(Version::parse);
    let fits = |v: &Version| {
        min.as_ref().is_none_or(|m| v >= m) && max.as_ref().is_none_or(|m| v.major() <= m.major())
    };
    match r.component.as_str() {
        "jdk" => {
            let preferred = r.version.as_deref().and_then(|v| v.parse::<u32>().ok());
            let mut ok: Vec<_> = env
                .jdks
                .iter()
                .filter(|j| j.is_jdk && j.health.is_ok())
                .filter(|j| j.major.is_some_and(|m| fits(&Version(vec![u64::from(m)]))))
                .collect();
            ok.sort_by_key(|j| (j.major != preferred, std::cmp::Reverse(j.major)));
            ok.first().map_or(Action::Install, |j| Action::Reuse {
                path: j.home.clone(),
                version: j.version.clone(),
            })
        }
        "node" | "git" | "flutter" => env
            .tool(&r.component)
            .filter(|t| t.health.is_ok())
            .filter(|t| {
                t.version
                    .as_deref()
                    .and_then(Version::parse)
                    .is_some_and(|v| fits(&v))
                    || (min.is_none() && max.is_none())
            })
            .map_or(Action::Install, |t| Action::Reuse {
                path: t.path.clone(),
                version: t.version.clone(),
            }),
        // SDK components are handled by `choose_sdk` / `sdk_has`.
        _ => Action::Install,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{AndroidFacts, Framework, ProjectFacts};
    use umr_env::model::{
        AndroidSdk, Health, HostInfo, Jdk, Origin, Proxy, SdkPackage, Tool, Virtualization,
    };

    const MANAGED: &str = r"C:\UMR\android-sdk";

    fn pack() -> Pack {
        serde_json::from_str(crate::pack::BUNDLED_PACK).unwrap()
    }

    fn env(accel: bool) -> EnvReport {
        EnvReport {
            scanned_unix: 0,
            duration_ms: 0,
            host: HostInfo {
                windows_build: 26300,
                windows_display_version: None,
                windows_11: true,
                arch: "x86_64".into(),
                ram_total_bytes: 16 << 30,
                ram_available_bytes: 8 << 30,
                disk_free_bytes: 100 << 30,
                disk_total_bytes: 500 << 30,
                long_paths_enabled: false,
                virtualization: Virtualization {
                    firmware_enabled: true,
                    hypervisor_present: accel,
                    whpx: if accel {
                        "usable".into()
                    } else {
                        "not-installed".into()
                    },
                    aehd_installed: false,
                    emulator_accel_check: None,
                    acceleration_available: accel,
                },
                proxy: Proxy::default(),
            },
            jdks: vec![Jdk {
                home: "C:\\jdk21".into(),
                major: Some(21),
                version: Some("21.0.5".into()),
                vendor: None,
                is_jdk: true,
                origins: vec![Origin::Path],
                health: Health::Ok,
            }],
            android_sdks: vec![AndroidSdk {
                root: "C:\\Sdk".into(),
                origins: vec![Origin::CommonDir],
                packages: vec![
                    SdkPackage {
                        path: "platform-tools".into(),
                        version: Some("36.0.0".into()),
                    },
                    SdkPackage {
                        path: "platforms/android-36".into(),
                        version: Some("2".into()),
                    },
                ],
                licenses: vec![],
                adb: Health::Ok,
                emulator: Health::Broken("missing".into()),
                cmdline_tools: false,
            }],
            tools: vec![Tool {
                name: "node".into(),
                path: "C:\\node\\node.exe".into(),
                version: Some("24.15.0".into()),
                origins: vec![Origin::Path],
                health: Health::Ok,
            }],
            conflicts: vec![],
            readiness: vec![],
        }
    }

    fn native() -> ProjectFacts {
        ProjectFacts {
            root: "C:\\p".into(),
            framework: Framework::AndroidNative,
            android: Some(AndroidFacts {
                dir: ".".into(),
                gradle: Some("9.1.0".into()),
                agp: Some("9.0.1".into()),
                compile_sdk: Some("36".into()),
                target_sdk: Some("36".into()),
                ..AndroidFacts::default()
            }),
            flutter_constraint: None,
            dart_constraint: None,
            flutter_pinned: None,
            react_native: None,
            expo: None,
            node_constraint: None,
            package_manager: None,
            notes: vec![],
        }
    }

    fn item<'a>(p: &'a Plan, c: &str) -> &'a PlanItem {
        p.items.iter().find(|i| i.component == c).unwrap()
    }

    #[test]
    fn reuses_compatible_installs_and_installs_the_rest() {
        let pk = pack();
        let res = crate::resolve::resolve(&native(), &pk);
        let p = build("C:\\p", &res, &env(true), &pk, MANAGED);
        assert!(
            matches!(&item(&p, "jdk").action, Action::Reuse { path, .. } if path == "C:\\jdk21"),
            "JDK 21 fits 17..25"
        );
        // The user SDK lacks the emulator (broken), image and cmdline-tools: managed SDK for all.
        assert_eq!(p.android_sdk.as_deref(), Some(MANAGED));
        for c in [
            "android-platform-tools",
            "android-platform",
            "android-emulator",
            "android-system-image",
        ] {
            assert!(
                matches!(item(&p, c).action, Action::Install),
                "{c} must not mix SDK roots"
            );
        }
        assert_eq!(item(&p, "jdk").approx_download_bytes, 0);
        assert!(
            p.total_download_bytes > 4_000_000_000,
            "system image dominates"
        );
        assert_eq!(
            p.licences.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(),
            vec!["android-sdk"]
        );
        assert!(
            p.admin_actions.is_empty(),
            "accel available; long paths only for RN/Flutter"
        );
        assert!(p.disk_ok);
    }

    #[test]
    fn jdk_outside_range_is_not_reused_and_admin_action_listed() {
        let pk = pack();
        let mut f = native();
        f.android.as_mut().unwrap().agp = Some("7.4.2".into());
        f.android.as_mut().unwrap().gradle = Some("7.5".into());
        let res = crate::resolve::resolve(&f, &pk);
        let mut e = env(false);
        e.host.disk_free_bytes = 1 << 30;
        let p = build("C:\\p", &res, &e, &pk, MANAGED);
        assert!(
            matches!(item(&p, "jdk").action, Action::Install),
            "Gradle 7.5 runs only up to Java 17"
        );
        assert!(p.licences.iter().any(|l| l.id == "temurin"));
        assert!(p
            .admin_actions
            .iter()
            .any(|a| a.id == "enable-whpx" && a.required && a.reboot));
        assert!(!p.disk_ok);
    }

    #[test]
    fn expo_plan_reuses_node() {
        let pk = pack();
        let f = ProjectFacts {
            framework: Framework::Expo,
            android: None,
            expo: Some("~58.0.24".into()),
            ..native()
        };
        let res = crate::resolve::resolve(&f, &pk);
        let p = build("C:\\p", &res, &env(true), &pk, MANAGED);
        assert!(matches!(item(&p, "node").action, Action::Reuse { .. }));
        assert!(p
            .admin_actions
            .iter()
            .any(|a| a.id == "enable-long-paths" && !a.required));
    }

    #[test]
    fn complete_user_sdk_is_reused_as_a_whole() {
        let pk = pack();
        let res = crate::resolve::resolve(&native(), &pk);
        let mut e = env(true);
        let s = &mut e.android_sdks[0];
        s.emulator = Health::Ok;
        s.cmdline_tools = true;
        for p in [
            "emulator",
            "build-tools/36.0.0",
            "system-images/android-36/google_apis/x86_64",
        ] {
            s.packages.push(SdkPackage {
                path: p.into(),
                version: Some("1".into()),
            });
        }
        let p = build("C:\\p", &res, &e, &pk, MANAGED);
        assert_eq!(p.android_sdk.as_deref(), Some("C:\\Sdk"));
        assert!(p
            .items
            .iter()
            .filter(|i| i.component.starts_with("android-"))
            .all(|i| matches!(i.action, Action::Reuse { .. })));
        assert_eq!(p.licences, Vec::<PlanLicence>::new());
    }
}
