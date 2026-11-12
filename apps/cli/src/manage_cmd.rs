//! `umr components`, `umr remove`, `umr repair`, `umr clean`, `umr admin` (INS-005/006).

use std::io::IsTerminal;
use std::process::ExitCode;

use serde_json::{json, Value};
use umr_api::methods::{self, AdminRunResult};
use umr_installer::manage::{ComponentHealth, DiskUsage, Removed};

use crate::core_process::CoreTarget;
use crate::fail;

fn size(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let b = bytes as f64;
    if b >= 1_073_741_824.0 {
        format!("{:.1} GB", b / 1_073_741_824.0)
    } else {
        format!("{} MB", bytes / 1_048_576)
    }
}

/// y/N question; non-interactive without `yes` = no.
fn confirm(question: &str, yes: bool) -> Result<bool, String> {
    if yes {
        return Ok(true);
    }
    if !std::io::stdin().is_terminal() {
        return Err("confirmation needed: run in a terminal, or pass --yes".into());
    }
    print!("{question} [y/N] ");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
    Ok(matches!(
        line.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

async fn client(target: &CoreTarget) -> Result<umr_api::client::Client, ExitCode> {
    match target.connect_or_start().await {
        Ok((c, _)) => Ok(c),
        Err(e) => Err(fail(&e)),
    }
}

pub async fn components(target: &CoreTarget, json_out: bool) -> ExitCode {
    let mut c = match client(target).await {
        Ok(c) => c,
        Err(code) => return code,
    };
    let v: Value = match c.call(methods::COMPONENTS_LIST, Value::Null).await {
        Ok(v) => v,
        Err(e) => return fail(&e.to_string()),
    };
    if json_out {
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return ExitCode::SUCCESS;
    }
    let list: Vec<ComponentHealth> =
        serde_json::from_value(v["components"].clone()).unwrap_or_default();
    let disk: DiskUsage = serde_json::from_value(v["disk"].clone()).unwrap_or_default();
    if list.is_empty() {
        println!("No components recorded yet. Run `umr install` for a project.");
    }
    for h in &list {
        let mark = if h.ok { "[ ok ]" } else { "[FAIL]" };
        println!(
            "{mark} {:<45} {:<16} {:>9}  {}",
            h.id,
            h.version,
            size(h.size_bytes),
            h.source
        );
        if let Some(p) = &h.problem {
            println!("       problem: {p}");
        }
        if let Some(n) = &h.note {
            println!("       note: {n}");
        }
    }
    println!(
        "\nDisk: components {} · download cache {} · Android user home {} · logs {} · temp {} · total {}",
        size(disk.components_bytes),
        size(disk.download_cache_bytes),
        size(disk.android_user_home_bytes),
        size(disk.logs_bytes),
        size(disk.temp_bytes),
        size(disk.total_bytes)
    );
    if list.iter().any(|h| !h.ok) {
        println!("Some components are broken: `umr repair <project>` reinstalls them.");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

pub async fn remove(target: &CoreTarget, id: String, force: bool, yes: bool) -> ExitCode {
    match confirm(&format!("Remove {id} from the managed folder?"), yes) {
        Ok(true) => {}
        Ok(false) => {
            println!("Nothing removed.");
            return ExitCode::FAILURE;
        }
        Err(e) => return fail(&e),
    }
    let mut c = match client(target).await {
        Ok(c) => c,
        Err(code) => return code,
    };
    match c
        .call::<Removed>(
            methods::COMPONENTS_REMOVE,
            json!({ "id": id, "force": force }),
        )
        .await
    {
        Ok(r) => {
            println!("Removed {} ({} freed).", r.id, size(r.freed_bytes));
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e.to_string()),
    }
}

/// Removes broken components; with a project folder, reinstalls what it needs.
pub async fn repair(target: &CoreTarget, project: Option<String>, yes: bool) -> ExitCode {
    let mut c = match client(target).await {
        Ok(c) => c,
        Err(code) => return code,
    };
    let v: Value = match c.call(methods::COMPONENTS_REPAIR, Value::Null).await {
        Ok(v) => v,
        Err(e) => return fail(&e.to_string()),
    };
    let removed: Vec<Removed> = serde_json::from_value(v["removed"].clone()).unwrap_or_default();
    if removed.is_empty() {
        println!("All recorded components are healthy.");
    }
    for r in &removed {
        println!("Removed broken {} ({})", r.id, r.path);
    }
    match project {
        Some(dir) => {
            println!();
            crate::install_cmd::install(target, dir, yes).await
        }
        None if !removed.is_empty() => {
            println!("Run `umr install <project>` to install them again.");
            ExitCode::SUCCESS
        }
        None => ExitCode::SUCCESS,
    }
}

pub async fn clean(target: &CoreTarget) -> ExitCode {
    let mut c = match client(target).await {
        Ok(c) => c,
        Err(code) => return code,
    };
    match c
        .call::<Value>(methods::COMPONENTS_CLEAN, Value::Null)
        .await
    {
        Ok(v) => {
            println!(
                "Freed {} (download cache and logs older than 14 days).",
                size(v["freed_bytes"].as_u64().unwrap_or(0))
            );
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e.to_string()),
    }
}

pub async fn admin(target: &CoreTarget, action: &str, yes: bool) -> ExitCode {
    let Some(a) = umr_elevate::Action::parse(action) else {
        let names: Vec<&str> = umr_elevate::Action::ALL.iter().map(|a| a.name()).collect();
        return fail(&format!(
            "unknown action '{action}'; choose one of: {}",
            names.join(", ")
        ));
    };
    println!("{}\n{}", a.title(), a.explanation());
    println!("Windows will ask for administrator permission (UAC) for this one action.");
    match confirm("Continue?", yes) {
        Ok(true) => {}
        Ok(false) => {
            println!("Nothing changed.");
            return ExitCode::FAILURE;
        }
        Err(e) => return fail(&e),
    }
    let mut c = match client(target).await {
        Ok(c) => c,
        Err(code) => return code,
    };
    match c
        .call::<AdminRunResult>(methods::ADMIN_RUN, json!({ "action": a.name() }))
        .await
    {
        Ok(r) => {
            println!("{}: {}", r.status, r.message);
            if matches!(
                r.status.as_str(),
                "done" | "restart-required" | "already-on"
            ) {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(e) => fail(&e.to_string()),
    }
}
