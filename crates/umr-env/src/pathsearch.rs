//! Finding executables on PATH (Windows semantics: PATHEXT, case-insensitive).

use std::path::{Path, PathBuf};

use crate::context::ScanContext;

/// Default extensions when PATHEXT is not set.
const DEFAULT_EXTS: &[&str] = &[".com", ".exe", ".bat", ".cmd"];

fn extensions(ctx: &ScanContext) -> Vec<String> {
    ctx.var("PATHEXT").map_or_else(
        || DEFAULT_EXTS.iter().map(|s| (*s).to_string()).collect(),
        |v| {
            v.split(';')
                .filter(|e| !e.is_empty())
                .map(str::to_lowercase)
                .collect()
        },
    )
}

/// Every match of `name` on PATH, in PATH order, without duplicates (first match = what a shell runs).
#[must_use]
pub fn find_all(ctx: &ScanContext, name: &str) -> Vec<PathBuf> {
    let exts = extensions(ctx);
    let has_ext = Path::new(name).extension().is_some();
    let mut out: Vec<PathBuf> = Vec::new();
    for dir in ctx.path_entries() {
        let candidates: Vec<PathBuf> = if has_ext {
            vec![dir.join(name)]
        } else {
            exts.iter()
                .map(|e| dir.join(format!("{name}{e}")))
                .collect()
        };
        for c in candidates {
            if c.is_file() {
                let key = normalize(&c);
                if !out.iter().any(|o| normalize(o) == key) {
                    out.push(c);
                }
                break; // first extension wins within one directory
            }
        }
    }
    out
}

/// First match on PATH.
#[must_use]
pub fn find(ctx: &ScanContext, name: &str) -> Option<PathBuf> {
    find_all(ctx, name).into_iter().next()
}

/// Lower-cased path without trailing separators, for comparisons.
#[must_use]
pub fn normalize(p: &Path) -> String {
    let s = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let s = s.to_string_lossy().replace('/', "\\");
    s.trim_start_matches(r"\\?\")
        .trim_end_matches('\\')
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_in_path_order_and_respects_pathext() {
        let base = std::env::temp_dir().join(format!("umr-pathsearch-{}", std::process::id()));
        let (a, b) = (base.join("a"), base.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("tool.cmd"), "").unwrap();
        std::fs::write(b.join("tool.exe"), "").unwrap();
        std::fs::write(b.join("other.txt"), "").unwrap();
        let path = format!("{};\"{}\";;", a.display(), b.display());
        let ctx = ScanContext::for_test(base.clone(), &[("PATH", &path), ("PATHEXT", ".EXE;.CMD")]);
        let all = find_all(&ctx, "tool");
        assert_eq!(all.len(), 2);
        assert!(all[0].ends_with("tool.cmd"));
        assert!(find(&ctx, "other").is_none(), ".txt is not executable");
        assert!(find(&ctx, "tool.exe").unwrap().ends_with(r"b\tool.exe"));
        let _ = std::fs::remove_dir_all(&base);
    }
}
