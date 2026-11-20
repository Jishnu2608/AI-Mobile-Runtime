//! `tumr path [status|add|remove|restore]` — W9 / DEC-042 (owner, 2026-11-19): the install
//! folder on the user's own PATH, **off by default**. Only the user turns it on (this command or
//! the desktop app's checkbox). The choice is remembered in the data folder (`path-on`), so the
//! uninstaller can take the folder out (`remove --keep-choice`) and the next install puts it back
//! (`restore`) without the user asking again.

use std::path::PathBuf;
use std::process::ExitCode;

use serde_json::json;

use crate::core_process::CoreTarget;

fn marker(target: &CoreTarget) -> PathBuf {
    target.paths.root().join("path-on")
}

fn install_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot locate tumr.exe: {e}"))?;
    exe.parent()
        .map(PathBuf::from)
        .ok_or_else(|| "tumr.exe has no folder".into())
}

pub fn run(target: &CoreTarget, action: &str, keep_choice: bool, json_out: bool) -> ExitCode {
    match act(target, action, keep_choice) {
        Ok((on, changed, dir)) => {
            if json_out {
                println!(
                    "{}",
                    json!({ "on": on, "changed": changed, "dir": dir.display().to_string(),
                            "chosen": marker(target).is_file() })
                );
            } else if on {
                println!(
                    "{} is on your PATH{}. New terminals can run `tumr`; open ones need a restart.",
                    dir.display(),
                    if changed { " now" } else { "" }
                );
            } else {
                println!(
                    "{} is not on your PATH{}. Turn it on with `tumr path add`.",
                    dir.display(),
                    if changed { " any more" } else { "" }
                );
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            if json_out {
                println!("{}", json!({ "error": e }));
            } else {
                eprintln!("tumr path: {e}");
            }
            ExitCode::FAILURE
        }
    }
}

/// Returns (on the PATH now, changed, folder).
fn act(
    target: &CoreTarget,
    action: &str,
    keep_choice: bool,
) -> Result<(bool, bool, PathBuf), String> {
    let dir = install_dir()?;
    let e = |e: std::io::Error| format!("cannot change your PATH: {e}");
    match action {
        "status" => Ok((
            tumr_platform::user_path::contains(&dir).map_err(e)?,
            false,
            dir,
        )),
        "add" => {
            let changed = tumr_platform::user_path::add(&dir).map_err(e)?;
            if let Some(d) = marker(target).parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(marker(target), dir.display().to_string());
            Ok((true, changed, dir))
        }
        "remove" => {
            let changed = tumr_platform::user_path::remove(&dir).map_err(e)?;
            if !keep_choice {
                let _ = std::fs::remove_file(marker(target));
            }
            Ok((false, changed, dir))
        }
        "restore" => {
            if marker(target).is_file() {
                let changed = tumr_platform::user_path::add(&dir).map_err(e)?;
                Ok((true, changed, dir))
            } else {
                Ok((
                    tumr_platform::user_path::contains(&dir).map_err(e)?,
                    false,
                    dir,
                ))
            }
        }
        other => Err(format!(
            "unknown action '{other}': status, add, remove or restore"
        )),
    }
}
