//! Windows Job Objects: a process group that is killed as a whole.
//!
//! The Supervisor (DEV-004) puts every tool it starts (emulator, Gradle, Metro, Flutter)
//! into a job with "kill on close". When the job is dropped — or the Core crashes and the
//! OS closes the handle — the whole process tree ends, so no orphans are left behind.
//! Processes started by a member are automatically members too.

use std::io;
use std::os::windows::io::AsRawHandle;
use std::process::Child;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

/// A Job Object that terminates all member processes when dropped.
#[derive(Debug)]
pub struct KillOnCloseJob {
    handle: HANDLE,
}

// SAFETY: a job handle may be used from any thread; the Windows job APIs are thread-safe.
unsafe impl Send for KillOnCloseJob {}
// SAFETY: see above; methods take &self and only pass the handle to thread-safe APIs.
unsafe impl Sync for KillOnCloseJob {}

impl KillOnCloseJob {
    /// Creates an anonymous job with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
    pub fn new() -> io::Result<Self> {
        // SAFETY: null attributes and name create an anonymous job; result checked below.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = Self { handle };
        // SAFETY: plain-data struct; zero is valid before setting the flag.
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let size = u32::try_from(std::mem::size_of_val(&info)).unwrap_or(0);
        // SAFETY: `info` is the struct type that matches JobObjectExtendedLimitInformation and `size` is its size.
        let ok = unsafe {
            SetInformationJobObject(
                job.handle,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                size,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }

    /// Adds a spawned child process (and its future descendants) to the job.
    pub fn assign(&self, child: &Child) -> io::Result<()> {
        let process: HANDLE = child.as_raw_handle().cast();
        // SAFETY: both handles are valid for the duration of this call.
        if unsafe { AssignProcessToJobObject(self.handle, process) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Terminates every process in the job immediately.
    pub fn terminate(&self, exit_code: u32) -> io::Result<()> {
        // SAFETY: valid job handle.
        if unsafe { TerminateJobObject(self.handle, exit_code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for KillOnCloseJob {
    fn drop(&mut self) {
        // SAFETY: closes our only handle; with KILL_ON_JOB_CLOSE this ends all members.
        unsafe { CloseHandle(self.handle) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::{descendants, is_running};
    use std::process::Command;
    use std::time::{Duration, Instant};

    fn wait_until(mut cond: impl FnMut() -> bool) -> bool {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            if cond() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    /// cmd.exe starts a ping grandchild; dropping the job must end both.
    #[test]
    fn dropping_job_kills_child_and_grandchild() {
        let job = KillOnCloseJob::new().unwrap();
        let mut child = Command::new("cmd.exe")
            .args(["/c", "ping -n 60 127.0.0.1 >nul"])
            .spawn()
            .unwrap();
        job.assign(&child).unwrap();

        let mut grandchild = None;
        assert!(
            wait_until(|| {
                grandchild = descendants(child.id())
                    .unwrap()
                    .into_iter()
                    .find(|p| p.exe_name.eq_ignore_ascii_case("PING.EXE"));
                grandchild.is_some()
            }),
            "ping grandchild did not start"
        );
        let grandchild = grandchild.unwrap();

        drop(job);

        assert!(
            wait_until(|| child.try_wait().unwrap().is_some()),
            "child still running"
        );
        assert!(
            wait_until(|| !is_running(grandchild.pid).unwrap()),
            "grandchild still running"
        );
    }

    #[test]
    fn terminate_ends_members_with_exit_code() {
        let job = KillOnCloseJob::new().unwrap();
        let mut child = Command::new("cmd.exe")
            .args(["/c", "ping -n 60 127.0.0.1 >nul"])
            .spawn()
            .unwrap();
        job.assign(&child).unwrap();
        job.terminate(7).unwrap();
        let status = child.wait().unwrap();
        assert_eq!(status.code(), Some(7));
    }
}
