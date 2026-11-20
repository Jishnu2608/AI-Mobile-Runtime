//! The `EnvironmentManager` (Phase 1, step 1.6).
//!
//! | Module | Task |
//! |---|---|
//! | [`host`] | ENV-001 host facts, ENV-007 virtualization detection |
//! | [`jdk`] | ENV-002 JDK detection + functional check |
//! | [`android`] | ENV-003 Android SDK + package detection, adb/emulator checks |
//! | [`tools`] | ENV-004 Flutter, Node, package managers, Git, Android CLI, Android Studio |
//! | [`conflicts`] | ENV-005 conflicting environment variables |
//! | [`readiness`] / [`scan`] | ENV-006 readiness report |
//! | [`recipe`] | ENV-008 isolation of child processes, ENV-009 JVM loopback fix |
//!
//! Detection is **read-only**: nothing is installed, changed or deleted. Functional checks run
//! tools with short timeouts (`--version`-style commands only).

// Report/probe structs legitimately carry many yes/no facts; probe helpers take the std HashMap.
#![allow(clippy::struct_excessive_bools, clippy::implicit_hasher)]

pub mod android;
pub mod conflicts;
pub mod context;
pub mod host;
pub mod jdk;
pub mod leaks;
pub mod model;
pub mod pathsearch;
pub mod readiness;
pub mod recipe;
pub mod runner;
pub mod tools;

mod scan;

pub use context::ScanContext;
pub use model::EnvReport;
pub use scan::{scan, scan_system};
