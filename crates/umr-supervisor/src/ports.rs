//! Port allocator.
//!
//! Tools need fixed-looking ports (emulator console 5554.., gRPC 8554.., Metro 8081..). Two
//! sessions, or a user's own emulator / Metro, must never collide. A port is handed out only
//! when it is not leased already and can be bound on localhost right now; the lease is returned
//! when it is dropped.

use std::collections::HashMap;
use std::net::{Ipv4Addr, TcpListener};
use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;

/// A range the allocator searches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortRange {
    pub first: u16,
    pub last: u16,
    /// Distance between candidates (2 for emulator console/adb pairs).
    pub step: u16,
    /// The port after each candidate must be free too (emulator: console + adb).
    pub pair: bool,
}

impl PortRange {
    /// Emulator console ports (`emulator-5554` …); each uses the next port for adb.
    pub const EMULATOR: Self = Self {
        first: 5554,
        last: 5682,
        step: 2,
        pair: true,
    };
    /// Emulator gRPC endpoints.
    pub const GRPC: Self = Self {
        first: 8554,
        last: 8653,
        step: 1,
        pair: false,
    };
    /// Metro / Expo dev servers (Phase 3).
    pub const METRO: Self = Self {
        first: 8081,
        last: 8180,
        step: 1,
        pair: false,
    };
}

/// True when `port` can be bound on 127.0.0.1 right now.
#[must_use]
pub fn bindable(port: u16) -> bool {
    TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_ok()
}

#[derive(Debug, Default)]
struct Inner {
    /// port → owner
    leased: Mutex<HashMap<u16, String>>,
}

/// Hands out port leases. Cloning shares the same table.
#[derive(Debug, Clone, Default)]
pub struct PortAllocator {
    inner: Arc<Inner>,
}

/// A leased port; returned to the allocator when dropped.
#[derive(Debug)]
pub struct Lease {
    pub port: u16,
    inner: Arc<Inner>,
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.inner
            .leased
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.port);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LeaseInfo {
    pub port: u16,
    pub owner: String,
}

impl PortAllocator {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Leases the first usable port of `range` for `owner`. `skip` lists ports known to be in
    /// use by others even if they look free (e.g. an emulator serial adb already reports).
    pub fn lease(&self, owner: &str, range: PortRange, skip: &[u16]) -> Result<Lease, String> {
        let mut leased = self
            .inner
            .leased
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut p = range.first;
        while p <= range.last {
            let free = |q: u16| !leased.contains_key(&q) && !skip.contains(&q) && bindable(q);
            if free(p) && (!range.pair || p.checked_add(1).is_some_and(free)) {
                leased.insert(p, owner.to_string());
                return Ok(Lease {
                    port: p,
                    inner: self.inner.clone(),
                });
            }
            match p.checked_add(range.step.max(1)) {
                Some(n) => p = n,
                None => break,
            }
        }
        Err(format!(
            "no free port for {owner} in {}..{}",
            range.first, range.last
        ))
    }

    /// Current leases (for diagnostics).
    #[must_use]
    pub fn leases(&self) -> Vec<LeaseInfo> {
        let mut v: Vec<LeaseInfo> = self
            .inner
            .leased
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|(p, o)| LeaseInfo {
                port: *p,
                owner: o.clone(),
            })
            .collect();
        v.sort_by_key(|l| l.port);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A private test range that starts at a port the OS just gave us.
    fn range(len: u16, step: u16, pair: bool) -> PortRange {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let first = l.local_addr().unwrap().port();
        drop(l);
        PortRange {
            first,
            last: first.saturating_add(len),
            step,
            pair,
        }
    }

    #[test]
    fn leases_are_unique_and_released_on_drop() {
        let a = PortAllocator::new();
        let r = range(40, 1, false);
        let l1 = a.lease("one", r, &[]).unwrap();
        let l2 = a.lease("two", r, &[]).unwrap();
        assert_ne!(l1.port, l2.port);
        assert_eq!(a.leases().len(), 2);
        let p1 = l1.port;
        drop(l1);
        assert_eq!(a.leases().len(), 1);
        let l3 = a.lease("three", r, &[]).unwrap();
        assert_eq!(l3.port, p1, "a returned port is handed out again");
    }

    #[test]
    fn busy_and_skipped_ports_are_not_handed_out() {
        let a = PortAllocator::new();
        let r = range(40, 2, true);
        // someone else listens on the first candidate's partner port
        let busy = TcpListener::bind((Ipv4Addr::LOCALHOST, r.first + 1));
        let l = a.lease("emu", r, &[r.first + 2]).unwrap();
        if busy.is_ok() {
            assert_ne!(l.port, r.first, "pair partner is busy");
        }
        assert_ne!(l.port, r.first + 2, "skipped");
        assert_eq!((l.port - r.first) % 2, 0, "even steps from the start");
    }

    #[test]
    fn exhausted_range_is_an_error() {
        let a = PortAllocator::new();
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let p = l.local_addr().unwrap().port();
        let r = PortRange {
            first: p,
            last: p,
            step: 1,
            pair: false,
        };
        assert!(a.lease("x", r, &[]).unwrap_err().contains("no free port"));
    }

    #[test]
    fn known_ranges() {
        assert_eq!(PortRange::EMULATOR.first, 5554);
        const { assert!(PortRange::EMULATOR.pair) };
        assert_eq!(PortRange::GRPC.first, 8554);
    }
}
