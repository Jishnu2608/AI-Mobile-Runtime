//! Files readable and writable only by the current Windows user.
//!
//! Used for secrets such as the Core's admin token and the client registry. The file is
//! (re)created with a protected DACL that grants access to the current user's SID only,
//! independent of the folder's inherited permissions (relevant when `UMR_HOME` points to a
//! shared location).

use std::fs::File;
use std::io::{self, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::Path;

use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, CREATE_NEW, FILE_ATTRIBUTE_NORMAL};

use crate::secure_pipe::{owner_only_sddl, SecurityDescriptor};
use crate::wide::to_wide;

/// Writes `data` to `path` atomically-enough for small secrets: any existing file is removed
/// first (an existing file would keep its old permissions), then a new file is created with an
/// owner-only DACL.
pub fn write_private(path: &Path, data: &[u8]) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let sid = crate::identity::current_user_sid()?;
    let sd = SecurityDescriptor::from_sddl(&owner_only_sddl(&sid))?;
    let mut attrs = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(std::mem::size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(0),
        lpSecurityDescriptor: sd.0,
        bInheritHandle: 0,
    };
    let wide = to_wide(path.as_os_str());
    // SAFETY: path is NUL-terminated; `attrs` and its descriptor outlive the call; no template file.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0, // no sharing while we write
            &raw mut attrs,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `handle` is a freshly created, owned file handle; File takes ownership and closes it.
    let mut file = unsafe { File::from_raw_handle(handle.cast()) };
    file.write_all(data)?;
    file.sync_all()?;
    Ok(())
}

/// Returns the DACL of an existing file as SDDL (for verification and diagnostics).
pub fn file_dacl_sddl(path: &Path) -> io::Result<String> {
    let f = File::open(path)?;
    crate::secure_pipe::dacl_sddl(f.as_raw_handle())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_file_has_owner_only_dacl_and_content() {
        let dir = std::env::temp_dir().join(format!("umr-secure-file-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("secret.txt");
        write_private(&path, b"first").unwrap();
        write_private(&path, b"second").unwrap(); // overwrite replaces the file
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        let dacl = file_dacl_sddl(&path).unwrap();
        let sid = crate::identity::current_user_sid().unwrap();
        assert!(dacl.starts_with("D:P"), "{dacl}");
        assert!(dacl.contains(&sid), "{dacl}");
        assert_eq!(dacl.matches("(A;").count(), 1, "{dacl}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
