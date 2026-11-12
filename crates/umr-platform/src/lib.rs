//! OS abstraction layer for the AI Mobile Runtime.
//!
//! All operating-system specific calls live here so the rest of the product stays
//! portable. The MVP supports Windows 11 x64 only; macOS/Linux implementations are
//! future work and currently do not exist.
//!
//! | Module | Purpose |
//! |---|---|
//! | [`identity`] | Current user's SID (named-pipe name and ACL, CORE-003) |
//! | [`folders`] | Known folders such as `%LOCALAPPDATA%` |
//! | [`registry`] | Read-only registry access (tool detection, OS settings) |
//! | [`os`] | Windows build / Windows 11 check, long-path setting |
//! | [`job`] | Job Objects: kill a whole process tree when the Core stops |
//! | [`process`] | Process tree inspection (child processes of a pid) |
//! | [`secure_file`] | Files only the current user can read (tokens, client registry) |
//! | [`secure_pipe`] | Named-pipe servers only the current user can open (Core API) |
//! | [`system`] | Memory, disk space, CPU architecture, virtualization / WHPX / AEHD |
//!
//! This is the only crate allowed to use `unsafe`; every unsafe block documents why it is sound.
#![allow(unsafe_code)]

#[cfg(windows)]
mod wide;

#[cfg(windows)]
pub mod elevate;
#[cfg(windows)]
pub mod folders;
#[cfg(windows)]
pub mod identity;
#[cfg(windows)]
pub mod job;
#[cfg(windows)]
pub mod os;
#[cfg(windows)]
pub mod process;
#[cfg(windows)]
pub mod registry;
#[cfg(windows)]
pub mod secure_file;
#[cfg(windows)]
pub mod secure_pipe;
#[cfg(windows)]
pub mod system;
