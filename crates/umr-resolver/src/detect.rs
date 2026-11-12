//! PROJ-001: which kind of project is this folder, how sure are we, and why.
//!
//! Each framework scores points for marker files; the evidence list names every file that
//! counted, so the user (and the agent) can see why the product decided what it did. When the
//! folder itself is not a project (or only weakly one), sub-folders up to three levels deep are
//! searched for projects — common in monorepos (`apps/mobile`, `packages/app`).
//! Read-only: files are only read, never changed.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::project::{self, Framework};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    /// File relative to the project root.
    pub file: String,
    /// What it tells us.
    pub says: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub path: String,
    pub framework: Framework,
    pub confidence: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Detection {
    pub root: String,
    pub framework: Framework,
    /// 0–100.
    pub confidence: u8,
    pub evidence: Vec<Evidence>,
    /// Projects found in sub-folders (when the root itself is not clearly one).
    pub candidates: Vec<Candidate>,
}

/// Folders never searched for sub-projects.
const SKIP: &[&str] = &[
    "node_modules",
    ".git",
    "build",
    ".gradle",
    ".dart_tool",
    "ios",
    "target",
    ".expo",
    "dist",
    ".idea",
    ".vscode",
    "Pods",
];

fn read(root: &Path, rel: &str) -> Option<String> {
    std::fs::read_to_string(root.join(rel)).ok()
}

fn exists(root: &Path, rel: &str) -> bool {
    root.join(rel).exists()
}

struct Score {
    points: u32,
    evidence: Vec<Evidence>,
}

impl Score {
    fn new() -> Self {
        Self {
            points: 0,
            evidence: Vec::new(),
        }
    }

    fn add(&mut self, cond: bool, points: u32, file: &str, says: &str) {
        if cond {
            self.points += points;
            self.evidence.push(Evidence {
                file: file.into(),
                says: says.into(),
            });
        }
    }

    fn confidence(&self) -> u8 {
        u8::try_from(self.points.min(100)).unwrap_or(100)
    }
}

#[allow(clippy::too_many_lines)] // one block of marker rules per framework
fn score(root: &Path, framework: Framework) -> Score {
    let mut s = Score::new();
    let pubspec = read(root, "pubspec.yaml").unwrap_or_default();
    let package: serde_json::Value = read(root, "package.json")
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let has_dep = |name: &str| {
        ["dependencies", "devDependencies"]
            .iter()
            .any(|k| package.get(*k).and_then(|d| d.get(name)).is_some())
    };
    let android =
        if exists(root, "android/settings.gradle") || exists(root, "android/settings.gradle.kts") {
            "android/"
        } else {
            ""
        };
    let gradle_files = |dir: &str| -> String {
        [
            "settings.gradle.kts",
            "settings.gradle",
            "build.gradle.kts",
            "build.gradle",
            "gradle/libs.versions.toml",
        ]
        .iter()
        .filter_map(|f| read(root, &format!("{dir}{f}")))
        .collect::<Vec<_>>()
        .join("\n")
    };
    match framework {
        Framework::Flutter => {
            s.add(
                pubspec.contains("sdk: flutter"),
                50,
                "pubspec.yaml",
                "depends on the Flutter SDK",
            );
            s.add(
                exists(root, "lib/main.dart"),
                20,
                "lib/main.dart",
                "Flutter entry point",
            );
            s.add(
                !android.is_empty(),
                20,
                "android/settings.gradle",
                "Android build folder",
            );
            s.add(
                exists(root, ".metadata"),
                10,
                ".metadata",
                "Flutter project metadata",
            );
        }
        Framework::Expo => {
            s.add(has_dep("expo"), 50, "package.json", "depends on expo");
            let app_json = read(root, "app.json").unwrap_or_default();
            s.add(
                app_json.contains("\"expo\""),
                20,
                "app.json",
                "Expo app configuration",
            );
            s.add(
                exists(root, "app.config.js") || exists(root, "app.config.ts"),
                20,
                "app.config.*",
                "Expo app configuration",
            );
            s.add(
                has_dep("react-native"),
                10,
                "package.json",
                "depends on react-native",
            );
            s.add(
                android.is_empty(),
                10,
                "(no android/)",
                "no native folders: runs in Expo Go",
            );
        }
        Framework::ReactNative => {
            s.add(
                has_dep("react-native"),
                50,
                "package.json",
                "depends on react-native",
            );
            s.add(
                !android.is_empty(),
                30,
                "android/settings.gradle",
                "native Android build folder",
            );
            s.add(
                exists(root, "metro.config.js"),
                10,
                "metro.config.js",
                "Metro bundler configuration",
            );
            s.add(
                ["index.js", "index.ts", "App.tsx", "App.js"]
                    .iter()
                    .any(|f| exists(root, f)),
                10,
                "index.js / App.tsx",
                "React Native entry point",
            );
            s.add(
                has_dep("expo"),
                0,
                "package.json",
                "also uses expo (dev build)",
            );
        }
        Framework::AndroidNative => {
            let g = gradle_files("");
            s.add(
                exists(root, "settings.gradle") || exists(root, "settings.gradle.kts"),
                30,
                "settings.gradle(.kts)",
                "Gradle build",
            );
            s.add(
                g.contains("com.android.application")
                    || g.contains("com.android.tools.build:gradle"),
                40,
                "build scripts / version catalog",
                "uses the Android Gradle Plugin",
            );
            s.add(
                exists(root, "app/src/main/AndroidManifest.xml"),
                20,
                "app/src/main/AndroidManifest.xml",
                "Android app module",
            );
            s.add(
                exists(root, "gradlew.bat") || exists(root, "gradlew"),
                10,
                "gradlew",
                "Gradle wrapper",
            );
        }
        Framework::Unknown => {}
    }
    s
}

/// Detects the project at `root` (and sub-projects when the root is not clearly one).
#[must_use]
pub fn detect(root: &Path) -> Detection {
    let framework = project::read(root).framework;
    let s = score(root, framework);
    let mut d = Detection {
        root: root.display().to_string(),
        framework,
        confidence: s.confidence(),
        evidence: s.evidence,
        candidates: Vec::new(),
    };
    if d.confidence < 50 {
        let mut found = Vec::new();
        walk(root, root, 0, &mut found);
        d.candidates = found;
    }
    d
}

fn walk(base: &Path, dir: &Path, depth: usize, out: &mut Vec<Candidate>) {
    if depth >= 3 || out.len() >= 20 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut subs: Vec<PathBuf> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            !n.starts_with('.') && !SKIP.contains(&n.as_str())
        })
        .map(|e| e.path())
        .collect();
    subs.sort();
    for sub in subs {
        let f = project::read(&sub).framework;
        let looks = sub.join("pubspec.yaml").is_file()
            || sub.join("package.json").is_file()
            || sub.join("settings.gradle").is_file()
            || sub.join("settings.gradle.kts").is_file();
        if looks && f != Framework::Unknown {
            let c = score(&sub, f).confidence();
            if c >= 50 {
                let rel = sub.strip_prefix(base).unwrap_or(&sub).display().to_string();
                out.push(Candidate {
                    path: rel,
                    framework: f,
                    confidence: c,
                });
                continue; // do not look inside a project for more projects
            }
        }
        walk(base, &sub, depth + 1, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("umr-detect-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn w(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn native_android_with_full_evidence() {
        let r = tmp("native");
        w(&r, "settings.gradle.kts", "include(\":app\")");
        w(
            &r,
            "gradle/libs.versions.toml",
            "[plugins]\na = { id = \"com.android.application\", version = \"9.0.1\" }\n",
        );
        w(&r, "app/src/main/AndroidManifest.xml", "<manifest/>");
        w(&r, "gradlew.bat", "");
        let d = detect(&r);
        assert_eq!((d.framework, d.confidence), (Framework::AndroidNative, 100));
        assert_eq!(d.evidence.len(), 4);
        assert_eq!(d.candidates, Vec::<Candidate>::new());
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn flutter_and_expo() {
        let r = tmp("flutter");
        w(
            &r,
            "pubspec.yaml",
            "name: a\ndependencies:\n  flutter:\n    sdk: flutter\n",
        );
        w(&r, "lib/main.dart", "void main(){}");
        let d = detect(&r);
        assert_eq!((d.framework, d.confidence), (Framework::Flutter, 70));
        assert!(d.evidence.iter().any(|e| e.file == "lib/main.dart"));
        let e = tmp("expo");
        w(
            &e,
            "package.json",
            r#"{"dependencies":{"expo":"~58.0.0","react-native":"0.88.1"}}"#,
        );
        w(&e, "app.json", r#"{"expo":{"name":"a"}}"#);
        let d = detect(&e);
        assert_eq!((d.framework, d.confidence), (Framework::Expo, 90));
        let _ = std::fs::remove_dir_all(&r);
        let _ = std::fs::remove_dir_all(&e);
    }

    #[test]
    fn monorepo_lists_sub_projects_and_skips_node_modules() {
        let r = tmp("mono");
        w(&r, "package.json", r#"{"name":"workspace","private":true}"#);
        w(
            &r,
            "apps/mobile/package.json",
            r#"{"dependencies":{"expo":"~58.0.0"}}"#,
        );
        w(&r, "apps/mobile/app.json", r#"{"expo":{}}"#);
        w(
            &r,
            "apps/flutter_app/pubspec.yaml",
            "dependencies:\n  flutter:\n    sdk: flutter\n",
        );
        w(&r, "apps/flutter_app/lib/main.dart", "");
        w(
            &r,
            "node_modules/some-lib/package.json",
            r#"{"dependencies":{"react-native":"0.88"}}"#,
        );
        let d = detect(&r);
        assert_eq!(d.framework, Framework::Unknown);
        let paths: Vec<(String, Framework)> = d
            .candidates
            .iter()
            .map(|c| (c.path.replace('\\', "/"), c.framework))
            .collect();
        assert_eq!(
            paths,
            [
                ("apps/flutter_app".to_string(), Framework::Flutter),
                ("apps/mobile".to_string(), Framework::Expo)
            ]
        );
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn empty_folder_is_unknown_with_no_evidence() {
        let r = tmp("empty");
        let d = detect(&r);
        assert_eq!((d.framework, d.confidence), (Framework::Unknown, 0));
        assert!(d.evidence.is_empty() && d.candidates.is_empty());
        let _ = std::fs::remove_dir_all(&r);
    }
}
