//! UTF-16 helpers for Windows API strings.

use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};

/// Encodes a string as a NUL-terminated UTF-16 buffer.
pub fn to_wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(std::iter::once(0)).collect()
}

/// Reads a NUL-terminated UTF-16 string from a raw pointer.
///
/// # Safety
/// `ptr` must be non-null and point to a valid NUL-terminated UTF-16 string.
pub unsafe fn from_wide_ptr(ptr: *const u16) -> OsString {
    let mut len = 0usize;
    // SAFETY: caller guarantees a NUL terminator exists, so every offset up to it is readable.
    while unsafe { *ptr.add(len) } != 0 {
        len += 1;
    }
    // SAFETY: `len` elements starting at `ptr` were just read and are valid.
    OsString::from_wide(unsafe { std::slice::from_raw_parts(ptr, len) })
}

/// Converts a UTF-16 buffer that may contain a trailing NUL into an `OsString`.
pub fn from_wide_buf(buf: &[u16]) -> OsString {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    OsString::from_wide(&buf[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let w = to_wide("héllo");
        assert_eq!(*w.last().unwrap(), 0);
        assert_eq!(from_wide_buf(&w), OsString::from("héllo"));
        // SAFETY: `w` is NUL-terminated.
        let decoded = unsafe { from_wide_ptr(w.as_ptr()) };
        assert_eq!(decoded, OsString::from("héllo"));
    }
}
