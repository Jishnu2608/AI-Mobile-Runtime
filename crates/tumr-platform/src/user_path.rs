//! W9 / DEC-042 (owner, 2026-11-19): the install folder on the user's own `PATH`, off by default,
//! switched on only by the user (`tumr path add`, the desktop app's checkbox). Only
//! `HKCU\Environment\Path` changes; the machine `PATH` and other entries are left as they are.
//! New terminals see the change (Windows is told with `WM_SETTINGCHANGE`); open ones do not.

use std::io;
use std::path::Path;

use crate::registry::{read_raw_string, write_user_string, Hive};

const KEY: &str = "Environment";
const VALUE: &str = "Path";

/// Whether `entry` names `dir` (case and a trailing `\` ignored).
fn same(entry: &str, dir: &str) -> bool {
    let norm = |s: &str| s.trim().trim_end_matches(['\\', '/']).to_ascii_lowercase();
    !entry.trim().is_empty() && norm(entry) == norm(dir)
}

/// `path` with `dir` appended (unchanged when it is already there).
#[must_use]
pub fn with_dir(path: &str, dir: &str) -> String {
    if path.split(';').any(|e| same(e, dir)) {
        return path.to_string();
    }
    let base = path.trim_end_matches(';');
    if base.is_empty() {
        dir.to_string()
    } else {
        format!("{base};{dir}")
    }
}

/// `path` without `dir` (every copy); other entries keep their order and spelling.
#[must_use]
pub fn without_dir(path: &str, dir: &str) -> String {
    path.split(';')
        .filter(|e| !same(e, dir))
        .collect::<Vec<_>>()
        .join(";")
}

fn current() -> io::Result<(String, bool)> {
    Ok(read_raw_string(Hive::CurrentUser, KEY, VALUE)?.unwrap_or((String::new(), true)))
}

/// Whether the user `PATH` holds `dir`.
pub fn contains(dir: &Path) -> io::Result<bool> {
    let d = dir.display().to_string();
    Ok(current()?.0.split(';').any(|e| same(e, &d)))
}

/// Adds `dir` to the user `PATH`; returns whether it changed.
pub fn add(dir: &Path) -> io::Result<bool> {
    let (old, expand) = current()?;
    let new = with_dir(&old, &dir.display().to_string());
    if new == old {
        return Ok(false);
    }
    write_user_string(KEY, VALUE, &new, expand)?;
    announce();
    Ok(true)
}

/// Takes `dir` out of the user `PATH`; returns whether it changed.
pub fn remove(dir: &Path) -> io::Result<bool> {
    let (old, expand) = current()?;
    let new = without_dir(&old, &dir.display().to_string());
    if new == old {
        return Ok(false);
    }
    write_user_string(KEY, VALUE, &new, expand)?;
    announce();
    Ok(true)
}

/// Tells running programs (Explorer, new terminals) that the environment changed.
#[allow(unsafe_code)]
fn announce() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };
    let what: Vec<u16> = "Environment".encode_utf16().chain([0]).collect();
    let mut result = 0usize;
    // SAFETY: `what` is a NUL-terminated UTF-16 string that outlives the call; the result
    // pointer is valid. A broadcast with a timeout cannot block on a hung window.
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            what.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            3000,
            &raw mut result,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const D: &str = r"C:\Users\u\AppData\Local\Toolchain Universal Mobile Runtime";

    #[test]
    fn adds_once_and_keeps_the_rest() {
        assert_eq!(with_dir("", D), D);
        assert_eq!(
            with_dir(r"C:\a;%USERPROFILE%\b", D),
            format!(r"C:\a;%USERPROFILE%\b;{D}")
        );
        assert_eq!(with_dir(r"C:\a;", D), format!(r"C:\a;{D}"));
        let once = with_dir(r"C:\a", D);
        assert_eq!(with_dir(&once, D), once, "not twice");
        assert_eq!(
            with_dir(&format!(r"C:\a;{}\", D.to_lowercase()), D),
            format!(r"C:\a;{}\", D.to_lowercase())
        );
    }

    #[test]
    fn removes_every_copy_only() {
        assert_eq!(
            without_dir(&format!(r"C:\a;{D};%X%\b;{D}\"), D),
            r"C:\a;%X%\b"
        );
        assert_eq!(without_dir(r"C:\a;C:\b", D), r"C:\a;C:\b");
        assert_eq!(without_dir(D, D), "");
    }
}
