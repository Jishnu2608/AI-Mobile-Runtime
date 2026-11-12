//! `umr project [path]` and `umr trust [path] [--revoke]` (PROJ-001).

use std::io::IsTerminal;
use std::process::ExitCode;

use serde_json::{json, Value};
use umr_api::methods::{self, TrustResult};
use umr_resolver::detect::Detection;

use crate::core_process::CoreTarget;
use crate::fail;

pub async fn show(target: &CoreTarget, dir: String, json_out: bool) -> ExitCode {
    let mut c = match target.connect_or_start().await {
        Ok((c, _)) => c,
        Err(e) => return fail(&e),
    };
    let v: Value = match c
        .call(methods::PROJECT_DETECT, json!({ "path": dir }))
        .await
    {
        Ok(v) => v,
        Err(e) => return fail(&e.to_string()),
    };
    if json_out {
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return ExitCode::SUCCESS;
    }
    let d: Detection = match serde_json::from_value(v["detection"].clone()) {
        Ok(d) => d,
        Err(e) => return fail(&format!("unexpected answer: {e}")),
    };
    println!("Project    {}", d.root);
    println!(
        "Framework  {:?}  (confidence {} %)",
        d.framework, d.confidence
    );
    for e in &d.evidence {
        println!("  evidence: {} — {}", e.file, e.says);
    }
    if !d.candidates.is_empty() {
        println!("\nProjects inside this folder:");
        for c in &d.candidates {
            println!("  {:<40} {:?} ({} %)", c.path, c.framework, c.confidence);
        }
        println!("Run `umr project <folder>` on one of them.");
    }
    let trusted = v["trusted"].as_bool().unwrap_or(false);
    println!(
        "\nTrusted    {}",
        if trusted {
            "yes"
        } else {
            "no — builds will ask first (`umr trust` to trust this folder)"
        }
    );
    ExitCode::SUCCESS
}

pub async fn trust(target: &CoreTarget, dir: String, revoke: bool, yes: bool) -> ExitCode {
    if !revoke {
        println!("Trust {dir}?");
        println!("Building a project runs its own code: Gradle build scripts, npm install scripts and Flutter plugins.");
        println!("Only trust folders whose source you know.");
        if !yes {
            if !std::io::stdin().is_terminal() {
                return fail("trust must be confirmed in a terminal (or pass --yes)");
            }
            print!("Trust this folder? [y/N] ");
            let _ = std::io::Write::flush(&mut std::io::stdout());
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            if !matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
                println!("Not trusted.");
                return ExitCode::FAILURE;
            }
        }
    }
    let mut c = match target.connect_or_start().await {
        Ok((c, _)) => c,
        Err(e) => return fail(&e),
    };
    match c
        .call::<TrustResult>(
            methods::PROJECT_TRUST,
            json!({ "path": dir, "trusted": !revoke }),
        )
        .await
    {
        Ok(r) => {
            println!(
                "{} is {}.",
                r.path,
                if r.trusted {
                    "trusted"
                } else {
                    "no longer trusted"
                }
            );
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e.to_string()),
    }
}
