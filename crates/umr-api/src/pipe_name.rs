//! Name of the Core's named pipe.
//!
//! Default: `\\.\pipe\umr-<SID>` — one Core per Windows user.
//! With a non-default data root (`UMR_HOME`), a stable hash of the root is appended so that
//! test instances and the user's real Core never collide. Client and server compute the same
//! name from the same root.

use std::path::Path;

/// Pipe name for `sid`, optionally qualified by a non-default data root.
#[must_use]
pub fn pipe_name(sid: &str, custom_root: Option<&Path>) -> String {
    match custom_root {
        None => format!(r"\\.\pipe\umr-{sid}"),
        Some(root) => format!(r"\\.\pipe\umr-{sid}-{:016x}", fnv1a(&normalise(root))),
    }
}

fn normalise(root: &Path) -> String {
    root.to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_lowercase()
}

/// FNV-1a 64-bit: stable across Rust versions and builds (unlike `DefaultHasher`).
fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_name_uses_sid() {
        assert_eq!(pipe_name("S-1-5-21-9", None), r"\\.\pipe\umr-S-1-5-21-9");
    }

    #[test]
    fn custom_root_is_stable_and_case_insensitive() {
        let a = pipe_name("S-1", Some(Path::new(r"D:\Tmp\X\")));
        let b = pipe_name("S-1", Some(Path::new(r"d:\tmp\x")));
        assert_eq!(a, b);
        assert!(a.starts_with(r"\\.\pipe\umr-S-1-"));
        assert_ne!(a, pipe_name("S-1", Some(Path::new(r"D:\Tmp\Y"))));
    }

    #[test]
    fn fnv_known_vector() {
        assert_eq!(fnv1a(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a("a"), 0xaf63_dc4c_8601_ec8c);
    }
}
