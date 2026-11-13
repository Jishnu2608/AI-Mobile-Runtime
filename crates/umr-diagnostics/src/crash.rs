//! Android crash buffer (`logcat -b crash`) and ANR extraction (DIA-001, DIA-006).
//!
//! Nothing else reads this buffer: a native crash, or a Java/Kotlin exception the app's own
//! process cannot catch, never reaches the tool logs the other adapters already parse
//! (Gradle/Flutter/Metro only see what their own process writes). This is the one place those
//! crashes show up.
//!
//! Two block shapes, both grouped from raw `logcat -d -b crash -v threadtime` (or the default
//! format, which this parser also accepts) text:
//!
//! ```text
//! 11-13 20:25:15.994  1835  1835 E AndroidRuntime: FATAL EXCEPTION: main
//! 11-13 20:25:15.994  1835  1835 E AndroidRuntime: Process: com.umr.flutter_demo, PID: 1835
//! 11-13 20:25:15.994  1835  1835 E AndroidRuntime: java.lang.RuntimeException: boom
//! 11-13 20:25:15.994  1835  1835 E AndroidRuntime:     at com.umr.flutter_demo.MainActivity.onCreate(MainActivity.kt:10)
//! ```
//! ```text
//! 11-13 20:25:15.994  5678  5678 F DEBUG   : *** *** *** *** *** *** *** *** *** *** *** ***
//! 11-13 20:25:15.994  5678  5678 F DEBUG   : pid: 5678, tid: 5678, name: app  >>> com.x <<<
//! 11-13 20:25:15.994  5678  5678 F DEBUG   : signal 11 (SIGSEGV), code 1 (SEGV_MAPERR), fault addr 0x0
//! 11-13 20:25:15.994  5678  5678 F DEBUG   : backtrace:
//! 11-13 20:25:15.994  5678  5678 F DEBUG   :       #00 pc 00001234  /system/lib64/libc.so
//! ```

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CrashKind {
    /// Uncaught Java/Kotlin exception (`AndroidRuntime: FATAL EXCEPTION`).
    Fatal,
    /// Native signal, read from a tombstone header in the crash buffer (`DEBUG` tag).
    Native,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CrashEvent {
    pub kind: CrashKind,
    pub package: Option<String>,
    pub pid: Option<u32>,
    /// The exception's type and message (`Fatal`), or the signal line (`Native`).
    pub message: String,
    /// Stack frames in order, outermost call first, as `logcat` printed them.
    pub stack: Vec<String>,
    /// File and line of the first stack frame that names one (`Fatal` only — native frames
    /// carry no source location without symbols).
    pub file: Option<String>,
    pub line: Option<u32>,
    /// Device time of the block's first line when read with `-v epoch` (milliseconds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub epoch_ms: Option<u64>,
}

/// Leading `sssss.mmm` of a `logcat -v epoch` line, in milliseconds.
#[must_use]
pub fn epoch_of(line: &str) -> Option<u64> {
    let t = line.split_whitespace().next()?;
    let (s, ms) = t.split_once('.')?;
    if s.is_empty()
        || !s.bytes().all(|b| b.is_ascii_digit())
        || !ms.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let frac: String = format!("{ms:0<3}").chars().take(3).collect();
    Some(s.parse::<u64>().ok()? * 1000 + frac.parse::<u64>().ok()?)
}

/// One `logcat` line's message (after `LEVEL TAG: `), or the whole line if it does not match
/// that shape (so free-form input in tests still works).
fn message_of(line: &str) -> &str {
    // `MM-DD HH:MM:SS.mmm  PID  TID L TAG    : msg` — take after the first " L ...: ".
    let Some(colon) = line.find(": ") else {
        return line.trim();
    };
    let head = &line[..colon];
    let tag_ok = head
        .trim_end()
        .rsplit(char::is_whitespace)
        .next()
        .is_some_and(|t| !t.is_empty());
    if tag_ok {
        line[colon + 2..].trim_end()
    } else {
        line.trim()
    }
}

/// First `(File.ext:NN)` in a stack frame line.
fn frame_location(line: &str) -> Option<(String, u32)> {
    let open = line.rfind('(')?;
    let close = line[open..].find(')')? + open;
    let inner = &line[open + 1..close];
    let (file, lineno) = inner.rsplit_once(':')?;
    if !file.contains('.') || !lineno.chars().all(|c| c.is_ascii_digit()) || lineno.is_empty() {
        return None;
    }
    Some((file.to_string(), lineno.parse().ok()?))
}

fn is_stack_frame(msg: &str) -> bool {
    msg.trim_start().starts_with("at ")
}

/// Parses one `FATAL EXCEPTION` block (lines already narrowed to this crash).
fn parse_fatal(lines: &[&str]) -> CrashEvent {
    let mut package = None;
    let mut pid = None;
    let mut message = String::new();
    let mut stack = Vec::new();
    for raw in lines {
        let msg = message_of(raw);
        if let Some(rest) = msg.strip_prefix("Process: ") {
            let (p, id) = rest.split_once(", PID: ").unwrap_or((rest, ""));
            package = Some(p.to_string());
            pid = id.trim().parse().ok();
        } else if is_stack_frame(msg) {
            stack.push(msg.trim_start().to_string());
        } else if msg.starts_with("FATAL EXCEPTION") {
            // the header line names the thread, not the exception; skip it
        } else if !msg.trim().is_empty() && message.is_empty() {
            message = msg.trim().to_string();
        }
    }
    let (file, line) = stack
        .iter()
        .find_map(|f| frame_location(f))
        .map_or((None, None), |(f, l)| (Some(f), Some(l)));
    CrashEvent {
        kind: CrashKind::Fatal,
        package,
        pid,
        message,
        stack,
        file,
        line,
        epoch_ms: lines.first().and_then(|l| epoch_of(l)),
    }
}

/// Parses one native tombstone block (the `DEBUG` tag's `*** ***` header onward).
fn parse_native(lines: &[&str]) -> CrashEvent {
    let mut package = None;
    let mut pid = None;
    let mut message = String::new();
    let mut stack = Vec::new();
    for raw in lines {
        let msg = message_of(raw);
        let t = msg.trim();
        if let Some(rest) = t.strip_prefix("pid: ") {
            pid = rest.split(',').next().and_then(|s| s.trim().parse().ok());
            if let Some(open) = t.find(">>> ") {
                if let Some(close) = t[open..].find(" <<<") {
                    package = Some(t[open + 4..open + close].to_string());
                }
            }
        } else if t.starts_with("signal ") {
            message = t.to_string();
        } else if t.starts_with('#') && t.contains(" pc ") {
            stack.push(t.to_string());
        }
    }
    CrashEvent {
        kind: CrashKind::Native,
        package,
        pid,
        message,
        stack,
        file: None,
        line: None,
        epoch_ms: lines.first().and_then(|l| epoch_of(l)),
    }
}

/// Splits raw `logcat -b crash` text into crash events. Accepts the default and `threadtime`
/// `logcat` formats (anything with a ` TAG: ` separator works via [`message_of`]); buffer
/// markers (`--------- beginning of …`) are ignored.
#[must_use]
pub fn parse_crash_buffer(text: &str) -> Vec<CrashEvent> {
    #[derive(Clone, Copy, PartialEq)]
    enum Block {
        None,
        Fatal,
        Native,
    }
    let mut out = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    let mut kind = Block::None;
    let flush = |kind: Block, cur: &[&str], out: &mut Vec<CrashEvent>| match kind {
        Block::Fatal => out.push(parse_fatal(cur)),
        Block::Native => out.push(parse_native(cur)),
        Block::None => {}
    };
    for line in text.lines() {
        if line.starts_with("---------") {
            continue;
        }
        let msg = message_of(line);
        let starts_fatal = msg.starts_with("FATAL EXCEPTION");
        let starts_native = msg.trim().starts_with("*** *** ***");
        if starts_fatal || starts_native {
            flush(kind, &cur, &mut out);
            cur = vec![line];
            kind = if starts_fatal {
                Block::Fatal
            } else {
                Block::Native
            };
        } else if kind != Block::None {
            cur.push(line);
        }
    }
    flush(kind, &cur, &mut out);
    out
}

/// Dumps and parses the device's crash buffer (`-d`: current contents, does not follow).
///
/// # Errors
/// The adb request to the device failed.
pub fn collect_crash_buffer(port: u16, serial: &str) -> Result<Vec<CrashEvent>, String> {
    let out = umr_device::adb::shell_bytes(
        port,
        serial,
        "logcat -d -b crash",
        std::time::Duration::from_secs(15),
    )
    .map_err(|e| e.to_string())?;
    Ok(parse_crash_buffer(&String::from_utf8_lossy(&out)))
}

/// An Application Not Responding report (main log, `ActivityManager` tag).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnrEvent {
    pub package: String,
    pub component: Option<String>,
    pub reason: Option<String>,
}

/// `ANR in com.pkg (com.pkg/.Activity)` (+ a following `Reason: …` line) from the main log.
#[must_use]
pub fn parse_anr(text: &str) -> Vec<AnrEvent> {
    let mut out = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let msg = message_of(line);
        let Some(rest) = msg.strip_prefix("ANR in ") else {
            continue;
        };
        let (package, component) = match rest.split_once(" (") {
            Some((p, c)) => (
                p.trim().to_string(),
                Some(c.trim_end_matches(')').to_string()),
            ),
            None => (rest.trim().to_string(), None),
        };
        let reason = lines
            .peek()
            .and_then(|l| message_of(l).trim().strip_prefix("Reason: "))
            .map(str::to_string);
        out.push(AnrEvent {
            package,
            component,
            reason,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real crash captured live: `adb shell am crash com.umr.flutter_demo` on the project's
    /// test emulator (Phase 5, 2026-11-13), then `adb logcat -d -b crash`.
    const LIVE_SHELL_CRASH: &str = "--------- beginning of crash\n\
11-13 20:25:15.994  1835  1835 E AndroidRuntime: FATAL EXCEPTION: main\n\
11-13 20:25:15.994  1835  1835 E AndroidRuntime: Process: com.umr.flutter_demo, PID: 1835\n\
11-13 20:25:15.994  1835  1835 E AndroidRuntime: android.app.RemoteServiceException$CrashedByAdbException: shell-induced crash\n\
11-13 20:25:15.994  1835  1835 E AndroidRuntime: \tat android.app.ActivityThread.throwRemoteServiceException(ActivityThread.java:2377)\n\
11-13 20:25:15.994  1835  1835 E AndroidRuntime: \tat android.app.ActivityThread.-$$Nest$mthrowRemoteServiceException(Unknown Source:0)\n\
11-13 20:25:15.994  1835  1835 E AndroidRuntime: \tat android.app.ActivityThread$H.handleMessage(ActivityThread.java:2741)\n\
11-13 20:25:15.994  1835  1835 E AndroidRuntime: \tat android.os.Handler.dispatchMessage(Handler.java:110)\n\
11-13 20:25:15.994  1835  1835 E AndroidRuntime: \tat android.os.Looper.loopOnce(Looper.java:248)\n\
11-13 20:25:15.994  1835  1835 E AndroidRuntime: \tat android.os.Looper.loop(Looper.java:338)\n\
11-13 20:25:15.994  1835  1835 E AndroidRuntime: \tat android.app.ActivityThread.main(ActivityThread.java:9067)\n\
11-13 20:25:15.994  1835  1835 E AndroidRuntime: \tat java.lang.reflect.Method.invoke(Native Method)\n\
11-13 20:25:15.994  1835  1835 E AndroidRuntime: \tat com.android.internal.os.RuntimeInit$MethodAndArgsCaller.run(RuntimeInit.java:593)\n\
11-13 20:25:15.994  1835  1835 E AndroidRuntime: \tat com.android.internal.os.ZygoteInit.main(ZygoteInit.java:932)\n";

    #[test]
    fn live_shell_crash_is_parsed() {
        let events = parse_crash_buffer(LIVE_SHELL_CRASH);
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!(e.kind, CrashKind::Fatal);
        assert_eq!(e.package.as_deref(), Some("com.umr.flutter_demo"));
        assert_eq!(e.pid, Some(1835));
        assert_eq!(
            e.message,
            "android.app.RemoteServiceException$CrashedByAdbException: shell-induced crash"
        );
        assert_eq!(e.stack.len(), 10);
        assert_eq!(
            e.stack[0],
            "at android.app.ActivityThread.throwRemoteServiceException(ActivityThread.java:2377)"
        );
        assert_eq!(e.file.as_deref(), Some("ActivityThread.java"));
        assert_eq!(e.line, Some(2377));
    }

    #[test]
    fn app_code_frame_is_found_among_framework_frames() {
        // seeded: an app's own exception, with its own frame mixed in among Android's
        let text = "11-13 20:30:00.000   900   900 E AndroidRuntime: FATAL EXCEPTION: main\n\
11-13 20:30:00.000   900   900 E AndroidRuntime: Process: com.umr.kotlindemo, PID: 900\n\
11-13 20:30:00.000   900   900 E AndroidRuntime: java.lang.IllegalStateException: boom\n\
11-13 20:30:00.000   900   900 E AndroidRuntime: \tat com.example.umrkotlindemo.MainScreenKt.MainScreen(MainScreen.kt:94)\n\
11-13 20:30:00.000   900   900 E AndroidRuntime: \tat android.app.Activity.performCreate(Activity.java:8294)\n";
        let events = parse_crash_buffer(text);
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!(e.package.as_deref(), Some("com.umr.kotlindemo"));
        assert_eq!(e.message, "java.lang.IllegalStateException: boom");
        assert_eq!(e.file.as_deref(), Some("MainScreen.kt"));
        assert_eq!(e.line, Some(94));
    }

    #[test]
    fn native_tombstone_header_is_parsed() {
        // seeded: canonical tombstone header shape (no source line info without symbols)
        let text = "11-13 20:40:00.000  5678  5678 F DEBUG   : *** *** *** *** *** *** *** *** *** *** *** *** *** *** *** ***\n\
11-13 20:40:00.000  5678  5678 F DEBUG   : Build fingerprint: 'google/sdk_gphone64_x86_64/emulator64_x86_64_arm64:16/BP22.250325.006/13208274:userdebug/dev-keys'\n\
11-13 20:40:00.000  5678  5678 F DEBUG   : pid: 5678, tid: 5678, name: app  >>> com.umr.flutter_demo <<<\n\
11-13 20:40:00.000  5678  5678 F DEBUG   : signal 11 (SIGSEGV), code 1 (SEGV_MAPERR), fault addr 0x0\n\
11-13 20:40:00.000  5678  5678 F DEBUG   : backtrace:\n\
11-13 20:40:00.000  5678  5678 F DEBUG   :       #00 pc 00001234  /system/lib64/libc.so (abort+100)\n\
11-13 20:40:00.000  5678  5678 F DEBUG   :       #01 pc 00005678  /system/lib64/libflutter.so\n";
        let events = parse_crash_buffer(text);
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!(e.kind, CrashKind::Native);
        assert_eq!(e.package.as_deref(), Some("com.umr.flutter_demo"));
        assert_eq!(e.pid, Some(5678));
        assert_eq!(
            e.message,
            "signal 11 (SIGSEGV), code 1 (SEGV_MAPERR), fault addr 0x0"
        );
        assert_eq!(e.stack.len(), 2);
        assert!(
            e.file.is_none() && e.line.is_none(),
            "no source location without symbols"
        );
    }

    #[test]
    fn multiple_crashes_in_one_dump_are_separated() {
        let text = format!("{LIVE_SHELL_CRASH}--------- beginning of crash\n11-13 21:00:00.000   222   222 E AndroidRuntime: FATAL EXCEPTION: main\n11-13 21:00:00.000   222   222 E AndroidRuntime: Process: com.other, PID: 222\n11-13 21:00:00.000   222   222 E AndroidRuntime: java.lang.NullPointerException\n");
        let events = parse_crash_buffer(&text);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].pid, Some(1835));
        assert_eq!(events[1].package.as_deref(), Some("com.other"));
        assert_eq!(events[1].message, "java.lang.NullPointerException");
    }

    #[test]
    fn epoch_format_is_read_too() {
        let text = "1731500000.123  1835  1835 E AndroidRuntime: FATAL EXCEPTION: main
1731500000.123  1835  1835 E AndroidRuntime: Process: com.x, PID: 1835
1731500000.123  1835  1835 E AndroidRuntime: java.lang.IllegalStateException: boom
1731500000.124  1835  1835 E AndroidRuntime: 	at com.x.Main.go(Main.kt:3)
";
        let e = &parse_crash_buffer(text)[0];
        assert_eq!(e.epoch_ms, Some(1_731_500_000_123));
        assert_eq!((e.package.as_deref(), e.line), (Some("com.x"), Some(3)));
        assert_eq!(parse_crash_buffer(LIVE_SHELL_CRASH)[0].epoch_ms, None);
    }

    #[test]
    fn empty_or_unrelated_log_yields_nothing() {
        assert_eq!(parse_crash_buffer(""), Vec::new());
        assert_eq!(
            parse_crash_buffer("11-13 20:00:00.000 1 1 I SomeTag: just chatting\n"),
            Vec::new()
        );
    }

    #[test]
    fn anr_is_extracted_with_its_reason() {
        let text = "11-13 20:50:00.000   300   300 I ActivityManager: ANR in com.umr.flutter_demo (com.umr.flutter_demo/.MainActivity)\n\
11-13 20:50:00.000   300   300 I ActivityManager: Reason: Input dispatching timed out\n";
        let events = parse_anr(text);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].package, "com.umr.flutter_demo");
        assert_eq!(
            events[0].component.as_deref(),
            Some("com.umr.flutter_demo/.MainActivity")
        );
        assert_eq!(
            events[0].reason.as_deref(),
            Some("Input dispatching timed out")
        );
    }

    #[test]
    fn anr_without_a_component_or_reason_still_parses() {
        let events = parse_anr("11-13 20:50:00.000 1 1 I ActivityManager: ANR in com.x\n");
        assert_eq!(events[0].package, "com.x");
        assert_eq!(events[0].component, None);
        assert_eq!(events[0].reason, None);
    }
}
