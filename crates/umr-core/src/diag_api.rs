//! Diagnostics of the current run (DIA-004): the per-run store, the device collector and the
//! `diagnostics.list`, `diagnostics.get` and `logs.tail` methods.
//!
//! Sources while a project runs:
//! * adapter events (`Problem`, log and phase lines) through the run's event sink;
//! * the adapter's error when a build, install or launch fails;
//! * the device ([`Collector`], every 1.5 s): crash buffer and ANR reports of the app's package,
//!   and the app process's own log lines (`logcat --pid`), all only from after the run started
//!   (`logcat -t <device time>`), so earlier crashes are not reported again.
//!
//! Everything a client reads is passed through the sanitiser first (DIA-005).

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use umr_api::methods::{DiagnosticGetParams, DiagnosticsParams, LogsParams};
use umr_api::rpc::{codes, Id, Response, RpcError};
use umr_diagnostics::{sanitize, sanitize_diagnostic, Category, Dedupe, Diagnostic, Entry};

use crate::api_server::{ok, ApiState};

#[must_use]
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(0))
}

#[derive(Debug, Clone, Serialize)]
pub struct LogLine {
    pub seq: u64,
    pub at_ms: u64,
    /// `tool` (build tool / framework CLI), `umr` (progress) or `logcat` (the app on the device).
    pub source: &'static str,
    /// `V` `D` `I` `W` `E` `F`.
    pub level: char,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    pub text: String,
}

/// Problems (deduplicated) and log lines of the current run.
#[derive(Debug)]
pub struct DiagStore {
    pub problems: Dedupe,
    logs: VecDeque<LogLine>,
    next_log: u64,
}

impl Default for DiagStore {
    fn default() -> Self {
        Self {
            problems: Dedupe::new(Self::KEEP_PROBLEMS),
            logs: VecDeque::new(),
            next_log: 0,
        }
    }
}

impl DiagStore {
    const KEEP_PROBLEMS: usize = 500;
    const KEEP_LOGS: usize = 5000;

    /// A new run starts empty. The log cursor keeps counting so a reader's old cursor never
    /// hides the new run's first lines.
    pub fn reset(&mut self) {
        self.problems.clear();
        self.logs.clear();
    }

    pub fn problem(&mut self, d: Diagnostic) {
        // the Core log is a file on disk: sanitised like everything else that leaves the store
        tracing::info!(category = ?d.category, origin = ?d.origin, line = ?d.line, "diagnostic: {} ({})", sanitize(&d.message), d.file.as_deref().map(sanitize).unwrap_or_default());
        self.problems.add(d, now_ms());
    }

    /// Adds `d` unless that problem is already known (an adapter error repeats what its events
    /// reported; counting it twice would be wrong).
    pub fn problem_once(&mut self, d: Diagnostic) {
        if !self.problems.contains(&d.fingerprint) {
            self.problem(d);
        }
    }

    /// A successful reload or rebuild compiled the code: its earlier build problems are fixed.
    pub fn build_fixed(&mut self) {
        self.problems
            .retain(|e| e.diagnostic.category != Category::Build);
    }

    pub fn log(
        &mut self,
        source: &'static str,
        level: char,
        tag: Option<String>,
        pid: Option<u32>,
        text: &str,
    ) {
        self.next_log += 1;
        self.logs.push_back(LogLine {
            seq: self.next_log,
            at_ms: now_ms(),
            source,
            level,
            tag,
            pid,
            text: text.to_string(),
        });
        while self.logs.len() > Self::KEEP_LOGS {
            self.logs.pop_front();
        }
    }

    #[must_use]
    pub fn log_cursor(&self) -> u64 {
        self.next_log
    }
}

fn lock(s: &Mutex<DiagStore>) -> std::sync::MutexGuard<'_, DiagStore> {
    s.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Severity order of logcat levels.
fn level_rank(c: char) -> u8 {
    match c.to_ascii_uppercase() {
        'V' => 0,
        'D' => 1,
        'W' => 3,
        'E' => 4,
        'F' | 'A' => 5,
        _ => 2,
    }
}

/// `1731500000.123  1835  1835 E Tag     : message` (`logcat -v epoch`).
#[must_use]
pub fn parse_epoch_line(line: &str) -> Option<(u64, u32, char, String, String)> {
    let ms = umr_diagnostics::crash::epoch_of(line)?;
    let mut it = line.split_whitespace();
    it.next()?;
    let pid = it.next()?.parse().ok()?;
    it.next()?; // tid
    let level = it.next()?.chars().next()?;
    // the tag runs to the first ": " after the level letter
    let after_level = line.find(&format!(" {level} "))? + 3;
    let tail = &line[after_level..];
    let (tag, msg) = tail.split_once(": ").unwrap_or((tail, ""));
    Some((ms, pid, level, tag.trim().to_string(), msg.to_string()))
}

/// Reads the device for the running app until dropped.
#[derive(Debug)]
pub struct Collector {
    stop: Arc<AtomicBool>,
}

impl Drop for Collector {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Device time in ms (`date +%s`), a little early so nothing at the run's start is missed.
#[must_use]
pub fn device_now_ms(port: u16, serial: &str) -> Option<u64> {
    let s = umr_device::adb::shell(port, serial, "date +%s").ok()?;
    let secs: u64 = s.trim().parse().ok()?;
    Some(secs.saturating_sub(2) * 1000)
}

fn since_arg(ms: u64) -> String {
    format!("{}.{:03}", ms / 1000, ms % 1000)
}

struct Poll {
    port: u16,
    serial: String,
    package: String,
    since_ms: u64,
    crashes: usize,
    anrs: usize,
    pid: Option<u32>,
    last_ms: u64,
    seen_at_last: HashSet<String>,
}

impl Poll {
    fn shell(&self, cmd: &str) -> String {
        umr_device::adb::shell_bytes(self.port, &self.serial, cmd, Duration::from_secs(10))
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default()
    }

    fn run(&mut self, store: &Mutex<DiagStore>) {
        let since = since_arg(self.since_ms);
        // crash buffer: count only this package's crashes after the run started
        let text = self.shell(&format!("logcat -d -b crash -v epoch -t {since}"));
        let crashes: Vec<_> = umr_diagnostics::parse_crash_buffer(&text)
            .into_iter()
            .filter(|c| c.package.as_deref() == Some(self.package.as_str()))
            .collect();
        if crashes.len() < self.crashes {
            self.crashes = 0; // the buffer was cleared
        }
        for c in &crashes[self.crashes..] {
            let mut s = lock(store);
            s.log("logcat", 'F', Some("crash".into()), c.pid, &c.message);
            s.problem(umr_diagnostics::from_crash(c));
        }
        self.crashes = crashes.len();
        // ANR reports
        let text = self.shell(&format!("logcat -d -v epoch -s ActivityManager -t {since}"));
        let anrs: Vec<_> = umr_diagnostics::parse_anr(&text)
            .into_iter()
            .filter(|a| a.package == self.package)
            .collect();
        if anrs.len() < self.anrs {
            self.anrs = 0;
        }
        for a in &anrs[self.anrs..] {
            lock(store).problem(umr_diagnostics::from_anr(a));
        }
        self.anrs = anrs.len();
        // the app's own log lines
        let pid = umr_device::app::pid(self.port, &self.serial, &self.package);
        if pid.is_none() {
            return;
        }
        if pid != self.pid {
            self.pid = pid;
            self.last_ms = self.since_ms;
            self.seen_at_last.clear();
        }
        let Some(p) = pid else { return };
        let text = self.shell(&format!(
            "logcat -d -v epoch --pid={p} -t {}",
            since_arg(self.last_ms)
        ));
        let mut s = lock(store);
        for line in text.lines() {
            let Some((ms, lpid, level, tag, msg)) = parse_epoch_line(line) else {
                continue;
            };
            if ms < self.last_ms || (ms == self.last_ms && self.seen_at_last.contains(line)) {
                continue;
            }
            if ms > self.last_ms {
                self.last_ms = ms;
                self.seen_at_last.clear();
            }
            self.seen_at_last.insert(line.to_string());
            s.log("logcat", level, Some(tag), Some(lpid), &msg);
        }
    }
}

/// Starts reading `package`'s crashes, ANRs and log lines on the device.
#[must_use]
pub fn start_collector(
    store: Arc<Mutex<DiagStore>>,
    port: u16,
    serial: &str,
    package: &str,
    since_ms: u64,
) -> Collector {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let mut poll = Poll {
        port,
        serial: serial.to_string(),
        package: package.to_string(),
        since_ms,
        crashes: 0,
        anrs: 0,
        pid: None,
        last_ms: since_ms,
        seen_at_last: HashSet::new(),
    };
    let _ = std::thread::Builder::new()
        .name("umr-diagnostics".into())
        .spawn(move || {
            while !flag.load(Ordering::SeqCst) {
                poll.run(&store);
                for _ in 0..15 {
                    if flag.load(Ordering::SeqCst) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        });
    Collector { stop }
}

fn parse_category(s: &str) -> Result<Category, RpcError> {
    Ok(match s {
        "environment" => Category::Environment,
        "build" => Category::Build,
        "runtime" => Category::Runtime,
        "crash" => Category::Crash,
        "device" => Category::Device,
        "network" => Category::Network,
        other => {
            return Err(RpcError::new(
                codes::INVALID_PARAMS,
                format!(
                "unknown category '{other}': environment, build, runtime, crash, device or network"
            ),
            ))
        }
    })
}

/// An entry as clients see it: sanitised, and without the stack unless asked.
fn entry_json(e: &Entry, stack: bool) -> Value {
    let mut d = sanitize_diagnostic(&e.diagnostic);
    let frames = d.stack.len();
    if !stack {
        d.stack.clear();
    }
    let mut v = serde_json::to_value(&d).unwrap_or(Value::Null);
    v["count"] = json!(e.count);
    v["first_seen_ms"] = json!(e.first_seen_ms);
    v["last_seen_ms"] = json!(e.last_seen_ms);
    v["seq"] = json!(e.seq);
    v["stack_frames"] = json!(frames);
    v
}

/// `diagnostics.list`.
pub fn list(state: &ApiState, id: Id, p: &DiagnosticsParams) -> Response {
    let cat = match p.category.as_deref().map(parse_category).transpose() {
        Ok(c) => c,
        Err(e) => return Response::err(Some(id), e),
    };
    let s = lock(&state.diag);
    let mut counts = serde_json::Map::new();
    for e in s.problems.entries() {
        let k = serde_json::to_value(e.diagnostic.category)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();
        let n = counts.get(&k).and_then(Value::as_u64).unwrap_or(0);
        counts.insert(k, json!(n + 1));
    }
    let mut picked: Vec<&Entry> = s
        .problems
        .since(p.since.unwrap_or(0))
        .filter(|e| cat.is_none_or(|c| e.diagnostic.category == c))
        .collect();
    let limit = p.limit.unwrap_or(50).clamp(1, 500);
    if picked.len() > limit {
        // keep the most recently seen
        picked.sort_by_key(|e| std::cmp::Reverse(e.seq));
        picked.truncate(limit);
        picked.sort_by_key(|e| e.first_seen_ms);
    }
    let entries: Vec<Value> = picked.iter().map(|e| entry_json(e, p.stack)).collect();
    ok(
        id,
        &json!({
            "entries": entries,
            "counts": counts,
            "total": s.problems.len(),
            "cursor": s.problems.cursor(),
            "run": state.run_info.lock().unwrap_or_else(PoisonError::into_inner).clone(),
        }),
    )
}

/// `diagnostics.get`.
pub fn get(state: &ApiState, id: Id, p: &DiagnosticGetParams) -> Response {
    let s = lock(&state.diag);
    match s.problems.get(&p.fingerprint) {
        Some(e) => ok(id, &entry_json(e, true)),
        None => Response::err(
            Some(id),
            RpcError::new(
                codes::INVALID_PARAMS,
                format!("no diagnostic {} in the current run", p.fingerprint),
            ),
        ),
    }
}

/// `logs.tail`.
pub fn logs(state: &ApiState, id: Id, p: &LogsParams) -> Response {
    let min = p
        .level
        .as_deref()
        .and_then(|l| l.chars().next())
        .map(level_rank);
    let grep = p.grep.as_deref().map(str::to_lowercase);
    let s = lock(&state.diag);
    let mut lines: Vec<Value> = s
        .logs
        .iter()
        .filter(|l| l.seq > p.since.unwrap_or(0))
        .filter(|l| p.source.as_deref().is_none_or(|src| src == l.source))
        .filter(|l| min.is_none_or(|m| level_rank(l.level) >= m))
        .filter_map(|l| {
            let mut l = l.clone();
            l.text = sanitize(&l.text);
            if grep.as_deref().is_some_and(|g| {
                !l.text.to_lowercase().contains(g)
                    && !l.tag.as_deref().unwrap_or("").to_lowercase().contains(g)
            }) {
                return None;
            }
            serde_json::to_value(l).ok()
        })
        .collect();
    let limit = p.limit.unwrap_or(200).clamp(1, 5000);
    let skip = lines.len().saturating_sub(limit);
    lines.drain(..skip);
    ok(id, &json!({ "lines": lines, "cursor": s.log_cursor() }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_lines_are_split() {
        let (ms, pid, level, tag, msg) =
            parse_epoch_line("  1731500000.123  1835  1840 E flutter : boom: x").unwrap();
        assert_eq!((ms, pid, level), (1_731_500_000_123, 1835, 'E'));
        assert_eq!((tag.as_str(), msg.as_str()), ("flutter", "boom: x"));
        assert!(parse_epoch_line("--------- beginning of main").is_none());
    }

    #[test]
    fn store_counts_and_keeps_the_cursor_across_runs() {
        let mut s = DiagStore::default();
        s.log("tool", 'I', None, None, "a");
        s.problem(umr_diagnostics::from_message("x", "one", Category::Build));
        let d = umr_diagnostics::from_message("x", "one", Category::Build);
        s.problem_once(d);
        assert_eq!(
            s.problems.entries()[0].count,
            1,
            "problem_once does not count twice"
        );
        s.reset();
        s.log("tool", 'I', None, None, "b");
        assert_eq!(s.log_cursor(), 2);
        assert_eq!(s.problems.len(), 0);
    }
}
