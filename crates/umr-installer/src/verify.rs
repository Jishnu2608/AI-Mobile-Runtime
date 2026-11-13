//! INS-002: checksum verification and safe extraction.
//!
//! An archive is only opened after its checksum matches the value published by the official
//! source. Extraction refuses entries that would leave the target folder (zip-slip: `..`,
//! absolute paths, drive letters, NTFS streams), symbolic links, and archives larger than
//! [`MAX_UNPACKED_BYTES`].

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use sha1::Digest as _;

use crate::InstallError;

/// Upper limit for unpacked size (the Android NDK is ~2.3 GB; Flutter ~3.5 GB).
pub const MAX_UNPACKED_BYTES: u64 = 16 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "algo", content = "hex", rename_all = "lowercase")]
pub enum Checksum {
    Sha256(String),
    Sha1(String),
}

impl Checksum {
    #[must_use]
    pub fn hex(&self) -> &str {
        match self {
            Self::Sha256(h) | Self::Sha1(h) => h,
        }
    }

    /// SHA-256 value, if this is one (stored with the component record).
    #[must_use]
    pub fn sha256(&self) -> Option<&str> {
        match self {
            Self::Sha256(h) => Some(h),
            Self::Sha1(_) => None,
        }
    }
}

pub(crate) fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// Hex SHA-256 of a byte string.
#[must_use]
pub fn sha256_hex(data: &[u8]) -> String {
    to_hex(&sha2::Sha256::digest(data))
}

/// Hashes a file with the algorithm of `kind`.
pub fn hash_file(path: &Path, kind: &Checksum) -> std::io::Result<String> {
    let mut r = BufReader::with_capacity(1 << 20, File::open(path)?);
    let mut buf = vec![0u8; 1 << 20];
    match kind {
        Checksum::Sha256(_) => {
            let mut h = sha2::Sha256::new();
            loop {
                let n = r.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
            }
            Ok(to_hex(&h.finalize()))
        }
        Checksum::Sha1(_) => {
            let mut h = sha1::Sha1::new();
            loop {
                let n = r.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
            }
            Ok(to_hex(&h.finalize()))
        }
    }
}

/// Fails unless the file matches the checksum.
pub fn check(path: &Path, expected: &Checksum) -> Result<(), InstallError> {
    let actual = hash_file(path, expected)?;
    if actual.eq_ignore_ascii_case(expected.hex()) {
        Ok(())
    } else {
        Err(InstallError::Checksum {
            expected: expected.hex().to_lowercase(),
            actual,
        })
    }
}

/// A path inside the archive that is safe to create below the target folder.
fn safe_relative(name: &str, enclosed: Option<PathBuf>) -> Result<PathBuf, InstallError> {
    // the raw name first: archive libraries may "repair" absolute or drive paths silently
    if name.starts_with(['/', '\\'])
        || name.contains(':')
        || name.split(['/', '\\']).any(|seg| seg == "..")
    {
        return Err(InstallError::Extract(format!(
            "unsafe path in archive: {name}"
        )));
    }
    let rel =
        enclosed.ok_or_else(|| InstallError::Extract(format!("unsafe path in archive: {name}")))?;
    for c in rel.components() {
        match c {
            Component::Normal(part) => {
                let s = part.to_string_lossy();
                if s.contains(':') {
                    return Err(InstallError::Extract(format!(
                        "unsafe path in archive: {name}"
                    )));
                }
            }
            Component::CurDir => {}
            _ => {
                return Err(InstallError::Extract(format!(
                    "unsafe path in archive: {name}"
                )))
            }
        }
    }
    Ok(rel)
}

/// Extracts a zip into `dest` (created if missing). With `strip_top`, the single top-level
/// folder every entry shares (e.g. `jdk-17.0.21+9/`) is removed. Returns unpacked bytes.
pub fn extract_zip(
    archive: &Path,
    dest: &Path,
    strip_top: bool,
    cancel: &AtomicBool,
) -> Result<u64, InstallError> {
    let file = File::open(archive)?;
    let mut zip = zip::ZipArchive::new(BufReader::new(file))
        .map_err(|e| InstallError::Extract(e.to_string()))?;

    // Pass 1: validate every entry before writing anything.
    let mut entries: Vec<(usize, PathBuf, bool, u64)> = Vec::with_capacity(zip.len());
    let mut total: u64 = 0;
    let mut top: Option<std::ffi::OsString> = None;
    for i in 0..zip.len() {
        let e = zip
            .by_index_raw(i)
            .map_err(|e| InstallError::Extract(e.to_string()))?;
        let name = e
            .name()
            .map_err(|e| InstallError::Extract(e.to_string()))?
            .to_string();
        if e.is_symlink() {
            return Err(InstallError::Extract(format!(
                "archive contains a link: {name}"
            )));
        }
        let rel = safe_relative(&name, e.enclosed_name())?;
        total = total.saturating_add(e.size());
        if total > MAX_UNPACKED_BYTES {
            return Err(InstallError::Extract(
                "archive unpacks to more than 16 GB".into(),
            ));
        }
        if strip_top {
            let first = rel
                .components()
                .next()
                .map(|c| c.as_os_str().to_os_string());
            match (&top, first) {
                (None, Some(f)) => top = Some(f),
                (Some(t), Some(f)) if *t == f => {}
                _ => {
                    return Err(InstallError::Extract(
                        "archive has no single top-level folder".into(),
                    ))
                }
            }
        }
        entries.push((i, rel, e.is_dir(), e.size()));
    }

    std::fs::create_dir_all(dest)?;
    let mut written = 0u64;
    for (i, rel, is_dir, declared) in entries {
        if cancel.load(Ordering::Relaxed) {
            return Err(InstallError::Cancelled);
        }
        let rel = if strip_top {
            rel.components().skip(1).collect::<PathBuf>()
        } else {
            rel
        };
        if rel.as_os_str().is_empty() {
            continue; // the stripped top folder itself
        }
        let out = dest.join(&rel);
        if is_dir {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut entry = zip
            .by_index(i)
            .map_err(|e| InstallError::Extract(e.to_string()))?;
        let mut f = File::create(&out)?;
        // never trust the declared size: stop one byte past it (zip bombs, cloaked entries)
        let n = std::io::copy(&mut (&mut entry).take(declared.saturating_add(1)), &mut f)
            .map_err(|e| InstallError::Extract(format!("{}: {e}", rel.display())))?;
        if n > declared {
            drop(f);
            let _ = std::fs::remove_file(&out);
            return Err(InstallError::Extract(format!(
                "{}: entry is larger than the archive says",
                rel.display()
            )));
        }
        written += n;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{make_zip, tmp_dir};

    #[test]
    fn checksums() {
        let d = tmp_dir("verify-sum");
        let f = d.join("a.bin");
        std::fs::write(&f, b"hello").unwrap();
        let sha256 = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        let sha1 = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";
        assert!(check(&f, &Checksum::Sha256(sha256.to_uppercase())).is_ok());
        assert!(check(&f, &Checksum::Sha1(sha1.into())).is_ok());
        assert!(matches!(
            check(&f, &Checksum::Sha1("00".into())),
            Err(InstallError::Checksum { .. })
        ));
        assert_eq!(sha256_hex(b"hello"), sha256);
    }

    #[test]
    fn extracts_and_strips_top_folder() {
        let d = tmp_dir("verify-extract");
        let z = make_zip(
            &d.join("a.zip"),
            &[
                ("jdk-17/bin/java.exe", b"x".as_slice()),
                ("jdk-17/release", b"JAVA_VERSION=17"),
            ],
        );
        let out = d.join("out");
        let n = extract_zip(&z, &out, true, &AtomicBool::new(false)).unwrap();
        assert_eq!(n, 16);
        assert!(out.join("bin").join("java.exe").is_file());
        assert!(out.join("release").is_file());
        // without strip
        let out2 = d.join("out2");
        extract_zip(&z, &out2, false, &AtomicBool::new(false)).unwrap();
        assert!(out2.join("jdk-17").join("bin").join("java.exe").is_file());
    }

    #[test]
    fn refuses_zip_slip_and_mixed_tops() {
        let d = tmp_dir("verify-slip");
        let out = d.join("out");
        for bad in [
            "../evil.txt",
            "a/../../evil.txt",
            "C:/evil.txt",
            "/abs.txt",
            "a/file.txt:stream",
        ] {
            let z = make_zip(
                &d.join("bad.zip"),
                &[("a/ok.txt", b"1".as_slice()), (bad, b"2")],
            );
            let r = extract_zip(&z, &out, false, &AtomicBool::new(false));
            assert!(matches!(r, Err(InstallError::Extract(_))), "{bad}: {r:?}");
        }
        assert!(!d.join("evil.txt").exists());
        assert!(
            !out.join("a").join("ok.txt").exists(),
            "nothing written when any entry is bad"
        );
        let z = make_zip(
            &d.join("two.zip"),
            &[("a/x", b"1".as_slice()), ("b/y", b"2")],
        );
        assert!(matches!(
            extract_zip(&z, &out, true, &AtomicBool::new(false)),
            Err(InstallError::Extract(_))
        ));
    }

    #[test]
    fn extraction_can_be_cancelled() {
        let d = tmp_dir("verify-cancel");
        let z = make_zip(&d.join("a.zip"), &[("t/a", b"1".as_slice())]);
        assert_eq!(
            extract_zip(&z, &d.join("o"), true, &AtomicBool::new(true)),
            Err(InstallError::Cancelled)
        );
    }
}
