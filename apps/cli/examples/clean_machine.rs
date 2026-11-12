//! TEST-001 driver for the "clean machine" workflow (`.github/workflows/clean-machine.yml`).
//! Test-only: it is an example, not part of the shipped CLI.
//!
//! ```text
//! cargo run --release -p umr-cli --example clean_machine -- <project> [--out summary.json] [--no-interrupt]
//! ```
//!
//! 1. `install.prepare` for the project; accepts the licences through the API, as a user does
//!    in the wizard (the product's CLI only accepts licences typed into a terminal).
//! 2. `install.start`; while a download is running, the Runtime Core is **killed** (hard), then
//!    started again: recovery must leave no broken component.
//! 3. `install.start` again: the install must finish (downloads resume).
//! 4. Re-plan: every component must now be reused.
//!
//! Exit code 0 = all checks passed.

use std::process::ExitCode;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use umr_api::client::Client;
use umr_api::methods;
use umr_core::launcher::CoreTarget;

struct Run {
    log: Vec<String>,
    failures: Vec<String>,
}

impl Run {
    fn say(&mut self, msg: impl Into<String>) {
        let m = msg.into();
        println!("{m}");
        self.log.push(m);
    }

    fn check(&mut self, ok: bool, what: &str) {
        let line = format!("[{}] {what}", if ok { "PASS" } else { "FAIL" });
        self.say(line);
        if !ok {
            self.failures.push(what.to_string());
        }
    }
}

async fn call(c: &mut Client, method: &str, params: Value) -> Result<Value, String> {
    c.call_raw(method, params)
        .await
        .map_err(|e| format!("{method}: {e}"))
}

async fn connect(target: &CoreTarget) -> Result<Client, String> {
    target.connect_or_start().await.map(|(c, _)| c)
}

/// Polls an install job. With `kill_at` (percent), returns `None` once any download passed it.
async fn follow(
    c: &mut Client,
    run: &mut Run,
    job: &str,
    kill_at: Option<u64>,
) -> Result<Option<Value>, String> {
    let start = Instant::now();
    let mut last = String::new();
    loop {
        let st = call(c, methods::INSTALL_STATUS, json!({ "job_id": job })).await?;
        let line: Vec<String> = st["items"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|i| {
                format!(
                    "{}={}",
                    i["component"].as_str().unwrap_or("?"),
                    i["state"].as_str().unwrap_or("?")
                )
            })
            .collect();
        let line = line.join(" ");
        if line != last {
            run.say(format!("  {line}"));
            last = line;
        }
        if let Some(pct) = kill_at {
            let downloading = st["items"].as_array().into_iter().flatten().any(|i| {
                i["state"] == "downloading"
                    && i["bytes_total"].as_u64().is_some_and(|t| {
                        t > 0 && i["bytes_done"].as_u64().unwrap_or(0) * 100 >= t * pct
                    })
            });
            if downloading {
                return Ok(None); // caller kills the Core now
            }
        }
        if st["state"] != "running" {
            return Ok(Some(st));
        }
        if start.elapsed() > Duration::from_secs(80 * 60) {
            return Err("install did not finish within 80 minutes".into());
        }
        tokio::time::sleep(Duration::from_millis(1000)).await;
    }
}

async fn accept_licences(c: &mut Client, run: &mut Run, project: &str) -> Result<Value, String> {
    let prep = call(c, methods::INSTALL_PREPARE, json!({ "path": project })).await?;
    let plan = &prep["plan"];
    run.say(format!(
        "plan: profile {} · {} items · download ~{} MB · disk ok {}",
        plan["profile"],
        plan["items"].as_array().map_or(0, Vec::len),
        plan["total_download_bytes"].as_u64().unwrap_or(0) / 1_048_576,
        plan["disk_ok"]
    ));
    for item in plan["items"].as_array().into_iter().flatten() {
        run.say(format!(
            "  {:<8} {} {}",
            item["action"]["kind"].as_str().unwrap_or("?"),
            item["title"].as_str().unwrap_or("?"),
            item["package"].as_str().unwrap_or("")
        ));
    }
    for a in plan["admin_actions"].as_array().into_iter().flatten() {
        run.say(format!(
            "  admin action listed: {} (required {})",
            a["id"], a["required"]
        ));
    }
    for l in prep["prepared"]["licences"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if l["accepted"] == true {
            continue;
        }
        run.say(format!(
            "accepting licence {} (sha256 {}) for this test run",
            l["id"], l["sha256"]
        ));
        call(
            c,
            methods::INSTALL_ACCEPT_LICENCE,
            json!({ "id": l["id"], "sha256": l["sha256"] }),
        )
        .await?;
    }
    Ok(prep)
}

async fn scenario(project: &str, interrupt: bool, run: &mut Run) -> Result<(), String> {
    let target = CoreTarget::resolve()?;
    let mut c = connect(&target).await?;
    let info = call(&mut c, methods::INFO, Value::Null).await?;
    run.say(format!(
        "core: home {} · toolchains {}",
        info["data_root"], info["toolchains_root"]
    ));

    let prep = accept_licences(&mut c, run, project).await?;
    let host_accel = call(&mut c, methods::ENV_SCAN, Value::Null).await?["host"]["virtualization"]
        ["acceleration_available"]
        == true;
    if !host_accel {
        let listed = prep["plan"]["admin_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|a| a["id"] == "enable-whpx" && a["required"] == true);
        run.check(listed, "no emulator acceleration on this machine: plan lists 'Turn on WHPX' as required (exp. 7)");
    }

    if interrupt {
        let job = call(&mut c, methods::INSTALL_START, json!({ "path": project })).await?;
        let job_id = job["job_id"].as_str().unwrap_or_default().to_string();
        run.say(format!(
            "install job {job_id} started; will kill the Core during a download"
        ));
        if let Some(st) = follow(&mut c, run, &job_id, Some(25)).await? {
            run.say(format!(
                "install finished before the kill point ({}); interruption step skipped",
                st["state"]
            ));
        } else {
            {
                let pid = info["pid"].as_u64().unwrap_or(0);
                run.say(format!("killing Runtime Core pid {pid} (taskkill /F)"));
                let killed = std::process::Command::new("taskkill")
                    .args(["/F", "/PID", &pid.to_string()])
                    .status()
                    .is_ok_and(|s| s.success());
                run.check(killed, "Core killed mid-download");
                drop(c);
                tokio::time::sleep(Duration::from_secs(3)).await;
                c = connect(&target).await?; // fresh Core: recovery runs before the API opens
                let comps = call(&mut c, methods::COMPONENTS_LIST, Value::Null).await?;
                let broken: Vec<String> = comps["components"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|x| x["ok"] != true)
                    .map(|x| x["id"].as_str().unwrap_or("?").to_string())
                    .collect();
                run.check(
                    broken.is_empty(),
                    &format!("after the kill no component is half-installed (broken: {broken:?})"),
                );
            }
        }
    }

    let job = call(&mut c, methods::INSTALL_START, json!({ "path": project })).await?;
    let job_id = job["job_id"].as_str().unwrap_or_default().to_string();
    run.say(format!("install job {job_id} started (to completion)"));
    let st = follow(&mut c, run, &job_id, None)
        .await?
        .unwrap_or(Value::Null);
    run.check(
        st["state"] == "done",
        &format!("install finished: state {} {}", st["state"], st["message"]),
    );

    let comps = call(&mut c, methods::COMPONENTS_LIST, Value::Null).await?;
    let all_ok = comps["components"]
        .as_array()
        .is_some_and(|a| !a.is_empty() && a.iter().all(|x| x["ok"] == true));
    run.check(
        all_ok,
        "every installed component verifies (JDK starts, key files present)",
    );

    let plan = call(
        &mut c,
        methods::PROJECT_PLAN,
        json!({ "path": project, "rescan": true }),
    )
    .await?;
    let installs: Vec<String> = plan["plan"]["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|i| i["action"]["kind"] == "install")
        .map(|i| i["component"].as_str().unwrap_or("?").to_string())
        .collect();
    run.check(
        installs.is_empty(),
        &format!("re-plan reuses everything (still to install: {installs:?})"),
    );
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(project) = args.first().cloned() else {
        eprintln!("usage: clean_machine <project> [--out summary.json] [--no-interrupt]");
        return ExitCode::from(2);
    };
    let out = args
        .iter()
        .position(|a| a == "--out")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let interrupt = !args.iter().any(|a| a == "--no-interrupt");
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("cannot start runtime: {e}");
            return ExitCode::from(2);
        }
    };
    let mut run = Run {
        log: Vec::new(),
        failures: Vec::new(),
    };
    let result = rt.block_on(scenario(&project, interrupt, &mut run));
    if let Err(e) = &result {
        run.say(format!("[FAIL] scenario stopped: {e}"));
        run.failures.push(e.clone());
    }
    if let Some(out) = out {
        let summary =
            json!({ "passed": run.failures.is_empty(), "failures": run.failures, "log": run.log });
        let _ = std::fs::write(
            &out,
            serde_json::to_vec_pretty(&summary).unwrap_or_default(),
        );
    }
    if run.failures.is_empty() {
        println!("CLEAN MACHINE: PASSED");
        ExitCode::SUCCESS
    } else {
        println!("CLEAN MACHINE: FAILED ({})", run.failures.len());
        ExitCode::FAILURE
    }
}
