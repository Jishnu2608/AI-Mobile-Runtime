//! Single-instance guarantee: at most one Runtime Core per user (per managed root).
//!
//! An exclusive OS file lock is held on `run/core.lock` for the whole life of the Core.
//! The lock is released automatically by the OS if the process dies, so a crash never
//! leaves a stale lock behind. The process id is written to a separate `run/core.pid`
//! file because a locked file cannot be read by other processes on Windows.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;

use crate::paths::CorePaths;

/// Why the instance lock could not be taken.
#[derive(Debug)]
pub enum LockError {
    /// Another Core already holds the lock. Contains its pid if known.
    AlreadyRunning { pid: Option<u32> },
    /// File system error while creating or locking the file.
    Io(io::Error),
}

impl std::fmt::Display for LockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyRunning { pid: Some(pid) } => {
                write!(f, "another Runtime Core is already running (pid {pid})")
            }
            Self::AlreadyRunning { pid: None } => {
                write!(f, "another Runtime Core is already running")
            }
            Self::Io(e) => write!(f, "cannot take the instance lock: {e}"),
        }
    }
}

impl std::error::Error for LockError {}

impl From<io::Error> for LockError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Held while this process is the active Runtime Core. Dropping it releases the lock
/// and removes the pid file.
#[derive(Debug)]
pub struct InstanceLock {
    file: File,
    paths: CorePaths,
}

impl InstanceLock {
    /// Takes the exclusive lock, or reports that another Core is running.
    pub fn acquire(paths: &CorePaths) -> Result<Self, LockError> {
        paths.ensure_dirs()?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(paths.lock_file())?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(LockError::AlreadyRunning {
                    pid: read_pid(paths),
                });
            }
            Err(TryLockError::Error(e)) => return Err(LockError::Io(e)),
        }
        std::fs::write(paths.pid_file(), std::process::id().to_string())?;
        Ok(Self {
            file,
            paths: paths.clone(),
        })
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        // Remove the pid file first, while we still own the lock.
        let _ = std::fs::remove_file(self.paths.pid_file());
        let _ = self.file.unlock();
    }
}

/// Reads the pid recorded in `run/core.pid`, if any.
///
/// Only trustworthy while the lock is held by another process: after a crash the file
/// stays behind with the dead process's pid until the next Core overwrites it.
#[must_use]
pub fn read_pid(paths: &CorePaths) -> Option<u32> {
    std::fs::read_to_string(paths.pid_file())
        .ok()?
        .trim()
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempRoot;

    #[test]
    fn second_lock_is_refused_while_first_is_held() {
        let tmp = TempRoot::new("lock-twice");
        let paths = CorePaths::with_root(tmp.path());
        let first = InstanceLock::acquire(&paths).expect("first lock");
        match InstanceLock::acquire(&paths) {
            Err(LockError::AlreadyRunning { pid }) => assert_eq!(pid, Some(std::process::id())),
            other => panic!("expected AlreadyRunning, got {other:?}"),
        }
        drop(first);
    }

    #[test]
    fn lock_can_be_taken_again_after_release() {
        let tmp = TempRoot::new("lock-again");
        let paths = CorePaths::with_root(tmp.path());
        drop(InstanceLock::acquire(&paths).expect("first lock"));
        let again = InstanceLock::acquire(&paths).expect("lock after release");
        drop(again);
    }

    #[test]
    fn pid_file_exists_only_while_locked() {
        let tmp = TempRoot::new("pid-file");
        let paths = CorePaths::with_root(tmp.path());
        let lock = InstanceLock::acquire(&paths).unwrap();
        assert_eq!(read_pid(&paths), Some(std::process::id()));
        drop(lock);
        assert_eq!(read_pid(&paths), None);
    }
}
