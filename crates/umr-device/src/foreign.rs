//! DEV-007: adb servers that are not the product's (Android Studio, the user's own `adb`).
//! They are only reported, never stopped or restarted.

use serde::{Deserialize, Serialize};

use crate::server::{probe, Probe};
use crate::PUBLIC_ADB_PORT;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForeignServer {
    pub port: u16,
    pub protocol: u32,
    pub devices: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Coexistence {
    /// adb servers on the public port (and on the port the user set in their own
    /// `ANDROID_ADB_SERVER_PORT`), excluding ours.
    pub servers: Vec<ForeignServer>,
    /// Number of `adb.exe` processes on the machine (ours included).
    pub adb_processes: usize,
}

/// Looks for adb servers other than the one on `our_port`.
#[must_use]
pub fn scan(our_port: u16, user_port: Option<u16>) -> Coexistence {
    let mut ports = vec![PUBLIC_ADB_PORT];
    if let Some(p) = user_port {
        if !ports.contains(&p) {
            ports.push(p);
        }
    }
    let servers = ports
        .into_iter()
        .filter(|p| *p != our_port)
        .filter_map(|port| match probe(port) {
            Probe::Adb(protocol) => Some(ForeignServer {
                port,
                protocol,
                devices: crate::adb::devices(port).map_or(0, |d| d.len()),
            }),
            _ => None,
        })
        .collect();
    let adb_processes = umr_platform::process::list_processes().map_or(0, |l| {
        l.iter()
            .filter(|p| p.exe_name.eq_ignore_ascii_case("adb.exe"))
            .count()
    });
    Coexistence {
        servers,
        adb_processes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adb::fake::FakeAdb;

    #[test]
    fn reports_other_servers_but_not_ours() {
        let theirs = FakeAdb::start(41, "emulator-5554\tdevice\n");
        let ours = FakeAdb::start(41, "");
        let c = scan(ours.port, Some(theirs.port));
        let found: Vec<u16> = c.servers.iter().map(|s| s.port).collect();
        assert!(found.contains(&theirs.port));
        assert!(!found.contains(&ours.port));
        assert_eq!(
            c.servers
                .iter()
                .find(|s| s.port == theirs.port)
                .unwrap()
                .devices,
            1
        );
        assert!(
            !theirs.killed.load(std::sync::atomic::Ordering::SeqCst),
            "left alone"
        );
    }
}
