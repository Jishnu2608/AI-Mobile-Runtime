//! DEV-007: the product's own adb server on a private port.
//!
//! The port is chosen once per Core start: the configured port (default [`DEFAULT_PORT`]) if it
//! is free or already runs a compatible adb server (ours from an earlier start), otherwise the
//! next such port. Port 5037 is never used. The server is started with `adb -P <port>
//! start-server` in the isolated environment (adb keys go to the managed Android user home) and
//! stopped with `host:kill` on that port only.

use std::collections::HashMap;
use std::net::{Ipv4Addr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::adb::{self, AdbError};
use crate::PUBLIC_ADB_PORT;

/// Default private port (5037 + 100).
pub const DEFAULT_PORT: u16 = 5137;
/// adb protocol version of current platform-tools.
pub const PROTOCOL: u32 = 41;

/// What listens on a port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    Free,
    Adb(u32),
    /// Some other program (or an adb we cannot talk to).
    Other,
}

#[must_use]
pub fn probe(port: u16) -> Probe {
    // Binding is instant; a connect to a closed port takes ~2 s on Windows.
    if TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_ok() {
        return Probe::Free;
    }
    match adb::server_version(port) {
        Ok(v) => Probe::Adb(v),
        Err(_) => Probe::Other,
    }
}

/// First usable private port from `preferred` (up to 64 tries): free, or a compatible adb.
pub fn choose_port(preferred: u16) -> Result<u16, String> {
    let start = if preferred == PUBLIC_ADB_PORT || preferred == 0 {
        DEFAULT_PORT
    } else {
        preferred
    };
    for port in start..start.saturating_add(64) {
        if port == PUBLIC_ADB_PORT {
            continue;
        }
        match probe(port) {
            Probe::Free => return Ok(port),
            Probe::Adb(v) if v == PROTOCOL => return Ok(port),
            _ => {}
        }
    }
    Err(format!(
        "no free port for the adb server in {start}..{}",
        start.saturating_add(64)
    ))
}

/// The product's adb server.
#[derive(Debug, Clone)]
pub struct AdbServer {
    pub adb_exe: PathBuf,
    pub port: u16,
    /// Isolated child environment (umr-env recipe, incl. `ANDROID_ADB_SERVER_PORT`).
    pub env: HashMap<String, String>,
}

impl AdbServer {
    #[must_use]
    pub fn new(adb_exe: &Path, port: u16, env: HashMap<String, String>) -> Self {
        Self {
            adb_exe: adb_exe.to_path_buf(),
            port,
            env,
        }
    }

    /// Starts the server if it is not running yet; returns its protocol version.
    pub fn ensure_running(&self) -> Result<u32, String> {
        if self.port == PUBLIC_ADB_PORT {
            return Err("the product never runs its adb server on port 5037".into());
        }
        match probe(self.port) {
            Probe::Adb(v) => return Ok(v),
            Probe::Other => return Err(format!("port {} is used by another program", self.port)),
            Probe::Free => {}
        }
        if !self.adb_exe.is_file() {
            return Err(format!("adb not found at {}", self.adb_exe.display()));
        }
        // `start-server` forks a background server and returns; its output is not needed and an
        // inherited pipe would keep the caller waiting for the long-lived server.
        let status = Command::new(&self.adb_exe)
            .args(["-P", &self.port.to_string(), "start-server"])
            .envs(&self.env)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| format!("cannot start adb: {e}"))?;
        if !status.success() {
            return Err(format!("adb start-server failed ({status})"));
        }
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            if let Ok(v) = adb::server_version(self.port) {
                return Ok(v);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Err("adb server did not answer within 10 s".into())
    }

    /// Stops this server (never the one on port 5037).
    pub fn stop(&self) -> Result<(), String> {
        match adb::kill_server(self.port) {
            Ok(()) | Err(AdbError::NotRunning) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adb::fake::FakeAdb;

    #[test]
    fn probing_and_port_choice() {
        let fake = FakeAdb::start(PROTOCOL, "");
        assert_eq!(probe(fake.port), Probe::Adb(PROTOCOL));
        assert_eq!(
            choose_port(fake.port).unwrap(),
            fake.port,
            "reuses a compatible adb (ours from before)"
        );

        let old = FakeAdb::start(39, "");
        assert_eq!(probe(old.port), Probe::Adb(39));
        assert_ne!(
            choose_port(old.port).unwrap(),
            old.port,
            "skips an incompatible adb"
        );

        let busy = TcpListener::bind("127.0.0.1:0").unwrap();
        let p = busy.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for s in busy.incoming().flatten() {
                drop(s);
            }
        });
        assert_eq!(probe(p), Probe::Other);
        assert_ne!(choose_port(p).unwrap(), p);
        assert_ne!(choose_port(PUBLIC_ADB_PORT).unwrap(), PUBLIC_ADB_PORT);
    }

    #[test]
    fn running_server_is_reused_and_stopped_only_on_its_port() {
        let fake = FakeAdb::start(PROTOCOL, "");
        let s = AdbServer::new(Path::new("does-not-exist.exe"), fake.port, HashMap::new());
        assert_eq!(s.ensure_running().unwrap(), PROTOCOL, "no start needed");
        s.stop().unwrap();
        assert!(fake.wait_killed());
        let public = AdbServer::new(Path::new("adb.exe"), PUBLIC_ADB_PORT, HashMap::new());
        assert!(public.ensure_running().is_err());
        assert!(public.stop().is_err());
    }
}
