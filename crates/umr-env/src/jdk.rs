//! ENV-002: JDK detection and functional check.
//!
//! Candidates (deduplicated by real path; one JDK may have several origins):
//! `JAVA_HOME`, every `java.exe` on PATH, the registry (`HKLM\SOFTWARE\JavaSoft\JDK\*`),
//! well-known folders under Program Files, Android Studio's bundled JBR, and the managed
//! `<root>\jdks\*`. Version and vendor come from the JDK's `release` file; the functional check
//! runs `java -version`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::context::ScanContext;
use crate::model::{Health, Jdk, Origin};
use crate::pathsearch::{find_all, normalize};
use crate::runner::{run, CHECK_TIMEOUT};

/// Folders under Program Files that hold JDK installs (one JDK per subfolder).
const VENDOR_DIRS: &[&str] = &[
    "Java",
    "Eclipse Adoptium",
    "Microsoft",
    "Zulu",
    "Amazon Corretto",
    "BellSoft",
    "OpenJDK",
    "Semeru",
];

/// Detects all JDKs/JREs.
#[must_use]
pub fn detect(ctx: &ScanContext, studio_home: Option<&Path>) -> Vec<Jdk> {
    let mut found: Vec<(PathBuf, Origin)> = Vec::new();

    if let Some(h) = ctx.var("JAVA_HOME") {
        found.push((PathBuf::from(h), Origin::EnvVar));
    }
    for java in find_all(ctx, "java.exe") {
        if let Some(home) = java.parent().and_then(Path::parent) {
            found.push((home.to_path_buf(), Origin::Path));
        }
    }
    if ctx.use_registry {
        for home in registry_homes() {
            found.push((home, Origin::Registry));
        }
    }
    for pf in &ctx.program_files {
        for vendor in VENDOR_DIRS {
            for home in subdirs(&pf.join(vendor)) {
                found.push((home, Origin::CommonDir));
            }
        }
    }
    if let Some(studio) = studio_home {
        found.push((studio.join("jbr"), Origin::AndroidStudio));
    }
    for home in subdirs(&ctx.managed_root.join("jdks")) {
        found.push((home, Origin::Managed));
    }

    // Merge duplicates, keep only folders that look like a Java install.
    let mut by_key: Vec<(String, PathBuf, Vec<Origin>)> = Vec::new();
    for (home, origin) in found {
        if !home.join("bin").join("java.exe").is_file() && origin != Origin::EnvVar {
            continue;
        }
        let key = normalize(&home);
        if let Some(entry) = by_key.iter_mut().find(|(k, _, _)| *k == key) {
            if !entry.2.contains(&origin) {
                entry.2.push(origin);
            }
        } else {
            by_key.push((key, home, vec![origin]));
        }
    }

    by_key
        .into_iter()
        .map(|(_, home, origins)| inspect(ctx, &home, origins))
        .collect()
}

fn inspect(ctx: &ScanContext, home: &Path, origins: Vec<Origin>) -> Jdk {
    let java = home.join("bin").join("java.exe");
    let release = std::fs::read_to_string(home.join("release")).ok();
    let (mut version, vendor) = release.as_deref().map(parse_release).unwrap_or_default();
    let health = if !java.is_file() {
        Health::Broken(format!("{} does not exist", java.display()))
    } else if !ctx.run_checks {
        Health::NotChecked("checks disabled".into())
    } else {
        match run(&java, &["-version"], &HashMap::new(), CHECK_TIMEOUT) {
            Ok(o) if o.success() => {
                if version.is_none() {
                    version = parse_java_version_output(&o.combined());
                }
                Health::Ok
            }
            Ok(o) => Health::Broken(format!(
                "java -version exited with {:?}: {}",
                o.code,
                first_line(&o.combined())
            )),
            Err(e) => Health::Broken(format!("java -version: {e}")),
        }
    };
    Jdk {
        home: home.display().to_string(),
        major: version.as_deref().and_then(major_version),
        version,
        vendor,
        is_jdk: home.join("bin").join("javac.exe").is_file(),
        origins,
        health,
    }
}

/// Parses the JDK `release` file: `JAVA_VERSION="17.0.21"`, `IMPLEMENTOR="Eclipse Adoptium"`.
#[must_use]
pub fn parse_release(text: &str) -> (Option<String>, Option<String>) {
    let get = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix('=')))
            .map(|v| v.trim().trim_matches('"').to_string())
            .filter(|v| !v.is_empty())
    };
    (get("JAVA_VERSION"), get("IMPLEMENTOR"))
}

/// Parses `openjdk version "17.0.21" 2026-10-20` / `java version "1.8.0_381"`.
#[must_use]
pub fn parse_java_version_output(text: &str) -> Option<String> {
    let line = text.lines().find(|l| l.contains(" version \""))?;
    let start = line.find('"')? + 1;
    let end = start + line[start..].find('"')?;
    Some(line[start..end].to_string())
}

/// Feature version: `17.0.21` → 17, `21` → 21, `1.8.0_381` → 8.
#[must_use]
pub fn major_version(v: &str) -> Option<u32> {
    let mut parts = v.split(['.', '_', '-', '+']);
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("").trim()
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default()
}

fn registry_homes() -> Vec<PathBuf> {
    use umr_platform::registry::{read_string, subkeys, Hive};
    let mut out = Vec::new();
    for base in [
        r"SOFTWARE\JavaSoft\JDK",
        r"SOFTWARE\JavaSoft\Java Development Kit",
    ] {
        for ver in subkeys(Hive::LocalMachine, base).unwrap_or_default() {
            if let Ok(Some(h)) =
                read_string(Hive::LocalMachine, &format!(r"{base}\{ver}"), "JavaHome")
            {
                out.push(PathBuf::from(h));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parsing() {
        assert_eq!(
            parse_java_version_output("openjdk version \"17.0.21\" 2026-10-20\nOpenJDK Runtime"),
            Some("17.0.21".into())
        );
        assert_eq!(
            parse_java_version_output("java version \"1.8.0_381\"\n"),
            Some("1.8.0_381".into())
        );
        assert_eq!(
            parse_java_version_output(
                "Picked up JAVA_TOOL_OPTIONS: x\nopenjdk version \"21\" 2023-09-19"
            ),
            Some("21".into())
        );
        assert_eq!(major_version("17.0.21"), Some(17));
        assert_eq!(major_version("1.8.0_381"), Some(8));
        assert_eq!(major_version("21"), Some(21));
        assert_eq!(major_version("25-ea"), Some(25));
    }

    #[test]
    fn release_file_parsing() {
        let (v, vendor) =
            parse_release("IMPLEMENTOR=\"Eclipse Adoptium\"\nJAVA_VERSION=\"17.0.21\"\n");
        assert_eq!(v.as_deref(), Some("17.0.21"));
        assert_eq!(vendor.as_deref(), Some("Eclipse Adoptium"));
    }

    fn fake_jdk(dir: &Path, version: &str, with_javac: bool) {
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(dir.join("bin").join("java.exe"), "").unwrap();
        if with_javac {
            std::fs::write(dir.join("bin").join("javac.exe"), "").unwrap();
        }
        std::fs::write(
            dir.join("release"),
            format!("JAVA_VERSION=\"{version}\"\nIMPLEMENTOR=\"Test\"\n"),
        )
        .unwrap();
    }

    #[test]
    fn merges_origins_and_finds_managed_and_jres() {
        let base = std::env::temp_dir().join(format!("umr-jdk-detect-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let managed = base.join("root");
        let a = managed.join("jdks").join("temurin-17");
        let jre = base.join("jre8");
        fake_jdk(&a, "17.0.21", true);
        fake_jdk(&jre, "1.8.0_381", false);
        let path = format!("{};{}", a.join("bin").display(), jre.join("bin").display());
        let ctx = ScanContext::for_test(
            managed,
            &[
                ("JAVA_HOME", &a.display().to_string()),
                ("PATH", &path),
                ("PATHEXT", ".EXE"),
            ],
        );
        let jdks = detect(&ctx, None);
        assert_eq!(jdks.len(), 2, "{jdks:#?}");
        let t = jdks.iter().find(|j| j.major == Some(17)).unwrap();
        assert!(t.is_jdk);
        assert_eq!(
            t.origins,
            vec![Origin::EnvVar, Origin::Path, Origin::Managed]
        );
        let j8 = jdks.iter().find(|j| j.major == Some(8)).unwrap();
        assert!(!j8.is_jdk, "JRE without javac");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn broken_java_home_is_reported() {
        let ctx = ScanContext::for_test("C:\\none".into(), &[("JAVA_HOME", r"Z:\missing\jdk")]);
        let jdks = detect(&ctx, None);
        assert_eq!(jdks.len(), 1);
        assert!(matches!(jdks[0].health, Health::Broken(_)));
    }
}
