//! Finding, starting and connecting to the Runtime Core (shared by the CLI and the desktop app).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::paths::CorePaths;
use umr_api::client::{Client, ClientError};

/// Environment variable that overrides the path of `umr-core.exe` (tests, custom installs).
pub const CORE_BIN_ENV: &str = "UMR_CORE_BIN";

/// How long `start` waits for a freshly started Core to answer.
pub const START_TIMEOUT: Duration = Duration::from_secs(15);

/// Everything a client needs to reach the Core.
#[derive(Debug, Clone)]
pub struct CoreTarget {
    pub paths: CorePaths,
    pub pipe: String,
}

impl CoreTarget {
    pub fn resolve() -> Result<Self, String> {
        let paths =
            CorePaths::resolve().map_err(|e| format!("cannot find the data folder: {e}"))?;
        let pipe = paths
            .pipe_name()
            .map_err(|e| format!("cannot compute the API pipe name: {e}"))?;
        Ok(Self { paths, pipe })
    }

    fn admin_token(&self) -> Result<String, String> {
        std::fs::read_to_string(self.paths.admin_token_file())
            .map(|t| t.trim().to_string())
            .map_err(|e| {
                format!(
                    "cannot read the Core's admin token ({}): {e}",
                    self.paths.admin_token_file().display()
                )
            })
    }

    /// Connects and logs in, or reports that the Core is not running.
    pub async fn connect(&self) -> Result<Option<Client>, String> {
        let mut client = match Client::connect(&self.pipe).await {
            Ok(c) => c,
            Err(ClientError::NotRunning) => return Ok(None),
            Err(e) => return Err(e.to_string()),
        };
        let token = self.admin_token()?;
        client
            .login(&token)
            .await
            .map_err(|e| format!("login to the Runtime Core failed: {e}"))?;
        Ok(Some(client))
    }

    /// Connects, starting the Core first if needed. Returns the client and whether it was started.
    pub async fn connect_or_start(&self) -> Result<(Client, bool), String> {
        if let Some(c) = self.connect().await? {
            return Ok((c, false));
        }
        spawn_core(&self.paths)?;
        let deadline = Instant::now() + START_TIMEOUT;
        loop {
            // The admin token is written just before the pipe opens; retry briefly on either.
            if let Ok(Some(c)) = self.connect().await {
                return Ok((c, true));
            }
            if Instant::now() > deadline {
                return Err(format!(
                    "the Runtime Core did not start within {} s; see logs in {}",
                    START_TIMEOUT.as_secs(),
                    self.paths.logs_dir().display()
                ));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Waits until the Core no longer answers.
    pub async fn wait_until_stopped(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if matches!(
                Client::connect(&self.pipe).await,
                Err(ClientError::NotRunning)
            ) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        false
    }
}

/// Path of `umr-core.exe`: `UMR_CORE_BIN`, else next to `umr.exe`.
pub fn core_binary() -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os(CORE_BIN_ENV).filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(p));
    }
    let exe = std::env::current_exe().map_err(|e| format!("cannot locate umr.exe: {e}"))?;
    let dir = exe.parent().unwrap_or(Path::new("."));
    let core = dir.join(if cfg!(windows) {
        "umr-core.exe"
    } else {
        "umr-core"
    });
    if core.is_file() {
        Ok(core)
    } else {
        Err(format!(
            "umr-core was not found next to umr ({}); set {CORE_BIN_ENV}",
            core.display()
        ))
    }
}

/// Starts `umr-core run` in the background: no console window, not tied to this terminal.
fn spawn_core(paths: &CorePaths) -> Result<(), String> {
    let bin = core_binary()?;
    // Otherwise the Core inherits our stdout/stderr pipes and callers capturing `umr` output hang.
    #[cfg(windows)]
    umr_platform::process::disinherit_std_handles()
        .map_err(|e| format!("cannot prepare background start: {e}"))?;
    let mut cmd = Command::new(&bin);
    cmd.arg("run")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if paths.is_custom_root() {
        cmd.env(crate::paths::HOME_ENV, paths.root());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        // Breaking away from the terminal's job keeps the Core alive when the terminal closes.
        // Some environments forbid breakaway; fall back to a plain detached start.
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB);
        if cmd.spawn().is_ok() {
            return Ok(());
        }
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("cannot start {}: {e}", bin.display()))
}
