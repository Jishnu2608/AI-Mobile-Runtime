//! Lifecycle of the Runtime Core process.
//!
//! `run` takes the single-instance lock, opens the local API pipe, reports the start, waits
//! until a shutdown is requested (OS signal or `core.shutdown` over the API), then shuts
//! down cleanly. Sessions and the process supervisor join this loop in later tasks.

use std::future::Future;
use std::sync::Arc;
use std::time::Instant;

use crate::api_server::{self, ApiState};
use crate::auth::AuthStore;
use crate::instance::{InstanceLock, LockError};
use crate::paths::CorePaths;

/// Result of a Core run.
#[derive(Debug, PartialEq, Eq)]
pub enum RunOutcome {
    /// Ran and shut down cleanly.
    Stopped,
    /// Did not start because another Core is running.
    AlreadyRunning { pid: Option<u32> },
}

/// The toolchains folder from `config.toml` (`[storage] toolchains_dir`), created owner-only
/// when it is a separate folder (a new folder at a drive root would otherwise inherit
/// "Authenticated Users: Modify", letting other accounts replace tools the product runs).
fn toolchains_folder(home: &std::path::Path) -> std::path::PathBuf {
    let config = umr_storage::config::Config::load_in(home).unwrap_or_default();
    let dir = match config.toolchains_root(home) {
        Ok(d) => d,
        Err(e) => {
            tracing::error!(error = %e, "invalid toolchains_dir; using the home folder");
            home.to_path_buf()
        }
    };
    if dir != home {
        match umr_platform::secure_file::create_private_dir(&dir) {
            Ok(true) => {
                tracing::info!(path = %dir.display(), "toolchains folder created (owner-only)");
            }
            Ok(false) => {}
            Err(e) => {
                tracing::error!(error = %e, path = %dir.display(), "cannot create the toolchains folder");
            }
        }
    }
    if let Some(pkg) = umr_platform::os::package_full_name() {
        let appdata = [
            umr_platform::folders::local_app_data(),
            umr_platform::folders::roaming_app_data(),
        ];
        if [home, dir.as_path()]
            .iter()
            .any(|p| appdata.iter().flatten().any(|a| p.starts_with(a)))
        {
            tracing::warn!(package = %pkg, "data folder is inside AppData while running in a packaged app: Windows redirects writes (CORE-007)");
        }
    }
    dir
}

/// Runs the Core until `shutdown` completes.
pub async fn run<F>(paths: &CorePaths, shutdown: F) -> Result<RunOutcome, LockError>
where
    F: Future<Output = ()>,
{
    let lock = match InstanceLock::acquire(paths) {
        Ok(lock) => lock,
        Err(LockError::AlreadyRunning { pid }) => {
            tracing::warn!(?pid, "another Runtime Core is already running; exiting");
            return Ok(RunOutcome::AlreadyRunning { pid });
        }
        Err(e) => return Err(e),
    };

    // Fresh admin token for first-party clients (CLI, desktop), readable only by this user.
    // Written before the pipe exists, so any client that can connect can also find the token.
    let admin_token = crate::auth::generate_token()?;
    crate::auth::write_secret(&paths.admin_token_file(), admin_token.as_bytes())?;
    let auth = AuthStore::load(&paths.clients_file(), &admin_token)?;
    let db = umr_storage::Db::open_in(paths.root()).map_err(|e| {
        tracing::error!(error = %e, "cannot open the database");
        let _ = std::fs::remove_file(paths.admin_token_file());
        std::io::Error::other(e.to_string())
    })?;
    tracing::info!(schema = db.schema_version().unwrap_or(0), path = %db.path().display(), "database ready");

    // Open the API before announcing the start, so a squatted or broken pipe fails early.
    let pipe_name = paths.pipe_name()?;
    let first = umr_platform::secure_pipe::create_user_only_server(&pipe_name, true).map_err(|e| {
        tracing::error!(error = %e, pipe = %pipe_name, "cannot open the API pipe (name in use by another program?)");
        let _ = std::fs::remove_file(paths.admin_token_file());
        e
    })?;
    let toolchains = toolchains_folder(paths.root());
    let state = Arc::new(ApiState::with_toolchains(
        paths.root().to_path_buf(),
        toolchains,
        auth,
        db,
    ));
    // INS-003: finish or roll back installs a crash interrupted, before any new one starts.
    match umr_installer::txn::recover(
        &umr_installer::layout::Layout::new(&state.toolchains),
        &state.db,
    ) {
        Ok(r) if r.completed.is_empty() && r.rolled_back.is_empty() => {}
        Ok(r) => {
            tracing::warn!(completed = ?r.completed, rolled_back = ?r.rolled_back, "recovered interrupted installs");
        }
        Err(e) => tracing::error!(error = %e, "install recovery failed"),
    }
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let server = tokio::spawn(api_server::serve(
        pipe_name.clone(),
        first,
        state.clone(),
        stop_rx,
    ));

    let started = Instant::now();
    tracing::info!(
        version = crate::VERSION,
        pid = std::process::id(),
        root = %paths.root().display(),
        pipe = %pipe_name,
        "Runtime Core started"
    );

    tokio::select! {
        () = shutdown => {}
        () = state.shutdown_requested.notified() => {}
    }

    tracing::info!(
        uptime_s = started.elapsed().as_secs(),
        "shutdown requested; stopping"
    );
    let _ = stop_tx.send(true);
    let _ = server.await;
    let _ = std::fs::remove_file(paths.admin_token_file());
    drop(lock);
    tracing::info!("Runtime Core stopped");
    Ok(RunOutcome::Stopped)
}

/// Completes when the process receives Ctrl+C / Ctrl+Break / console close.
pub async fn os_shutdown_signal() {
    #[cfg(windows)]
    {
        use tokio::signal::windows;
        let (Ok(mut c), Ok(mut brk), Ok(mut close)) = (
            windows::ctrl_c(),
            windows::ctrl_break(),
            windows::ctrl_close(),
        ) else {
            tracing::error!("cannot install console signal handlers; falling back to ctrl_c");
            let _ = tokio::signal::ctrl_c().await;
            return;
        };
        tokio::select! {
            _ = c.recv() => tracing::info!("Ctrl+C received"),
            _ = brk.recv() => tracing::info!("Ctrl+Break received"),
            _ = close.recv() => tracing::info!("console close received"),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("Ctrl+C received");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::read_pid;
    use crate::test_support::TempRoot;

    #[tokio::test]
    async fn run_stops_cleanly_and_removes_pid_file() {
        let tmp = TempRoot::new("run-stop");
        let paths = CorePaths::with_root(tmp.path());
        let outcome = run(&paths, async {}).await.unwrap();
        assert_eq!(outcome, RunOutcome::Stopped);
        assert_eq!(read_pid(&paths), None);
    }

    #[tokio::test]
    async fn second_run_reports_already_running() {
        let tmp = TempRoot::new("run-twice");
        let paths = CorePaths::with_root(tmp.path());
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let p1 = paths.clone();
        let first = tokio::spawn(async move {
            run(&p1, async {
                let _ = rx.await;
            })
            .await
        });
        // wait until the first instance holds the lock
        for _ in 0..100 {
            if read_pid(&paths).is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let second = run(&paths, async {}).await.unwrap();
        assert_eq!(
            second,
            RunOutcome::AlreadyRunning {
                pid: Some(std::process::id())
            }
        );
        tx.send(()).unwrap();
        assert_eq!(first.await.unwrap().unwrap(), RunOutcome::Stopped);
    }
}
