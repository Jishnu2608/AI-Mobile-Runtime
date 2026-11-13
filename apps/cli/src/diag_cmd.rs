//! `umr problems` and `umr logs` (DIA-004): the current run's classified problems and its log
//! lines (tool output, progress, the app's device log). Text arrives sanitised from the Core.

use std::process::ExitCode;
use std::time::Duration;

use serde_json::{json, Value};
use umr_api::methods;

use crate::core_process::CoreTarget;
use crate::fail;

const ORDER: [&str; 6] = [
    "crash",
    "build",
    "runtime",
    "environment",
    "device",
    "network",
];

fn location(e: &Value) -> String {
    match (e["file"].as_str(), e["line"].as_u64()) {
        (Some(f), Some(l)) => format!("{f}:{l}  "),
        (Some(f), None) => format!("{f}  "),
        _ => String::new(),
    }
}

/// One problem as a terminal line.
#[must_use]
pub fn problem_line(e: &Value) -> String {
    let count = e["count"].as_u64().unwrap_or(1);
    format!(
        "  {} {}  {}{}{}  [{}]",
        if e["severity"] == "warning" {
            "warning"
        } else {
            "ERROR"
        },
        match e["origin"].as_str() {
            Some("app") => "(your code)",
            Some("environment") => "(environment)",
            _ => "",
        },
        location(e),
        e["message"]
            .as_str()
            .unwrap_or("")
            .lines()
            .next()
            .unwrap_or(""),
        if count > 1 {
            format!("  ×{count}")
        } else {
            String::new()
        },
        e["fingerprint"].as_str().unwrap_or("")
    )
}

pub async fn problems(target: &CoreTarget, category: Option<String>, json: bool) -> ExitCode {
    let mut c = match target.connect_or_start().await {
        Ok((c, _)) => c,
        Err(e) => return fail(&e),
    };
    let v: Value = match c
        .call(
            methods::DIAGNOSTICS_LIST,
            json!({ "category": category, "limit": 500 }),
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
    let entries = v["entries"].as_array().cloned().unwrap_or_default();
    let what = v["run"]["project"]
        .as_str()
        .map_or_else(String::new, |p| format!(" in {p}"));
    if entries.is_empty() {
        println!("No problems{what}.");
        return ExitCode::SUCCESS;
    }
    println!("{} problem(s){what}:", entries.len());
    for cat in ORDER {
        let group: Vec<&Value> = entries.iter().filter(|e| e["category"] == cat).collect();
        if group.is_empty() {
            continue;
        }
        println!("{cat}:");
        for e in group {
            println!("{}", problem_line(e));
        }
    }
    println!("Details and stack: `umr problems show <fingerprint>`.");
    ExitCode::SUCCESS
}

pub async fn show(target: &CoreTarget, fingerprint: &str, json: bool) -> ExitCode {
    let mut c = match target.connect_or_start().await {
        Ok((c, _)) => c,
        Err(e) => return fail(&e),
    };
    let e: Value = match c
        .call(
            methods::DIAGNOSTICS_GET,
            json!({ "fingerprint": fingerprint }),
        )
        .await
    {
        Ok(v) => v,
        Err(e) => return fail(&e.to_string()),
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&e).unwrap_or_default());
        return ExitCode::SUCCESS;
    }
    println!(
        "{} · {} · {} · seen {}×",
        e["category"].as_str().unwrap_or("?"),
        e["origin"].as_str().unwrap_or("?"),
        e["source"].as_str().unwrap_or("?"),
        e["count"]
    );
    let loc = location(&e);
    if !loc.is_empty() {
        println!("at {}", loc.trim_end());
    }
    println!("{}", e["message"].as_str().unwrap_or(""));
    for f in e["stack"].as_array().into_iter().flatten() {
        println!("    {}", f.as_str().unwrap_or(""));
    }
    ExitCode::SUCCESS
}

fn log_line(l: &Value) -> String {
    let tag = l["tag"]
        .as_str()
        .map_or_else(String::new, |t| format!("{t}: "));
    format!(
        "{} {:<6} {}{}",
        l["level"].as_str().unwrap_or("I"),
        l["source"].as_str().unwrap_or(""),
        tag,
        l["text"].as_str().unwrap_or("")
    )
}

#[allow(clippy::too_many_arguments)]
pub async fn logs(
    target: &CoreTarget,
    source: Option<String>,
    level: Option<String>,
    grep: Option<String>,
    lines: usize,
    follow: bool,
    json: bool,
) -> ExitCode {
    let mut c = match target.connect_or_start().await {
        Ok((c, _)) => c,
        Err(e) => return fail(&e),
    };
    let mut since = 0u64;
    let mut limit = lines;
    loop {
        let v: Value = match c
            .call(
                methods::LOGS_TAIL,
                json!({ "since": since, "limit": limit, "source": source, "level": level, "grep": grep }),
            )
            .await
        {
            Ok(v) => v,
            Err(e) => return fail(&e.to_string()),
        };
        for l in v["lines"].as_array().into_iter().flatten() {
            if json {
                println!("{l}");
            } else {
                println!("{}", log_line(l));
            }
        }
        since = v["cursor"].as_u64().unwrap_or(since);
        if !follow {
            return ExitCode::SUCCESS;
        }
        limit = 5000;
        tokio::time::sleep(Duration::from_millis(700)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_read_well() {
        let e = json!({"severity": "error", "origin": "app", "file": "lib/main.dart", "line": 30,
            "message": "Expected ';'\nmore", "count": 3, "fingerprint": "ab12"});
        assert_eq!(
            problem_line(&e),
            "  ERROR (your code)  lib/main.dart:30  Expected ';'  ×3  [ab12]"
        );
        let l = json!({"level": "E", "source": "logcat", "tag": "flutter", "text": "boom"});
        assert_eq!(log_line(&l), "E logcat flutter: boom");
    }
}
