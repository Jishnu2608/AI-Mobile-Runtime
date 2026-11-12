//! Administrator rights (INS-005).
//!
//! The Core and CLI always run as the normal user. The few actions that need administrator
//! rights (turning on Windows Hypervisor Platform, enabling long paths) are done by a small
//! separate helper, started with [`run_as_admin`]: Windows then shows its UAC consent prompt
//! for that one action. Declining the prompt is reported as [`Elevation::Declined`].

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_CANCELLED, HANDLE, WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
};
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetExitCodeProcess, OpenProcessToken, WaitForSingleObject, INFINITE,
};
use windows_sys::Win32::UI::Shell::{
    ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
};

use crate::wide::{from_wide_buf, to_wide};

/// Does this process run with an elevated (administrator) token?
pub fn is_elevated() -> io::Result<bool> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: GetCurrentProcess returns a pseudo-handle; `token` receives a new handle we close.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut len = 0u32;
    #[allow(clippy::cast_possible_truncation)]
    let size = std::mem::size_of::<TOKEN_ELEVATION>() as u32;
    // SAFETY: the buffer is a TOKEN_ELEVATION of `size` bytes; `token` is valid.
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            (&raw mut elevation).cast(),
            size,
            &raw mut len,
        )
    };
    let err = io::Error::last_os_error();
    // SAFETY: `token` was opened above and is closed exactly once.
    unsafe { CloseHandle(token) };
    if ok == 0 {
        return Err(err);
    }
    Ok(elevation.TokenIsElevated != 0)
}

/// `C:\Windows\System32` from the API (not from a variable the environment could change).
pub fn system_directory() -> io::Result<PathBuf> {
    let mut buf = vec![0u16; 520];
    #[allow(clippy::cast_possible_truncation)]
    // SAFETY: `buf` has `buf.len()` u16 slots.
    let n = unsafe { GetSystemDirectoryW(buf.as_mut_ptr(), buf.len() as u32) } as usize;
    if n == 0 || n >= buf.len() {
        return Err(io::Error::last_os_error());
    }
    buf.truncate(n);
    Ok(PathBuf::from(from_wide_buf(&buf)))
}

/// Result of [`run_as_admin`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Elevation {
    /// The helper ran and exited with this code.
    Exited(u32),
    /// The user declined the UAC prompt.
    Declined,
}

/// Quotes one argument for the Windows command line (`CommandLineToArgvW` rules).
#[must_use]
pub fn quote_arg(a: &str) -> String {
    if !a.is_empty() && !a.contains([' ', '\t', '"']) {
        return a.to_string();
    }
    let mut out = String::from("\"");
    let mut backslashes = 0;
    for c in a.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

/// Starts `exe` with administrator rights (UAC prompt) and waits for it to finish.
pub fn run_as_admin(exe: &Path, args: &[&str]) -> io::Result<Elevation> {
    let verb = to_wide("runas");
    let file = to_wide(exe.as_os_str());
    let params: Vec<String> = args.iter().map(|a| quote_arg(a)).collect();
    let params = to_wide(OsStr::new(&params.join(" ")));
    // SAFETY: zero is a valid initial value for this plain C struct.
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    #[allow(clippy::cast_possible_truncation)]
    {
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    }
    info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = params.as_ptr();
    info.nShow = 0; // SW_HIDE: the helper has no window of its own
                    // SAFETY: all string pointers are NUL-terminated and outlive the call.
    if unsafe { ShellExecuteExW(&raw mut info) } == 0 {
        // SAFETY: plain thread-local error query.
        let code = unsafe { GetLastError() };
        if code == ERROR_CANCELLED {
            return Ok(Elevation::Declined);
        }
        #[allow(clippy::cast_possible_wrap)]
        return Err(io::Error::from_raw_os_error(code as i32));
    }
    let process = info.hProcess;
    if process.is_null() {
        return Err(io::Error::other("elevated helper did not start"));
    }
    // SAFETY: `process` is a valid process handle owned by us (SEE_MASK_NOCLOSEPROCESS).
    let waited = unsafe { WaitForSingleObject(process, INFINITE) };
    let mut code = 0u32;
    // SAFETY: as above; `code` receives the exit code.
    let got = unsafe { GetExitCodeProcess(process, &raw mut code) };
    // SAFETY: closed exactly once.
    unsafe { CloseHandle(process) };
    if waited != WAIT_OBJECT_0 || got == 0 {
        return Err(io::Error::other(
            "cannot read the elevated helper's exit code",
        ));
    }
    Ok(Elevation::Exited(code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_follows_windows_rules() {
        assert_eq!(quote_arg("enable-whpx"), "enable-whpx");
        assert_eq!(quote_arg(""), "\"\"");
        assert_eq!(quote_arg(r"C:\Program Files\x"), r#""C:\Program Files\x""#);
        assert_eq!(quote_arg(r#"a"b"#), r#""a\"b""#);
        assert_eq!(quote_arg(r"C:\dir with space\"), r#""C:\dir with space\\""#);
    }

    #[test]
    fn system_directory_and_elevation_are_readable() {
        let d = system_directory().unwrap();
        assert!(d.join("dism.exe").is_file(), "{}", d.display());
        let _ = is_elevated().unwrap(); // either value is fine; it must not fail
    }
}
