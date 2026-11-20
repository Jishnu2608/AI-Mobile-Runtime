//! Identity of the current user.

use std::io;

use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE};
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use crate::wide::from_wide_ptr;

/// Closes a handle on drop.
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle was returned by a successful Windows call and is closed once.
        unsafe { CloseHandle(self.0) };
    }
}

/// String SID of the user running this process, e.g. `S-1-5-21-…-1001`.
///
/// Used to build per-user names (named pipe) and access-control lists.
pub fn current_user_sid() -> io::Result<String> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: GetCurrentProcess returns a pseudo-handle; `token` is a valid out-pointer.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = Handle(token);

    let mut len = 0u32;
    // SAFETY: size query with a null buffer; Windows writes the required size into `len`.
    unsafe { GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &raw mut len) };
    if len == 0 {
        return Err(io::Error::last_os_error());
    }
    // u64 buffer keeps the TOKEN_USER struct suitably aligned.
    let mut buf = vec![0u64; (len as usize).div_ceil(8)];
    // SAFETY: `buf` has at least `len` bytes; `len` is passed as its size.
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            buf.as_mut_ptr().cast(),
            len,
            &raw mut len,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: on success the buffer starts with a TOKEN_USER structure.
    let user = unsafe { &*buf.as_ptr().cast::<TOKEN_USER>() };

    let mut sid_str: *mut u16 = std::ptr::null_mut();
    // SAFETY: `user.User.Sid` points into `buf`, which outlives this call.
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &raw mut sid_str) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: on success `sid_str` is a NUL-terminated string allocated with LocalAlloc.
    let s = unsafe { from_wide_ptr(sid_str) };
    // SAFETY: frees the LocalAlloc'd buffer exactly once.
    unsafe { LocalFree(sid_str.cast()) };
    s.into_string()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "SID is not valid UTF-16"))
}

/// Does an SDDL string grant something to `sid`?
///
/// Windows prints some well-known accounts as two-letter aliases instead of their SID: the
/// built-in Administrator (RID 500, e.g. the GitHub runner user) appears as `LA`.
#[must_use]
pub fn sddl_names_user(sddl: &str, sid: &str) -> bool {
    sddl.contains(sid)
        || (sid.starts_with("S-1-5-21-") && sid.ends_with("-500") && sddl.contains(";LA)"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn sddl_user_match_handles_the_builtin_administrator_alias() {
        let user = "S-1-5-21-1-2-3-1001";
        let admin = "S-1-5-21-1-2-3-500";
        assert!(super::sddl_names_user(
            "D:P(A;;FA;;;S-1-5-21-1-2-3-1001)",
            user
        ));
        assert!(super::sddl_names_user("D:P(A;;FA;;;LA)", admin));
        assert!(!super::sddl_names_user("D:P(A;;FA;;;LA)", user));
        assert!(!super::sddl_names_user("D:P(A;;FA;;;BA)", admin));
    }

    use super::*;

    #[test]
    fn sid_looks_like_a_user_sid() {
        let sid = current_user_sid().unwrap();
        assert!(sid.starts_with("S-1-"), "{sid}");
        assert!(sid.split('-').count() >= 4, "{sid}");
    }

    #[test]
    fn sid_is_stable() {
        assert_eq!(current_user_sid().unwrap(), current_user_sid().unwrap());
    }
}
