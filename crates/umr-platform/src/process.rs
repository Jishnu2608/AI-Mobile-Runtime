//! Process inspection.

use std::io;

use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};

/// Basic information about a running process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub parent_pid: u32,
    pub exe_name: String,
}

/// Snapshot of all processes on the machine.
pub fn list_processes() -> io::Result<Vec<ProcessInfo>> {
    // SAFETY: plain snapshot call; result checked below.
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snap == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let mut out = Vec::new();
    // SAFETY: PROCESSENTRY32W is plain data; zero is a valid initial value.
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = u32::try_from(std::mem::size_of::<PROCESSENTRY32W>()).unwrap_or(0);
    // SAFETY: `snap` is a valid snapshot handle and `entry.dwSize` is set.
    let mut ok = unsafe { Process32FirstW(snap, &raw mut entry) } != 0;
    while ok {
        out.push(ProcessInfo {
            pid: entry.th32ProcessID,
            parent_pid: entry.th32ParentProcessID,
            exe_name: crate::wide::from_wide_buf(&entry.szExeFile)
                .to_string_lossy()
                .into_owned(),
        });
        // SAFETY: same handle and initialised entry as above.
        ok = unsafe { Process32NextW(snap, &raw mut entry) } != 0;
    }
    // SAFETY: closes the snapshot handle exactly once.
    unsafe { CloseHandle(snap) };
    Ok(out)
}

/// All descendants (children, grandchildren, …) of `pid`.
pub fn descendants(pid: u32) -> io::Result<Vec<ProcessInfo>> {
    let all = list_processes()?;
    let mut result = Vec::new();
    let mut frontier = vec![pid];
    while let Some(parent) = frontier.pop() {
        for p in all
            .iter()
            .filter(|p| p.parent_pid == parent && p.pid != parent)
        {
            if !result.iter().any(|r: &ProcessInfo| r.pid == p.pid) {
                frontier.push(p.pid);
                result.push(p.clone());
            }
        }
    }
    Ok(result)
}

/// Stops this process's stdin/stdout/stderr handles from being inherited by children.
///
/// Needed before starting a long-lived background process (the Runtime Core) from a
/// short-lived one (the `umr` CLI): Windows passes *all* inheritable handles to a child, so the
/// Core would otherwise keep the CLI's output pipe open and anything capturing `umr start`
/// output (scripts, coding agents) would wait forever for end-of-file.
pub fn disinherit_std_handles() -> io::Result<()> {
    use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: GetStdHandle has no preconditions; it may return null or INVALID_HANDLE_VALUE.
        let h = unsafe { GetStdHandle(which) };
        if h.is_null() || h == INVALID_HANDLE_VALUE {
            continue;
        }
        // SAFETY: `h` is a valid handle of this process; only the inherit flag is cleared.
        if unsafe { SetHandleInformation(h, HANDLE_FLAG_INHERIT, 0) } == 0 {
            let err = io::Error::last_os_error();
            // Console handles on some systems reject the call; that is harmless (consoles are not pipes).
            if err.raw_os_error() != Some(6) {
                return Err(err);
            }
        }
    }
    Ok(())
}

/// Whether a process with this id currently exists.
pub fn is_running(pid: u32) -> io::Result<bool> {
    Ok(list_processes()?.iter().any(|p| p.pid == pid))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_process_is_listed() {
        let me = std::process::id();
        let all = list_processes().unwrap();
        assert!(all.iter().any(|p| p.pid == me));
        assert!(is_running(me).unwrap());
    }

    #[test]
    fn child_is_found_as_descendant() {
        let mut child = std::process::Command::new("cmd.exe")
            .args(["/c", "ping -n 30 127.0.0.1 >nul"])
            .spawn()
            .unwrap();
        let mut found = false;
        for _ in 0..50 {
            if descendants(std::process::id())
                .unwrap()
                .iter()
                .any(|p| p.pid == child.id())
            {
                found = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let _ = child.kill();
        let _ = child.wait();
        assert!(found);
    }
}
