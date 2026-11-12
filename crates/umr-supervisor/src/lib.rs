//! Process Supervisor (DEV-004, Phase 2 step 2.4).
//!
//! | Module | Task |
//! |---|---|
//! | [`ports`] | Port allocator: leases from known ranges (emulator console pairs, gRPC, Metro, ...), checked by binding, released on drop |
//! | [`process`] | Starts long-lived tools (emulator, later Gradle, Metro, Flutter) in kill-on-close Job Objects, logs their output, applies restart policies, stops whole process trees |
//!
//! No orphans: every process tree sits in a Job Object whose only handle the Supervisor holds.
//! When the Core exits or crashes, Windows closes the handle and ends the tree.

pub mod ports;
pub mod process;

pub use ports::{Lease, PortAllocator, PortRange};
pub use process::{ProcessSpec, ProcessStatus, RestartPolicy, RunState, Supervisor};
