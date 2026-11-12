//! `umr run`, `umr reload`, `umr adapter check` (Phase 3).

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use umr_api::methods;

use crate::core_process::CoreTarget;
use crate::fail;

fn abs(p: &Path) -> String {
    crate::session_cmd::absolute(p)
}

/// One event as a terminal line (progress and problems; plain log lines are left out).
fn event_line(e: &Value) -> Option<String> {
    match e["event"].as_str()? {
        "phase" => Some(format!(
            "  [{}] {}",
            e["phase"].as_str().unwrap_or("?"),
            e["message"].as_str().unwrap_or("")
        )),
        "problem" => {
            let loc = match (e["file"].as_str(), e["line"].as_u64()) {
                (Some(f), Some(l)) => format!("{f}:{l}: "),
                (Some(f), None) => format!("{f}: "),
                _ => String::new(),
            };
            Some(format!(
                "  {} {loc}{}",
                if e["severity"] == "warning" {
                    "warning"
                } else {
                    "ERROR"
                },
                e["message"].as_str().unwrap_or("")
            ))
        }
        "app_started" => Some(format!(
            "  app started: {}",
            e["package"].as_str().unwrap_or("?")
        )),
        "reloaded" => Some(format!(
            "  reloaded ({}) in {} ms",
            e["kind"].as_str().unwrap_or("?"),
            e["ms"]
        )),
        "app_stopped" => Some(format!(
            "  app stopped: {}",
            e["reason"].as_str().unwrap_or("")
        )),
        _ => None,
    }
}

/// Prints adapter events from a second connection while `call` runs.
async fn with_progress(
    target: &CoreTarget,
    method: &str,
    params: Value,
    quiet: bool,
) -> Result<Value, String> {
    let done = Arc::new(AtomicBool::new(false));
    let watcher = if quiet {
        None
    } else {
        let (mut w, _) = target.connect_or_start().await?;
        let d = done.clone();
        Some(tokio::spawn(async move {
            let mut since = 0u64;
            // phase lines repeat for every Gradle task: show at most one per second
            let mut last_phase: Option<std::time::Instant> = None;
            let mut first = true;
            loop {
                let stop = d.load(Ordering::SeqCst);
                if let Ok(v) = w
                    .call::<Value>(
                        methods::PROJECT_STATUS,
                        serde_json::json!({ "since": since, "limit": 200 }),
                    )
                    .await
                {
                    // the first poll may still show the previous run's events
                    if !first {
                        for e in v["events"].as_array().into_iter().flatten() {
                            let is_phase = e["event"] == "phase";
                            if is_phase
                                && last_phase.is_some_and(|t| t.elapsed() < Duration::from_secs(1))
                            {
                                continue;
                            }
                            if let Some(l) = event_line(e) {
                                if is_phase {
                                    last_phase = Some(std::time::Instant::now());
                                }
                                println!("{l}");
                            }
                        }
                    }
                    since = v["cursor"].as_u64().unwrap_or(since);
                }
                first = false;
                if stop {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(700)).await;
            }
        }))
    };
    let mut c = target.connect_or_start().await.map(|(c, _)| c)?;
    let r = c
        .call::<Value>(method, params)
        .await
        .map_err(|e| e.to_string());
    done.store(true, Ordering::SeqCst);
    if let Some(w) = watcher {
        let _ = w.await;
    }
    r
}

fn print_problems(v: &Value) {
    let ps = v["problems"].as_array().map_or(&[][..], Vec::as_slice);
    if ps.is_empty() {
        return;
    }
    println!("Problems:");
    for p in ps {
        let mut e = p.clone();
        e["event"] = "problem".into();
        if let Some(l) = event_line(&e) {
            println!("{l}");
        }
    }
}

pub async fn run(
    target: &CoreTarget,
    path: Option<PathBuf>,
    profile: Option<String>,
    cold: bool,
    watch: bool,
    json: bool,
) -> ExitCode {
    let dir = path.unwrap_or_else(|| PathBuf::from("."));
    let params =
        serde_json::json!({ "path": abs(&dir), "profile": profile, "cold": cold, "watch": watch });
    if !json {
        println!("Running {} ...", abs(&dir));
    }
    let v = match with_progress(target, methods::PROJECT_RUN, params, json).await {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return if v["status"] == "running" {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    if v["status"] == "running" {
        println!(
            "{} is running on {} ({} adapter, {} s)",
            v["package"].as_str().unwrap_or("?"),
            v["serial"].as_str().unwrap_or("?"),
            v["adapter"].as_str().unwrap_or("?"),
            v["seconds"]
        );
        let c = &v["capabilities"];
        let mut kinds = vec!["full"];
        if c["hot_reload"] == true {
            kinds.insert(0, "hot");
        }
        if c["hot_restart"] == true {
            kinds.insert(kinds.len() - 1, "restart");
        }
        if v["watching"] == true {
            println!(
                "Watching for saved files (reloads by itself); `umr reload [{}]` also works.",
                kinds.join("|")
            );
        } else {
            println!("Reload with `umr reload [{}]`.", kinds.join("|"));
        }
        ExitCode::SUCCESS
    } else {
        println!(
            "{}: {}",
            v["status"].as_str().unwrap_or("failed"),
            v["error"].as_str().unwrap_or("")
        );
        print_problems(&v);
        ExitCode::FAILURE
    }
}

pub async fn stop(target: &CoreTarget) -> ExitCode {
    let mut c = match target.connect_or_start().await {
        Ok((c, _)) => c,
        Err(e) => return fail(&e),
    };
    match c.call::<Value>(methods::PROJECT_STOP, Value::Null).await {
        Ok(v) if v["stopped"].is_null() => {
            println!("Nothing was running.");
            ExitCode::SUCCESS
        }
        Ok(v) => {
            println!(
                "Stopped {}.",
                v["stopped"]["package"].as_str().unwrap_or("?")
            );
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e.to_string()),
    }
}

pub async fn reload(target: &CoreTarget, kind: Option<String>, json: bool) -> ExitCode {
    let params = serde_json::json!({ "kind": kind });
    let v = match with_progress(target, methods::PROJECT_RELOAD, params, json).await {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
    } else if v["status"] == "reloaded" {
        println!(
            "Reloaded ({}) in {} ms.",
            v["kind"].as_str().unwrap_or("?"),
            v["ms"]
        );
    } else {
        println!(
            "{}: {}",
            v["status"].as_str().unwrap_or("failed"),
            v["error"].as_str().unwrap_or("")
        );
        print_problems(&v);
    }
    if v["status"] == "reloaded" {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

pub async fn check(
    target: &CoreTarget,
    path: Option<PathBuf>,
    profile: Option<String>,
    json: bool,
) -> ExitCode {
    let dir = path.unwrap_or_else(|| PathBuf::from("."));
    let params = serde_json::json!({ "path": abs(&dir), "profile": profile });
    if !json {
        println!(
            "Adapter contract check on {} (start, reloads, stop) ...",
            abs(&dir)
        );
    }
    let v = match with_progress(target, methods::ADAPTER_CHECK, params, true).await {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
    } else {
        println!("Adapter {}", v["report"]["adapter"].as_str().unwrap_or("?"));
        for s in v["report"]["steps"].as_array().into_iter().flatten() {
            println!(
                "  [{}] {:<14} {:>7} ms  {}",
                if s["ok"] == true { "PASS" } else { "FAIL" },
                s["name"].as_str().unwrap_or("?"),
                s["ms"],
                s["detail"].as_str().unwrap_or("")
            );
        }
        println!(
            "{}",
            if v["passed"] == true {
                "CONTRACT: PASSED"
            } else {
                "CONTRACT: FAILED"
            }
        );
    }
    if v["passed"] == true {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_lines() {
        let p = serde_json::json!({"event":"problem","severity":"error","message":"Unresolved reference 'x'","file":"app/src/A.kt","line":3,"source":"kotlinc"});
        assert_eq!(
            event_line(&p).unwrap(),
            "  ERROR app/src/A.kt:3: Unresolved reference 'x'"
        );
        let ph = serde_json::json!({"event":"phase","phase":"building","message":":app:compileDebugKotlin"});
        assert_eq!(
            event_line(&ph).unwrap(),
            "  [building] :app:compileDebugKotlin"
        );
        assert!(event_line(&serde_json::json!({"event":"log","line":"x"})).is_none());
    }
}
