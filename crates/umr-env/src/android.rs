//! ENV-003: Android SDK detection, installed packages, and adb/emulator functional checks.
//!
//! Candidates: `ANDROID_HOME`, `ANDROID_SDK_ROOT`, Android Studio's default
//! `%LOCALAPPDATA%\Android\Sdk`, and the managed `<root>\android-sdk`. Packages are read from
//! each package's `source.properties` (`Pkg.Revision`), without running any tool.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::context::ScanContext;
use crate::model::{AndroidSdk, Health, Origin, SdkPackage};
use crate::pathsearch::normalize;
use crate::runner::{run, CHECK_TIMEOUT};

/// Package folders: fixed ones, and parents whose children are packages.
const SINGLE: &[&str] = &["platform-tools", "emulator", "tools"];
const FAMILIES: &[&str] = &[
    "build-tools",
    "platforms",
    "cmdline-tools",
    "ndk",
    "cmake",
    "sources",
];

/// Detects Android SDKs. `child_env` is used for the functional checks (isolated environment).
#[must_use]
pub fn detect(ctx: &ScanContext, child_env: &HashMap<String, String>) -> Vec<AndroidSdk> {
    let mut found: Vec<(PathBuf, Origin)> = Vec::new();
    for var in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Some(v) = ctx.var(var) {
            found.push((PathBuf::from(v), Origin::EnvVar));
        }
    }
    if let Some(local) = &ctx.local_app_data {
        found.push((local.join("Android").join("Sdk"), Origin::CommonDir));
    }
    found.push((ctx.managed_root.join("android-sdk"), Origin::Managed));

    let mut merged: Vec<(String, PathBuf, Vec<Origin>)> = Vec::new();
    for (root, origin) in found {
        // Env vars are kept even when wrong, so the report can say so.
        if !root.is_dir() && origin != Origin::EnvVar {
            continue;
        }
        let key = normalize(&root);
        if let Some(e) = merged.iter_mut().find(|(k, _, _)| *k == key) {
            if !e.2.contains(&origin) {
                e.2.push(origin);
            }
        } else {
            merged.push((key, root, vec![origin]));
        }
    }
    merged
        .into_iter()
        .map(|(_, root, origins)| inspect(ctx, &root, origins, child_env))
        .collect()
}

fn inspect(
    ctx: &ScanContext,
    root: &Path,
    origins: Vec<Origin>,
    child_env: &HashMap<String, String>,
) -> AndroidSdk {
    let packages = if root.is_dir() {
        list_packages(root)
    } else {
        Vec::new()
    };
    let licenses = std::fs::read_dir(root.join("licenses"))
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    let adb = root.join("platform-tools").join("adb.exe");
    let emulator = root.join("emulator").join("emulator.exe");
    AndroidSdk {
        root: root.display().to_string(),
        origins,
        cmdline_tools: packages
            .iter()
            .any(|p| p.path.starts_with("cmdline-tools/")),
        packages,
        licenses,
        adb: check(ctx, &adb, &["version"], child_env),
        emulator: check(ctx, &emulator, &["-version"], child_env),
    }
}

fn check(ctx: &ScanContext, exe: &Path, args: &[&str], env: &HashMap<String, String>) -> Health {
    if !exe.is_file() {
        return Health::Broken("not installed".into());
    }
    if !ctx.run_checks {
        return Health::NotChecked("checks disabled".into());
    }
    match run(exe, args, env, CHECK_TIMEOUT) {
        Ok(o) if o.success() => Health::Ok,
        Ok(o) => Health::Broken(format!("exited with {:?}", o.code)),
        Err(e) => Health::Broken(e.to_string()),
    }
}

/// Runs `emulator -accel-check` with the isolated environment; `None` if no emulator.
#[must_use]
pub fn accel_check(
    ctx: &ScanContext,
    sdks: &[AndroidSdk],
    env: &HashMap<String, String>,
) -> Option<String> {
    if !ctx.run_checks {
        return None;
    }
    let exe = sdks
        .iter()
        .map(|s| Path::new(&s.root).join("emulator").join("emulator.exe"))
        .find(|p| p.is_file())?;
    run(&exe, &["-accel-check"], env, CHECK_TIMEOUT)
        .ok()
        .map(|o| o.combined().trim().to_string())
}

/// Lists installed packages under an SDK root.
#[must_use]
pub fn list_packages(root: &Path) -> Vec<SdkPackage> {
    let mut out = Vec::new();
    for name in SINGLE {
        let dir = root.join(name);
        if dir.is_dir() {
            out.push(SdkPackage {
                path: (*name).to_string(),
                version: revision(&dir),
            });
        }
    }
    for family in FAMILIES {
        for child in children(&root.join(family)) {
            let name = child
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            out.push(SdkPackage {
                path: format!("{family}/{name}"),
                version: revision(&child),
            });
        }
    }
    // system-images/<api>/<tag>/<abi>
    for api in children(&root.join("system-images")) {
        for tag in children(&api) {
            for abi in children(&tag) {
                let rel = |p: &Path| {
                    p.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                };
                out.push(SdkPackage {
                    path: format!("system-images/{}/{}/{}", rel(&api), rel(&tag), rel(&abi)),
                    version: revision(&abi),
                });
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

fn children(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir() && !is_hidden(p))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn is_hidden(p: &Path) -> bool {
    p.file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with('.'))
}

/// `Pkg.Revision=37.0.0` from `source.properties`.
fn revision(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("source.properties")).ok()?;
    parse_revision(&text)
}

#[must_use]
pub fn parse_revision(text: &str) -> Option<String> {
    text.lines()
        .find_map(|l| l.trim().strip_prefix("Pkg.Revision="))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(root: &Path, rel: &str, rev: &str) {
        let d = root.join(rel);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("source.properties"),
            format!("Pkg.Desc=x\nPkg.Revision={rev}\n"),
        )
        .unwrap();
    }

    #[test]
    fn revision_parsing() {
        assert_eq!(
            parse_revision("Pkg.Desc=a\nPkg.Revision=36.5.11\n"),
            Some("36.5.11".into())
        );
        assert_eq!(parse_revision("nothing"), None);
    }

    #[test]
    fn lists_packages_of_a_fake_sdk() {
        let root = std::env::temp_dir().join(format!("umr-sdk-list-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        pkg(&root, "platform-tools", "37.0.1");
        pkg(&root, "emulator", "37.2.13");
        pkg(&root, "build-tools/36.0.0", "36.0.0");
        pkg(&root, "platforms/android-36", "2");
        pkg(&root, "ndk/28.2.13676358", "28.2.13676358");
        pkg(&root, "cmdline-tools/latest", "23.0");
        pkg(&root, "system-images/android-36/google_apis/x86_64", "7");
        std::fs::create_dir_all(root.join(".temp")).unwrap();
        let p = list_packages(&root);
        let paths: Vec<_> = p.iter().map(|x| x.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "build-tools/36.0.0",
                "cmdline-tools/latest",
                "emulator",
                "ndk/28.2.13676358",
                "platform-tools",
                "platforms/android-36",
                "system-images/android-36/google_apis/x86_64"
            ]
        );
        assert_eq!(
            p.iter()
                .find(|x| x.path == "emulator")
                .unwrap()
                .version
                .as_deref(),
            Some("37.2.13")
        );

        let ctx = ScanContext::for_test(
            root.join("managed"),
            &[("ANDROID_HOME", &root.display().to_string())],
        );
        let sdks = detect(&ctx, &HashMap::new());
        assert_eq!(sdks.len(), 1);
        assert!(sdks[0].cmdline_tools);
        assert!(
            matches!(sdks[0].adb, Health::Broken(_)),
            "no adb.exe in the fake SDK"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn wrong_android_home_is_kept_for_reporting() {
        let ctx = ScanContext::for_test(r"C:\none".into(), &[("ANDROID_HOME", r"Z:\nope\sdk")]);
        let sdks = detect(&ctx, &HashMap::new());
        assert_eq!(sdks.len(), 1);
        assert_eq!(sdks[0].packages, Vec::<SdkPackage>::new());
    }
}
