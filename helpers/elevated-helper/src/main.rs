//! `tumr-elevate <action>` — runs one fixed administrator action (see the library docs).
//! Started by the Core through UAC (`runas`); refuses to run without administrator rights.

use std::process::ExitCode;

use tumr_elevate::{exit, map_dism_exit, Action, LONG_PATHS_KEY, LONG_PATHS_VALUE};

fn code(c: u32) -> ExitCode {
    ExitCode::from(u8::try_from(c).unwrap_or(1))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [one] = args.as_slice() else {
        eprintln!(
            "usage: tumr-elevate <{}>",
            Action::ALL.map(Action::name).join("|")
        );
        return code(exit::USAGE);
    };
    let Some(action) = Action::parse(one) else {
        eprintln!("tumr-elevate: unknown action");
        return code(exit::USAGE);
    };
    if !matches!(tumr_platform::elevate::is_elevated(), Ok(true)) {
        eprintln!("tumr-elevate: needs administrator rights");
        return code(exit::NOT_ELEVATED);
    }
    code(run(action))
}

fn run(action: Action) -> u32 {
    match action {
        Action::EnableWhpx => {
            let Ok(sys) = tumr_platform::elevate::system_directory() else {
                return exit::FAILED;
            };
            let (exe, args) = Action::dism_command(&sys);
            match std::process::Command::new(exe).args(args).status() {
                Ok(s) => map_dism_exit(s.code()),
                Err(e) => {
                    eprintln!("tumr-elevate: cannot run DISM: {e}");
                    exit::FAILED
                }
            }
        }
        Action::EnableLongPaths => {
            match tumr_platform::registry::write_dword(
                tumr_platform::registry::Hive::LocalMachine,
                LONG_PATHS_KEY,
                LONG_PATHS_VALUE,
                1,
            ) {
                Ok(()) => exit::DONE,
                Err(e) => {
                    eprintln!("tumr-elevate: cannot set {LONG_PATHS_VALUE}: {e}");
                    exit::FAILED
                }
            }
        }
    }
}
