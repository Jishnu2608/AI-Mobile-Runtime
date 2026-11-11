//! ENV-001 host facts and ENV-007 virtualization detection.
//!
//! Virtualization rule (from POC-000/POC-002 notes): never trust the firmware flag alone —
//! it reads false while Hyper-V/WHPX is running. Acceleration is available when WHPX reports a
//! running hypervisor, or AEHD is installed, or `emulator -accel-check` says so.
//! Enabling virtualization is not done here (needs admin consent: INS-005).

use crate::context::ScanContext;
use crate::model::{HostInfo, Proxy, Virtualization};

/// Collects host facts. `accel_check` is the output of `emulator -accel-check`, if run.
#[must_use]
pub fn probe(ctx: &ScanContext, accel_check: Option<String>) -> HostInfo {
    let version = umr_platform::os::windows_version().ok();
    let mem = umr_platform::system::memory().ok();
    let disk_path = nearest_existing(&ctx.managed_root);
    let disk = disk_path
        .as_deref()
        .and_then(|p| umr_platform::system::disk_space(p).ok());
    HostInfo {
        windows_build: version.as_ref().map_or(0, |v| v.build),
        windows_display_version: version.as_ref().and_then(|v| v.display_version.clone()),
        windows_11: version
            .as_ref()
            .is_some_and(umr_platform::os::WindowsVersion::is_windows_11_or_later),
        arch: umr_platform::system::native_arch().to_string(),
        ram_total_bytes: mem.map_or(0, |m| m.total_bytes),
        ram_available_bytes: mem.map_or(0, |m| m.available_bytes),
        disk_free_bytes: disk.map_or(0, |d| d.free_bytes),
        disk_total_bytes: disk.map_or(0, |d| d.total_bytes),
        long_paths_enabled: umr_platform::os::long_paths_enabled().unwrap_or(false),
        virtualization: virtualization(accel_check),
        proxy: proxy(ctx),
    }
}

fn virtualization(accel_check: Option<String>) -> Virtualization {
    use umr_platform::system::{
        aehd_installed, hypervisor_present, virtualization_firmware_enabled, whpx_status,
        WhpxStatus,
    };
    let whpx = whpx_status();
    let aehd = aehd_installed();
    let accel_says_ok = accel_check.as_deref().map(accel_check_ok);
    let acceleration_available =
        accel_says_ok.unwrap_or(matches!(whpx, WhpxStatus::Usable) || aehd);
    Virtualization {
        firmware_enabled: virtualization_firmware_enabled(),
        hypervisor_present: hypervisor_present(),
        whpx: match whpx {
            WhpxStatus::Usable => "usable".into(),
            WhpxStatus::NoHypervisor => "no-hypervisor".into(),
            WhpxStatus::NotInstalled => "not-installed".into(),
            WhpxStatus::Error(e) => format!("error: {e}"),
        },
        aehd_installed: aehd,
        emulator_accel_check: accel_check,
        acceleration_available,
    }
}

/// `emulator -accel-check` prints `accel:\n0\n<text>\naccel` where 0 means usable.
#[must_use]
pub fn accel_check_ok(output: &str) -> bool {
    let mut lines = output.lines().map(str::trim).skip_while(|l| *l != "accel:");
    lines.next();
    lines.next() == Some("0")
}

fn proxy(ctx: &ScanContext) -> Proxy {
    let system_proxy = if ctx.use_registry {
        let key = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";
        let enabled = umr_platform::registry::read_dword(
            umr_platform::registry::Hive::CurrentUser,
            key,
            "ProxyEnable",
        )
        .ok()
        .flatten()
            == Some(1);
        if enabled {
            umr_platform::registry::read_string(
                umr_platform::registry::Hive::CurrentUser,
                key,
                "ProxyServer",
            )
            .ok()
            .flatten()
        } else {
            None
        }
    } else {
        None
    };
    Proxy {
        http_proxy: ctx.var("HTTP_PROXY").map(String::from),
        https_proxy: ctx.var("HTTPS_PROXY").map(String::from),
        no_proxy: ctx.var("NO_PROXY").map(String::from),
        system_proxy,
    }
}

/// The path itself or its closest existing parent (the data folder may not exist yet).
fn nearest_existing(p: &std::path::Path) -> Option<std::path::PathBuf> {
    p.ancestors()
        .find(|a| a.exists())
        .map(std::path::Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accel_check_parsing() {
        assert!(accel_check_ok(
            "accel:\n0\nWHPX(10.0.26300) is installed and usable.\naccel\n"
        ));
        assert!(!accel_check_ok("accel:\n4\nWHPX is not installed\naccel\n"));
        assert!(!accel_check_ok("garbage"));
    }

    #[test]
    fn proxy_from_env_vars() {
        let ctx = ScanContext::for_test(
            "C:\\x".into(),
            &[("https_proxy", "http://p:8080"), ("NO_PROXY", "localhost")],
        );
        let p = proxy(&ctx);
        assert_eq!(p.https_proxy.as_deref(), Some("http://p:8080"));
        assert_eq!(p.no_proxy.as_deref(), Some("localhost"));
        assert_eq!(p.http_proxy, None);
    }

    #[test]
    fn probe_this_machine() {
        let ctx = ScanContext::for_test(std::env::temp_dir().join("umr-host-probe-missing"), &[]);
        let h = probe(&ctx, None);
        assert!(h.windows_build > 10_000);
        assert!(h.ram_total_bytes > 0);
        assert!(
            h.disk_total_bytes > 0,
            "uses the nearest existing parent folder"
        );
    }
}
