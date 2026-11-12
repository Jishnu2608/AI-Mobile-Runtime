//! Named pipes that only the current Windows user can open.
//!
//! The Runtime Core's local API listens on a named pipe. Its DACL grants access to the
//! current user's SID only (`D:P(A;;GA;;;<SID>)` — protected, no inherited entries), so
//! other users on the same machine, and remote clients, cannot connect.

use std::ffi::c_void;
use std::io;
use std::os::windows::io::RawHandle;

use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows_sys::Win32::Foundation::{LocalFree, ERROR_SUCCESS};
use windows_sys::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW,
    ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1,
    SE_KERNEL_OBJECT,
};
use windows_sys::Win32::Security::{
    DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
};

use crate::wide::{from_wide_ptr, to_wide};

/// SDDL that allows only `sid` full access, with inheritance blocked.
#[must_use]
pub fn owner_only_sddl(sid: &str) -> String {
    format!("D:P(A;;GA;;;{sid})")
}

/// A security descriptor built from SDDL, owned and freed on drop.
#[derive(Debug)]
pub(crate) struct SecurityDescriptor(pub(crate) PSECURITY_DESCRIPTOR);

impl SecurityDescriptor {
    pub(crate) fn from_sddl(sddl: &str) -> io::Result<Self> {
        let w = to_wide(sddl);
        let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // SAFETY: `w` is NUL-terminated; `sd` is a valid out-pointer; size out-param may be null.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                w.as_ptr(),
                SDDL_REVISION_1,
                &raw mut sd,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(sd))
    }
}

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        // SAFETY: the descriptor was allocated by ConvertString…W with LocalAlloc; freed once.
        unsafe { LocalFree(self.0) };
    }
}

/// Creates one instance of a named pipe server restricted to the current user.
///
/// `first_instance = true` makes creation fail if *any* process already owns a pipe with
/// this name — this stops another program from squatting the name before the Core starts.
pub fn create_user_only_server(name: &str, first_instance: bool) -> io::Result<NamedPipeServer> {
    let sid = crate::identity::current_user_sid()?;
    let sd = SecurityDescriptor::from_sddl(&owner_only_sddl(&sid))?;
    let mut attrs = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(std::mem::size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(0),
        lpSecurityDescriptor: sd.0,
        bInheritHandle: 0,
    };
    let mut opts = ServerOptions::new();
    opts.first_pipe_instance(first_instance)
        .reject_remote_clients(true);
    // SAFETY: `attrs` is a valid SECURITY_ATTRIBUTES whose descriptor (`sd`) outlives this call.
    unsafe { opts.create_with_security_attributes_raw(name, (&raw mut attrs).cast::<c_void>()) }
}

/// Reads the DACL of a kernel object handle back as SDDL (used to verify pipe security).
pub fn dacl_sddl(handle: RawHandle) -> io::Result<String> {
    let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: handle is valid for the call; only the security descriptor out-param is requested.
    let rc = unsafe {
        GetSecurityInfo(
            handle.cast(),
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &raw mut sd,
        )
    };
    if rc != ERROR_SUCCESS {
        #[allow(clippy::cast_possible_wrap)]
        return Err(io::Error::from_raw_os_error(rc as i32));
    }
    let sd = SecurityDescriptor(sd);
    let mut out: *mut u16 = std::ptr::null_mut();
    // SAFETY: `sd.0` is a valid descriptor; `out` receives a LocalAlloc'd string.
    let ok = unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            sd.0,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &raw mut out,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `out` is a NUL-terminated string on success.
    let s = unsafe { from_wide_ptr(out) };
    // SAFETY: frees the LocalAlloc'd string exactly once.
    unsafe { LocalFree(out.cast()) };
    Ok(s.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::windows::named_pipe::ClientOptions;

    fn unique_name(tag: &str) -> String {
        format!(r"\\.\pipe\umr-test-{tag}-{}", std::process::id())
    }

    #[test]
    fn sddl_format() {
        assert_eq!(owner_only_sddl("S-1-5-21-1"), "D:P(A;;GA;;;S-1-5-21-1)");
    }

    #[tokio::test]
    async fn pipe_dacl_grants_only_current_user() {
        let name = unique_name("dacl");
        let server = create_user_only_server(&name, true).unwrap();
        let dacl = dacl_sddl(server.as_raw_handle()).unwrap();
        let sid = crate::identity::current_user_sid().unwrap();
        assert!(dacl.starts_with("D:P"), "DACL must be protected: {dacl}");
        assert!(
            crate::identity::sddl_names_user(&dacl, &sid),
            "DACL must name the user: {dacl}"
        );
        assert_eq!(
            dacl.matches("(A;").count(),
            1,
            "exactly one allow entry: {dacl}"
        );
        assert!(
            !dacl.contains(";;;WD)") && !dacl.contains(";;;BU)"),
            "{dacl}"
        );
    }

    #[tokio::test]
    async fn second_first_instance_is_refused() {
        let name = unique_name("first");
        let _a = create_user_only_server(&name, true).unwrap();
        assert!(create_user_only_server(&name, true).is_err());
        // additional (non-first) instances by the same owner are allowed
        assert!(create_user_only_server(&name, false).is_ok());
    }

    #[tokio::test]
    async fn current_user_can_connect_and_exchange_bytes() {
        let name = unique_name("io");
        let server = create_user_only_server(&name, true).unwrap();
        let name2 = name.clone();
        let client = tokio::spawn(async move {
            let mut c = ClientOptions::new().open(&name2).unwrap();
            c.write_all(b"hi").await.unwrap();
            let mut buf = [0u8; 2];
            c.read_exact(&mut buf).await.unwrap();
            buf
        });
        server.connect().await.unwrap();
        let mut server = server;
        let mut buf = [0u8; 2];
        server.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"hi");
        server.write_all(b"ok").await.unwrap();
        assert_eq!(&client.await.unwrap(), b"ok");
    }
}
