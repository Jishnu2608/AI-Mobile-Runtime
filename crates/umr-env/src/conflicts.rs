//! ENV-005: environment-variable conflicts.
//!
//! These affect the user's own terminals and IDEs. The product itself is protected because it
//! runs tools with an isolated environment ([`crate::recipe`]); the report explains, it never
//! changes the user's variables.

use std::path::{Path, PathBuf};

use crate::context::ScanContext;
use crate::model::{Conflict, Severity};
use crate::pathsearch::{find_all, normalize};

/// Finds conflicts in the environment.
///
/// One linear list of independent rules; kept in one function so the rule order (and report order) is obvious.
#[must_use]
#[allow(clippy::too_many_lines, clippy::if_not_else)]
pub fn detect(ctx: &ScanContext) -> Vec<Conflict> {
    let mut out = Vec::new();
    let warn = |id: &str, message: String, hint: &str| Conflict {
        id: id.into(),
        severity: Severity::Warn,
        message,
        hint: hint.into(),
    };

    // JAVA_HOME
    let java_on_path = find_all(ctx, "java.exe");
    if let Some(jh) = ctx.var("JAVA_HOME") {
        let java = Path::new(jh).join("bin").join("java.exe");
        if !java.is_file() {
            out.push(warn(
                "java_home.invalid",
                format!("JAVA_HOME points to {jh}, which has no bin\\java.exe"),
                "Fix or remove JAVA_HOME in your user environment variables.",
            ));
        } else {
            if !Path::new(jh).join("bin").join("javac.exe").is_file() {
                out.push(warn(
                    "java_home.not_jdk",
                    format!("JAVA_HOME ({jh}) is a JRE without javac; Gradle builds need a JDK"),
                    "Point JAVA_HOME to a JDK, or leave it: the runtime uses its own JDK for builds.",
                ));
            }
            if let Some(first) = java_on_path.first() {
                if normalize(first) != normalize(&java) {
                    out.push(warn(
                        "java_home.path_mismatch",
                        format!(
                            "JAVA_HOME ({jh}) differs from the first java on PATH ({})",
                            first.display()
                        ),
                        "Terminals and IDEs may use different Java versions.",
                    ));
                }
            }
        }
    }
    if java_on_path.len() > 1 {
        out.push(Conflict {
            id: "path.multiple_java".into(),
            severity: Severity::Info,
            message: format!(
                "{} java.exe on PATH: {}",
                java_on_path.len(),
                java_on_path
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            hint: "Only the first one is used by terminals.".into(),
        });
    }

    // ANDROID_HOME / ANDROID_SDK_ROOT
    let home = ctx.var("ANDROID_HOME");
    let root = ctx.var("ANDROID_SDK_ROOT");
    if let (Some(h), Some(r)) = (home, root) {
        if normalize(Path::new(h)) != normalize(Path::new(r)) {
            out.push(warn(
                "android_home.mismatch",
                format!("ANDROID_HOME ({h}) and ANDROID_SDK_ROOT ({r}) point to different SDKs"),
                "Tools pick one or the other; set both to the same folder (ANDROID_SDK_ROOT is deprecated).",
            ));
        }
    }
    for (var, value) in [("ANDROID_HOME", home), ("ANDROID_SDK_ROOT", root)] {
        if let Some(v) = value {
            if !Path::new(v).is_dir() {
                out.push(warn(
                    &format!("{}.missing", var.to_lowercase()),
                    format!("{var} points to {v}, which does not exist"),
                    "Fix or remove the variable.",
                ));
            }
        }
    }

    // Obsolete SDK "tools" on PATH (old sdkmanager/avdmanager, need Java 8)
    let obsolete: Vec<PathBuf> = ctx
        .path_entries()
        .into_iter()
        .filter(|p| {
            let s = p.to_string_lossy().to_lowercase().replace('/', "\\");
            (s.ends_with("\\tools") || s.ends_with("\\tools\\bin"))
                && p.join("..").join("platform-tools").is_dir()
                || s.ends_with("\\tools\\bin") && p.join("sdkmanager.bat").is_file()
        })
        .collect();
    if !obsolete.is_empty() {
        out.push(warn(
            "path.obsolete_sdk_tools",
            format!(
                "PATH contains the obsolete Android SDK 'tools' folder: {}",
                obsolete.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
            ),
            "Remove it from PATH; its sdkmanager needs Java 8 and is replaced by cmdline-tools / Android CLI.",
        ));
    }

    // Global JVM options affect every Java process, including builds.
    for var in ["JAVA_TOOL_OPTIONS", "_JAVA_OPTIONS", "JDK_JAVA_OPTIONS"] {
        if let Some(v) = ctx.var(var) {
            out.push(Conflict {
                id: format!("jvm_options.{}", var.to_lowercase()),
                severity: Severity::Info,
                message: format!("{var} is set globally: {v}"),
                hint: "It applies to every Java program, including Gradle builds started by the runtime.".into(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(c: &[Conflict]) -> Vec<&str> {
        c.iter().map(|x| x.id.as_str()).collect()
    }

    #[test]
    fn clean_environment_has_no_conflicts() {
        let ctx = ScanContext::for_test("C:\\m".into(), &[("PATH", r"C:\Windows\System32")]);
        assert_eq!(detect(&ctx), Vec::<Conflict>::new());
    }

    #[test]
    fn invalid_java_home_and_missing_android_home() {
        let ctx = ScanContext::for_test(
            "C:\\m".into(),
            &[("JAVA_HOME", r"Z:\old\jdk"), ("ANDROID_HOME", r"Z:\nope")],
        );
        let c = detect(&ctx);
        assert_eq!(ids(&c), ["java_home.invalid", "android_home.missing"]);
    }

    #[test]
    fn android_home_mismatch_and_global_jvm_options() {
        let base = std::env::temp_dir().join(format!("umr-conflicts-{}", std::process::id()));
        let (a, b) = (base.join("a"), base.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let ctx = ScanContext::for_test(
            "C:\\m".into(),
            &[
                ("ANDROID_HOME", &a.display().to_string()),
                ("ANDROID_SDK_ROOT", &b.display().to_string()),
                ("_JAVA_OPTIONS", "-Xmx1g"),
            ],
        );
        assert_eq!(
            ids(&detect(&ctx)),
            ["android_home.mismatch", "jvm_options._java_options"]
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn jre_java_home_and_path_mismatch_and_multiple_java() {
        let base = std::env::temp_dir().join(format!("umr-conflicts-java-{}", std::process::id()));
        let (jre, jdk) = (base.join("jre"), base.join("jdk"));
        for d in [&jre, &jdk] {
            std::fs::create_dir_all(d.join("bin")).unwrap();
            std::fs::write(d.join("bin").join("java.exe"), "").unwrap();
        }
        std::fs::write(jdk.join("bin").join("javac.exe"), "").unwrap();
        let path = format!(
            "{};{}",
            jdk.join("bin").display(),
            jre.join("bin").display()
        );
        let ctx = ScanContext::for_test(
            "C:\\m".into(),
            &[
                ("JAVA_HOME", &jre.display().to_string()),
                ("PATH", &path),
                ("PATHEXT", ".EXE"),
            ],
        );
        assert_eq!(
            ids(&detect(&ctx)),
            [
                "java_home.not_jdk",
                "java_home.path_mismatch",
                "path.multiple_java"
            ]
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn obsolete_tools_folder_on_path() {
        let base = std::env::temp_dir().join(format!("umr-conflicts-tools-{}", std::process::id()));
        let tools_bin = base.join("sdk").join("tools").join("bin");
        std::fs::create_dir_all(&tools_bin).unwrap();
        std::fs::write(tools_bin.join("sdkmanager.bat"), "").unwrap();
        let ctx = ScanContext::for_test(
            "C:\\m".into(),
            &[("PATH", &tools_bin.display().to_string())],
        );
        assert_eq!(ids(&detect(&ctx)), ["path.obsolete_sdk_tools"]);
        let _ = std::fs::remove_dir_all(&base);
    }
}
