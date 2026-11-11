//! Read-only registry access.
//!
//! Used for tool detection (e.g. JDK installs under `HKLM\SOFTWARE\...`) and OS settings.
//! The product never writes to the registry outside the consented elevated helper (INS-005).

use std::io;

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
};

use crate::wide::{from_wide_buf, to_wide};

/// Registry root key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hive {
    LocalMachine,
    CurrentUser,
}

impl Hive {
    fn raw(self) -> HKEY {
        match self {
            Self::LocalMachine => HKEY_LOCAL_MACHINE,
            Self::CurrentUser => HKEY_CURRENT_USER,
        }
    }
}

/// Names of the direct subkeys of `key` (empty if the key does not exist).
pub fn subkeys(hive: Hive, key: &str) -> io::Result<Vec<String>> {
    use windows_sys::Win32::Foundation::ERROR_NO_MORE_ITEMS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, KEY_READ,
    };

    let key_w = to_wide(key);
    let mut h: HKEY = std::ptr::null_mut();
    // SAFETY: NUL-terminated subkey; `h` is a valid out-pointer.
    let rc = unsafe { RegOpenKeyExW(hive.raw(), key_w.as_ptr(), 0, KEY_READ, &raw mut h) };
    if rc == ERROR_FILE_NOT_FOUND {
        return Ok(Vec::new());
    }
    if rc != ERROR_SUCCESS {
        #[allow(clippy::cast_possible_wrap)]
        return Err(io::Error::from_raw_os_error(rc as i32));
    }
    let mut names = Vec::new();
    let mut index = 0u32;
    loop {
        let mut buf = [0u16; 256]; // registry key names are at most 255 chars
        let mut len = u32::try_from(buf.len()).unwrap_or(0);
        // SAFETY: `h` is open; `buf` holds `len` UTF-16 units; optional out-params are null.
        let rc = unsafe {
            RegEnumKeyExW(
                h,
                index,
                buf.as_mut_ptr(),
                &raw mut len,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if rc == ERROR_NO_MORE_ITEMS {
            break;
        }
        if rc != ERROR_SUCCESS {
            // SAFETY: closes the key opened above.
            unsafe { RegCloseKey(h) };
            #[allow(clippy::cast_possible_wrap)]
            return Err(io::Error::from_raw_os_error(rc as i32));
        }
        names.push(
            from_wide_buf(&buf[..len as usize])
                .to_string_lossy()
                .into_owned(),
        );
        index += 1;
    }
    // SAFETY: closes the key opened above, exactly once.
    unsafe { RegCloseKey(h) };
    Ok(names)
}

/// Reads a `REG_SZ` value. Returns `Ok(None)` if the key or value does not exist.
pub fn read_string(hive: Hive, key: &str, value: &str) -> io::Result<Option<String>> {
    let key_w = to_wide(key);
    let value_w = to_wide(value);
    let mut buf: Vec<u16> = vec![0; 256];
    loop {
        let mut size = u32::try_from(buf.len() * 2).unwrap_or(u32::MAX);
        // SAFETY: key/value are NUL-terminated; `buf` has `size` bytes; type pointer may be null.
        let rc = unsafe {
            RegGetValueW(
                hive.raw(),
                key_w.as_ptr(),
                value_w.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &raw mut size,
            )
        };
        match rc {
            ERROR_SUCCESS => {
                buf.truncate(size as usize / 2);
                return Ok(Some(from_wide_buf(&buf).to_string_lossy().into_owned()));
            }
            ERROR_MORE_DATA => buf.resize(size as usize / 2 + 1, 0),
            ERROR_FILE_NOT_FOUND => return Ok(None),
            #[allow(clippy::cast_possible_wrap)]
            other => return Err(io::Error::from_raw_os_error(other as i32)),
        }
    }
}

/// Reads a `REG_DWORD` value. Returns `Ok(None)` if the key or value does not exist.
pub fn read_dword(hive: Hive, key: &str, value: &str) -> io::Result<Option<u32>> {
    let key_w = to_wide(key);
    let value_w = to_wide(value);
    let mut data = 0u32;
    let mut size = 4u32;
    // SAFETY: key/value are NUL-terminated; `data` is a 4-byte buffer and `size` says so.
    let rc = unsafe {
        RegGetValueW(
            hive.raw(),
            key_w.as_ptr(),
            value_w.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&raw mut data).cast(),
            &raw mut size,
        )
    };
    match rc {
        ERROR_SUCCESS => Ok(Some(data)),
        ERROR_FILE_NOT_FOUND => Ok(None),
        #[allow(clippy::cast_possible_wrap)]
        other => Err(io::Error::from_raw_os_error(other as i32)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NT: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";

    #[test]
    fn reads_existing_string_value() {
        let v = read_string(Hive::LocalMachine, NT, "CurrentBuildNumber").unwrap();
        let build: u32 = v.expect("build number").parse().unwrap();
        assert!(build > 10_000);
    }

    #[test]
    fn reads_existing_dword_value() {
        let v = read_dword(Hive::LocalMachine, NT, "CurrentMajorVersionNumber").unwrap();
        assert_eq!(v, Some(10)); // Windows 10 and 11 both report major version 10
    }

    #[test]
    fn subkeys_of_existing_and_missing_keys() {
        let keys = subkeys(Hive::LocalMachine, r"SOFTWARE\Microsoft").unwrap();
        assert!(
            keys.iter().any(|k| k.eq_ignore_ascii_case("Windows")),
            "{keys:?}"
        );
        assert_eq!(
            subkeys(Hive::LocalMachine, r"SOFTWARE\UmrDoesNotExist").unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn missing_value_is_none() {
        assert_eq!(
            read_string(Hive::LocalMachine, NT, "UmrDoesNotExist").unwrap(),
            None
        );
        assert_eq!(
            read_dword(Hive::CurrentUser, r"Software\UmrDoesNotExist", "x").unwrap(),
            None
        );
    }
}
