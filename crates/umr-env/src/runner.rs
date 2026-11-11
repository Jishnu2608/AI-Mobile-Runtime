//! Runs a tool with a timeout and captures its output.
//!
//! The child is placed in a kill-on-close Job Object, so a tool that hangs (or starts its own
//! helpers, like Gradle daemons) is fully terminated when the timeout expires.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Captured result of a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutput {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl RunOutput {
    /// stdout followed by stderr (many tools print versions to stderr, e.g. `java -version`).
    #[must_use]
    pub fn combined(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }

    #[must_use]
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}

/// Why a run failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    Spawn(String),
    Timeout(Duration),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(e) => write!(f, "cannot start: {e}"),
            Self::Timeout(d) => write!(f, "no answer within {} s", d.as_secs()),
        }
    }
}

/// Default timeout for `--version`-style checks.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(15);

/// Runs `program args…` with extra environment variables; stdin is closed.
pub fn run(
    program: &Path,
    args: &[&str],
    env: &HashMap<String, String>,
    timeout: Duration,
) -> Result<RunOutput, RunError> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn().map_err(|e| RunError::Spawn(e.to_string()))?;
    #[cfg(windows)]
    let job = umr_platform::job::KillOnCloseJob::new().ok();
    #[cfg(windows)]
    if let Some(j) = &job {
        let _ = j.assign(&child);
    }

    let mut out_pipe = child.stdout.take();
    let mut err_pipe = child.stderr.take();
    let out_t = std::thread::spawn(move || {
        let mut s = Vec::new();
        if let Some(p) = out_pipe.as_mut() {
            let _ = p.read_to_end(&mut s);
        }
        String::from_utf8_lossy(&s).into_owned()
    });
    let err_t = std::thread::spawn(move || {
        let mut s = Vec::new();
        if let Some(p) = err_pipe.as_mut() {
            let _ = p.read_to_end(&mut s);
        }
        String::from_utf8_lossy(&s).into_owned()
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => break None,
        }
    };
    let Some(status) = status else {
        let _ = child.kill();
        #[cfg(windows)]
        drop(job); // ends any helpers the tool started
        let _ = child.wait();
        return Err(RunError::Timeout(timeout));
    };
    // Helpers that inherited the pipes may keep them open; the job ends them so reads finish.
    #[cfg(windows)]
    drop(job);
    Ok(RunOutput {
        code: status.code(),
        stdout: out_t.join().unwrap_or_default(),
        stderr: err_t.join().unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd_exe() -> std::path::PathBuf {
        std::path::PathBuf::from(
            std::env::var("COMSPEC").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".into()),
        )
    }

    #[test]
    fn captures_stdout_stderr_and_code() {
        let o = run(
            &cmd_exe(),
            &["/c", "echo out& echo err 1>&2& exit /b 3"],
            &HashMap::new(),
            CHECK_TIMEOUT,
        )
        .unwrap();
        assert_eq!(o.code, Some(3));
        assert!(o.stdout.contains("out"));
        assert!(o.stderr.contains("err"));
    }

    #[test]
    fn passes_extra_environment() {
        let env = HashMap::from([("UMR_TEST_VAR".to_string(), "hello".to_string())]);
        let o = run(
            &cmd_exe(),
            &["/c", "echo %UMR_TEST_VAR%"],
            &env,
            CHECK_TIMEOUT,
        )
        .unwrap();
        assert!(o.stdout.contains("hello"));
    }

    #[test]
    fn hanging_tool_is_killed_after_timeout() {
        let start = Instant::now();
        let r = run(
            &cmd_exe(),
            &["/c", "ping -n 30 127.0.0.1 >nul"],
            &HashMap::new(),
            Duration::from_millis(500),
        );
        assert!(matches!(r, Err(RunError::Timeout(_))), "{r:?}");
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "must not wait for the tool"
        );
    }

    #[test]
    fn missing_program_is_spawn_error() {
        let r = run(
            Path::new(r"Z:\nope\missing.exe"),
            &[],
            &HashMap::new(),
            CHECK_TIMEOUT,
        );
        assert!(matches!(r, Err(RunError::Spawn(_))));
    }
}
