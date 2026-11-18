//! Top-level windows of another process (AUD-P1-5: one preview window, brought to the front
//! instead of opening a second one).

use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, IsIconic, IsWindowVisible, SetForegroundWindow,
    ShowWindow, SW_RESTORE,
};

struct Search {
    pid: u32,
    found: HWND,
}

unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: `lparam` is the `Search` passed to `EnumWindows` below, alive for the whole call.
    let s = unsafe { &mut *(lparam as *mut Search) };
    let mut pid = 0u32;
    // SAFETY: plain query on a window handle given by EnumWindows.
    unsafe { GetWindowThreadProcessId(hwnd, &raw mut pid) };
    // SAFETY: as above.
    if pid == s.pid && unsafe { IsWindowVisible(hwnd) } != 0 {
        s.found = hwnd;
        return 0; // stop
    }
    1
}

/// Brings the first visible top-level window of process `pid` to the front (restored if it
/// was minimised). Returns whether such a window exists.
#[must_use]
pub fn focus_window_of(pid: u32) -> bool {
    let mut s = Search {
        pid,
        found: std::ptr::null_mut(),
    };
    // SAFETY: `visit` only reads `s` through the pointer while EnumWindows runs.
    unsafe { EnumWindows(Some(visit), (&raw mut s) as LPARAM) };
    if s.found.is_null() {
        return false;
    }
    // SAFETY: a window handle just returned by EnumWindows; failures are harmless.
    unsafe {
        if IsIconic(s.found) != 0 {
            ShowWindow(s.found, SW_RESTORE);
        }
        SetForegroundWindow(s.found);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_process_without_windows_is_not_found() {
        // the test runner has no visible top-level window; pid 0 never has one
        assert!(!focus_window_of(0));
    }
}
