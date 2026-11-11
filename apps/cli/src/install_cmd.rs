//! `umr install [path] [--yes]` — installs what `umr plan` lists (INS-001…004).
//!
//! 1. `install.prepare`: exact downloads and the full licence texts.
//! 2. Each licence that is not accepted yet is printed in full; the user must type `accept`.
//!    This works only in an interactive terminal: a script or agent cannot accept for the user.
//! 3. Confirmation (skipped with `--yes`), then `install.start` and live progress.
//!    Ctrl+C cancels; downloads are kept and resume next time.

use std::io::{BufRead, IsTerminal, Write};
use std::process::ExitCode;
use std::time::Duration;

use serde_json::{json, Value};
use umr_api::client::Client;
use umr_api::methods::{self, AcceptLicenceParams, InstallStarted};
use umr_installer::engine::{ItemState, JobState, JobStatus, Prepared, StepKind};

use crate::core_process::CoreTarget;
use crate::fail;

fn gb(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let g = bytes as f64 / 1_073_741_824.0;
    if g >= 1.0 {
        format!("{g:.1} GB")
    } else {
        format!("{} MB", bytes / 1_048_576)
    }
}

fn ask(prompt: &str) -> Option<String> {
    print!("{prompt}");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).ok()?;
    Some(line.trim().to_string())
}

fn state_word(s: ItemState) -> &'static str {
    match s {
        ItemState::Pending => "waiting",
        ItemState::Downloading => "downloading",
        ItemState::Extracting => "extracting",
        ItemState::Committing => "finishing",
        ItemState::Installing => "installing with Android CLI",
        ItemState::Done => "installed",
        ItemState::AlreadyInstalled => "already installed",
        ItemState::Failed => "FAILED",
        ItemState::Skipped => "skipped",
    }
}

pub async fn install(target: &CoreTarget, dir: String, yes: bool) -> ExitCode {
    let (mut client, started) = match target.connect_or_start().await {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    if started {
        println!("(started the Runtime Core)\n");
    }
    println!("Reading release information from official sources…");
    let v: Value = match client
        .call(methods::INSTALL_PREPARE, json!({ "path": dir }))
        .await
    {
        Ok(v) => v,
        Err(e) => return fail(&e.to_string()),
    };
    let prepared: Prepared = match serde_json::from_value(v["prepared"].clone()) {
        Ok(p) => p,
        Err(e) => return fail(&format!("unexpected answer: {e}")),
    };
    let installable: Vec<_> = prepared
        .steps
        .iter()
        .filter(|s| !matches!(s.kind, StepKind::Unsupported { .. }))
        .collect();
    for s in &prepared.steps {
        if let StepKind::Unsupported { reason } = &s.kind {
            println!("[manual]  {}: {reason}", s.title);
        }
    }
    if installable.is_empty() {
        println!("Nothing to install: everything this project needs is already available.");
        return ExitCode::SUCCESS;
    }
    println!("\nWill install into the managed folder (nothing outside it, no global PATH/JAVA_HOME/ANDROID_HOME changes):");
    let mut total = 0;
    for s in &installable {
        total += s.approx_download_bytes;
        let ver = s
            .version
            .as_deref()
            .map(|v| format!(" {v}"))
            .unwrap_or_default();
        println!("  {}{ver}  ~{}", s.title, gb(s.approx_download_bytes));
    }
    println!("  Total download ~{}", gb(total));

    if !accept_licences(&mut client, &prepared).await {
        return ExitCode::FAILURE;
    }
    if !yes {
        if !std::io::stdin().is_terminal() {
            return fail("confirmation needed: run in a terminal, or pass --yes");
        }
        match ask(&format!(
            "\nInstall {} components now? [y/N] ",
            installable.len()
        )) {
            Some(a) if a.eq_ignore_ascii_case("y") || a.eq_ignore_ascii_case("yes") => {}
            _ => {
                println!("Stopped. Nothing was installed.");
                return ExitCode::FAILURE;
            }
        }
    }

    let started: InstallStarted = match client
        .call(methods::INSTALL_START, json!({ "path": dir }))
        .await
    {
        Ok(s) => s,
        Err(e) => return fail(&e.to_string()),
    };
    println!("\nInstall job {} started (Ctrl+C cancels).", started.job_id);
    follow(&mut client, &started.job_id).await
}

/// Prints and asks for every licence that is not accepted yet. False = stop.
async fn accept_licences(client: &mut Client, prepared: &Prepared) -> bool {
    for l in prepared.unaccepted() {
        if !std::io::stdin().is_terminal() {
            eprintln!(
                "umr: the {} must be accepted by you before anything is installed.\n     Run `umr install` in a terminal to read and accept it.",
                l.title
            );
            return false;
        }
        println!("\n────────── {} ──────────", l.title);
        println!("{}", l.text.trim());
        println!("────────── end of {} ──────────", l.title);
        if let Some(u) = &l.url {
            println!("Also at {u}");
        }
        match ask(&format!(
            "Type 'accept' to accept the {}, anything else to stop: ",
            l.title
        )) {
            Some(a) if a == "accept" => {}
            _ => {
                println!("Not accepted. Nothing was installed.");
                return false;
            }
        }
        let p = AcceptLicenceParams {
            id: l.id.clone(),
            sha256: l.sha256.clone(),
        };
        if let Err(e) = client
            .call_raw(
                methods::INSTALL_ACCEPT_LICENCE,
                serde_json::to_value(p).unwrap_or_default(),
            )
            .await
        {
            eprintln!("umr: {e}");
            return false;
        }
        println!("Accepted (recorded with the text's SHA-256).");
    }
    true
}

async fn follow(client: &mut Client, job_id: &str) -> ExitCode {
    let mut last: Vec<(ItemState, u64)> = Vec::new();
    let mut cancel_sent = false;
    loop {
        let st: JobStatus = match client
            .call(methods::INSTALL_STATUS, json!({ "job_id": job_id }))
            .await
        {
            Ok(s) => s,
            Err(e) => return fail(&e.to_string()),
        };
        last.resize(st.items.len(), (ItemState::Pending, 0));
        for (i, it) in st.items.iter().enumerate() {
            let pct = match (it.state, it.bytes_total) {
                (ItemState::Downloading, Some(t)) if t > 0 => it.bytes_done * 100 / t / 10 * 10,
                _ => 0,
            };
            if (it.state, pct) != last[i] {
                let extra = if it.state == ItemState::Downloading && it.bytes_total.is_some() {
                    format!(" {pct}%")
                } else {
                    String::new()
                };
                let msg = it
                    .message
                    .as_deref()
                    .map(|m| format!(" — {m}"))
                    .unwrap_or_default();
                println!("  {}: {}{extra}{msg}", it.title, state_word(it.state));
                last[i] = (it.state, pct);
            }
        }
        if st.state != JobState::Running {
            println!();
            return match st.state {
                JobState::Done => {
                    println!("Install finished. Run `umr plan` to see the updated state.");
                    ExitCode::SUCCESS
                }
                JobState::Cancelled => {
                    println!("Install cancelled. {}", st.message.unwrap_or_default());
                    ExitCode::FAILURE
                }
                _ => {
                    println!("Install failed: {}", st.message.unwrap_or_default());
                    println!("Log: {}", st.log);
                    ExitCode::FAILURE
                }
            };
        }
        tokio::select! {
            () = tokio::time::sleep(Duration::from_millis(500)) => {}
            _ = tokio::signal::ctrl_c(), if !cancel_sent => {
                cancel_sent = true;
                println!("\nCancelling…");
                let _ = client.call_raw(methods::INSTALL_CANCEL, json!({ "job_id": job_id })).await;
            }
        }
    }
}
