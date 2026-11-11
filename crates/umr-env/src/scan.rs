//! Orchestrates a full environment scan (ENV-006).

use std::path::PathBuf;
use std::time::Instant;

use crate::context::ScanContext;
use crate::model::EnvReport;
use crate::recipe::{child_env, IsolationDirs, Selection};

/// Runs every probe. Independent probes run in parallel; total time is bounded by the
/// per-tool timeouts. Read-only: nothing is installed or changed (except creating the managed
/// isolation folders used as the tools' home/temp).
#[must_use]
pub fn scan(ctx: &ScanContext) -> EnvReport {
    let started = Instant::now();
    if ctx.run_checks {
        let _ = IsolationDirs::under(&ctx.managed_root).create();
    }
    // Functional checks run with the isolated environment, never polluting the user's profile.
    let env = child_env(ctx, &Selection::default());

    let (sdks, (tools, studio_home), conflicts) = std::thread::scope(|s| {
        let sdks = s.spawn(|| crate::android::detect(ctx, &env));
        let conflicts = s.spawn(|| crate::conflicts::detect(ctx));
        let sdks = sdks.join().unwrap_or_default();
        let sdk_roots: Vec<String> = sdks.iter().map(|x| x.root.clone()).collect();
        let tools = crate::tools::detect(ctx, &sdk_roots, &env);
        (sdks, tools, conflicts.join().unwrap_or_default())
    });
    let (jdks, host) = std::thread::scope(|s| {
        let jdks = s.spawn(|| crate::jdk::detect(ctx, studio_home.as_deref()));
        let accel = crate::android::accel_check(ctx, &sdks, &env);
        let host = crate::host::probe(ctx, accel);
        (jdks.join().unwrap_or_default(), host)
    });

    let mut report = EnvReport {
        scanned_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
        duration_ms: 0,
        host,
        jdks,
        android_sdks: sdks,
        tools,
        conflicts,
        readiness: Vec::new(),
    };
    report.readiness = crate::readiness::evaluate(&report);
    report.duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    report
}

/// Convenience for callers: scan the real machine with this managed root.
#[must_use]
pub fn scan_system(managed_root: PathBuf) -> EnvReport {
    scan(&ScanContext::from_system(managed_root))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real scan of this machine: structure and plausibility only (machines differ).
    #[test]
    fn real_scan_completes_quickly_and_is_serialisable() {
        let root = std::env::temp_dir().join(format!("umr-scan-{}", std::process::id()));
        let report = scan_system(root.clone());
        assert!(report.host.windows_build > 10_000);
        assert_eq!(report.readiness.len(), 4);
        assert!(
            report.duration_ms < 60_000,
            "scan took {} ms",
            report.duration_ms
        );
        let json = serde_json::to_string(&report).unwrap();
        let back: EnvReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back, report);
        eprintln!(
            "scan: {} ms, jdks {}, sdks {}, tools {:?}, conflicts {}",
            report.duration_ms,
            report.jdks.len(),
            report.android_sdks.len(),
            report
                .tools
                .iter()
                .map(|t| format!("{} {:?}", t.name, t.version))
                .collect::<Vec<_>>(),
            report.conflicts.len()
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
