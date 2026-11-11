//! `umr` command-line interface (skeleton, CORE-001).
//!
//! Commands (`run`, `status`, `doctor`, `device`, `mcp`, ...) arrive with CLI-001.

use std::process::ExitCode;

fn usage() -> String {
    format!(
        "{}\n\nUsage: umr <command>\n\nCommands available now:\n  --version, -V   Print version\n  --help, -h      Print this help\n\nPlanned: run, stop, status, doctor, device, reload, restart, rebuild, logs, problems, mcp",
        umr_core::version_line("umr")
    )
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version" | "-V") => {
            println!("{}", umr_core::version_line("umr"));
            ExitCode::SUCCESS
        }
        None | Some("--help" | "-h") => {
            println!("{}", usage());
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("umr: unknown command '{other}'\n\n{}", usage());
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::usage;

    #[test]
    fn usage_lists_version_and_help() {
        let u = usage();
        assert!(u.contains("--version"));
        assert!(u.contains("--help"));
        assert!(u.starts_with("umr "));
    }
}
