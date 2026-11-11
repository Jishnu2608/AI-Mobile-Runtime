//! ENV-004: Flutter, Node.js, package managers, Git, Android CLI and Android Studio.
//!
//! Functional checks are `--version`-style runs. Flutter itself is **not** run (it can be slow,
//! download artefacts or send analytics on first use); its version comes from the SDK files and
//! its health from the presence of the Dart SDK cache.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::context::ScanContext;
use crate::model::{Health, Origin, Tool};
use crate::pathsearch::{find, normalize};
use crate::runner::{run, CHECK_TIMEOUT};

/// Detects all tools. `child_env` is the isolated environment for functional checks.
#[must_use]
pub fn detect(
    ctx: &ScanContext,
    sdk_roots: &[String],
    child_env: &HashMap<String, String>,
) -> (Vec<Tool>, Option<PathBuf>) {
    let mut tools = Vec::new();
    tools.extend(flutter(ctx));
    tools.extend(simple(
        ctx,
        "node",
        "node",
        &["--version"],
        &managed_bins(ctx, "node", &[""]),
        child_env,
    ));
    for (name, exe) in [
        ("npm", "npm"),
        ("pnpm", "pnpm"),
        ("yarn", "yarn"),
        ("bun", "bun"),
    ] {
        tools.extend(simple(ctx, name, exe, &["--version"], &[], child_env));
    }
    tools.extend(simple(ctx, "git", "git", &["--version"], &[], child_env));
    let android_cli_extra: Vec<PathBuf> = sdk_roots
        .iter()
        .map(|r| {
            Path::new(r)
                .join("cmdline-tools")
                .join("latest")
                .join("bin")
                .join("android.exe")
        })
        .collect();
    tools.extend(simple(
        ctx,
        "android-cli",
        "android",
        &["-V", "--no-metrics"],
        &android_cli_extra,
        child_env,
    ));
    let studio = android_studio(ctx);
    let studio_home = studio.as_ref().map(|t| PathBuf::from(&t.path));
    tools.extend(studio);
    (tools, studio_home)
}

/// A tool found on PATH or at extra locations, checked with `args`.
fn simple(
    ctx: &ScanContext,
    name: &str,
    exe: &str,
    args: &[&str],
    extra: &[PathBuf],
    env: &HashMap<String, String>,
) -> Vec<Tool> {
    let mut found: Vec<(PathBuf, Origin)> = Vec::new();
    if let Some(p) = find(ctx, exe) {
        found.push((p, Origin::Path));
    }
    for p in extra.iter().filter(|p| p.is_file()) {
        let origin = if p.starts_with(&ctx.managed_root) {
            Origin::Managed
        } else {
            Origin::CommonDir
        };
        found.push((p.clone(), origin));
    }
    let mut out: Vec<Tool> = Vec::new();
    for (path, origin) in found {
        let key = normalize(&path);
        if let Some(t) = out
            .iter_mut()
            .find(|t| normalize(Path::new(&t.path)) == key)
        {
            if !t.origins.contains(&origin) {
                t.origins.push(origin);
            }
            continue;
        }
        let (version, health) = if ctx.run_checks {
            match run(&path, args, env, CHECK_TIMEOUT) {
                Ok(o) if o.success() => (parse_version(&o.combined()), Health::Ok),
                Ok(o) => (
                    None,
                    Health::Broken(format!(
                        "{name} {} exited with {:?}",
                        args.join(" "),
                        o.code
                    )),
                ),
                Err(e) => (None, Health::Broken(e.to_string())),
            }
        } else {
            (None, Health::NotChecked("checks disabled".into()))
        };
        out.push(Tool {
            name: name.into(),
            path: path.display().to_string(),
            version,
            origins: vec![origin],
            health,
        });
    }
    out
}

/// First version-looking token: `v26.13.0` → `26.13.0`, `git version 2.52.0.windows.1` → `2.52.0.windows.1`.
#[must_use]
pub fn parse_version(text: &str) -> Option<String> {
    text.split_whitespace()
        .map(|w| {
            w.trim_start_matches('v')
                .trim_matches(|c: char| c == ',' || c == '"')
        })
        .find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()) && w.contains('.'))
        .map(String::from)
}

fn managed_bins(ctx: &ScanContext, family: &str, rel: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for dir in std::fs::read_dir(ctx.managed_root.join(family))
        .into_iter()
        .flatten()
        .flatten()
    {
        for r in rel {
            let base = if r.is_empty() {
                dir.path()
            } else {
                dir.path().join(r)
            };
            out.push(base.join(format!("{family}.exe")));
        }
    }
    out
}

fn flutter(ctx: &ScanContext) -> Vec<Tool> {
    let mut roots: Vec<(PathBuf, Origin)> = Vec::new();
    if let Some(bat) = find(ctx, "flutter") {
        if let Some(root) = bat.parent().and_then(Path::parent) {
            roots.push((root.to_path_buf(), Origin::Path));
        }
    }
    for dir in std::fs::read_dir(ctx.managed_root.join("flutter"))
        .into_iter()
        .flatten()
        .flatten()
    {
        roots.push((dir.path(), Origin::Managed));
    }
    let mut out: Vec<Tool> = Vec::new();
    for (root, origin) in roots {
        if !root.join("bin").join("flutter.bat").is_file() {
            continue;
        }
        let key = normalize(&root);
        if let Some(t) = out
            .iter_mut()
            .find(|t| normalize(Path::new(&t.path)) == key)
        {
            t.origins.push(origin);
            continue;
        }
        let version = flutter_version(&root);
        let dart = root
            .join("bin")
            .join("cache")
            .join("dart-sdk")
            .join("bin")
            .join("dart.exe");
        let health = if dart.is_file() {
            Health::Ok
        } else {
            Health::NotChecked(
                "Flutter has not been run yet (its first run downloads the Dart SDK)".into(),
            )
        };
        out.push(Tool {
            name: "flutter".into(),
            path: root.display().to_string(),
            version,
            origins: vec![origin],
            health,
        });
    }
    out
}

/// Flutter version from `bin/cache/flutter.version.json` (`frameworkVersion`) or the legacy `version` file.
fn flutter_version(root: &Path) -> Option<String> {
    if let Ok(text) =
        std::fs::read_to_string(root.join("bin").join("cache").join("flutter.version.json"))
    {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(s) = v.get("frameworkVersion").and_then(|x| x.as_str()) {
                return Some(s.to_string());
            }
        }
    }
    std::fs::read_to_string(root.join("version"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn android_studio(ctx: &ScanContext) -> Option<Tool> {
    let mut candidates: Vec<(PathBuf, Origin)> = Vec::new();
    if ctx.use_registry {
        if let Ok(Some(p)) = umr_platform::registry::read_string(
            umr_platform::registry::Hive::LocalMachine,
            r"SOFTWARE\Android Studio",
            "Path",
        ) {
            candidates.push((PathBuf::from(p), Origin::Registry));
        }
    }
    for pf in &ctx.program_files {
        candidates.push((pf.join("Android").join("Android Studio"), Origin::CommonDir));
    }
    if let Some(local) = &ctx.local_app_data {
        candidates.push((
            local.join("Programs").join("Android Studio"),
            Origin::CommonDir,
        ));
    }
    let (home, origin) = candidates
        .into_iter()
        .find(|(p, _)| p.join("bin").join("studio64.exe").is_file())?;
    let version = std::fs::read_to_string(home.join("product-info.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("version").and_then(|x| x.as_str()).map(String::from));
    Some(Tool {
        name: "android-studio".into(),
        path: home.display().to_string(),
        version,
        origins: vec![origin],
        health: Health::NotChecked("not started (IDE)".into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parsing() {
        assert_eq!(parse_version("v26.13.0\n"), Some("26.13.0".into()));
        assert_eq!(
            parse_version("git version 2.52.0.windows.1"),
            Some("2.52.0.windows.1".into())
        );
        assert_eq!(parse_version("11.20.0"), Some("11.20.0".into()));
        assert_eq!(parse_version("1.1.16717390"), Some("1.1.16717390".into()));
        assert_eq!(parse_version("no digits here"), None);
    }

    #[test]
    fn flutter_found_via_path_with_version_file_and_missing_cache() {
        let base = std::env::temp_dir().join(format!("umr-flutter-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("flutter");
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(root.join("bin").join("flutter.bat"), "").unwrap();
        std::fs::write(root.join("version"), "3.47.9\n").unwrap();
        let path = root.join("bin").display().to_string();
        let ctx = ScanContext::for_test(
            base.join("managed"),
            &[("PATH", &path), ("PATHEXT", ".BAT")],
        );
        let f = flutter(&ctx);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].version.as_deref(), Some("3.47.9"));
        assert!(matches!(f[0].health, Health::NotChecked(_)));
        let _ = std::fs::remove_dir_all(&base);
    }
}
