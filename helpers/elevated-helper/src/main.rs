//! `umr-elevate <action>` — runs one fixed administrator action (see the library docs).
//! Started by the Core through UAC (`runas`); refuses to run without administrator rights.

use std::process::ExitCode;

use umr_elevate::{exit, map_dism_exit, Action, LONG_PATHS_KEY, LONG_PATHS_VALUE};

fn code(c: u32) -> ExitCode {
    ExitCode::from(u8::try_from(c).unwrap_or(1))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [one] = args.as_slice() else {
        eprintln!(
            "usage: umr-elevate <{}>",
            Action::ALL.map(Action::name).join("|")
        );
        return code(exit::USAGE);
    };
    let Some(action) = Action::parse(one) else {
        eprintln!("umr-elevate: unknown action");
        return code(exit::USAGE);
    };
    if !matches!(umr_platform::elevate::is_elevated(), Ok(true)) {
        eprintln!("umr-elevate: needs administrator rights");
        return code(exit::NOT_ELEVATED);
    }
    code(run(action))
}

fn run(action: Action) -> u32 {
    match action {
        Action::EnableWhpx => {
            let Ok(sys) = umr_platform::elevate::system_directory() else {
                return exit::FAILED;
            };
            let (exe, args) = Action::dism_command(&sys);
            match std::process::Command::new(exe).args(args).status() {
                Ok(s) => map_dism_exit(s.code()),
                Err(e) => {
                    eprintln!("umr-elevate: cannot run DISM: {e}");
                    exit::FAILED
                }
            }
        }
        Action::EnableLongPaths => {
            match umr_platform::registry::write_dword(
                umr_platform::registry::Hive::LocalMachine,
                LONG_PATHS_KEY,
                LONG_PATHS_VALUE,
                1,
            ) {
                Ok(()) => exit::DONE,
                Err(e) => {
                    eprintln!("umr-elevate: cannot set {LONG_PATHS_VALUE}: {e}");
                    exit::FAILED
                }
            }
        }
    }
}
