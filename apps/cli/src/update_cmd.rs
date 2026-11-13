//! `umr update` (REL-002): checks the release manifest and installs a newer version.
//!
//! Installed copies (the NSIS per-user installer leaves `uninstall.exe` next to `umr.exe`) are
//! updated by downloading the new installer, verifying its size and SHA-256 against the manifest
//! (and its Authenticode signature when `[update] require_signed`), stopping the Runtime Core and
//! running the installer silently. Portable / developer copies are told where to download.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use serde_json::json;
use umr_installer::self_update::{self, Release};

use crate::core_process::CoreTarget;
use crate::fail;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

fn settings(target: &CoreTarget) -> Result<(String, String, bool), String> {
    let cfg =
        umr_storage::config::Config::load_in(target.paths.root()).map_err(|e| e.to_string())?;
    let url = std::env::var("UMR_UPDATE_URL")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or(cfg.update.manifest_url);
    Ok((url, cfg.update.channel, cfg.update.require_signed))
}

/// The installer leaves `uninstall.exe` in its folder; a portable or developer copy has none.
fn installed_dir() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    if !dir.join("uninstall.exe").is_file() {
        return None;
    }
    // the full long path: the uninstaller removes shortcuts only when their target matches the
    // install folder exactly, so an 8.3 short path (C:\Users\ABCDEF~1\…) would leave them behind
    let long = std::fs::canonicalize(&dir).ok()?;
    let s = long.display().to_string();
    Some(PathBuf::from(s.strip_prefix(r"\\?\").unwrap_or(&s)))
}

fn windows_build() -> u32 {
    umr_platform::os::windows_version().map_or(0, |v| v.build)
}

fn signature_ok(path: &Path) -> bool {
    let script = format!(
        "(Get-AuthenticodeSignature -LiteralPath '{}').Status -eq 'Valid'",
        path.display().to_string().replace('\'', "''")
    );
    Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "True")
}

fn describe(r: &Release) -> String {
    let size = r.file("installer").map_or(0, |f| f.size);
    #[allow(clippy::cast_precision_loss)]
    let mb = size as f64 / 1_048_576.0;
    format!(
        "{} ({}, {}{}, installer {mb:.1} MB){}",
        r.version,
        r.channel,
        r.date,
        if r.signed { ", signed" } else { ", unsigned" },
        if r.notes_url.is_empty() {
            String::new()
        } else {
            format!("\n  notes: {}", r.notes_url)
        }
    )
}

pub async fn run(target: &CoreTarget, check_only: bool, yes: bool, json_out: bool) -> ExitCode {
    let (url, channel, require_signed) = match settings(target) {
        Ok(s) => s,
        Err(e) => return fail(&e),
    };
    if url.is_empty() {
        if json_out {
            println!("{}", json!({ "current": VERSION, "configured": false }));
        } else {
            println!("AI Mobile Runtime {VERSION}. No update channel is configured ([update] manifest_url in config.toml).");
        }
        return ExitCode::SUCCESS;
    }
    let u = url.clone();
    let manifest = match tokio::task::spawn_blocking(move || self_update::fetch_manifest(&u)).await
    {
        Ok(Ok(m)) => m,
        Ok(Err(e)) => return fail(&format!("cannot read the release manifest: {e}")),
        Err(e) => return fail(&e.to_string()),
    };
    let newer = self_update::pick(&manifest, VERSION, &channel, windows_build()).cloned();
    if json_out {
        println!(
            "{}",
            json!({ "current": VERSION, "channel": channel, "available": newer })
        );
        if check_only || newer.is_none() {
            return ExitCode::SUCCESS;
        }
    }
    let Some(rel) = newer else {
        println!("AI Mobile Runtime {VERSION} is up to date ({channel}).");
        return ExitCode::SUCCESS;
    };
    println!("Update available: {VERSION} → {}", describe(&rel));
    if check_only {
        println!("Install it with `umr update`.");
        return ExitCode::SUCCESS;
    }
    let Some(dir) = installed_dir() else {
        let link = rel
            .file("portable")
            .map_or(rel.notes_url.as_str(), |f| f.url.as_str());
        println!("This copy was not installed with the installer (portable or developer build); download the new version: {link}");
        return ExitCode::SUCCESS;
    };
    let Some(file) = rel.file("installer").cloned() else {
        return fail("the release has no installer");
    };
    if require_signed && !rel.signed {
        return fail("this release is unsigned and [update] require_signed is on");
    }
    if !yes {
        print!("Download and install {} now? The Runtime Core and its emulators stop while it installs. [y/N] ", rel.version);
        let _ = std::io::Write::flush(&mut std::io::stdout());
        let mut a = String::new();
        let _ = std::io::stdin().read_line(&mut a);
        if !matches!(a.trim(), "y" | "Y" | "yes") {
            println!("Not updated.");
            return ExitCode::SUCCESS;
        }
    }
    // a fresh folder per run: nothing left from an earlier (or someone else's) download is used
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let tmp = std::env::temp_dir().join(format!("umr-update-{}-{nonce}", std::process::id()));
    let u = url.clone();
    let f = file.clone();
    let t = tmp.clone();
    println!("Downloading {} …", file.name);
    let installer =
        match tokio::task::spawn_blocking(move || self_update::download(&u, &f, &t)).await {
            Ok(Ok(p)) => p,
            Ok(Err(e)) => return fail(&format!("download failed: {e}")),
            Err(e) => return fail(&e.to_string()),
        };
    println!("Verified size and SHA-256.");
    if require_signed && !signature_ok(&installer) {
        let _ = std::fs::remove_file(&installer);
        return fail("the installer has no valid Authenticode signature; not installed");
    }
    // the installer replaces umr-core.exe: close the preview window first (it would start the
    // Core again), then stop the Core; the installer's hook does the same
    let _ = Command::new("taskkill")
        .args(["/F", "/IM", "umr-desktop.exe"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    crate::stop_quiet(target).await;
    match spawn_installer(&installer, &dir) {
        Ok(()) => {
            println!("Installing {} in the background (about a minute). Run `umr --version` afterwards to check.", rel.version);
            ExitCode::SUCCESS
        }
        Err(e) => fail(&format!("cannot start the installer: {e}")),
    }
}

/// Runs the installer silently into the same folder, detached so it can replace this umr.exe
/// after we exit.
fn spawn_installer(installer: &Path, dir: &Path) -> std::io::Result<()> {
    let mut cmd = Command::new(installer);
    cmd.arg("/S").arg(format!("/D={}", dir.display()));
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    cmd.spawn().map(|_| ())
}
