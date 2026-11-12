//! `umr device use`, `umr session`, `umr app`, `umr screenshot`, `umr ui`, `umr input`,
//! `umr device posture` (steps 2.5 / 2.6).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde_json::Value;
use umr_api::methods;

use crate::core_process::CoreTarget;
use crate::fail;

/// Absolute form of a user-given path (relative to the current folder).
pub fn absolute(p: &Path) -> String {
    std::path::absolute(p)
        .unwrap_or_else(|_| p.to_path_buf())
        .display()
        .to_string()
}

async fn call(target: &CoreTarget, method: &str, params: Value) -> Result<Value, String> {
    let mut c = target.connect_or_start().await.map(|(c, _)| c)?;
    c.call(method, params).await.map_err(|e| e.to_string())
}

/// Calls a method and prints the result as JSON.
pub async fn call_print(target: &CoreTarget, method: &str, params: Value) -> ExitCode {
    match call(target, method, params).await {
        Ok(v) => {
            println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e),
    }
}

fn print_session(v: &Value) {
    let s = &v["session"];
    if s.is_null() {
        println!("No session. Start one with `umr device use <profile>`.");
        return;
    }
    println!(
        "Session {}  {}  [{}]",
        s["id"].as_str().unwrap_or("?"),
        s["avd"].as_str().unwrap_or("?"),
        s["state"].as_str().unwrap_or("?")
    );
    if let Some(serial) = s["serial"].as_str() {
        println!("  device: {serial}");
    }
    if let Some(app) = s["app"].as_str() {
        println!("  app:    {app}");
    }
    if let Some(d) = s["detail"].as_str() {
        println!("  note:   {d}");
    }
    let b = &v["boot"];
    if !b.is_null() {
        println!(
            "  boot:   {} in {} s (screen {})",
            b["mode"].as_str().unwrap_or("?"),
            b["seconds"],
            if b["health"]["ok"] == true {
                "renders"
            } else {
                "BLACK"
            }
        );
    }
}

#[allow(clippy::fn_params_excessive_bools)]
pub async fn use_profile(
    target: &CoreTarget,
    profile: &str,
    api: Option<String>,
    cold: bool,
    apk: Option<PathBuf>,
    package: Option<String>,
    json: bool,
) -> ExitCode {
    if !json {
        println!("Starting a session on {profile}...");
    }
    let params = serde_json::json!({
        "profile": profile, "api": api, "cold": cold,
        "apk": apk.as_deref().map(absolute), "package": package,
    });
    match call(target, methods::SESSION_START, params).await {
        Ok(v) if json => {
            println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            ExitCode::SUCCESS
        }
        Ok(v) => {
            print_session(&v);
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e),
    }
}

pub async fn status(target: &CoreTarget, json: bool) -> ExitCode {
    let v = match call(target, methods::SESSION_STATUS, Value::Null).await {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return ExitCode::SUCCESS;
    }
    print_session(&v);
    if let Some(ev) = v["events"].as_array().filter(|e| !e.is_empty()) {
        println!("  recent events:");
        for e in ev.iter().rev().take(6).rev() {
            println!(
                "    {} → {} ({}){}",
                e["from_state"].as_str().unwrap_or("?"),
                e["to_state"].as_str().unwrap_or("?"),
                e["event"].as_str().unwrap_or("?"),
                e["detail"]
                    .as_str()
                    .map(|d| format!(": {d}"))
                    .unwrap_or_default()
            );
        }
    }
    if let Some(ps) = v["processes"].as_array().filter(|p| !p.is_empty()) {
        println!("  supervised processes:");
        for p in ps {
            println!(
                "    {:<34} {:<10} pid {}  restarts {}",
                p["name"].as_str().unwrap_or("?"),
                p["state"].as_str().unwrap_or("?"),
                p["pid"],
                p["restarts"]
            );
        }
    }
    if let Some(ports) = v["ports"].as_array().filter(|p| !p.is_empty()) {
        let list: Vec<String> = ports
            .iter()
            .map(|p| format!("{} ({})", p["port"], p["owner"].as_str().unwrap_or("?")))
            .collect();
        println!("  ports: {}", list.join(", "));
    }
    ExitCode::SUCCESS
}

pub async fn stop(target: &CoreTarget) -> ExitCode {
    match call(target, methods::SESSION_STOP, Value::Null).await {
        Ok(v) => {
            print_session(&v);
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e),
    }
}

pub async fn screenshot(
    target: &CoreTarget,
    out: Option<PathBuf>,
    serial: Option<String>,
    json: bool,
) -> ExitCode {
    let v = match call(
        target,
        methods::DEVICE_SCREENSHOT,
        serde_json::json!({ "target": serial }),
    )
    .await
    {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    let saved = v["path"].as_str().unwrap_or_default().to_string();
    if let Some(o) = &out {
        if let Err(e) = std::fs::copy(&saved, o) {
            return fail(&format!(
                "cannot copy the screenshot to {}: {e}",
                o.display()
            ));
        }
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
    } else {
        println!(
            "{}x{} screenshot via {} in {} ms\n  {}",
            v["width"],
            v["height"],
            v["source"].as_str().unwrap_or("?"),
            v["ms"],
            out.as_deref().map_or(saved.clone(), absolute)
        );
    }
    ExitCode::SUCCESS
}

pub async fn ui(target: &CoreTarget, serial: Option<String>, json: bool) -> ExitCode {
    let v = match call(
        target,
        methods::DEVICE_UI,
        serde_json::json!({ "target": serial }),
    )
    .await
    {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return ExitCode::SUCCESS;
    }
    println!("{} elements ({} ms)", v["count"], v["ms"]);
    for n in v["nodes"].as_array().into_iter().flatten() {
        let label = [n["text"].as_str(), n["desc"].as_str(), n["id"].as_str()]
            .into_iter()
            .flatten()
            .find(|s| !s.is_empty())
            .unwrap_or("");
        println!(
            "  {:<14} {:<40} at {},{}{}",
            n["class"].as_str().unwrap_or("?"),
            label.chars().take(40).collect::<String>(),
            n["center"][0],
            n["center"][1],
            if n["clickable"] == true {
                "  (clickable)"
            } else {
                ""
            }
        );
    }
    ExitCode::SUCCESS
}
