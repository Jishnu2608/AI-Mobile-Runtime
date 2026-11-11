//! RES-004 / RES-005: turns project facts + Knowledge Pack rules into requirements.
//!
//! The resolver never changes the project. When the project's own pins cannot work together
//! (e.g. the Gradle wrapper is too old for the AGP version), it reports a conflict with an
//! explanation and the source of the rule.
//!
//! Version choice:
//! 1. Hard pins from the project (wrapper, AGP, `compileSdk`, `ndkVersion`, `CMake`, `.fvmrc`,
//!    `engines.node`, Expo SDK).
//! 2. Ranges derived through pack rules (AGP → min JDK, Gradle → max JDK, Expo/RN → min Node).
//! 3. Pack defaults inside that range (JDK 17, Node 24, `google_apis` `x86_64` image, `CMake` 3.22.1).

use std::cmp::Ordering;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::pack::{Agp, Pack};
use crate::project::{Framework, ProjectFacts};
use crate::version::{Range, Version};

/// One thing the project needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Requirement {
    /// Knowledge Pack component id (`jdk`, `android-platform`, `node`, …).
    pub component: String,
    /// Android SDK package path, for SDK components (`platforms/android-36`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    /// Version to install when nothing reusable is found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Lowest acceptable version (for reuse of an existing install).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<String>,
    /// Highest acceptable major/version (for reuse of an existing install).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<String>,
    /// Why it is needed, in plain words.
    pub reason: String,
    /// Source URLs of the rules used.
    pub sources: Vec<String>,
}

/// The project's own pins cannot work together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveConflict {
    pub id: String,
    pub message: String,
    pub hint: String,
    pub sources: Vec<String>,
}

/// Resolver output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolution {
    pub framework: Framework,
    /// Environment profile used for readiness (`android-native`, `flutter-android`, …).
    pub profile: String,
    pub pack_version: String,
    pub requirements: Vec<Requirement>,
    pub conflicts: Vec<ResolveConflict>,
    /// Best-effort choices and values that could not be read.
    pub notes: Vec<String>,
}

struct Ctx<'a> {
    pack: &'a Pack,
    reqs: Vec<Requirement>,
    conflicts: Vec<ResolveConflict>,
    notes: Vec<String>,
}

impl Ctx<'_> {
    fn url(&self, source: &str) -> Vec<String> {
        self.pack
            .sources
            .get(source)
            .map(|s| s.url.clone())
            .into_iter()
            .collect()
    }

    fn need(
        &mut self,
        component: &str,
        package: Option<String>,
        version: Option<String>,
        reason: String,
        sources: Vec<String>,
    ) {
        self.reqs.push(Requirement {
            component: component.into(),
            package,
            version,
            min: None,
            max: None,
            reason,
            sources,
        });
    }

    fn conflict(&mut self, id: &str, message: String, hint: String, sources: Vec<String>) {
        self.conflicts.push(ResolveConflict {
            id: id.into(),
            message,
            hint,
            sources,
        });
    }
}

fn eq(a: &Version, b: &Version) -> bool {
    a.cmp(b) == Ordering::Equal
}

/// AGP rule for a version: same major.minor line, else the newest older line (best effort).
fn agp_rule<'p>(pack: &'p Pack, agp: &Version) -> (Option<&'p Agp>, bool) {
    let line = Version(agp.0.iter().take(2).copied().collect());
    let mut rules: Vec<(&Agp, Version)> = pack
        .agp
        .iter()
        .filter_map(|r| Some((r, Version::parse(&r.agp)?)))
        .collect();
    rules.sort_by(|a, b| a.1.cmp(&b.1));
    if let Some((r, _)) = rules.iter().find(|(_, v)| eq(v, &line)) {
        return (Some(r), true);
    }
    (
        rules.iter().rev().find(|(_, v)| *v < line).map(|(r, _)| *r),
        false,
    )
}

/// Newest rule at or below `agp` that names a default (build-tools / NDK).
fn agp_default<'p>(
    pack: &'p Pack,
    agp: &Version,
    field: impl Fn(&Agp) -> Option<&String>,
) -> Option<(&'p Agp, String)> {
    let line = Version(agp.0.iter().take(2).copied().collect());
    pack.agp
        .iter()
        .filter_map(|r| Some((r, Version::parse(&r.agp)?, field(r)?.clone())))
        .filter(|(_, v, _)| *v <= line)
        .max_by(|a, b| a.1.cmp(&b.1))
        .map(|(r, _, d)| (r, d))
}

/// Highest Java version a Gradle version can run on.
fn max_java_for_gradle(pack: &Pack, gradle: &Version) -> Option<u32> {
    pack.gradle_java
        .iter()
        .filter(|r| Version::parse(&r.min_gradle).is_some_and(|m| m <= *gradle))
        .map(|r| r.java)
        .max()
}

/// SDK package for a platform API level (`36` → `platforms/android-36`; overrides from the pack).
#[must_use]
pub fn platform_package(pack: &Pack, api: &str) -> String {
    pack.defaults
        .platform_package_overrides
        .get(api)
        .cloned()
        .unwrap_or_else(|| format!("platforms/android-{api}"))
}

/// System image package for an API level, using the same `android-<n>` name as the platform.
#[must_use]
pub fn system_image_package(pack: &Pack, api: &str) -> String {
    let platform = platform_package(pack, api);
    let name = platform.trim_start_matches("platforms/");
    format!(
        "system-images/{name}/{}/{}",
        pack.defaults.system_image_tag, pack.defaults.system_image_abi
    )
}

fn profile(framework: Framework) -> &'static str {
    match framework {
        Framework::Flutter => "flutter-android",
        Framework::ReactNative => "react-native-android",
        Framework::Expo => "expo-go",
        Framework::AndroidNative | Framework::Unknown => "android-native",
    }
}

/// Resolves what a project needs.
#[must_use]
pub fn resolve(facts: &ProjectFacts, pack: &Pack) -> Resolution {
    let mut c = Ctx {
        pack,
        reqs: Vec::new(),
        conflicts: Vec::new(),
        notes: facts.notes.clone(),
    };
    let mut preview_api: Option<String> = None;

    if let Some(a) = &facts.android {
        preview_api = resolve_android(&mut c, facts, a);
    } else if facts.framework == Framework::Unknown {
        c.notes.push(
            "no Android, Flutter or React Native project found; showing the base Android toolchain"
                .into(),
        );
    }

    match facts.framework {
        Framework::Flutter => resolve_flutter(&mut c, facts),
        Framework::ReactNative | Framework::Expo => resolve_node(&mut c, facts),
        Framework::AndroidNative | Framework::Unknown => {}
    }

    if facts.android.is_none() && facts.framework != Framework::Expo {
        // No Gradle build to read: base toolchain with defaults.
        let jdk = pack.defaults.jdk_major;
        c.reqs.push(Requirement {
            component: "jdk".into(),
            package: None,
            version: Some(jdk.to_string()),
            min: Some(jdk.to_string()),
            max: None,
            reason: format!(
                "JDK {jdk} (Knowledge Pack default) runs Gradle and the Android SDK tools"
            ),
            sources: c.url(&pack.defaults.source),
        });
    }

    // Preview: platform-tools, emulator, system image (RES-005: image for the target API).
    let api = preview_api
        .or_else(|| newest_api(pack))
        .unwrap_or_else(|| "36".into());
    let src = c.url(&pack.defaults.source);
    // Always: Android CLI (inside cmdline-tools) installs every other SDK package, Expo Go too.
    c.need(
        "android-cmdline-tools",
        Some("cmdline-tools/latest".into()),
        None,
        "Android SDK command-line tools (Android CLI) install and manage the SDK packages".into(),
        src.clone(),
    );
    c.need(
        "android-platform-tools",
        Some("platform-tools".into()),
        None,
        "adb installs and starts the app on the emulator".into(),
        src.clone(),
    );
    c.need(
        "android-emulator",
        Some("emulator".into()),
        None,
        "runs the Android emulator for the live preview".into(),
        src.clone(),
    );
    c.need(
        "android-system-image",
        Some(system_image_package(pack, &api)),
        Some(api.clone()),
        format!(
            "emulator system image for API {api} ({}, {})",
            pack.defaults.system_image_tag, pack.defaults.system_image_abi
        ),
        src,
    );

    Resolution {
        framework: facts.framework,
        profile: profile(facts.framework).into(),
        pack_version: pack.pack_version.clone(),
        requirements: c.reqs,
        conflicts: c.conflicts,
        notes: c.notes,
    }
}

fn newest_api(pack: &Pack) -> Option<String> {
    pack.api_min_agp
        .iter()
        .filter_map(|r| Some((Version::parse(&r.api)?, r.api.clone())))
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, s)| s)
}

/// Returns the API level the preview image should use.
#[allow(clippy::too_many_lines)]
fn resolve_android(
    c: &mut Ctx<'_>,
    facts: &ProjectFacts,
    a: &crate::project::AndroidFacts,
) -> Option<String> {
    let pack = c.pack;
    let agp = a.agp.as_deref().and_then(Version::parse);
    let gradle = a.gradle.as_deref().and_then(Version::parse);
    let rule = agp.as_ref().map(|v| agp_rule(pack, v));

    // JDK range: AGP min JDK .. highest Java the wrapper's Gradle runs on.
    let mut min_jdk = pack.defaults.jdk_major.min(17);
    let mut jdk_src = c.url(&pack.defaults.source);
    if let (Some(v), Some((r, exact))) = (&agp, rule) {
        match r {
            Some(r) => {
                min_jdk = r.min_jdk;
                jdk_src = c.url(&r.source);
                if !exact {
                    c.notes.push(format!("AGP {v} is newer than this Knowledge Pack ({}); using the AGP {} rules", pack.pack_version, r.agp));
                }
                if let Some(g) = &gradle {
                    if let Some(min_g) = Version::parse(&r.min_gradle) {
                        if *g < min_g {
                            c.conflict(
                                "gradle-too-old-for-agp",
                                format!("Gradle {g} (wrapper) is older than AGP {v} needs (Gradle {min_g} or newer)"),
                                format!("Update distributionUrl in {}/gradle/wrapper/gradle-wrapper.properties to Gradle {min_g} or newer, or use an older AGP", a.dir),
                                c.url(&r.source),
                            );
                        }
                    }
                }
            }
            None => c.notes.push(format!("AGP {v} is older than every rule in this Knowledge Pack; requirements are best effort")),
        }
    }
    let max_jdk = gradle.as_ref().and_then(|g| max_java_for_gradle(pack, g));
    if gradle.is_some() && max_jdk.is_none() {
        c.notes.push(
            "Gradle wrapper is older than every Gradle/Java rule in this Knowledge Pack".into(),
        );
    }
    if let Some(max) = max_jdk {
        if max < min_jdk {
            c.conflict(
                "no-jdk-fits",
                format!("AGP needs JDK {min_jdk} or newer, but Gradle {} runs only up to Java {max}", a.gradle.as_deref().unwrap_or("?")),
                "Update the Gradle wrapper to a version that supports the JDK the Android Gradle Plugin needs".into(),
                [jdk_src.clone(), c.url("gradle-compat")].concat(),
            );
        }
    }
    let preferred = pack
        .defaults
        .jdk_major
        .max(min_jdk)
        .min(max_jdk.unwrap_or(u32::MAX))
        .max(min_jdk);
    let mut sources = jdk_src;
    if max_jdk.is_some() {
        sources.extend(c.url("gradle-compat"));
    }
    sources.dedup();
    c.reqs.push(Requirement {
        component: "jdk".into(),
        package: None,
        version: Some(preferred.to_string()),
        min: Some(min_jdk.to_string()),
        max: max_jdk.map(|m| m.to_string()),
        reason: match (a.agp.as_deref(), a.gradle.as_deref()) {
            (Some(p), Some(g)) => format!(
                "AGP {p} needs JDK {min_jdk}+; Gradle {g} runs on Java up to {}",
                max_jdk.map_or("?".into(), |m| m.to_string())
            ),
            (Some(p), None) => format!("AGP {p} needs JDK {min_jdk}+"),
            _ => {
                format!("JDK {preferred} runs Gradle (AGP version unknown; Knowledge Pack default)")
            }
        },
        sources,
    });

    // compileSdk platform (RES-005), checked against AGP.
    if let Some(api) = &a.compile_sdk {
        if let (Some(v), Some(api_v)) = (&agp, Version::parse(api)) {
            if let Some(r) = pack
                .api_min_agp
                .iter()
                .find(|r| Version::parse(&r.api).is_some_and(|x| eq(&x, &api_v)))
            {
                if let Some(min) = Version::parse(&r.min_agp) {
                    if *v < min {
                        c.conflict(
                            "agp-too-old-for-compile-sdk",
                            format!("compileSdk {api} needs AGP {min} or newer; the project uses AGP {v}"),
                            "Update the Android Gradle Plugin, or lower compileSdk".into(),
                            c.url(&r.source),
                        );
                    }
                }
            } else if newest_api(pack)
                .and_then(|n| Version::parse(&n))
                .is_some_and(|n| api_v > n)
            {
                c.notes.push(format!(
                    "compileSdk {api} is newer than this Knowledge Pack knows"
                ));
            }
        }
        c.need(
            "android-platform",
            Some(platform_package(pack, api)),
            Some(api.clone()),
            format!("compileSdk = {api} in {}/app/build.gradle(.kts)", a.dir),
            c.url(&pack.defaults.source),
        );
    }

    // Build-tools: AGP's default for its line.
    if let Some((r, bt)) = agp
        .as_ref()
        .and_then(|v| agp_default(pack, v, |r| r.default_build_tools.as_ref()))
    {
        c.need(
            "android-build-tools",
            Some(format!("build-tools/{bt}")),
            Some(bt.clone()),
            format!(
                "default build-tools of AGP {} (AGP downloads it if missing)",
                r.agp
            ),
            c.url(&r.source),
        );
    }

    // NDK and CMake (RES-005).
    let rn = matches!(facts.framework, Framework::ReactNative);
    if let Some(ndk) = &a.ndk_version {
        c.need(
            "android-ndk",
            Some(format!("ndk/{ndk}")),
            Some(ndk.clone()),
            format!("ndkVersion = \"{ndk}\" in {}/app/build.gradle(.kts)", a.dir),
            Vec::new(),
        );
    } else if a.uses_cmake || rn {
        if let Some((r, ndk)) = agp
            .as_ref()
            .and_then(|v| agp_default(pack, v, |r| r.default_ndk.as_ref()))
        {
            let why = if rn {
                "React Native native modules"
            } else {
                "the CMake build"
            };
            c.need(
                "android-ndk",
                Some(format!("ndk/{ndk}")),
                Some(ndk.clone()),
                format!("{why} need the NDK; AGP {} default NDK {ndk}", r.agp),
                c.url(&r.source),
            );
        } else {
            c.notes
                .push("project needs the NDK but its version could not be determined".into());
        }
    }
    if a.uses_cmake {
        let (v, why) = match &a.cmake_version {
            Some(v) => (
                v.clone(),
                format!("externalNativeBuild cmake version \"{v}\""),
            ),
            None => (
                pack.defaults.default_cmake.clone(),
                format!(
                    "project uses CMake without a version; AGP default CMake {}",
                    pack.defaults.default_cmake
                ),
            ),
        };
        c.need(
            "android-cmake",
            Some(format!("cmake/{v}")),
            Some(v),
            why,
            c.url(&pack.defaults.source),
        );
    }

    a.target_sdk.clone().or_else(|| a.compile_sdk.clone())
}

fn resolve_flutter(c: &mut Ctx<'_>, facts: &ProjectFacts) {
    let (version, min, reason) = match (&facts.flutter_pinned, &facts.flutter_constraint) {
        (Some(p), _) => (
            Some(p.clone()),
            Some(p.clone()),
            format!("Flutter {p} pinned in .fvmrc"),
        ),
        (None, Some(r)) => {
            let min = Range::parse(r)
                .and_then(|r| r.minimum())
                .map(|v| v.to_string());
            (None, min, format!("pubspec.yaml environment.flutter: {r}"))
        }
        (None, None) => (
            None,
            None,
            "Flutter SDK (stable channel) builds and hot-reloads the app".into(),
        ),
    };
    c.reqs.push(Requirement {
        component: "flutter".into(),
        package: None,
        version,
        min,
        max: None,
        reason,
        sources: c.url("poc-006"),
    });
    c.reqs.push(Requirement {
        component: "git".into(),
        package: None,
        version: None,
        min: None,
        max: None,
        reason: "the Flutter tool needs Git".into(),
        sources: c.url("poc-006"),
    });
}

fn resolve_node(c: &mut Ctx<'_>, facts: &ProjectFacts) {
    let pack = c.pack;
    let mut min: Option<(Version, String, Vec<String>)> = None;
    let mut bump = |v: Option<Version>, why: String, src: Vec<String>| {
        if let Some(v) = v {
            if min.as_ref().is_none_or(|(m, _, _)| v > *m) {
                min = Some((v, why, src));
            }
        }
    };
    if let Some(expo) = &facts.expo {
        let major = Range::parse(expo)
            .and_then(|r| r.minimum())
            .or_else(|| Version::parse(expo))
            .map(|v| v.major());
        match major.and_then(|m| pack.expo_sdk.iter().find(|r| u64::from(r.sdk) == m)) {
            Some(r) => bump(
                Version::parse(&r.min_node),
                format!("Expo SDK {} needs Node {}+", r.sdk, r.min_node),
                c.url(&r.source),
            ),
            None => c.notes.push(format!(
                "Expo SDK {expo} is not in this Knowledge Pack ({})",
                pack.pack_version
            )),
        }
    }
    if let Some(rn) = &facts.react_native {
        let v = Range::parse(rn)
            .and_then(|r| r.minimum())
            .or_else(|| Version::parse(rn));
        let line = v.map(|v| Version(v.0.iter().take(2).copied().collect()));
        match line.and_then(|l| {
            pack.react_native
                .iter()
                .find(|r| Version::parse(&r.react_native).is_some_and(|x| eq(&x, &l)))
        }) {
            Some(r) => bump(
                Version::parse(&r.min_node),
                format!("React Native {} needs Node {}+", r.react_native, r.min_node),
                c.url(&r.source),
            ),
            None => c.notes.push(format!(
                "React Native {rn} is not in this Knowledge Pack; Node requirement is best effort"
            )),
        }
    }
    let project_range = facts.node_constraint.as_deref().and_then(Range::parse);
    let default = Version(vec![u64::from(pack.defaults.node_major)]);
    let preferred = match &project_range {
        Some(r) if !r.contains(&Version(vec![default.major(), 999, 999])) => {
            r.minimum().map_or(default.major(), |m| m.major())
        }
        _ => default.major(),
    };
    let (min_v, why, mut sources) = min.unwrap_or((
        Version(vec![18]),
        "React Native tooling (Metro) needs Node".into(),
        c.url("rn-env-setup"),
    ));
    let mut reason = why;
    if let Some(r) = &facts.node_constraint {
        let _ = write!(reason, "; project asks for Node {r}");
    }
    if Version(vec![preferred, 999, 999]) < min_v {
        c.conflict(
            "node-too-old",
            format!(
                "the project asks for Node {} but {reason}",
                facts.node_constraint.as_deref().unwrap_or("?")
            ),
            "Update engines.node / .nvmrc to a supported Node version".into(),
            sources.clone(),
        );
    }
    sources.dedup();
    c.reqs.push(Requirement {
        component: "node".into(),
        package: None,
        version: Some(preferred.to_string()),
        min: Some(min_v.to_string()),
        max: None,
        reason,
        sources,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::AndroidFacts;

    fn pack() -> Pack {
        serde_json::from_str(crate::pack::BUNDLED_PACK).unwrap()
    }

    fn facts(framework: Framework, android: Option<AndroidFacts>) -> ProjectFacts {
        ProjectFacts {
            root: ".".into(),
            framework,
            android,
            flutter_constraint: None,
            dart_constraint: None,
            flutter_pinned: None,
            react_native: None,
            expo: None,
            node_constraint: None,
            package_manager: None,
            notes: Vec::new(),
        }
    }

    fn native(agp: &str, gradle: &str, compile: &str) -> AndroidFacts {
        AndroidFacts {
            dir: ".".into(),
            gradle: Some(gradle.into()),
            agp: Some(agp.into()),
            compile_sdk: Some(compile.into()),
            target_sdk: Some(compile.into()),
            ..AndroidFacts::default()
        }
    }

    fn req<'a>(r: &'a Resolution, c: &str) -> &'a Requirement {
        r.requirements
            .iter()
            .find(|x| x.component == c)
            .unwrap_or_else(|| panic!("no {c} in {:#?}", r.requirements))
    }

    #[test]
    fn android_cli_template_resolves_cleanly() {
        let r = resolve(
            &facts(
                Framework::AndroidNative,
                Some(native("9.0.1", "9.1.0", "36")),
            ),
            &pack(),
        );
        assert_eq!(r.conflicts, Vec::<ResolveConflict>::new());
        let jdk = req(&r, "jdk");
        assert_eq!(
            (
                jdk.version.as_deref(),
                jdk.min.as_deref(),
                jdk.max.as_deref()
            ),
            (Some("17"), Some("17"), Some("25"))
        );
        assert!(jdk.sources.iter().all(|s| s.starts_with("https://")));
        assert_eq!(
            req(&r, "android-platform").package.as_deref(),
            Some("platforms/android-36")
        );
        assert_eq!(
            req(&r, "android-build-tools").package.as_deref(),
            Some("build-tools/36.0.0")
        );
        assert_eq!(
            req(&r, "android-system-image").package.as_deref(),
            Some("system-images/android-36/google_apis/x86_64")
        );
        assert!(r.requirements.iter().all(|x| x.component != "android-ndk"));
    }

    #[test]
    fn api_37_uses_platform_override_and_flags_old_agp() {
        let r = resolve(
            &facts(
                Framework::AndroidNative,
                Some(native("9.0.1", "9.1.0", "37")),
            ),
            &pack(),
        );
        assert_eq!(
            req(&r, "android-platform").package.as_deref(),
            Some("platforms/android-37.0")
        );
        assert_eq!(
            req(&r, "android-system-image").package.as_deref(),
            Some("system-images/android-37.0/google_apis/x86_64")
        );
        assert!(
            r.conflicts
                .iter()
                .any(|c| c.id == "agp-too-old-for-compile-sdk"),
            "{:?}",
            r.conflicts
        );
    }

    #[test]
    fn old_wrapper_conflicts_with_agp() {
        let r = resolve(
            &facts(Framework::AndroidNative, Some(native("8.7.3", "8.4", "35"))),
            &pack(),
        );
        let c = r
            .conflicts
            .iter()
            .find(|c| c.id == "gradle-too-old-for-agp")
            .expect("conflict");
        assert!(c.message.contains("8.9"), "{}", c.message);
        assert_ne!(c.sources, Vec::<String>::new());
    }

    #[test]
    fn agp7_gradle7_uses_jdk11_to_17() {
        let r = resolve(
            &facts(Framework::AndroidNative, Some(native("7.4.2", "7.5", "33"))),
            &pack(),
        );
        let jdk = req(&r, "jdk");
        assert_eq!(
            (
                jdk.min.as_deref(),
                jdk.max.as_deref(),
                jdk.version.as_deref()
            ),
            (Some("11"), Some("17"), Some("17"))
        );
        assert_eq!(
            req(&r, "android-build-tools").version.as_deref(),
            Some("30.0.2")
        );
    }

    #[test]
    fn newer_agp_than_pack_is_best_effort() {
        let r = resolve(
            &facts(
                Framework::AndroidNative,
                Some(native("9.9.0", "9.8.0", "36")),
            ),
            &pack(),
        );
        assert!(
            r.notes
                .iter()
                .any(|n| n.contains("newer than this Knowledge Pack")),
            "{:?}",
            r.notes
        );
        assert_eq!(req(&r, "jdk").min.as_deref(), Some("17"));
    }

    #[test]
    fn cmake_and_ndk_defaults() {
        let mut a = native("8.13.0", "8.13", "36");
        a.uses_cmake = true;
        let r = resolve(&facts(Framework::AndroidNative, Some(a)), &pack());
        assert_eq!(
            req(&r, "android-cmake").package.as_deref(),
            Some("cmake/3.22.1")
        );
        assert_eq!(
            req(&r, "android-ndk").package.as_deref(),
            Some("ndk/25.1.8937393")
        );
    }

    #[test]
    fn expo_go_needs_node_but_no_jdk() {
        let mut f = facts(Framework::Expo, None);
        f.expo = Some("~58.0.24".into());
        let r = resolve(&f, &pack());
        assert_eq!(r.profile, "expo-go");
        let node = req(&r, "node");
        assert_eq!(
            (node.min.as_deref(), node.version.as_deref()),
            (Some("22.13"), Some("24"))
        );
        assert!(r.requirements.iter().all(|x| x.component != "jdk"));
        assert!(r
            .requirements
            .iter()
            .any(|x| x.component == "android-emulator"));
        assert!(
            r.requirements
                .iter()
                .any(|x| x.component == "android-cmdline-tools"),
            "Android CLI is needed to install the emulator and image"
        );
    }

    #[test]
    fn project_node_pin_too_old_is_a_conflict() {
        let mut f = facts(Framework::Expo, None);
        f.expo = Some("58.0.0".into());
        f.node_constraint = Some("20".into());
        let r = resolve(&f, &pack());
        assert!(
            r.conflicts.iter().any(|c| c.id == "node-too-old"),
            "{:?}",
            r.conflicts
        );
        assert_eq!(req(&r, "node").version.as_deref(), Some("20"));
    }

    #[test]
    fn flutter_with_computed_compile_sdk() {
        let a = AndroidFacts {
            dir: "android".into(),
            gradle: Some("8.14".into()),
            agp: Some("8.11.1".into()),
            ndk_version: Some("28.2.13676358".into()),
            ..AndroidFacts::default()
        };
        let mut f = facts(Framework::Flutter, Some(a));
        f.flutter_pinned = Some("3.38.1".into());
        let r = resolve(&f, &pack());
        assert_eq!(r.profile, "flutter-android");
        assert_eq!(req(&r, "flutter").version.as_deref(), Some("3.38.1"));
        assert!(r.requirements.iter().any(|x| x.component == "git"));
        assert_eq!(
            req(&r, "android-ndk").package.as_deref(),
            Some("ndk/28.2.13676358")
        );
        assert!(r
            .requirements
            .iter()
            .all(|x| x.component != "android-platform"));
        // preview image falls back to the newest API the pack knows
        assert_eq!(
            req(&r, "android-system-image").version.as_deref(),
            Some("37.0")
        );
    }
}
