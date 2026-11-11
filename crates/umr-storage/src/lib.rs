//! Local storage of the Runtime Core (CORE-005).
//!
//! - [`db`]: one SQLite database per data root (`<root>/umr.db`, WAL mode) with ordered,
//!   transactional migrations tracked in `PRAGMA user_version`.
//! - [`components`]: records of SDK components and tools the installer manages.
//! - [`install`]: install journal (INS-003), licence acceptances (INS-004), audit log.
//! - [`config`]: the user-editable `<root>/config.toml` with safe defaults.
//!
//! Nothing here talks to the network; all data stays on the machine.

pub mod components;
pub mod config;
pub mod db;
pub mod env_scans;
pub mod install;
mod migrations;

pub use db::{Db, StorageError};
