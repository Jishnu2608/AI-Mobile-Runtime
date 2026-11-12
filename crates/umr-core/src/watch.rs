//! ADP-006: file watching for `project.run`.
//!
//! The project folder is watched recursively (`ReadDirectoryChangesW` via `notify`). Changes in
//! generated folders are dropped ([`umr_adapter::ignored`]); after 300 ms without new changes the
//! adapter decides what the saved files need ([`umr_adapter::Adapter::change_kind`]): the
//! strongest of them is run as one reload. React Native JS changes need nothing (Metro's Fast
//! Refresh applies them), so the watcher stays out of its way.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use notify::{EventKind, RecursiveMode, Watcher};
use umr_adapter::{Event, ReloadKind};

use crate::api_server::ApiState;

/// Quiet time after the last change before reloading (editors save in bursts).
pub const DEBOUNCE: Duration = Duration::from_millis(300);

/// A running watcher; dropping it stops it.
pub struct Watch {
    _watcher: notify::RecommendedWatcher,
    stop: Arc<AtomicBool>,
}

impl std::fmt::Debug for Watch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Watch")
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// `C:\p\lib\main.dart` → `lib/main.dart`.
#[must_use]
pub fn relative(root: &Path, p: &Path) -> Option<String> {
    let rel = p.strip_prefix(root).ok()?;
    let s = rel.to_string_lossy().replace('\\', "/");
    (!s.is_empty()).then_some(s)
}

/// The reload a batch of saved files needs (`None`: nothing to do).
#[must_use]
pub fn decide(adapter: &dyn umr_adapter::Adapter, rels: &BTreeSet<String>) -> Option<ReloadKind> {
    rels.iter()
        .filter(|r| !umr_adapter::ignored(r))
        .fold(None, |k, r| {
            umr_adapter::stronger(k, adapter.change_kind(r))
        })
}

fn kind_name(k: ReloadKind) -> &'static str {
    match k {
        ReloadKind::Hot => "hot",
        ReloadKind::Restart => "restart",
        ReloadKind::Full => "full",
    }
}

/// Starts watching `root` for the adapter `adapter_id`.
pub fn start(state: &ApiState, root: &Path, adapter_id: &str) -> Result<Watch, String> {
    let weak: Weak<ApiState> = state
        .self_ref
        .get()
        .cloned()
        .ok_or("the Core state is not shared yet")?;
    let (tx, rx) = channel::<PathBuf>();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res {
            if matches!(
                ev.kind,
                EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
            ) {
                for p in ev.paths {
                    let _ = tx.send(p);
                }
            }
        }
    })
    .map_err(|e| format!("cannot watch files: {e}"))?;
    watcher
        .watch(root, RecursiveMode::Recursive)
        .map_err(|e| format!("cannot watch {}: {e}", root.display()))?;
    let stop = Arc::new(AtomicBool::new(false));
    let (stop2, root2, id) = (stop.clone(), root.to_path_buf(), adapter_id.to_string());
    std::thread::Builder::new()
        .name("umr-watch".into())
        .spawn(move || {
            let adapters = crate::run_api::adapters();
            let Some(adapter) = adapters.iter().find(|a| a.id() == id) else {
                return;
            };
            let mut pending = BTreeSet::new();
            let mut last_change = Instant::now();
            loop {
                if stop2.load(Ordering::SeqCst) {
                    return;
                }
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(p) => {
                        if let Some(r) = relative(&root2, &p) {
                            if !umr_adapter::ignored(&r) {
                                pending.insert(r);
                                last_change = Instant::now();
                            }
                        }
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                    Err(RecvTimeoutError::Timeout) => {}
                }
                if pending.is_empty() || last_change.elapsed() < DEBOUNCE {
                    continue;
                }
                let files = std::mem::take(&mut pending);
                let Some(state) = weak.upgrade() else { return };
                let Some(kind) = decide(adapter.as_ref(), &files) else {
                    continue;
                };
                let list: Vec<&str> = files.iter().map(String::as_str).take(5).collect();
                state
                    .run_events
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(&Event::Log {
                        line: format!("saved {} → {} reload", list.join(", "), kind_name(kind)),
                    });
                tracing::info!(files = ?list, kind = kind_name(kind), "file change");
                match crate::run_api::reload_kind(&state, kind) {
                    Ok(v) => tracing::info!(status = %v["status"], ms = %v["ms"], "watch reload"),
                    Err(e) => tracing::warn!(error = %e.message, "watch reload"),
                }
                // changes made while reloading are handled in the next round
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(Watch {
        _watcher: watcher,
        stop,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decides_the_strongest_reload_and_skips_generated_files() {
        let flutter = umr_adapter_flutter::Flutter;
        let set = |v: &[&str]| v.iter().map(|s| (*s).to_string()).collect::<BTreeSet<_>>();
        assert_eq!(
            decide(&flutter, &set(&["lib/main.dart"])),
            Some(ReloadKind::Hot)
        );
        assert_eq!(
            decide(&flutter, &set(&["lib/main.dart", "pubspec.yaml"])),
            Some(ReloadKind::Full)
        );
        assert_eq!(
            decide(&flutter, &set(&["build/app/x.dex", ".dart_tool/a"])),
            None
        );
        let rn = umr_adapter_react_native::ReactNative;
        assert_eq!(
            decide(&rn, &set(&["App.js"])),
            None,
            "Metro does Fast Refresh"
        );
        assert_eq!(
            relative(Path::new(r"C:\p"), Path::new(r"C:\p\lib\main.dart")).as_deref(),
            Some("lib/main.dart")
        );
        assert_eq!(relative(Path::new(r"C:\p"), Path::new(r"D:\x")), None);
    }
}
