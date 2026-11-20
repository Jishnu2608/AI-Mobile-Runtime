//! Registry access (mostly reads).
//!
//! Used for tool detection (e.g. JDK installs under `HKLM\SOFTWARE\...`) and OS settings.
//! The product writes to the registry only in the consented elevated helper (INS-005) and, when
//! the user switches the option on, to their own `PATH` (`user_path`, W9 / DEC-042).

use std::io;

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    RegGetValueW, RegSetKeyValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_DWORD,
    RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
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

/// Reads a `REG_SZ` or `REG_EXPAND_SZ` value as stored (variables like `%USERPROFILE%` are not
/// expanded). Returns the text and whether it is `REG_EXPAND_SZ`; `Ok(None)` if it is missing.
pub fn read_raw_string(hive: Hive, key: &str, value: &str) -> io::Result<Option<(String, bool)>> {
    use windows_sys::Win32::System::Registry::{REG_EXPAND_SZ, RRF_NOEXPAND, RRF_RT_REG_EXPAND_SZ};
    let key_w = to_wide(key);
    let value_w = to_wide(value);
    let mut buf: Vec<u16> = vec![0; 512];
    loop {
        let mut size = u32::try_from(buf.len() * 2).unwrap_or(u32::MAX);
        let mut kind = 0u32;
        // SAFETY: key/value are NUL-terminated; `buf` has `size` bytes; `kind` is a valid out.
        let rc = unsafe {
            RegGetValueW(
                hive.raw(),
                key_w.as_ptr(),
                value_w.as_ptr(),
                RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND,
                &raw mut kind,
                buf.as_mut_ptr().cast(),
                &raw mut size,
            )
        };
        match rc {
            ERROR_SUCCESS => {
                buf.truncate(size as usize / 2);
                let text = from_wide_buf(&buf).to_string_lossy().into_owned();
                return Ok(Some((text, kind == REG_EXPAND_SZ)));
            }
            ERROR_MORE_DATA => buf.resize(size as usize / 2 + 1, 0),
            ERROR_FILE_NOT_FOUND => return Ok(None),
            #[allow(clippy::cast_possible_wrap)]
            other => return Err(io::Error::from_raw_os_error(other as i32)),
        }
    }
}

/// Writes a `REG_SZ` (or, with `expand`, `REG_EXPAND_SZ`) value under the current user only.
/// Used for the user's own `PATH` when they switch that option on (W9 / DEC-042).
pub fn write_user_string(key: &str, value: &str, data: &str, expand: bool) -> io::Result<()> {
    use windows_sys::Win32::System::Registry::{REG_EXPAND_SZ, REG_SZ};
    let key_w = to_wide(key);
    let value_w = to_wide(value);
    let data_w = to_wide(data);
    let bytes = u32::try_from(data_w.len() * 2)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "value too long"))?;
    // SAFETY: key/value/data are NUL-terminated; `bytes` is the size of `data_w` in bytes.
    let rc = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            key_w.as_ptr(),
            value_w.as_ptr(),
            if expand { REG_EXPAND_SZ } else { REG_SZ },
            data_w.as_ptr().cast(),
            bytes,
        )
    };
    if rc == ERROR_SUCCESS {
        Ok(())
    } else {
        #[allow(clippy::cast_possible_wrap)]
        Err(io::Error::from_raw_os_error(rc as i32))
    }
}

/// Writes a `REG_DWORD` value (creates the key if missing). HKLM needs administrator rights;
/// only the elevated helper (INS-005) calls this for HKLM.
pub fn write_dword(hive: Hive, key: &str, value: &str, data: u32) -> io::Result<()> {
    let key_w = to_wide(key);
    let value_w = to_wide(value);
    // SAFETY: key/value are NUL-terminated; `data` is 4 bytes, matching the size passed.
    let rc = unsafe {
        RegSetKeyValueW(
            hive.raw(),
            key_w.as_ptr(),
            value_w.as_ptr(),
            REG_DWORD,
            (&raw const data).cast(),
            4,
        )
    };
    if rc == ERROR_SUCCESS {
        Ok(())
    } else {
        #[allow(clippy::cast_possible_wrap)]
        Err(io::Error::from_raw_os_error(rc as i32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dword_round_trip_in_hkcu() {
        let key = r"Software\TUMR-test\registry-write";
        write_dword(Hive::CurrentUser, key, "v", 7).unwrap();
        assert_eq!(read_dword(Hive::CurrentUser, key, "v").unwrap(), Some(7));
        write_dword(Hive::CurrentUser, key, "v", 1).unwrap();
        assert_eq!(read_dword(Hive::CurrentUser, key, "v").unwrap(), Some(1));
        // clean up the test key
        let _ = std::process::Command::new("reg")
            .args(["delete", r"HKCU\Software\TUMR-test", "/f"])
            .output();
    }

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
            subkeys(Hive::LocalMachine, r"SOFTWARE\TumrDoesNotExist").unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn missing_value_is_none() {
        assert_eq!(
            read_string(Hive::LocalMachine, NT, "TumrDoesNotExist").unwrap(),
            None
        );
        assert_eq!(
            read_dword(Hive::CurrentUser, r"Software\TumrDoesNotExist", "x").unwrap(),
            None
        );
    }
}
