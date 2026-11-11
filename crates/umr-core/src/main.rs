//! `umr-core` binary entry point (skeleton, CORE-001).

use std::process::ExitCode;

fn main() -> ExitCode {
    let first = std::env::args().nth(1);
    if let Some("--version" | "-V") = first.as_deref() {
        println!("{}", umr_core::version_line("umr-core"));
        return ExitCode::SUCCESS;
    }
    eprintln!(
        "{}\nThe Runtime Core service is not implemented yet (CORE-002).",
        umr_core::version_line("umr-core")
    );
    ExitCode::from(2)
}
