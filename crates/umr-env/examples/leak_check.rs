//! Runs one command with the product's isolated environment and reports any file it left in
//! the known leak locations (ENV-008/010; reused by the clean-VM tests, TEST-001).
//!
//! ```text
//! cargo run -p umr-env --example leak_check -- <toolchains folder> <program> [args…]
//! ```
//! Exit code 0 = no leaks, 1 = leaks found, 2 = usage / start error.

use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

use umr_env::leaks::{diff, known_locations, snapshot};
use umr_env::recipe::{child_env, IsolationDirs, Selection};
use umr_env::ScanContext;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [root, program, rest @ ..] = args.as_slice() else {
        eprintln!("usage: leak_check <toolchains folder> <program> [args…]");
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
        ..Selection::default()
    };
    let env = child_env(&ctx, &sel);
    let watched = known_locations(&ctx);
    let before = snapshot(&watched);
    let status = Command::new(program)
        .args(rest)
        .envs(&env)
        .stdin(Stdio::null())
        .status();
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
