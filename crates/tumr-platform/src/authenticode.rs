//! Authenticode signatures of downloaded executables (INS-009).
//!
//! [`signer`] asks Windows whether a file carries a valid Authenticode signature
//! (`WinVerifyTrust`, generic verify action, offline: no revocation lookups, no UI) and returns the
//! signer's display name from its certificate. The installer compares that name with the
//! publisher it expects for each component, so a correctly checksummed archive that contains
//! someone else's executable is still refused.

/// A valid signature and who made it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signer {
    /// Display name of the signing certificate (e.g. `Eclipse Foundation`, `Google LLC`).
    pub name: String,
}

/// Why a file is not accepted as signed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureError {
    /// No signature at all.
    NotSigned,
    /// A signature that Windows rejects (tampered file, untrusted or expired chain, …): the
    /// `WinVerifyTrust` status code.
    Invalid(i32),
    /// The file could not be checked (missing file, API failure).
    Unavailable(String),
}

impl std::fmt::Display for SignatureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotSigned => write!(f, "not signed"),
            Self::Invalid(code) => write!(f, "invalid signature (0x{code:08x})"),
            Self::Unavailable(why) => write!(f, "signature not checked: {why}"),
        }
    }
}

impl std::error::Error for SignatureError {}

/// The valid Authenticode signer of `path`.
pub fn signer(path: &std::path::Path) -> Result<Signer, SignatureError> {
    if !path.is_file() {
        return Err(SignatureError::Unavailable(format!(
            "{} not found",
            path.display()
        )));
    }
    imp::signer(path)
}

#[cfg(windows)]
mod imp {
    use super::{SignatureError, Signer};
    use crate::wide::to_wide;
    use windows_sys::Win32::Foundation::{HANDLE, TRUST_E_NOSIGNATURE};
    use windows_sys::Win32::Security::Cryptography::{
        CertGetNameStringW, CERT_NAME_SIMPLE_DISPLAY_TYPE,
    };
    use windows_sys::Win32::Security::WinTrust::{
        WTHelperGetProvCertFromChain, WTHelperGetProvSignerFromChain,
        WTHelperProvDataFromStateData, WinVerifyTrust, WINTRUST_ACTION_GENERIC_VERIFY_V2,
        WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO, WTD_CACHE_ONLY_URL_RETRIEVAL,
        WTD_CHOICE_FILE, WTD_REVOCATION_CHECK_NONE, WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE,
        WTD_STATEACTION_VERIFY, WTD_UI_NONE,
    };

    // Status codes WinVerifyTrust returns for an unsigned file (besides TRUST_E_NOSIGNATURE).
    #[allow(clippy::cast_possible_wrap)]
    const TRUST_E_SUBJECT_FORM_UNKNOWN: i32 = 0x800B_0003_u32 as i32;
    #[allow(clippy::cast_possible_wrap)]
    const TRUST_E_PROVIDER_UNKNOWN: i32 = 0x800B_0001_u32 as i32;

    pub(super) fn signer(path: &std::path::Path) -> Result<Signer, SignatureError> {
        let wide = to_wide(path.as_os_str());
        let mut file = WINTRUST_FILE_INFO {
            cbStruct: u32::try_from(std::mem::size_of::<WINTRUST_FILE_INFO>()).unwrap_or(0),
            pcwszFilePath: wide.as_ptr(),
            hFile: std::ptr::null_mut(),
            pgKnownSubject: std::ptr::null_mut(),
        };
        let mut data = WINTRUST_DATA {
            cbStruct: u32::try_from(std::mem::size_of::<WINTRUST_DATA>()).unwrap_or(0),
            pPolicyCallbackData: std::ptr::null_mut(),
            pSIPClientData: std::ptr::null_mut(),
            dwUIChoice: WTD_UI_NONE,
            fdwRevocationChecks: WTD_REVOKE_NONE,
            dwUnionChoice: WTD_CHOICE_FILE,
            Anonymous: WINTRUST_DATA_0 {
                pFile: &raw mut file,
            },
            dwStateAction: WTD_STATEACTION_VERIFY,
            hWVTStateData: std::ptr::null_mut(),
            pwszURLReference: std::ptr::null_mut(),
            // offline: never fetch CRLs or certificates while installing
            dwProvFlags: WTD_REVOCATION_CHECK_NONE | WTD_CACHE_ONLY_URL_RETRIEVAL,
            dwUIContext: 0,
            pSignatureSettings: std::ptr::null_mut(),
        };
        let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
        // SAFETY: `data` and `file` are fully initialised, live until the CLOSE call below, and
        // `wide` (the path) outlives both; INVALID_HANDLE_VALUE-like window handle 0 = no UI.
        let status = unsafe {
            WinVerifyTrust(
                std::ptr::null_mut(),
                &raw mut action,
                std::ptr::from_mut(&mut data).cast(),
            )
        };
        let result = if status == 0 {
            signer_name(data.hWVTStateData).map(|name| Signer { name })
        } else if status == TRUST_E_NOSIGNATURE
            || status == TRUST_E_SUBJECT_FORM_UNKNOWN
            || status == TRUST_E_PROVIDER_UNKNOWN
        {
            Err(SignatureError::NotSigned)
        } else {
            Err(SignatureError::Invalid(status))
        };
        data.dwStateAction = WTD_STATEACTION_CLOSE;
        // SAFETY: closes the state WinVerifyTrust opened above (required even after a failure).
        unsafe {
            WinVerifyTrust(
                std::ptr::null_mut(),
                &raw mut action,
                std::ptr::from_mut(&mut data).cast(),
            );
        }
        result
    }

    /// Display name of the leaf certificate of the primary signer.
    fn signer_name(state: HANDLE) -> Result<String, SignatureError> {
        let fail = |what: &str| SignatureError::Unavailable(format!("{what} not available"));
        // SAFETY: `state` comes from a successful WTD_STATEACTION_VERIFY and is still open; the
        // helpers return pointers into that state (or null), which are only read before CLOSE.
        unsafe {
            let prov = WTHelperProvDataFromStateData(state);
            if prov.is_null() {
                return Err(fail("provider data"));
            }
            let sgnr = WTHelperGetProvSignerFromChain(prov, 0, 0, 0);
            if sgnr.is_null() {
                return Err(fail("signer"));
            }
            let cert = WTHelperGetProvCertFromChain(sgnr, 0);
            if cert.is_null() || (*cert).pCert.is_null() {
                return Err(fail("signer certificate"));
            }
            let ctx = (*cert).pCert;
            let len = CertGetNameStringW(
                ctx,
                CERT_NAME_SIMPLE_DISPLAY_TYPE,
                0,
                std::ptr::null(),
                std::ptr::null_mut(),
                0,
            );
            if len <= 1 {
                return Err(fail("signer name"));
            }
            let mut buf = vec![0u16; len as usize];
            let n = CertGetNameStringW(
                ctx,
                CERT_NAME_SIMPLE_DISPLAY_TYPE,
                0,
                std::ptr::null(),
                buf.as_mut_ptr(),
                len,
            );
            let n = (n as usize).saturating_sub(1).min(buf.len());
            Ok(String::from_utf16_lossy(&buf[..n]))
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{SignatureError, Signer};

    pub(super) fn signer(_path: &std::path::Path) -> Result<Signer, SignatureError> {
        Err(SignatureError::Unavailable(
            "Authenticode is checked on Windows only".into(),
        ))
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn a_system_file_is_signed_by_microsoft() {
        let windir = std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into());
        let notepad = std::path::Path::new(&windir)
            .join("System32")
            .join("notepad.exe");
        // notepad is catalog-signed on some builds; only check the result is well-formed
        match signer(&notepad) {
            Ok(s) => assert!(s.name.contains("Microsoft"), "{s:?}"),
            Err(SignatureError::NotSigned) => {}
            Err(e) => panic!("{e}"),
        }
    }

    /// `TUMR_SIGNED_FILE=<exe> TUMR_SIGNER=<name> cargo test -p tumr-platform -- --ignored live_signer`
    #[test]
    #[ignore = "needs a real signed file (TUMR_SIGNED_FILE, TUMR_SIGNER)"]
    fn live_signer() {
        let f = std::env::var("TUMR_SIGNED_FILE").unwrap();
        let want = std::env::var("TUMR_SIGNER").unwrap();
        assert_eq!(signer(std::path::Path::new(&f)).unwrap().name, want);
    }

    #[test]
    fn an_unsigned_file_is_reported_as_such() {
        let dir = std::env::temp_dir().join(format!("tumr-authenticode-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("plain.exe");
        std::fs::write(&f, b"MZ not really a program").unwrap();
        assert!(matches!(
            signer(&f),
            Err(SignatureError::NotSigned | SignatureError::Invalid(_))
        ));
        assert!(matches!(
            signer(&dir.join("missing.exe")),
            Err(SignatureError::Unavailable(_))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
