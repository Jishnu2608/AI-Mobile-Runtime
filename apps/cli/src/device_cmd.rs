//! `umr device list` (DEV-001/007).

use std::process::ExitCode;

use serde_json::Value;
use umr_api::methods;

use crate::core_process::CoreTarget;
use crate::fail;

pub async fn list(target: &CoreTarget, json_out: bool) -> ExitCode {
    let mut c = match target.connect_or_start().await {
        Ok((c, _)) => c,
        Err(e) => return fail(&e),
    };
    let v: Value = match c.call(methods::DEVICE_LIST, Value::Null).await {
        Ok(v) => v,
        Err(e) => return fail(&e.to_string()),
    };
    if json_out {
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return ExitCode::SUCCESS;
    }
    let server = &v["server"];
    println!(
        "adb server  port {} (private) · protocol {} · {}",
        server["port"],
        server["protocol"]
            .as_u64()
            .map_or_else(|| "?".into(), |p| p.to_string()),
        if server["connected"] == true {
            "tracking"
        } else {
            "not connected"
        }
    );
    if let Some(e) = server["last_error"].as_str() {
        println!("            {e}");
    }
    let devices = v["devices"].as_array().cloned().unwrap_or_default();
    if devices.is_empty() {
        println!("\nNo devices. (Emulators started by the product appear here; Phase 2.3 adds `umr device use`.)");
    } else {
        println!();
        for d in &devices {
            let model = d["model"].as_str().unwrap_or("");
            println!(
                "  {:<22} {:<13} {model}",
                d["serial"].as_str().unwrap_or("?"),
                d["state"].as_str().unwrap_or("?")
            );
        }
    }
    let others = v["coexistence"]["servers"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for s in &others {
        println!(
            "\nOther adb server on port {} (protocol {}, {} devices): left alone — probably Android Studio or your own `adb`.",
            s["port"], s["protocol"], s["devices"]
        );
    }
    ExitCode::SUCCESS
}

pub async fn profiles(target: &CoreTarget, json_out: bool) -> ExitCode {
    let mut c = match target.connect_or_start().await {
        Ok((c, _)) => c,
        Err(e) => return fail(&e),
    };
    let v: Value = match c.call(methods::DEVICE_PROFILES, Value::Null).await {
        Ok(v) => v,
        Err(e) => return fail(&e.to_string()),
    };
    if json_out {
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return ExitCode::SUCCESS;
    }
    let api = v["api"].as_str().unwrap_or("none installed");
    println!("System image API {api}\n");
    for item in v["profiles"].as_array().into_iter().flatten() {
        let p = &item["profile"];
        let fold = match p["fold"]["kind"].as_str() {
            Some(k) => format!("  {k} foldable"),
            None => String::new(),
        };
        let created = if item["avd"]["exists"] == true {
            "created"
        } else {
            "not created yet"
        };
        println!(
            "  {:<9} {:<15} {:>4}\"  {}×{} @{}dpi{}{}  [{created}]",
            p["id"].as_str().unwrap_or("?"),
            p["class"].as_str().unwrap_or("?"),
            p["diagonal_in"],
            p["width"],
            p["height"],
            p["density"],
            if p["default"] == true {
                "  (default)"
            } else {
                ""
            },
            fold
        );
    }
    ExitCode::SUCCESS
}

pub async fn create(target: &CoreTarget, profile: &str, api: Option<String>) -> ExitCode {
    let mut c = match target.connect_or_start().await {
        Ok((c, _)) => c,
        Err(e) => return fail(&e),
    };
    let params = serde_json::json!({ "profile": profile, "api": api });
    match c.call::<Value>(methods::DEVICE_CREATE, params).await {
        Ok(v) => {
            println!(
                "{} ready (API {})\n  {}",
                v["name"].as_str().unwrap_or("?"),
                v["api"].as_str().unwrap_or("?"),
                v["dir"].as_str().unwrap_or("")
            );
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e.to_string()),
    }
}

pub async fn boot(
    target: &CoreTarget,
    profile: &str,
    api: Option<String>,
    cold: bool,
    window: bool,
    gpu: Option<String>,
    json_out: bool,
) -> ExitCode {
    let mut c = match target.connect_or_start().await {
        Ok((c, _)) => c,
        Err(e) => return fail(&e),
    };
    if !json_out {
        println!("Booting {profile} (this takes up to a few minutes on a cold boot)...");
    }
    let params = serde_json::json!({ "profile": profile, "api": api, "cold": cold, "window": window, "gpu": gpu });
    let v: Value = match c.call(methods::DEVICE_BOOT, params).await {
        Ok(v) => v,
        Err(e) => return fail(&e.to_string()),
    };
    if json_out {
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return ExitCode::SUCCESS;
    }
    println!(
        "{} running as {} ({} boot, {} s; boot completed after {} s)",
        v["avd"].as_str().unwrap_or("?"),
        v["serial"].as_str().unwrap_or("?"),
        v["mode"].as_str().unwrap_or("?"),
        v["seconds"],
        v["boot_completed_seconds"]
    );
    if let Some(r) = v["cold_reason"].as_str() {
        println!("  cold boot because: {r}");
    }
    println!(
        "  screen check: {} ({}x{}, {:.0} % lit)",
        if v["health"]["ok"] == true {
            "renders"
        } else {
            "BLACK"
        },
        v["health"]["width"],
        v["health"]["height"],
        v["health"]["lit"].as_f64().unwrap_or(0.0) * 100.0
    );
    if v["snapshot_saved"] == true {
        println!(
            "  warm snapshot saved (GPU mode {})",
            v["gpu"].as_str().unwrap_or("?")
        );
    }
    ExitCode::SUCCESS
}

pub async fn stop(target: &CoreTarget, profile: &str, api: Option<String>) -> ExitCode {
    let mut c = match target.connect_or_start().await {
        Ok((c, _)) => c,
        Err(e) => return fail(&e),
    };
    let params = serde_json::json!({ "profile": profile, "api": api });
    match c.call::<Value>(methods::DEVICE_STOP, params).await {
        Ok(v) if v["stopped"] == true => {
            println!("{} stopped", v["avd"].as_str().unwrap_or("?"));
            ExitCode::SUCCESS
        }
        Ok(v) => {
            println!(
                "{} was not running (started by the product)",
                v["avd"].as_str().unwrap_or("?")
            );
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e.to_string()),
    }
}
