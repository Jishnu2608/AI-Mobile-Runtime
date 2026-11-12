//! ENV-008/010: watch the places tools write to when they are *not* isolated.
//!
//! [`snapshot`] records, for each known location, how many files it has and the newest change
//! time; [`diff`] lists what appeared or changed. Used by the leak-check example (and the
//! clean-VM tests, TEST-001) around a tool run: an isolated run must leave all of them as
//! they were.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::context::ScanContext;

/// A location that must not change while the product runs tools.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Watched {
    pub path: PathBuf,
    pub why: String,
}

/// Known leak locations for this user (only those that can be resolved).
#[must_use]
pub fn known_locations(ctx: &ScanContext) -> Vec<Watched> {
    let mut out = Vec::new();
    let mut add = |p: Option<PathBuf>, why: &str| {
        if let Some(p) = p {
            out.push(Watched {
                path: p,
                why: why.into(),
            });
        }
    };
    let profile = ctx.var("USERPROFILE").map(PathBuf::from);
    let appdata = ctx.var("APPDATA").map(PathBuf::from);
    let temp = ctx
        .var("TEMP")
        .map(PathBuf::from)
        .or_else(|| ctx.local_app_data.as_ref().map(|l| l.join("Temp")));
    add(
        profile.as_ref().map(|p| p.join(".android")),
        "Android user home (adb keys, emulator settings, Android CLI)",
    );
    add(
        profile.as_ref().map(|p| p.join(".expo")),
        "Expo home (state, login token cache)",
    );
    add(
        temp.as_ref().map(|t| t.join("netsimd")),
        "emulator network simulator files",
    );
    add(
        temp.as_ref().map(|t| t.join("avd")),
        "emulator discovery files",
    );
    add(
        appdata.as_ref().map(|a| a.join(".flutter_settings")),
        "Flutter settings (`flutter config`)",
    );
    add(
        appdata.as_ref().map(|a| a.join("dart")),
        "Dart telemetry settings",
    );
    add(
        ctx.local_app_data
            .as_ref()
            .map(|l| l.join("Pub").join("Cache")),
        "pub package cache",
    );
    out
}

/// File count and newest modification time below one location.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct State {
    pub exists: bool,
    pub files: u64,
    /// Seconds since the Unix epoch.
    pub newest: u64,
}

pub type Snapshot = BTreeMap<PathBuf, State>;

fn secs(t: SystemTime) -> u64 {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn state_of(p: &Path) -> State {
    let Ok(meta) = std::fs::symlink_metadata(p) else {
        return State::default();
    };
    let mut s = State {
        exists: true,
        files: 0,
        newest: meta.modified().map_or(0, secs),
    };
    if meta.is_file() {
        s.files = 1;
        return s;
    }
    let mut stack = vec![p.to_path_buf()];
    let mut seen = 0u32;
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            seen += 1;
            if seen > 50_000 {
                return s; // big caches: enough to notice changes near the top
            }
            let Ok(m) = e.metadata() else { continue };
            s.newest = s.newest.max(m.modified().map_or(0, secs));
            if m.is_dir() {
                stack.push(e.path());
            } else {
                s.files += 1;
            }
        }
    }
    s
}

#[must_use]
pub fn snapshot(watched: &[Watched]) -> Snapshot {
    watched
        .iter()
        .map(|w| (w.path.clone(), state_of(&w.path)))
        .collect()
}

/// A location that changed between two snapshots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub path: PathBuf,
    pub what: String,
}

#[must_use]
pub fn diff(before: &Snapshot, after: &Snapshot) -> Vec<Change> {
    let mut out = Vec::new();
    for (p, a) in after {
        let b = before.get(p).copied().unwrap_or_default();
        let what = if a.exists && !b.exists {
            Some(format!("created ({} files)", a.files))
        } else if a.files > b.files {
            Some(format!("{} new files", a.files - b.files))
        } else if a.newest > b.newest {
            Some("files changed".to_string())
        } else {
            None
        };
        if let Some(what) = what {
            out.push(Change {
                path: p.clone(),
                what,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_created_and_grown_locations() {
        let base = std::env::temp_dir().join(format!("umr-leaks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("existing")).unwrap();
        let w = vec![
            Watched {
                path: base.join("existing"),
                why: "x".into(),
            },
            Watched {
                path: base.join("fresh"),
                why: "y".into(),
            },
            Watched {
                path: base.join("quiet"),
                why: "z".into(),
            },
        ];
        let before = snapshot(&w);
        assert_eq!(diff(&before, &snapshot(&w)), Vec::new());
        std::fs::write(base.join("existing").join("a.txt"), "1").unwrap();
        std::fs::create_dir_all(base.join("fresh")).unwrap();
        std::fs::write(base.join("fresh").join("b.txt"), "2").unwrap();
        let d = diff(&before, &snapshot(&w));
        assert_eq!(d.len(), 2, "{d:?}");
        assert!(d
            .iter()
            .any(|c| c.path.ends_with("fresh") && c.what.starts_with("created")));
        assert!(d
            .iter()
            .any(|c| c.path.ends_with("existing") && c.what == "1 new files"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn known_locations_resolve_from_the_environment() {
        let mut ctx = ScanContext::for_test(
            r"D:\UMR".into(),
            &[
                ("USERPROFILE", r"C:\Users\u"),
                ("APPDATA", r"C:\Users\u\AppData\Roaming"),
            ],
        );
        ctx.local_app_data = Some(r"C:\Users\u\AppData\Local".into());
        let l: Vec<String> = known_locations(&ctx)
            .iter()
            .map(|w| w.path.display().to_string())
            .collect();
        assert!(l.contains(&r"C:\Users\u\.android".to_string()));
        assert!(l.contains(&r"C:\Users\u\.expo".to_string()));
        assert!(l.contains(&r"C:\Users\u\AppData\Local\Temp\netsimd".to_string()));
        assert!(l.contains(&r"C:\Users\u\AppData\Roaming\.flutter_settings".to_string()));
    }
}
