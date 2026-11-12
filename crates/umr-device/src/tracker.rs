//! DEV-001: device tracking. A background thread keeps a `host:track-devices-l` connection to
//! the product's adb server and updates the device list; if the connection drops (server
//! killed, adb crashed) it restarts the server and reconnects.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::adb::{parse_devices, Connection, Device};
use crate::server::AdbServer;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub port: u16,
    pub protocol: Option<u32>,
    /// Tracking connection is up.
    pub connected: bool,
    pub devices: Vec<Device>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// Device-list changes seen so far (for "what changed since" cursors).
    pub changes: u64,
    pub updated_unix: u64,
}

#[derive(Debug)]
pub struct Tracker {
    state: Arc<Mutex<Snapshot>>,
    stop: Arc<AtomicBool>,
    server: AdbServer,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Tracker {
    /// Starts the server (if needed) and the tracking thread.
    #[must_use]
    pub fn start(server: AdbServer) -> Self {
        let state = Arc::new(Mutex::new(Snapshot {
            port: server.port,
            ..Snapshot::default()
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let (st, sp, sv) = (state.clone(), stop.clone(), server.clone());
        std::thread::Builder::new()
            .name("adb-tracker".into())
            .spawn(move || run(&sv, &st, &sp))
            .ok();
        Self {
            state,
            stop,
            server,
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Waits until the first device list arrived (or the timeout passed).
    #[must_use]
    pub fn wait_ready(&self, timeout: Duration) -> Snapshot {
        let start = std::time::Instant::now();
        loop {
            let s = self.snapshot();
            if s.connected || start.elapsed() >= timeout {
                return s;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Stops tracking and the product's adb server.
    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.server.stop();
    }

    #[must_use]
    pub fn port(&self) -> u16 {
        self.server.port
    }
}

fn update(state: &Mutex<Snapshot>, f: impl FnOnce(&mut Snapshot)) {
    let mut s = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f(&mut s);
    s.updated_unix = now();
}

fn run(server: &AdbServer, state: &Mutex<Snapshot>, stop: &AtomicBool) {
    let mut backoff = Duration::from_millis(250);
    while !stop.load(Ordering::SeqCst) {
        let result = (|| -> Result<(), String> {
            let protocol = server.ensure_running()?;
            update(state, |s| s.protocol = Some(protocol));
            let mut c =
                Connection::open(server.port, Duration::from_secs(3)).map_err(|e| e.to_string())?;
            c.request("host:track-devices-l")
                .map_err(|e| e.to_string())?;
            c.wait_forever().map_err(|e| e.to_string())?;
            update(state, |s| {
                s.connected = true;
                s.last_error = None;
            });
            loop {
                let text = c.read_payload().map_err(|e| e.to_string())?;
                if stop.load(Ordering::SeqCst) {
                    return Ok(());
                }
                let devices = parse_devices(&text);
                update(state, |s| {
                    s.connected = true;
                    s.last_error = None;
                    if s.devices != devices {
                        s.devices = devices;
                        s.changes += 1;
                    }
                });
            }
        })();
        if stop.load(Ordering::SeqCst) {
            break;
        }
        update(state, |s| {
            s.connected = false;
            s.last_error = result.err();
        });
        std::thread::sleep(backoff);
        backoff = (backoff * 2).min(Duration::from_secs(5));
    }
    update(state, |s| s.connected = false);
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;

    use super::*;
    use crate::adb::fake::FakeAdb;

    #[test]
    fn tracks_changes_and_notices_a_dead_server() {
        let fake = FakeAdb::start(41, "");
        let t = Tracker::start(AdbServer::new(
            Path::new("no-adb.exe"),
            fake.port,
            HashMap::new(),
        ));
        let s = t.wait_ready(Duration::from_secs(5));
        assert!(s.connected && s.devices.is_empty(), "{s:?}");
        *fake.devices.lock().unwrap() =
            "emulator-5554\tdevice product:p model:m device:d transport_id:1\n".into();
        let start = std::time::Instant::now();
        while t.snapshot().devices.is_empty() && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let s = t.snapshot();
        assert_eq!(s.devices.len(), 1);
        assert!(s.changes >= 1);
        // the server dies; adb.exe cannot be started in the test → disconnected with an error
        fake.killed.store(true, Ordering::SeqCst);
        let start = std::time::Instant::now();
        while t.snapshot().connected && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!t.snapshot().connected);
        t.shutdown();
    }
}
