//! `umr preview` (Phase 4): the docked preview window, its status and the latency self-test.

use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

use serde_json::Value;
use umr_api::methods;

use crate::core_process::CoreTarget;
use crate::fail;

/// `umr-desktop.exe` next to this `umr.exe`.
fn desktop_exe() -> Result<PathBuf, String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let p = me.with_file_name("umr-desktop.exe");
    if p.is_file() {
        Ok(p)
    } else {
        Err(format!(
            "the desktop app is not installed next to umr ({})",
            p.display()
        ))
    }
}

/// Opens the preview window unless one is connected already. Returns whether one was started.
pub async fn open_window(target: &CoreTarget, dock: &str) -> Result<bool, String> {
    let mut c = target.connect_or_start().await.map(|(c, _)| c)?;
    let st: Value = c
        .call(methods::PREVIEW_STATUS, Value::Null)
        .await
        .map_err(|e| e.to_string())?;
    if st["windows"].as_u64().unwrap_or(0) > 0 {
        return Ok(false);
    }
    // otherwise the window inherits our stdout/stderr pipes and callers capturing `umr` hang
    umr_platform::process::disinherit_std_handles()
        .map_err(|e| format!("cannot prepare the window start: {e}"))?;
    Command::new(desktop_exe()?)
        .args(["--preview", "--dock", dock])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot open the preview window: {e}"))?;
    Ok(true)
}

pub async fn open(target: &CoreTarget, dock: &str) -> ExitCode {
    if !matches!(dock, "right" | "left" | "float") {
        return fail("--dock is right, left or float");
    }
    match open_window(target, dock).await {
        Ok(true) => {
            println!("Preview window opened ({dock}).");
            ExitCode::SUCCESS
        }
        Ok(false) => {
            println!("A preview window is already open.");
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e),
    }
}

pub async fn status(target: &CoreTarget, json: bool) -> ExitCode {
    let mut c = match target.connect_or_start().await {
        Ok((c, _)) => c,
        Err(e) => return fail(&e),
    };
    let v: Value = match c.call(methods::PREVIEW_STATUS, Value::Null).await {
        Ok(v) => v,
        Err(e) => return fail(&e.to_string()),
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return ExitCode::SUCCESS;
    }
    if v["running"] != true {
        println!("No preview window has been opened yet (`umr preview`).");
        return ExitCode::SUCCESS;
    }
    let s = &v["stats"];
    println!(
        "Preview: {} window(s) · device {} · {} frames sent ({} fps) · {} skipped",
        v["windows"],
        v["device"].as_str().unwrap_or("none"),
        s["frames"],
        s["fps_sent"],
        s["dropped"]
    );
    println!(
        "  ours (NFR-02a, p95): input {} ms + frame {} ms + hop {} ms + draw {} ms = {} ms (target ≤ 20)",
        s["input"]["p95_ms"], s["frame"]["p95_ms"], s["ui"]["hop_p95"], s["ui"]["draw_p95"], s["pipeline_p95_ms"]
    );
    println!("  window: {} fps drawn", s["ui"]["fps"]);
    ExitCode::SUCCESS
}

pub async fn selftest(target: &CoreTarget, taps: u32, json: bool) -> ExitCode {
    let mut c = match target.connect_or_start().await {
        Ok((c, _)) => c,
        Err(e) => return fail(&e),
    };
    if !json {
        println!("Latency self-test: {taps} taps on the home screen, then a 2 s drag…");
    }
    let v: Value = match c
        .call(
            methods::PREVIEW_SELFTEST,
            serde_json::json!({ "taps": taps }),
        )
        .await
    {
        Ok(v) => v,
        Err(e) => return fail(&e.to_string()),
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return ExitCode::SUCCESS;
    }
    let t = &v["selftest"];
    let r = &v["verdict"];
    let mark = |b: &Value| if *b == true { "met" } else { "NOT met" };
    println!(
        "  taps detected {} of {}; touch → frame at the Core p50 {} ms, p95 {} ms",
        t["detected"], t["taps"], t["latency"]["p50_ms"], t["latency"]["p95_ms"]
    );
    println!(
        "  NFR-02b end to end (window): p50 {} ms, p95 {} ms (≤ 80 / ≤ 150) — {}",
        r["nfr_02b"]["p50_ms"],
        r["nfr_02b"]["p95_ms"],
        mark(&r["nfr_02b"]["met"])
    );
    println!(
        "  NFR-02a ours: {} ms p95 (≤ 20) — {}{}",
        r["nfr_02a"]["pipeline_p95_ms"],
        mark(&r["nfr_02a"]["met"]),
        if r["nfr_02a"]["window_measured"] == true {
            ""
        } else {
            " (no preview window open: window part not included)"
        }
    );
    println!(
        "  NFR-03 frame rate: {} fps (≥ 30) — {}",
        r["nfr_03"]["fps"],
        mark(&r["nfr_03"]["met"])
    );
    if r["host_overloaded"] == true {
        println!("  The host looks overloaded (high latency or low frame rate): close heavy apps and try again.");
    }
    ExitCode::SUCCESS
}

pub async fn rotate(target: &CoreTarget, direction: &str) -> ExitCode {
    let mut c = match target.connect_or_start().await {
        Ok((c, _)) => c,
        Err(e) => return fail(&e),
    };
    match c
        .call::<Value>(
            methods::DEVICE_ROTATE,
            serde_json::json!({ "direction": direction }),
        )
        .await
    {
        Ok(v) => {
            println!(
                "Rotated {} ({direction}).",
                v["serial"].as_str().unwrap_or("?")
            );
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e.to_string()),
    }
}
