//! `umr-core` binary: the per-user Runtime Core service.
//!
//! Usage:
//!   umr-core run [--foreground]   Start the Core (logs also to stderr with --foreground)
//!   umr-core --version            Print version
//!
//! Exit codes: 0 stopped cleanly · 2 usage error · 3 another Core is already running · 1 other error.
//! Stopping from another process arrives with the local API (CORE-003); for now Ctrl+C stops it.

use std::process::ExitCode;

use umr_core::daemon::{self, RunOutcome};
use umr_core::paths::CorePaths;

const EXIT_USAGE: u8 = 2;
const EXIT_ALREADY_RUNNING: u8 = 3;

fn usage() -> String {
    format!(
        "{}\n\nUsage:\n  umr-core run [--foreground]\n  umr-core --version",
        umr_core::version_line("umr-core")
    )
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["--version" | "-V"] => {
            println!("{}", umr_core::version_line("umr-core"));
            ExitCode::SUCCESS
        }
        ["--help" | "-h"] => {
            println!("{}", usage());
            ExitCode::SUCCESS
        }
        ["run"] => run(false),
        ["run", "--foreground"] => run(true),
        _ => {
            eprintln!("{}", usage());
            ExitCode::from(EXIT_USAGE)
        }
    }
}

fn run(foreground: bool) -> ExitCode {
    let paths = match CorePaths::resolve() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("umr-core: {e}");
            return ExitCode::FAILURE;
        }
    };
    // Config is read before logging starts so its log level applies from the first line.
    if let Err(e) = paths.ensure_dirs().and_then(|()| {
        umr_storage::config::Config::write_template_if_missing(paths.root()).map(|_| ())
    }) {
        eprintln!(
            "umr-core: cannot prepare data folder {}: {e}",
            paths.root().display()
        );
        return ExitCode::FAILURE;
    }
    let config = match umr_storage::config::Config::load_in(paths.root()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("umr-core: {e}");
            return ExitCode::FAILURE;
        }
    };
    let _log_guard = match umr_core::logging::init(&paths, foreground, &config.core.log) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("umr-core: cannot initialise logging: {e}");
            return ExitCode::FAILURE;
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            tracing::error!(error = %e, "cannot start async runtime");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(daemon::run(&paths, daemon::os_shutdown_signal())) {
        Ok(RunOutcome::Stopped) => ExitCode::SUCCESS,
        Ok(RunOutcome::AlreadyRunning { pid }) => {
            match pid {
                Some(pid) => eprintln!("umr-core: already running (pid {pid})"),
                None => eprintln!("umr-core: already running"),
            }
            ExitCode::from(EXIT_ALREADY_RUNNING)
        }
        Err(e) => {
            tracing::error!(error = %e, "Runtime Core failed");
            eprintln!("umr-core: {e}");
            ExitCode::FAILURE
        }
    }
}
