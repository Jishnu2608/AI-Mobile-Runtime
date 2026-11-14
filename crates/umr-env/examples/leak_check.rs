//! Runs one command with the product's isolated environment and reports any file it left in
//! the known leak locations (ENV-008/010; reused by the clean-VM tests, TEST-001).
//!
//! ```text
//! cargo run -p umr-env --example leak_check -- [--appdata] [--for <secs>] <toolchains folder> <program> [args…]
//! ```
//! `--appdata` uses the Flutter recipe (`APPDATA` / `LOCALAPPDATA` redirected); `--for` stops a
//! long-running program (a daemon) after that many seconds.
//! Exit code 0 = no leaks, 1 = leaks found, 2 = usage / start error.

use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

use umr_env::leaks::{diff, known_locations, snapshot};
use umr_env::recipe::{child_env, IsolationDirs, Selection};
use umr_env::ScanContext;

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut isolate_appdata = false;
    let mut run_for: Option<u64> = None;
    loop {
        match args.first().map(String::as_str) {
            Some("--appdata") => {
                isolate_appdata = true;
                args.remove(0);
            }
            Some("--for") if args.len() > 1 => {
                run_for = args[1].parse().ok();
                args.drain(..2);
            }
            _ => break,
        }
    }
    let [root, program, rest @ ..] = args.as_slice() else {
        eprintln!(
            "usage: leak_check [--appdata] [--for <secs>] <toolchains folder> <program> [args…]"
        );
        return ExitCode::from(2);
    };
    let root = PathBuf::from(root);
    let ctx = ScanContext::from_system(root.clone());
    if let Err(e) = IsolationDirs::under(&root).create() {
        eprintln!("cannot create managed folders: {e}");
        return ExitCode::from(2);
    }
    let sdk = root.join("android-sdk");
    let sel = Selection {
        android_sdk: sdk.is_dir().then_some(sdk),
        isolate_appdata,
        ..Selection::default()
    };
    let env = child_env(&ctx, &sel);
    let watched = known_locations(&ctx);
    let before = snapshot(&watched);
    let status = Command::new(program)
        .args(rest)
        .envs(&env)
        .stdin(Stdio::null())
        .spawn()
        .and_then(|mut child| match run_for {
            Some(secs) => {
                let end = std::time::Instant::now() + std::time::Duration::from_secs(secs);
                while std::time::Instant::now() < end {
                    if let Some(s) = child.try_wait()? {
                        return Ok(s);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
                let _ = child.kill();
                child.wait()
            }
            None => child.wait(),
        });
    match status {
        Ok(s) => println!("command exited with {s}"),
        Err(e) => {
            eprintln!("cannot start {program}: {e}");
            return ExitCode::from(2);
        }
    }
    let changes = diff(&before, &snapshot(&watched));
    for w in &watched {
        let state = changes
            .iter()
            .find(|c| c.path == w.path)
            .map_or("unchanged", |c| c.what.as_str());
        println!("  {:<60} {state}", w.path.display());
    }
    if changes.is_empty() {
        println!("no leaks");
        ExitCode::SUCCESS
    } else {
        println!("LEAKS: {}", changes.len());
        ExitCode::from(1)
    }
}
