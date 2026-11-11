//! Installer (Phase 1, step 1.8).
//!
//! | Module | Task |
//! |---|---|
//! | [`http`] | INS-001 resumable downloads (HTTP range), retries, proxy, official-host allowlist |
//! | [`verify`] | INS-002 SHA-256 / SHA-1 verification before use; safe zip extraction (no zip-slip, no links) |
//! | [`txn`] | INS-003 transactional install: download → verify → extract to staging → check → commit, crash-safe journal, rollback, recovery |
//! | [`licence`] | INS-004 licence texts and the acceptance gate (no install without recorded acceptance of the exact text) |
//! | [`catalog`] | Official metadata: Adoptium (JDK), nodejs.org, Flutter releases, Google SDK repository |
//! | [`engine`] | Turns an install plan into steps and runs them as a job with progress and cancel |
//!
//! Rules (master build §5, §11): only official HTTPS hosts; nothing is executed or extracted
//! before its checksum matches; everything goes into the managed root; global PATH,
//! `JAVA_HOME` and `ANDROID_HOME` are never changed; Android SDK packages are installed by
//! Android CLI (`--no-metrics`, isolated environment) only after the user accepted the licence.

pub mod catalog;
pub mod engine;
pub mod error;
pub mod http;
pub mod layout;
pub mod licence;
pub mod txn;
pub mod verify;

#[cfg(test)]
pub(crate) mod testkit;

pub use error::InstallError;

/// Seconds since the Unix epoch.
#[must_use]
pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Random id (16 hex characters) for transactions and jobs.
#[must_use]
pub fn random_id() -> String {
    let mut b = [0u8; 8];
    if getrandom::fill(&mut b).is_err() {
        // Fall back to time + counter; ids only need to be unique on this machine.
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        b = (u64::try_from(now_unix()).unwrap_or(0) ^ (n << 40)).to_le_bytes();
    }
    verify::to_hex(&b)
}
