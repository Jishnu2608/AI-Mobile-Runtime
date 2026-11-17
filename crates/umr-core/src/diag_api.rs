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

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
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
    /// AUD-P1-7: problems the user sent to the agent ("Send to agent"): its next stop hook
    /// reports them even when they were reported before.
    pub flagged: HashSet<String>,
    /// AUD-P3-24: problems the agent was told about (fingerprint → when, what), to notice when
    /// they are gone, and the feed of both for the home screen (newest last, survives runs).
    pub told: HashMap<String, (u64, String)>,
    pub feed: VecDeque<FeedEvent>,
}

/// One line of the agent-fix feed on the home screen.
#[derive(Debug, Clone, Serialize)]
pub struct FeedEvent {
    pub at_ms: u64,
    /// `told`: the agent got the problem at the end of a task; `fixed`: it was gone at a later
    /// check (with `seconds` since it was told).
    pub kind: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seconds: Option<u64>,
}

impl Default for DiagStore {
    fn default() -> Self {
        Self {
            problems: Dedupe::new(Self::KEEP_PROBLEMS),
            logs: VecDeque::new(),
            next_log: 0,
            flagged: HashSet::new(),
            told: HashMap::new(),
            feed: VecDeque::new(),
        }
    }
}

impl DiagStore {
    const KEEP_PROBLEMS: usize = 500;
    const KEEP_LOGS: usize = 5000;
    const KEEP_FEED: usize = 50;

    /// A new run starts empty. The log cursor keeps counting so a reader's old cursor never
    /// hides the new run's first lines. The feed stays: it is the history of the agent's work.
    pub fn reset(&mut self) {
        self.problems.clear();
        self.logs.clear();
        self.flagged.clear();
        self.told.clear();
    }

    /// Records what an agent check found: problems it reports now are "told"; problems told
    /// earlier and gone now are "fixed". `reported` holds (fingerprint, message).
    pub fn note_check(&mut self, reported: &[(String, String)]) {
        let now = now_ms();
        let present: HashSet<&String> = self
            .problems
            .entries()
            .iter()
            .map(|e| &e.diagnostic.fingerprint)
            .collect();
        let gone: Vec<String> = self
            .told
            .keys()
            .filter(|fp| !present.contains(fp))
            .cloned()
            .collect();
        for fp in gone {
            if let Some((at, message)) = self.told.remove(&fp) {
                self.push_feed(FeedEvent {
                    at_ms: now,
                    kind: "fixed",
                    message,
                    seconds: Some(now.saturating_sub(at) / 1000),
                });
            }
        }
        for (fp, message) in reported {
            self.flagged.remove(fp);
            if !self.told.contains_key(fp) {
                self.told.insert(fp.clone(), (now, message.clone()));
                self.push_feed(FeedEvent {
                    at_ms: now,
                    kind: "told",
                    message: message.clone(),
                    seconds: None,
                });
            }
        }
    }

    fn push_feed(&mut self, e: FeedEvent) {
        self.feed.push_back(e);
        while self.feed.len() > Self::KEEP_FEED {
            self.feed.pop_front();
        }
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

    /// A reload or rebuild starts: the new code reports its own compile errors and, when it runs,
    /// its own runtime errors again. Earlier ones (at old lines, or already fixed) would
    /// otherwise stay listed after the fix (AUD-P0-1). Crashes stay: they are history.
    pub fn reload_started(&mut self) {
        self.problems
            .retain(|e| !matches!(e.diagnostic.category, Category::Build | Category::Runtime));
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
    /// The project folder: React Native errors are located inside it (AUD-P0-2).
    project: PathBuf,
    /// Metro's answers per bundle position (`None`: not in the project / no answer), so an error
    /// that repeats on every render is mapped once.
    mapped: HashMap<umr_diagnostics::JsFrame, Option<(String, u32, Option<u32>)>>,
}

/// One `POST` to a server on this PC; the body of a 200 answer. Plain HTTP/1.1 with
/// `Connection: close` (Metro is the only caller's peer), chunked answers decoded.
fn post_local(port: u16, path: &str, body: &str, timeout: Duration) -> Option<String> {
    use std::io::{Read, Write};
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(1)).ok()?;
    s.set_read_timeout(Some(timeout)).ok()?;
    s.set_write_timeout(Some(timeout)).ok()?;
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).ok()?;
    let mut raw = Vec::new();
    let _ = s.take(4 << 20).read_to_end(&mut raw); // a timeout still leaves what arrived
    let text = String::from_utf8_lossy(&raw);
    let (head, rest) = text.split_once("\r\n\r\n")?;
    if !head.starts_with("HTTP/1.1 200") && !head.starts_with("HTTP/1.0 200") {
        return None;
    }
    if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        Some(dechunk(rest))
    } else {
        Some(rest.to_string())
    }
}

/// A chunked HTTP body → its content.
fn dechunk(mut rest: &str) -> String {
    let mut out = String::new();
    while let Some((size, tail)) = rest.split_once("\r\n") {
        let Ok(n) = usize::from_str_radix(size.trim(), 16) else {
            break;
        };
        if n == 0 || tail.len() < n {
            break;
        }
        out.push_str(&tail[..n]);
        rest = tail[n..].strip_prefix("\r\n").unwrap_or(&tail[n..]);
    }
    out
}

/// A path Metro reported, relative to the project when inside it and outside `node_modules`.
fn project_file(file: &str, project: &Path) -> Option<String> {
    let rel = umr_adapter::problems::relative(file, project);
    umr_diagnostics::classify::is_app_file(&rel).then_some(rel)
}

impl Poll {
    /// Maps the error's innermost app frame in a Metro bundle to its source line (at most three
    /// frames, each asked once). Without an answer the error keeps its component name only.
    fn locate_js(&mut self, e: &mut umr_diagnostics::JsError) {
        for f in umr_diagnostics::bundle_frames(e).into_iter().take(3) {
            let found = if let Some(v) = self.mapped.get(&f) {
                v.clone()
            } else {
                let v = umr_diagnostics::metro_port(&f.url)
                    .and_then(|port| {
                        post_local(
                            port,
                            "/symbolicate",
                            &umr_diagnostics::symbolicate_body(&f),
                            Duration::from_secs(3),
                        )
                    })
                    .and_then(|b| umr_diagnostics::parse_symbolicated(&b))
                    .and_then(|(file, l, c)| {
                        project_file(&file, &self.project).map(|rel| (rel, l, c))
                    });
                self.mapped.insert(f, v.clone());
                v
            };
            if let Some((file, line, column)) = found {
                e.file = Some(file);
                e.line = Some(line);
                e.column = column;
                return;
            }
        }
    }

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
        // new lines, kept for the React Native error reader below
        let mut fresh: Vec<(u64, char, String, String)> = Vec::new();
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
            s.log("logcat", level, Some(tag.clone()), Some(lpid), &msg);
            fresh.push((ms, level, tag, msg));
        }
        drop(s);
        // AUD-P0-2: React Native 0.77+ logs JS errors only here (`ReactNativeJS` at E), not
        // through Metro, so they are read from the app's device log and mapped to their source
        // line through Metro (outside the store's lock: that asks Metro over HTTP)
        for text in umr_diagnostics::group_js_errors(&fresh) {
            if let Some(mut e) = umr_diagnostics::parse_js_error(&text) {
                self.locate_js(&mut e);
                lock(store).problem(umr_diagnostics::from_js_error(&e));
            }
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
    project: &Path,
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
        project: project.to_path_buf(),
        mapped: HashMap::new(),
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

/// `diagnostics.flag` (AUD-P1-7, "Send to agent"): the agent's next stop hook reports this
/// problem even if it was reported before.
pub fn flag(state: &ApiState, id: Id, p: &DiagnosticGetParams) -> Response {
    let mut s = lock(&state.diag);
    if !s.problems.contains(&p.fingerprint) {
        return Response::err(
            Some(id),
            RpcError::new(
                codes::INVALID_PARAMS,
                format!("no diagnostic {} in the current run", p.fingerprint),
            ),
        );
    }
    s.flagged.insert(p.fingerprint.clone());
    ok(id, &json!({ "flagged": p.fingerprint }))
}

/// The source lines around a problem (AUD-P1-11), for "Copy" and agents: `around` lines before
/// and after `line`, sanitised like every other text that leaves the Core. Only a file inside
/// `project` (after resolving `..` and links) and at most 2 MB is read.
#[must_use]
pub fn code_context(project: &Path, file: &str, line: u32, around: u32) -> Option<Value> {
    if line == 0 {
        return None;
    }
    let root = std::fs::canonicalize(project).ok()?;
    let path = Path::new(file);
    let full = if path.is_absolute() {
        path.to_path_buf()
    } else {
        project.join(path)
    };
    let full = std::fs::canonicalize(full).ok()?;
    if !full.starts_with(&root) || std::fs::metadata(&full).ok()?.len() > 2 << 20 {
        return None;
    }
    let text = String::from_utf8_lossy(&std::fs::read(&full).ok()?).into_owned();
    let first = line.saturating_sub(around).max(1);
    let lines: Vec<String> = text
        .lines()
        .skip(usize::try_from(first - 1).ok()?)
        .take(usize::try_from(line - first + around + 1).ok()?)
        .map(|l| sanitize(l.trim_end()))
        .collect();
    (!lines.is_empty()).then(|| json!({ "first": first, "line": line, "lines": lines }))
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
    let project = state
        .run_info
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map(|r| PathBuf::from(&r.project));
    let s = lock(&state.diag);
    match s.problems.get(&p.fingerprint) {
        Some(e) => {
            let mut v = entry_json(e, true);
            if let (Some(root), Some(file), Some(line)) =
                (&project, &e.diagnostic.file, e.diagnostic.line)
            {
                if let Some(code) = code_context(root, file, line, 3) {
                    v["code"] = code;
                }
            }
            ok(id, &v)
        }
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

    #[test]
    fn a_reload_drops_old_build_and_runtime_problems_but_keeps_crashes() {
        let mut s = DiagStore::default();
        s.problem(umr_diagnostics::from_message(
            "dart",
            "old compile error",
            Category::Build,
        ));
        s.problem(umr_diagnostics::from_message(
            "flutter",
            "old widget error",
            Category::Runtime,
        ));
        s.problem(umr_diagnostics::from_message(
            "logcat",
            "crashed earlier",
            Category::Crash,
        ));
        s.reload_started();
        let left: Vec<_> = s
            .problems
            .entries()
            .iter()
            .map(|e| e.diagnostic.message.clone())
            .collect();
        assert_eq!(left, ["crashed earlier"]);
    }

    #[test]
    fn chunked_bodies_are_joined() {
        assert_eq!(
            dechunk("5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n"),
            "hello world"
        );
        assert_eq!(dechunk("zz\r\n"), "");
    }

    #[test]
    fn only_files_inside_the_project_count() {
        let root = Path::new(r"D:\Work\my app");
        assert_eq!(
            project_file(r"D:\Work\my app\src\App.tsx", root).as_deref(),
            Some("src/App.tsx")
        );
        assert_eq!(
            project_file(r"D:\Work\my app\node_modules\react\x.js", root),
            None
        );
        assert_eq!(project_file(r"C:\elsewhere\x.js", root), None);
    }

    /// A stand-in Metro on a free local port answers `/symbolicate` once (chunked, like Node).
    fn fake_metro(answer: &'static str) -> u16 {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut c, _)) = l.accept() {
                let mut buf = [0u8; 4096];
                let _ = c.read(&mut buf);
                let r = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{answer}\r\n0\r\n\r\n",
                    answer.len()
                );
                let _ = c.write_all(r.as_bytes());
            }
        });
        port
    }

    fn poll_for(project: &str) -> Poll {
        Poll {
            port: 0,
            serial: String::new(),
            package: String::new(),
            since_ms: 0,
            crashes: 0,
            anrs: 0,
            pid: None,
            last_ms: 0,
            seen_at_last: HashSet::new(),
            project: PathBuf::from(project),
            mapped: HashMap::new(),
        }
    }

    #[test]
    fn a_react_native_error_is_mapped_through_metro() {
        let port = fake_metro(
            r#"{"stack":[{"file":"D:/Work/rn/src/Keypad.tsx","lineNumber":41,"column":6,"methodName":"Keypad"}]}"#,
        );
        let text = format!("{{ [Error: boom]\n  componentStack: '\n    at Keypad (http://10.0.2.2:{port}/src/Keypad.bundle//&platform=android:120:9)\n    at RCTView (<anonymous>)',\n  isComponentError: true }}");
        let mut e = umr_diagnostics::parse_js_error(&text).unwrap();
        let mut poll = poll_for(r"D:\Work\rn");
        poll.locate_js(&mut e);
        assert_eq!(
            (e.file.as_deref(), e.line, e.column),
            (Some("src/Keypad.tsx"), Some(41), Some(7))
        );
        // asked once: the same position again is answered from memory (the fake is gone)
        let mut again = umr_diagnostics::parse_js_error(&text).unwrap();
        poll.locate_js(&mut again);
        assert_eq!(again.line, Some(41));
    }

    #[test]
    fn without_metro_the_error_keeps_its_component_only() {
        // a port nothing listens on
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let text = format!("{{ [Error: boom]\n  componentStack: '\n    at App (http://10.0.2.2:{port}/App.bundle//&platform=android:92:40)',\n  isComponentError: true }}");
        let mut e = umr_diagnostics::parse_js_error(&text).unwrap();
        poll_for(r"D:\Work\rn").locate_js(&mut e);
        assert_eq!(e.file, None);
        let d = umr_diagnostics::from_js_error(&e);
        assert_eq!(d.message, "Error: boom (in component <App>)");
    }

    #[test]
    fn the_feed_records_what_the_agent_was_told_and_what_it_fixed() {
        let mut s = DiagStore::default();
        let d = umr_diagnostics::from_message("dart", "Expected ';'", Category::Build);
        let fp = d.fingerprint.clone();
        s.problem(d);
        s.flagged.insert(fp.clone());
        s.note_check(&[(fp.clone(), "Expected ';'".into())]);
        assert!(
            s.flagged.is_empty(),
            "a reported problem is no longer flagged"
        );
        assert_eq!(s.feed.len(), 1);
        assert_eq!(s.feed[0].kind, "told");
        // reported again while it is still there: no second line
        s.note_check(&[(fp.clone(), "Expected ';'".into())]);
        assert_eq!(s.feed.len(), 1);
        // gone at the next check: fixed
        s.reload_started();
        s.note_check(&[]);
        assert_eq!(s.feed.len(), 2);
        assert_eq!(
            (s.feed[1].kind, s.feed[1].message.as_str()),
            ("fixed", "Expected ';'")
        );
        assert!(s.feed[1].seconds.is_some());
        // a new run keeps the history
        s.reset();
        assert_eq!(s.feed.len(), 2);
    }

    #[test]
    fn code_context_reads_only_inside_the_project() {
        let base = std::env::temp_dir().join(format!("umr-code-{}", std::process::id()));
        let proj = base.join("app");
        std::fs::create_dir_all(proj.join("src")).unwrap();
        let body = (1..=10).fold(String::new(), |mut s, i| {
            use std::fmt::Write as _;
            let _ = writeln!(s, "line {i}");
            s
        });
        std::fs::write(proj.join("src").join("App.tsx"), &body).unwrap();
        std::fs::write(base.join("secret.txt"), "outside\n").unwrap();
        let c = code_context(&proj, "src/App.tsx", 5, 2).unwrap();
        assert_eq!(c["first"], 3);
        assert_eq!(c["line"], 5);
        assert_eq!(
            c["lines"],
            json!(["line 3", "line 4", "line 5", "line 6", "line 7"])
        );
        // near the top the window starts at line 1
        assert_eq!(
            code_context(&proj, "src/App.tsx", 1, 3).unwrap()["lines"][0],
            "line 1"
        );
        // an absolute path inside the project works too
        let abs = proj.join("src").join("App.tsx");
        assert!(code_context(&proj, &abs.display().to_string(), 2, 1).is_some());
        // never outside the project, never line 0, never a missing file
        assert_eq!(code_context(&proj, "../secret.txt", 1, 1), None);
        assert_eq!(
            code_context(&proj, &base.join("secret.txt").display().to_string(), 1, 1),
            None
        );
        assert_eq!(code_context(&proj, "src/App.tsx", 0, 1), None);
        assert_eq!(code_context(&proj, "MainActivity.kt", 3, 1), None);
        let _ = std::fs::remove_dir_all(&base);
    }
}
