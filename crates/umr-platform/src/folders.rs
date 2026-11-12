//! Windows known folders (resolved through the shell, not environment variables).

use std::io;
use std::path::PathBuf;

use windows_sys::core::GUID;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::UI::Shell::{
    FOLDERID_LocalAppData, FOLDERID_Profile, FOLDERID_RoamingAppData, SHGetKnownFolderPath,
    KF_FLAG_DEFAULT,
};

use crate::wide::from_wide_ptr;

fn known_folder(id: &GUID) -> io::Result<PathBuf> {
    let mut out: *mut u16 = std::ptr::null_mut();
    // SAFETY: `id` is a valid KNOWNFOLDERID; `out` is a valid out-pointer; token null = current user.
    #[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
    let hr = unsafe {
        SHGetKnownFolderPath(id, KF_FLAG_DEFAULT as _, std::ptr::null_mut(), &raw mut out)
    };
    if hr < 0 {
        // SAFETY: on failure the output must still be freed (it may be null; CoTaskMemFree accepts null).
        unsafe { CoTaskMemFree(out.cast()) };
        return Err(io::Error::from_raw_os_error(hr));
    }
    // SAFETY: on success `out` is a NUL-terminated string allocated with CoTaskMemAlloc.
    let path = unsafe { from_wide_ptr(out) };
    // SAFETY: frees the shell-allocated buffer exactly once.
    unsafe { CoTaskMemFree(out.cast()) };
    Ok(PathBuf::from(path))
}

/// `%LOCALAPPDATA%` of the current user (e.g. `C:\Users\<name>\AppData\Local`).
pub fn local_app_data() -> io::Result<PathBuf> {
    known_folder(&FOLDERID_LocalAppData)
}

/// The user's profile folder (`%USERPROFILE%`, e.g. `C:\Users\<name>`).
pub fn profile() -> io::Result<PathBuf> {
    known_folder(&FOLDERID_Profile)
}

/// `%APPDATA%` (roaming) of the current user.
pub fn roaming_app_data() -> io::Result<PathBuf> {
    known_folder(&FOLDERID_RoamingAppData)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_app_data_exists_and_is_absolute() {
        let p = local_app_data().unwrap();
        assert!(p.is_absolute(), "{}", p.display());
        assert!(p.is_dir(), "{}", p.display());
    }

    #[test]
    fn profile_contains_local_app_data() {
        let p = profile().unwrap();
        assert!(p.is_dir());
        assert!(local_app_data().unwrap().starts_with(&p));
    }

    #[test]
    fn roaming_differs_from_local() {
        assert_ne!(roaming_app_data().unwrap(), local_app_data().unwrap());
    }
}
