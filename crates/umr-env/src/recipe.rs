//! ENV-008 / ENV-009: the environment every tool started by the runtime gets.
//!
//! Goals (from POC-001/006 findings):
//! - **No global changes**: user/system variables are never modified; only child processes
//!   get these values.
//! - **Our tools first**: chosen JDK / SDK / Node / Flutter `bin` folders lead PATH; other
//!   `java.exe` folders are removed from the child PATH so a stray JRE is never picked.
//! - **Isolation**: Android user state (AVDs, emulator config, adb keys), temp files (netsimd,
//!   Android CLI build logs, Java unix-socket files) go below the managed root instead of
//!   `%USERPROFILE%\.android` and `%TEMP%`.
//! - **JVM loopback fix (ENV-009)**: `-Djdk.net.unixdomain.tmpdir=<managed>\tmp\jvm` is appended
//!   to `JAVA_TOOL_OPTIONS`; without it Android CLI and Gradle fail with
//!   "Unable to establish loopback connection" on some machines.
//! - **No telemetry** from Flutter, Dart, Expo, Android CLI unless the user opts in.
//!
//! Gradle, pub and npm caches stay in their usual user locations (shared with the user's other
//! projects), as decided in the planning document §11.6.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::context::ScanContext;
use crate::pathsearch::normalize;

/// The JVM option that fixes the loopback-socket crash.
pub const JVM_TMPDIR_PROPERTY: &str = "-Djdk.net.unixdomain.tmpdir=";

/// What a child process should run with.
#[derive(Debug, Clone, Default)]
pub struct Selection {
    pub jdk_home: Option<PathBuf>,
    pub android_sdk: Option<PathBuf>,
    pub node_dir: Option<PathBuf>,
    pub flutter_root: Option<PathBuf>,
    /// Extra folders to put first on PATH.
    pub extra_path: Vec<PathBuf>,
}

/// Managed folders used by the recipe.
#[derive(Debug, Clone)]
pub struct IsolationDirs {
    pub android_user_home: PathBuf,
    pub avd_home: PathBuf,
    pub tmp: PathBuf,
    pub jvm_tmp: PathBuf,
}

impl IsolationDirs {
    #[must_use]
    pub fn under(managed_root: &Path) -> Self {
        let android_user_home = managed_root.join("android-user-home");
        let tmp = managed_root.join("tmp");
        Self {
            avd_home: android_user_home.join("avd"),
            jvm_tmp: tmp.join("jvm"),
            android_user_home,
            tmp,
        }
    }

    /// Creates the folders (tools fail if they do not exist).
    pub fn create(&self) -> std::io::Result<()> {
        for d in [
            &self.android_user_home,
            &self.avd_home,
            &self.tmp,
            &self.jvm_tmp,
        ] {
            std::fs::create_dir_all(d)?;
        }
        Ok(())
    }
}

/// Builds the variables to set on a child process (on top of the inherited environment).
#[must_use]
pub fn child_env(ctx: &ScanContext, sel: &Selection) -> HashMap<String, String> {
    let dirs = IsolationDirs::under(&ctx.managed_root);
    let mut env = HashMap::new();
    let s = |p: &Path| p.display().to_string();

    // Tool locations
    if let Some(jdk) = &sel.jdk_home {
        env.insert("JAVA_HOME".into(), s(jdk));
    }
    if let Some(sdk) = &sel.android_sdk {
        env.insert("ANDROID_HOME".into(), s(sdk));
        env.insert("ANDROID_SDK_ROOT".into(), s(sdk));
    }
    if let Some(f) = &sel.flutter_root {
        env.insert("FLUTTER_ROOT".into(), s(f));
    }

    // Isolation of Android user state and temp files
    env.insert("ANDROID_USER_HOME".into(), s(&dirs.android_user_home));
    env.insert("ANDROID_EMULATOR_HOME".into(), s(&dirs.android_user_home));
    env.insert("ANDROID_AVD_HOME".into(), s(&dirs.avd_home));
    env.insert("TEMP".into(), s(&dirs.tmp));
    env.insert("TMP".into(), s(&dirs.tmp));

    // ENV-009: JVM loopback fix, keeping the user's own options
    let fix = format!("{JVM_TMPDIR_PROPERTY}{}", s(&dirs.jvm_tmp));
    let jto = match ctx.var("JAVA_TOOL_OPTIONS") {
        Some(existing) if existing.contains(JVM_TMPDIR_PROPERTY) => existing.to_string(),
        Some(existing) => format!("{existing} {fix}"),
        None => fix,
    };
    env.insert("JAVA_TOOL_OPTIONS".into(), jto);

    // No telemetry by default
    env.insert("FLUTTER_SUPPRESS_ANALYTICS".into(), "true".into());
    env.insert("DART_SUPPRESS_ANALYTICS".into(), "true".into());
    env.insert("EXPO_NO_TELEMETRY".into(), "1".into());
    env.insert("NEXT_TELEMETRY_DISABLED".into(), "1".into());

    env.insert("PATH".into(), child_path(ctx, sel));
    env
}

/// PATH for children: chosen tool folders first, then the user's PATH minus other Java folders.
#[must_use]
pub fn child_path(ctx: &ScanContext, sel: &Selection) -> String {
    let mut first: Vec<PathBuf> = Vec::new();
    first.extend(sel.extra_path.iter().cloned());
    if let Some(j) = &sel.jdk_home {
        first.push(j.join("bin"));
    }
    if let Some(sdk) = &sel.android_sdk {
        first.push(sdk.join("platform-tools"));
        first.push(sdk.join("emulator"));
        first.push(sdk.join("cmdline-tools").join("latest").join("bin"));
    }
    if let Some(n) = &sel.node_dir {
        first.push(n.clone());
    }
    if let Some(f) = &sel.flutter_root {
        first.push(f.join("bin"));
    }
    let chosen_java = sel.jdk_home.as_ref().map(|j| normalize(&j.join("bin")));
    let mut out: Vec<PathBuf> = Vec::new();
    for p in first.into_iter().chain(ctx.path_entries()) {
        let is_other_java = p.join("java.exe").is_file()
            && Some(normalize(&p)) != chosen_java
            && sel.jdk_home.is_some();
        if is_other_java {
            continue;
        }
        if !out.iter().any(|o| normalize(o) == normalize(&p)) {
            out.push(p);
        }
    }
    out.iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(";")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolation_and_telemetry_variables_are_set() {
        let ctx = ScanContext::for_test(r"D:\UMR".into(), &[("PATH", r"C:\Windows")]);
        let env = child_env(&ctx, &Selection::default());
        assert_eq!(env["ANDROID_USER_HOME"], r"D:\UMR\android-user-home");
        assert_eq!(env["ANDROID_EMULATOR_HOME"], r"D:\UMR\android-user-home");
        assert_eq!(env["ANDROID_AVD_HOME"], r"D:\UMR\android-user-home\avd");
        assert_eq!(env["TEMP"], r"D:\UMR\tmp");
        assert_eq!(
            env["JAVA_TOOL_OPTIONS"],
            r"-Djdk.net.unixdomain.tmpdir=D:\UMR\tmp\jvm"
        );
        assert_eq!(env["FLUTTER_SUPPRESS_ANALYTICS"], "true");
        assert_eq!(env["EXPO_NO_TELEMETRY"], "1");
        assert!(
            !env.contains_key("GRADLE_USER_HOME"),
            "Gradle cache stays shared"
        );
    }

    #[test]
    fn user_jvm_options_are_kept_and_fix_not_duplicated() {
        let ctx = ScanContext::for_test(r"D:\UMR".into(), &[("JAVA_TOOL_OPTIONS", "-Xmx2g")]);
        assert_eq!(
            child_env(&ctx, &Selection::default())["JAVA_TOOL_OPTIONS"],
            r"-Xmx2g -Djdk.net.unixdomain.tmpdir=D:\UMR\tmp\jvm"
        );
        let ctx = ScanContext::for_test(
            r"D:\UMR".into(),
            &[("JAVA_TOOL_OPTIONS", r"-Djdk.net.unixdomain.tmpdir=E:\t")],
        );
        assert_eq!(
            child_env(&ctx, &Selection::default())["JAVA_TOOL_OPTIONS"],
            r"-Djdk.net.unixdomain.tmpdir=E:\t"
        );
    }

    #[test]
    fn chosen_tools_lead_path_and_other_java_is_removed() {
        let base = std::env::temp_dir().join(format!("umr-recipe-{}", std::process::id()));
        let (old_jre, jdk) = (base.join("oldjre").join("bin"), base.join("jdk"));
        std::fs::create_dir_all(&old_jre).unwrap();
        std::fs::write(old_jre.join("java.exe"), "").unwrap();
        std::fs::create_dir_all(jdk.join("bin")).unwrap();
        std::fs::write(jdk.join("bin").join("java.exe"), "").unwrap();
        let user_path = format!(r"{};C:\Windows;C:\Windows", old_jre.display());
        let ctx = ScanContext::for_test(base.join("root"), &[("PATH", &user_path)]);
        let sel = Selection {
            jdk_home: Some(jdk.clone()),
            android_sdk: Some(PathBuf::from(r"D:\sdk")),
            ..Selection::default()
        };
        let path = child_path(&ctx, &sel);
        let parts: Vec<&str> = path.split(';').collect();
        assert_eq!(parts[0], jdk.join("bin").display().to_string());
        assert_eq!(parts[1], r"D:\sdk\platform-tools");
        assert!(
            !path.contains("oldjre"),
            "other Java folder must be removed: {path}"
        );
        assert_eq!(
            parts.iter().filter(|p| **p == r"C:\Windows").count(),
            1,
            "duplicates removed"
        );
        let env = child_env(&ctx, &sel);
        assert_eq!(env["ANDROID_HOME"], r"D:\sdk");
        assert_eq!(env["ANDROID_SDK_ROOT"], r"D:\sdk");
        let _ = std::fs::remove_dir_all(&base);
    }
}
