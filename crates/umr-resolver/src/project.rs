//! RES-003: reads the version constraints a project declares. Read-only.
//!
//! | File | What is read |
//! |---|---|
//! | `gradle/wrapper/gradle-wrapper.properties` | Gradle version (`distributionUrl`) |
//! | `gradle/libs.versions.toml`, `settings.gradle(.kts)`, `build.gradle(.kts)` | AGP version |
//! | `app/build.gradle(.kts)` | `compileSdk`, `minSdk`, `targetSdk`, `ndkVersion`, `CMake` use/version |
//! | `pubspec.yaml`, `.fvmrc` | Flutter / Dart constraints |
//! | `package.json` | `react-native`, `expo` versions, `engines.node` |
//! | `.nvmrc`, `.node-version` | Node version |
//! | lockfiles | package manager |
//!
//! Flutter and React Native keep their Android build in `android/`; it is read the same way as a
//! native project. Values that are computed in build scripts (e.g. `flutter.compileSdkVersion`)
//! cannot be read statically and are reported as unknown.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Kind of project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Framework {
    AndroidNative,
    Flutter,
    ReactNative,
    /// Expo project without native folders (runs in Expo Go or needs a dev build).
    Expo,
    Unknown,
}

/// Facts read from the Android (Gradle) part of a project.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AndroidFacts {
    /// Folder holding the Gradle build (`.` for native, `android` for Flutter/RN).
    pub dir: String,
    pub gradle: Option<String>,
    pub agp: Option<String>,
    pub compile_sdk: Option<String>,
    pub min_sdk: Option<String>,
    pub target_sdk: Option<String>,
    pub ndk_version: Option<String>,
    pub uses_cmake: bool,
    pub cmake_version: Option<String>,
}

/// Everything the resolver needs from a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectFacts {
    pub root: String,
    pub framework: Framework,
    pub android: Option<AndroidFacts>,
    /// pubspec `environment.flutter`.
    pub flutter_constraint: Option<String>,
    /// pubspec `environment.sdk` (Dart).
    pub dart_constraint: Option<String>,
    /// Exact Flutter version pinned by `.fvmrc`.
    pub flutter_pinned: Option<String>,
    pub react_native: Option<String>,
    pub expo: Option<String>,
    /// `engines.node` or `.nvmrc` / `.node-version`.
    pub node_constraint: Option<String>,
    pub package_manager: Option<String>,
    /// Notes about values that could not be read.
    pub notes: Vec<String>,
}

/// Reads a project folder.
#[must_use]
pub fn read(root: &Path) -> ProjectFacts {
    let mut notes = Vec::new();
    let pubspec = std::fs::read_to_string(root.join("pubspec.yaml")).ok();
    let package_json: Option<serde_json::Value> =
        std::fs::read_to_string(root.join("package.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok());

    let dep = |name: &str| -> Option<String> {
        let pj = package_json.as_ref()?;
        ["dependencies", "devDependencies"]
            .iter()
            .find_map(|s| pj.get(*s)?.get(name)?.as_str().map(String::from))
    };
    let react_native = dep("react-native");
    let expo = dep("expo");

    let android_dir = if root.join("android").join("settings.gradle").is_file()
        || root.join("android").join("settings.gradle.kts").is_file()
    {
        Some(root.join("android"))
    } else if root.join("settings.gradle").is_file() || root.join("settings.gradle.kts").is_file() {
        Some(root.to_path_buf())
    } else {
        None
    };

    let framework = if pubspec.as_deref().is_some_and(|p| p.contains("flutter")) {
        Framework::Flutter
    } else if expo.is_some() && android_dir.is_none() {
        Framework::Expo
    } else if react_native.is_some() {
        Framework::ReactNative
    } else if android_dir.is_some() {
        Framework::AndroidNative
    } else {
        Framework::Unknown
    };

    let android = android_dir.map(|d| read_android(root, &d, &mut notes));
    let (flutter_constraint, dart_constraint) = pubspec
        .as_deref()
        .map(parse_pubspec_environment)
        .unwrap_or_default();
    let flutter_pinned = std::fs::read_to_string(root.join(".fvmrc"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("flutter").and_then(|x| x.as_str()).map(String::from));

    let node_constraint = package_json
        .as_ref()
        .and_then(|pj| pj.get("engines")?.get("node")?.as_str().map(String::from))
        .or_else(|| first_line(&root.join(".nvmrc")))
        .or_else(|| first_line(&root.join(".node-version")));

    let package_manager = [
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        ("bun.lockb", "bun"),
        ("bun.lock", "bun"),
        ("package-lock.json", "npm"),
    ]
    .iter()
    .find(|(f, _)| root.join(f).is_file())
    .map(|(_, pm)| (*pm).to_string())
    .or_else(|| package_json.as_ref().map(|_| "npm".to_string()));

    ProjectFacts {
        root: root.display().to_string(),
        framework,
        android,
        flutter_constraint,
        dart_constraint,
        flutter_pinned,
        react_native,
        expo,
        node_constraint,
        package_manager,
        notes,
    }
}

fn first_line(p: &Path) -> Option<String> {
    std::fs::read_to_string(p)
        .ok()?
        .lines()
        .next()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
}

fn read_android(root: &Path, dir: &Path, notes: &mut Vec<String>) -> AndroidFacts {
    let rel = dir.strip_prefix(root).map_or_else(
        |_| ".".into(),
        |p| {
            if p.as_os_str().is_empty() {
                ".".into()
            } else {
                p.display().to_string()
            }
        },
    );
    let gradle = std::fs::read_to_string(
        dir.join("gradle")
            .join("wrapper")
            .join("gradle-wrapper.properties"),
    )
    .ok()
    .and_then(|t| parse_wrapper_version(&t));
    if gradle.is_none() {
        notes.push(
            "no Gradle wrapper version found (gradle/wrapper/gradle-wrapper.properties)".into(),
        );
    }
    let toml = std::fs::read_to_string(dir.join("gradle").join("libs.versions.toml")).ok();
    let build_files: Vec<String> = [
        "settings.gradle.kts",
        "settings.gradle",
        "build.gradle.kts",
        "build.gradle",
    ]
    .iter()
    .filter_map(|f| std::fs::read_to_string(dir.join(f)).ok())
    .collect();
    // React Native pins AGP in its Gradle plugin (`node_modules/@react-native/gradle-plugin`)
    let rn_catalog = std::fs::read_to_string(
        root.join("node_modules/@react-native/gradle-plugin/gradle/libs.versions.toml"),
    )
    .ok();
    let agp = toml
        .as_deref()
        .and_then(agp_from_version_catalog)
        .or_else(|| build_files.iter().find_map(|t| agp_from_build_script(t)))
        .or_else(|| rn_catalog.as_deref().and_then(agp_from_version_catalog));
    if agp.is_none() {
        notes.push(
            "Android Gradle Plugin version not found in version catalog or build scripts".into(),
        );
    }

    let app = ["app/build.gradle.kts", "app/build.gradle"]
        .iter()
        .find_map(|f| std::fs::read_to_string(dir.join(f)).ok())
        .unwrap_or_default();
    let catalog = toml.as_deref().unwrap_or("");
    // `rootProject.ext.<name>` (React Native) → the root build script's `ext { <name> = … }`
    let ext = build_files.iter().map(|t| ext_values(t)).fold(
        std::collections::HashMap::new(),
        |mut m, e| {
            m.extend(e);
            m
        },
    );
    let from_ext = |key: &str| {
        app.contains(&format!("rootProject.ext.{key}"))
            .then(|| ext.get(key).cloned())
            .flatten()
    };
    let level = |keys: &[&str]| {
        keys.iter()
            .find_map(|k| sdk_level(&app, k, catalog))
            .or_else(|| keys.iter().find_map(|k| from_ext(k)))
    };
    let compile_sdk = level(&["compileSdk", "compileSdkVersion"]);
    if compile_sdk.is_none() {
        notes.push("compileSdk is computed in the build script (e.g. flutter.compileSdkVersion); resolved later by the build".into());
    }
    let ndk_version = quoted_assignment(&app, "ndkVersion").or_else(|| from_ext("ndkVersion"));
    let uses_cmake = app.contains("cmake") && app.contains("externalNativeBuild")
        || dir
            .join("app")
            .join("src")
            .join("main")
            .join("cpp")
            .join("CMakeLists.txt")
            .is_file();
    let cmake_version = cmake_version(&app);
    AndroidFacts {
        dir: rel,
        gradle,
        agp,
        compile_sdk,
        min_sdk: level(&["minSdk", "minSdkVersion"]),
        target_sdk: level(&["targetSdk", "targetSdkVersion"]),
        ndk_version,
        uses_cmake,
        cmake_version,
    }
}

/// The defaults a Flutter SDK gives `flutter.compileSdkVersion`, `flutter.minSdkVersion`,
/// `flutter.targetSdkVersion` and `flutter.ndkVersion` (read from its `FlutterExtension.kt`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FlutterDefaults {
    pub compile_sdk: Option<String>,
    pub min_sdk: Option<String>,
    pub target_sdk: Option<String>,
    pub ndk_version: Option<String>,
}

/// Parses `FlutterExtension.kt` (`val compileSdkVersion: Int = 36`, `val ndkVersion: String = "…"`).
#[must_use]
pub fn parse_flutter_defaults(kotlin: &str) -> FlutterDefaults {
    let val = |name: &str| {
        kotlin.lines().find_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix("val ")?.strip_prefix(name)?;
            let v = rest.split_once('=')?.1.trim().trim_matches('"');
            (!v.is_empty()).then(|| v.to_string())
        })
    };
    FlutterDefaults {
        compile_sdk: val("compileSdkVersion"),
        min_sdk: val("minSdkVersion"),
        target_sdk: val("targetSdkVersion"),
        ndk_version: val("ndkVersion"),
    }
}

/// Fills values the app's build script takes from `flutter.*` with the installed Flutter SDK's
/// defaults, so the plan can include them (e.g. the NDK, which the Android build cannot
/// install itself: POC-006 finding 4). Values the project sets itself are kept.
pub fn apply_flutter_defaults(facts: &mut ProjectFacts, root: &Path, flutter_root: &Path) {
    let Some(a) = facts.android.as_mut() else {
        return;
    };
    let ext =
        flutter_root.join("packages/flutter_tools/gradle/src/main/kotlin/FlutterExtension.kt");
    let Ok(kotlin) = std::fs::read_to_string(&ext) else {
        return;
    };
    let d = parse_flutter_defaults(&kotlin);
    let app = ["app/build.gradle.kts", "app/build.gradle"]
        .iter()
        .find_map(|f| std::fs::read_to_string(root.join(&a.dir).join(f)).ok())
        .unwrap_or_default();
    let mut used = Vec::new();
    let mut fill = |slot: &mut Option<String>, value: Option<String>, key: &str| {
        if slot.is_none() && app.contains(&format!("flutter.{key}")) {
            if let Some(v) = value {
                used.push(format!("{key} = {v}"));
                *slot = Some(v);
            }
        }
    };
    fill(&mut a.compile_sdk, d.compile_sdk, "compileSdkVersion");
    fill(&mut a.min_sdk, d.min_sdk, "minSdkVersion");
    fill(&mut a.target_sdk, d.target_sdk, "targetSdkVersion");
    fill(&mut a.ndk_version, d.ndk_version, "ndkVersion");
    if !used.is_empty() {
        facts.notes.push(format!(
            "Flutter SDK defaults used for flutter.* values: {}",
            used.join(", ")
        ));
    }
}

/// `distributionUrl=https\://services.gradle.org/distributions/gradle-9.1.0-bin.zip` → `9.1.0`.
#[must_use]
pub fn parse_wrapper_version(text: &str) -> Option<String> {
    let line = text
        .lines()
        .find(|l| l.trim_start().starts_with("distributionUrl"))?;
    let start = line.find("gradle-")? + "gradle-".len();
    let rest = &line[start..];
    let end = rest.find("-bin.zip").or_else(|| rest.find("-all.zip"))?;
    Some(rest[..end].to_string())
}

/// AGP from a version catalog: the `[plugins]` entry with id `com.android.application`
/// (version or version.ref), or a well-known `[versions]` key.
#[must_use]
pub fn agp_from_version_catalog(toml: &str) -> Option<String> {
    let versions = section(toml, "versions");
    let plugins = section(toml, "plugins");
    for line in plugins.lines() {
        if line.contains("\"com.android.application\"")
            || line.contains("'com.android.application'")
        {
            if let Some(v) = inline_value(line, "version.ref") {
                return key_value(&versions, &v);
            }
            if let Some(v) = inline_value(line, "version") {
                return Some(v);
            }
        }
    }
    [
        "agp",
        "androidGradlePlugin",
        "android-gradle-plugin",
        "androidGradle",
    ]
    .iter()
    .find_map(|k| key_value(&versions, k))
}

/// AGP from a build script: `id("com.android.application") version "9.0.1"` or the
/// `com.android.tools.build:gradle:8.1.0` classpath.
#[must_use]
pub fn agp_from_build_script(text: &str) -> Option<String> {
    for line in text.lines() {
        if line.contains("com.android.application") && line.contains("version") {
            if let Some(v) = quoted_after(line, "version") {
                return Some(v);
            }
        }
        if let Some(i) = line.find("com.android.tools.build:gradle:") {
            let rest = &line[i + "com.android.tools.build:gradle:".len()..];
            let v: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-')
                .collect();
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

/// `compileSdk = 37`, `compileSdkVersion 34`, `compileSdk = libs.versions.compileSdk.get().toInt()`.
fn sdk_level(app: &str, key: &str, catalog: &str) -> Option<String> {
    // The key may follow other code on the line: `defaultConfig { minSdk = 24 }`.
    let starts = |line: &str| -> Option<String> {
        let t = line.trim_start();
        if t.starts_with("//") {
            return None;
        }
        let mut from = 0;
        while let Some(i) = t[from..].find(key) {
            let at = from + i;
            let before_ok = at == 0
                || !t.as_bytes()[at - 1].is_ascii_alphanumeric() && t.as_bytes()[at - 1] != b'.';
            if before_ok {
                return Some(t[at + key.len()..].to_string());
            }
            from = at + key.len();
        }
        None
    };
    for line in app.lines() {
        if let Some(rest) = starts(line) {
            let rest = rest.as_str();
            if !rest.starts_with([' ', '=', '(']) {
                continue; // e.g. compileSdkPreview when looking for compileSdk
            }
            let rest = rest.trim_start_matches([' ', '=', '(']).trim();
            let num: String = rest
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            if !num.is_empty() {
                return Some(num);
            }
            if let Some(name) = rest.strip_prefix("libs.versions.") {
                let name: String = name
                    .chars()
                    .take_while(|c| {
                        c.is_ascii_alphanumeric() || *c == '.' || *c == '-' || *c == '_'
                    })
                    .collect();
                let name = name.trim_end_matches(".get").replace('.', "-");
                let versions = section(catalog, "versions");
                return key_value(&versions, &name)
                    .or_else(|| key_value(&versions, &name.replace('-', "")));
            }
            return None;
        }
    }
    None
}

/// `ext { name = value … }` assignments of a root build script (quotes removed).
#[must_use]
pub fn ext_values(text: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let Some(start) = text.find("ext {").or_else(|| text.find("ext{")) else {
        return out;
    };
    let body = &text[start..];
    let Some(open) = body.find('{') else {
        return out;
    };
    let mut depth = 0usize;
    for (i, c) in body[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    for line in body[open + 1..open + i].lines() {
                        if let Some((k, v)) = line.split_once('=') {
                            let k = k.trim().trim_start_matches("set(").trim();
                            let v = v
                                .trim()
                                .trim_end_matches(')')
                                .trim()
                                .trim_matches(['"', '\'']);
                            if !k.is_empty()
                                && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                            {
                                out.insert(k.to_string(), v.to_string());
                            }
                        }
                    }
                    break;
                }
            }
            _ => {}
        }
    }
    out
}

fn quoted_assignment(text: &str, key: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| l.starts_with(key))
        .find_map(|l| quoted_after(l, key))
}

fn cmake_version(app: &str) -> Option<String> {
    let i = app.find("cmake")?;
    let block = &app[i..app.len().min(i + 400)];
    block
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("version"))
        .and_then(|l| quoted_after(l, "version"))
}

/// First quoted string after `after` on the line.
fn quoted_after(line: &str, after: &str) -> Option<String> {
    let rest = &line[line.find(after)? + after.len()..];
    let start = rest.find(['"', '\''])?;
    let quote = rest.as_bytes()[start] as char;
    let rest = &rest[start + 1..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

/// Text of a TOML `[name]` section.
fn section(toml: &str, name: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for line in toml.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            inside = t == format!("[{name}]");
            continue;
        }
        if inside {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// `key = "value"` inside a section.
fn key_value(section: &str, key: &str) -> Option<String> {
    section.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').trim_matches('\'').to_string())
    })
}

/// `version.ref = "agp"` or `version = "9.0.1"` inside an inline table.
fn inline_value(line: &str, key: &str) -> Option<String> {
    let mut search = line;
    while let Some(i) = search.find(key) {
        let after = &search[i + key.len()..];
        let before_ok = i == 0
            || !search.as_bytes()[i - 1].is_ascii_alphanumeric()
                && search.as_bytes()[i - 1] != b'.';
        if before_ok && after.trim_start().starts_with('=') {
            return quoted_after(after, "=");
        }
        search = after;
    }
    None
}

/// `environment:` block of pubspec.yaml → (flutter constraint, dart sdk constraint).
#[must_use]
pub fn parse_pubspec_environment(text: &str) -> (Option<String>, Option<String>) {
    let mut inside = false;
    let (mut flutter, mut sdk) = (None, None);
    for line in text.lines() {
        if line.trim_start().starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if !line.starts_with(' ') && !line.starts_with('\t') {
            inside = line.trim_end() == "environment:";
            continue;
        }
        if inside {
            if let Some((k, v)) = line.trim().split_once(':') {
                let v = v.trim().trim_matches('"').trim_matches('\'').to_string();
                match k.trim() {
                    "sdk" => sdk = Some(v),
                    "flutter" => flutter = Some(v),
                    _ => {}
                }
            }
        }
    }
    (flutter, sdk)
}

/// Project root as a path.
#[must_use]
pub fn root_path(f: &ProjectFacts) -> PathBuf {
    PathBuf::from(&f.root)
}

#[cfg(test)]
mod react_native_ext_tests {
    use super::*;

    #[test]
    fn root_ext_values_resolve_root_project_references() {
        let root_build = "buildscript {\n    ext {\n        buildToolsVersion = \"37.0.0\"\n        minSdkVersion = 24\n        compileSdkVersion = 37\n        targetSdkVersion = 36\n        ndkVersion = \"27.1.12297006\"\n    }\n}\n";
        let e = ext_values(root_build);
        assert_eq!(e["ndkVersion"], "27.1.12297006");
        assert_eq!(e["compileSdkVersion"], "37");

        let base = std::env::temp_dir().join(format!("umr-rn-ext-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("android/app")).unwrap();
        std::fs::write(
            base.join("package.json"),
            r#"{"dependencies":{"react-native":"0.88.1"}}"#,
        )
        .unwrap();
        std::fs::write(base.join("android/settings.gradle"), "include ':app'").unwrap();
        std::fs::write(base.join("android/build.gradle"), root_build).unwrap();
        std::fs::write(
            base.join("android/app/build.gradle"),
            "android {\n    ndkVersion rootProject.ext.ndkVersion\n    compileSdk rootProject.ext.compileSdkVersion\n    defaultConfig {\n        minSdkVersion rootProject.ext.minSdkVersion\n        targetSdkVersion rootProject.ext.targetSdkVersion\n    }\n}\n",
        )
        .unwrap();
        let rn = base.join("node_modules/@react-native/gradle-plugin/gradle");
        std::fs::create_dir_all(&rn).unwrap();
        std::fs::write(
            rn.join("libs.versions.toml"),
            "[versions]\nagp = \"8.12.0\"\n",
        )
        .unwrap();
        let f = read(&base);
        let a = f.android.unwrap();
        assert_eq!(a.compile_sdk.as_deref(), Some("37"));
        assert_eq!(a.target_sdk.as_deref(), Some("36"));
        assert_eq!(a.min_sdk.as_deref(), Some("24"));
        assert_eq!(a.ndk_version.as_deref(), Some("27.1.12297006"));
        assert_eq!(a.agp.as_deref(), Some("8.12.0"));
        let _ = std::fs::remove_dir_all(&base);
    }
}

#[cfg(test)]
mod flutter_default_tests {
    use super::*;

    #[test]
    fn flutter_defaults_fill_symbolic_values_only() {
        let kt = "open class FlutterExtension {\n    val compileSdkVersion: Int = 36\n    val minSdkVersion: Int = 24\n    val targetSdkVersion: Int = 36\n    val ndkVersion: String = \"28.2.13676358\"\n}\n";
        let d = parse_flutter_defaults(kt);
        assert_eq!(d.ndk_version.as_deref(), Some("28.2.13676358"));
        assert_eq!(d.compile_sdk.as_deref(), Some("36"));

        let base =
            std::env::temp_dir().join(format!("umr-flutter-defaults-{}", std::process::id()));
        let (proj, sdk) = (base.join("p"), base.join("flutter"));
        let ext = sdk.join("packages/flutter_tools/gradle/src/main/kotlin");
        std::fs::create_dir_all(&ext).unwrap();
        std::fs::write(ext.join("FlutterExtension.kt"), kt).unwrap();
        std::fs::create_dir_all(proj.join("android/app")).unwrap();
        std::fs::write(proj.join("android/settings.gradle.kts"), "").unwrap();
        std::fs::write(
            proj.join("pubspec.yaml"),
            "name: x\ndependencies:\n  flutter:\n    sdk: flutter\n",
        )
        .unwrap();
        std::fs::write(
            proj.join("android/app/build.gradle.kts"),
            "android {\n    compileSdk = flutter.compileSdkVersion\n    ndkVersion = flutter.ndkVersion\n    defaultConfig { minSdk = 26; targetSdk = flutter.targetSdkVersion }\n}\n",
        )
        .unwrap();
        let mut facts = read(&proj);
        apply_flutter_defaults(&mut facts, &proj, &sdk);
        let a = facts.android.as_ref().unwrap();
        assert_eq!(a.ndk_version.as_deref(), Some("28.2.13676358"));
        assert_eq!(a.compile_sdk.as_deref(), Some("36"));
        assert_eq!(
            a.min_sdk.as_deref(),
            Some("26"),
            "the project's own value is kept"
        );
        assert!(facts
            .notes
            .iter()
            .any(|n| n.contains("Flutter SDK defaults")));
        let _ = std::fs::remove_dir_all(&base);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("umr-proj-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn wrapper_and_catalog_parsing() {
        assert_eq!(
            parse_wrapper_version(
                "distributionUrl=https\\://services.gradle.org/distributions/gradle-9.1.0-bin.zip"
            ),
            Some("9.1.0".into())
        );
        assert_eq!(
            parse_wrapper_version("distributionUrl=https\\://x/gradle-8.10.2-all.zip"),
            Some("8.10.2".into())
        );
        let toml = "[versions]\nandroidGradlePlugin = \"9.0.1\"\nkotlin = \"2.3.20\"\n[plugins]\nandroid-application = { id = \"com.android.application\", version.ref = \"androidGradlePlugin\" }\n";
        assert_eq!(agp_from_version_catalog(toml), Some("9.0.1".into()));
        let toml2 = "[plugins]\nandroid-application = { id = \"com.android.application\", version = \"8.7.3\" }\n";
        assert_eq!(agp_from_version_catalog(toml2), Some("8.7.3".into()));
        assert_eq!(
            agp_from_build_script(
                "    id(\"com.android.application\") version \"8.9.1\" apply false"
            ),
            Some("8.9.1".into())
        );
        assert_eq!(
            agp_from_build_script("id \"com.android.application\" version \"8.11.1\" apply false"),
            Some("8.11.1".into())
        );
        assert_eq!(
            agp_from_build_script("classpath 'com.android.tools.build:gradle:7.4.2'"),
            Some("7.4.2".into())
        );
    }

    #[test]
    fn pubspec_environment() {
        let p = "name: app\nenvironment:\n  sdk: ^3.9.0\n  flutter: \">=3.35.0\"\ndependencies:\n  flutter:\n    sdk: flutter\n";
        assert_eq!(
            parse_pubspec_environment(p),
            (Some(">=3.35.0".into()), Some("^3.9.0".into()))
        );
    }

    #[test]
    fn native_kotlin_project_like_android_cli_template() {
        let r = tmp("native");
        write(&r, "settings.gradle.kts", "rootProject.name = \"x\"\n");
        write(
            &r,
            "gradle/wrapper/gradle-wrapper.properties",
            "distributionUrl=https\\://services.gradle.org/distributions/gradle-9.1.0-bin.zip\n",
        );
        write(&r, "gradle/libs.versions.toml", "[versions]\nandroidGradlePlugin = \"9.0.1\"\n[plugins]\nandroid-application = { id = \"com.android.application\", version.ref = \"androidGradlePlugin\" }\n");
        write(&r, "app/build.gradle.kts", "android {\n  compileSdk = 37\n  defaultConfig {\n    minSdk = 24\n    targetSdk = 37\n  }\n}\n");
        let f = read(&r);
        assert_eq!(f.framework, Framework::AndroidNative);
        let a = f.android.unwrap();
        assert_eq!(
            (a.gradle.as_deref(), a.agp.as_deref()),
            (Some("9.1.0"), Some("9.0.1"))
        );
        assert_eq!(
            (
                a.compile_sdk.as_deref(),
                a.min_sdk.as_deref(),
                a.target_sdk.as_deref()
            ),
            (Some("37"), Some("24"), Some("37"))
        );
        assert!(!a.uses_cmake);
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn flutter_project_with_computed_sdk_and_ndk() {
        let r = tmp("flutter");
        write(
            &r,
            "pubspec.yaml",
            "name: app\nenvironment:\n  sdk: ^3.9.0\ndependencies:\n  flutter:\n    sdk: flutter\n",
        );
        write(
            &r,
            "android/settings.gradle.kts",
            "plugins {\n    id(\"com.android.application\") version \"8.11.1\" apply false\n}\n",
        );
        write(
            &r,
            "android/gradle/wrapper/gradle-wrapper.properties",
            "distributionUrl=https\\://services.gradle.org/distributions/gradle-8.14-all.zip\n",
        );
        write(&r, "android/app/build.gradle.kts", "android {\n    compileSdk = flutter.compileSdkVersion\n    ndkVersion = \"28.2.13676358\"\n}\n");
        let f = read(&r);
        assert_eq!(f.framework, Framework::Flutter);
        assert_eq!(f.dart_constraint.as_deref(), Some("^3.9.0"));
        let a = f.android.unwrap();
        assert_eq!(a.dir, "android");
        assert_eq!(a.agp.as_deref(), Some("8.11.1"));
        assert_eq!(a.compile_sdk, None, "computed value is not guessed");
        assert_eq!(a.ndk_version.as_deref(), Some("28.2.13676358"));
        assert!(f.notes.iter().any(|n| n.contains("computed")));
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn expo_and_react_native_projects() {
        let r = tmp("expo");
        write(
            &r,
            "package.json",
            "{\"dependencies\":{\"expo\":\"~58.0.24\",\"react-native\":\"0.88.1\"}}",
        );
        write(&r, "package-lock.json", "{}");
        let f = read(&r);
        assert_eq!(f.framework, Framework::Expo);
        assert_eq!(f.expo.as_deref(), Some("~58.0.24"));
        assert_eq!(f.package_manager.as_deref(), Some("npm"));
        assert!(f.android.is_none());
        // after prebuild / bare RN: android folder exists
        write(&r, "android/settings.gradle", "include ':app'\n");
        write(
            &r,
            "android/app/build.gradle",
            "android {\n    compileSdkVersion 36\n}\n",
        );
        write(&r, ".nvmrc", "22.13.0\n");
        write(&r, "yarn.lock", "");
        let f = read(&r);
        assert_eq!(f.framework, Framework::ReactNative);
        assert_eq!(f.android.unwrap().compile_sdk.as_deref(), Some("36"));
        assert_eq!(f.node_constraint.as_deref(), Some("22.13.0"));
        assert_eq!(f.package_manager.as_deref(), Some("yarn"));
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn cmake_use_and_version() {
        let app = "android {\n  externalNativeBuild {\n    cmake {\n      path \"src/main/cpp/CMakeLists.txt\"\n      version \"3.31.6\"\n    }\n  }\n}\n";
        assert_eq!(cmake_version(app), Some("3.31.6".into()));
        assert_eq!(
            sdk_level("    defaultConfig { minSdk = 24 }", "minSdk", ""),
            Some("24".into())
        );
        assert_eq!(
            sdk_level(
                "// compileSdk = 30
compileSdkPreview = \"X\"
compileSdk 34",
                "compileSdk",
                ""
            ),
            Some("34".into())
        );
        assert_eq!(
            sdk_level(
                "compileSdk = libs.versions.compileSdk.get().toInt()",
                "compileSdk",
                "[versions]\ncompileSdk = \"36\"\n"
            ),
            Some("36".into())
        );
    }
}
