//! HOOK-001: the stop hook that closes the agent loop.
//!
//! `umr agent setup claude-code|codex|cursor|antigravity` registers `umr agent hook <client>` as the agent's
//! *stop* hook. When the agent ends its turn, the hook asks the Runtime Core (`agent.check`) to
//! wait for the rebuild or reload the agent's last saves started, then for the problems the agent
//! should still fix in the code: an open build error (Kotlin, Java, Dart, JavaScript …) or a new
//! runtime error or crash. If there is one, the hook keeps the agent going with a short report
//! (`file:line`, message, how to get details); if not, the agent stops as usual.
//!
//! * Claude Code and Codex: stdout `{"decision":"block","reason":…}` keeps the agent working with
//!   `reason` as its next instruction. Cursor: `{"followup_message":…}` is sent as the next user
//!   message. Antigravity: `{"decision":"continue","reason":…}` re-enters its loop with `reason`
//!   as a system message.
//! * Never in the way: no Core, no run of this folder, no token, a rebuild still running at the
//!   timeout or any error → the agent stops normally. After [`MAX_ROUNDS`] reports in a row the
//!   hook lets the agent stop, so a problem it cannot fix does not loop forever.
//! * Uses the agent's own client token (scope observe), never the admin token, and never starts
//!   the Core.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde_json::{json, Map, Value};
use umr_api::methods;

use crate::core_process::CoreTarget;
use crate::mcp_cmd::token_path;

/// Reports in a row before the hook lets the agent stop anyway.
pub const MAX_ROUNDS: u32 = 3;
/// Seconds the agent gives the hook (a cold Gradle build fits; the Core waits up to 120 s).
pub const HOOK_TIMEOUT_S: u64 = 150;
/// The clients that get a stop hook.
pub const HOOK_CLIENTS: [&str; 4] = ["claude-code", "codex", "cursor", "antigravity"];

// ---- what the agent sent ------------------------------------------------------------------

/// The project folder of the agent's session: `cwd` (Claude Code, Codex), the first
/// `workspace_roots` entry (Cursor, which may write `/c:/path`) or the first `workspacePaths`
/// entry (Antigravity; a `file:///` URI is accepted too).
#[must_use]
pub fn project_from_input(input: &Value) -> Option<PathBuf> {
    // Antigravity sends `"cwd": ""` next to its workspace paths
    fn text(v: &Value) -> Option<&str> {
        v.as_str().filter(|s| !s.trim().is_empty())
    }
    let s = text(&input["cwd"])
        .or_else(|| text(&input["workspace_roots"][0]))
        .or_else(|| text(&input["workspacePaths"][0]))?;
    let s = s.strip_prefix("file://").unwrap_or(s);
    let s = s
        .replace("%20", " ")
        .replace("%3A", ":")
        .replace("%3a", ":");
    let s = s.as_str();
    let t = s
        .strip_prefix('/')
        .filter(|r| r.as_bytes().get(1) == Some(&b':'))
        .unwrap_or(s);
    (!t.is_empty()).then(|| PathBuf::from(t.replace('/', "\\")))
}

// ---- the decision -------------------------------------------------------------------------

/// What the hook tells the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Stop as usual.
    Allow,
    /// Keep working: this is the report.
    Continue(String),
}

/// The problems as `fp:count` pairs: a fix attempt that rebuilt and failed again raises a count,
/// so an unchanged signature means the agent left the code alone since the last report.
#[must_use]
pub fn signature(problems: &[Value]) -> String {
    let mut v: Vec<String> = problems
        .iter()
        .map(|p| format!("{}:{}", p["fp"].as_str().unwrap_or("?"), p["count"]))
        .collect();
    v.sort();
    v.join(",")
}

/// The verdict for these problems, given the reports in a row so far (`rounds`) and the last
/// report's [`signature`]; returns the new round count. Each problem set is reported once until
/// the agent changes something, and never more than [`MAX_ROUNDS`] times in a row.
#[must_use]
pub fn decide(problems: &[Value], rounds: u32, last: &str) -> (Verdict, u32) {
    if problems.is_empty() || rounds >= MAX_ROUNDS || (rounds > 0 && signature(problems) == last) {
        return (Verdict::Allow, 0);
    }
    (Verdict::Continue(report(problems)), rounds + 1)
}

/// The text the agent gets.
#[must_use]
pub fn report(problems: &[Value]) -> String {
    let mut out = String::from(
        "AI Mobile Runtime: the app on the emulator still has problems after your changes.\n",
    );
    for p in problems {
        let what = match p["category"].as_str() {
            Some("build") => "build error",
            Some("crash") => "crash",
            _ => "runtime error",
        };
        let at = p["at"]
            .as_str()
            .map(|a| format!(" at {a}"))
            .unwrap_or_default();
        let times = match p["count"].as_u64() {
            Some(n) if n > 1 => format!(" (seen {n} times)"),
            _ => String::new(),
        };
        let _ = writeln!(
            out,
            "- {what}{at}: {}{times} [fp {}]",
            p["msg"].as_str().unwrap_or(""),
            p["fp"].as_str().unwrap_or("?")
        );
    }
    out.push_str(
        "Fix them in the code and save: the runtime rebuilds or reloads by itself. Use the umr \
         MCP tools for details (`get_diagnostic` with the fp gives the full message and stack; \
         `runtime_status` and `screenshot` confirm the fix). If a problem cannot be fixed in the \
         code, or the user told you not to change that code, say so in one line and stop: you \
         will not be asked again for it in this turn.",
    );
    out
}

/// What the hook prints for `client` (nothing = stop as usual).
#[must_use]
pub fn output(client: &str, v: &Verdict) -> Option<String> {
    match (client, v) {
        ("cursor", Verdict::Allow) => Some("{}".into()),
        // Antigravity needs a `decision`; anything but "continue" lets the agent stop
        ("antigravity", Verdict::Allow) => Some(json!({ "decision": "stop" }).to_string()),
        (_, Verdict::Allow) => None,
        ("cursor", Verdict::Continue(m)) => Some(json!({ "followup_message": m }).to_string()),
        ("antigravity", Verdict::Continue(m)) => {
            Some(json!({ "decision": "continue", "reason": m }).to_string())
        }
        (_, Verdict::Continue(m)) => Some(json!({ "decision": "block", "reason": m }).to_string()),
    }
}

// ---- per-project state (cursor + rounds) ----------------------------------------------------

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct HookState {
    cursor: u64,
    /// The Core start the cursor belongs to.
    epoch: u64,
    rounds: u32,
    /// [`signature`] of the last report.
    last: String,
}

fn state_file(target: &CoreTarget) -> PathBuf {
    target.paths.root().join("agents").join("hook-state.json")
}

fn state_key(client: &str, project: &Path) -> String {
    format!("{client}|{}", project.display().to_string().to_lowercase())
}

fn load_states(path: &Path) -> BTreeMap<String, HookState> {
    let v: Value = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    v.as_object()
        .map(|o| {
            o.iter()
                .map(|(k, s)| {
                    let n = |f: &str| s[f].as_u64().unwrap_or(0);
                    let rounds = u32::try_from(n("rounds")).unwrap_or(0);
                    (
                        k.clone(),
                        HookState {
                            cursor: n("cursor"),
                            epoch: n("epoch"),
                            rounds,
                            last: s["last"].as_str().unwrap_or("").to_string(),
                        },
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn save_state(path: &Path, key: &str, s: HookState) {
    let mut all = load_states(path);
    all.insert(key.to_string(), s);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let v: Map<String, Value> = all
        .iter()
        .map(|(k, s)| {
            (
                k.clone(),
                json!({ "cursor": s.cursor, "epoch": s.epoch, "rounds": s.rounds, "last": s.last }),
            )
        })
        .collect();
    if let Ok(t) = serde_json::to_string_pretty(&v) {
        let _ = std::fs::write(path, t);
    }
}

// ---- the command --------------------------------------------------------------------------

/// `umr agent hook <client>`: reads the hook input on stdin, prints the verdict. Always exits 0.
pub async fn run(target: &CoreTarget, client: &str) -> ExitCode {
    let mut raw = String::new();
    let _ = std::io::stdin().take(1 << 20).read_to_string(&mut raw);
    let input: Value = serde_json::from_str(raw.trim_start_matches('\u{feff}')).unwrap_or_default();
    let verdict = match check(target, client, &input).await {
        Ok(v) => v,
        Err(e) => {
            // the agent must never be blocked by the hook itself
            eprintln!("umr agent hook: {e}");
            Verdict::Allow
        }
    };
    if let Some(text) = output(client, &verdict) {
        println!("{text}");
    }
    ExitCode::SUCCESS
}

async fn check(target: &CoreTarget, client: &str, input: &Value) -> Result<Verdict, String> {
    if !HOOK_CLIENTS.contains(&client) {
        return Err(format!(
            "no stop hook for '{client}' (only {})",
            HOOK_CLIENTS.join(", ")
        ));
    }
    let project = match project_from_input(input) {
        Some(p) => p,
        None => std::env::current_dir().map_err(|e| e.to_string())?,
    };
    let Ok(token) = std::fs::read_to_string(token_path(target, client)) else {
        return Ok(Verdict::Allow); // not connected through `umr agent setup`
    };
    let Some(mut core) = target.connect_as(token.trim()).await? else {
        return Ok(Verdict::Allow); // the Core is not running: nothing to check
    };
    let file = state_file(target);
    let key = state_key(client, &project);
    let state = load_states(&file).get(&key).cloned().unwrap_or_default();
    let r = core
        .call_raw(
            methods::AGENT_CHECK,
            json!({ "project": project.display().to_string(), "since": state.cursor,
                    "epoch": (state.epoch > 0).then_some(state.epoch),
                    "timeout_ms": (HOOK_TIMEOUT_S - 30) * 1000 }),
        )
        .await
        .map_err(|e| e.to_string())?;
    if r["status"] != "ok" {
        return Ok(Verdict::Allow); // not this folder's run, or still building at the timeout
    }
    let problems = r["problems"].as_array().cloned().unwrap_or_default();
    // AUD-P1-7: problems the user sent with "Send to agent" pass the loop guard
    let sent_by_user = r["flagged"].as_array().is_some_and(|f| !f.is_empty());
    let (verdict, rounds) = if sent_by_user && !problems.is_empty() {
        (Verdict::Continue(report(&problems)), 1)
    } else {
        decide(&problems, state.rounds, &state.last)
    };
    save_state(
        &file,
        &key,
        HookState {
            cursor: r["cursor"].as_u64().unwrap_or(state.cursor),
            epoch: r["epoch"].as_u64().unwrap_or(0),
            rounds,
            last: signature(&problems),
        },
    );
    Ok(verdict)
}

// ---- during the turn (AUD-P1-32) ------------------------------------------------------------

/// Seconds Claude Code gives the after-edit hook; it never waits for a build.
pub const AFTER_EDIT_TIMEOUT_S: u64 = 10;
/// The edit tools after which Claude Code runs the after-edit hook.
pub const EDIT_TOOLS: &str = "Edit|MultiEdit|Write|NotebookEdit";

/// Claude Code's way to add text to the agent's context after a tool call.
#[must_use]
pub fn after_edit_output(report: &str) -> String {
    json!({ "hookSpecificOutput": { "hookEventName": "PostToolUse", "additionalContext": report } })
        .to_string()
}

/// `umr agent hook claude-code --after-edit`: the stop hook only speaks when the agent ends its
/// turn; this one runs after each file edit and passes on problems the running app shows *now*
/// (from earlier saves). It does not wait for a build, does not count toward the stop hook's
/// rounds, leaves the fix feed alone (`probe`) and says each set of problems once.
pub async fn run_after_edit(target: &CoreTarget, client: &str) -> ExitCode {
    let mut raw = String::new();
    let _ = std::io::stdin().take(1 << 20).read_to_string(&mut raw);
    let input: Value = serde_json::from_str(raw.trim_start_matches('\u{feff}')).unwrap_or_default();
    match after_edit(target, client, &input).await {
        Ok(Some(report)) => println!("{}", after_edit_output(&report)),
        Ok(None) => {}
        Err(e) => eprintln!("umr agent hook: {e}"),
    }
    ExitCode::SUCCESS
}

async fn after_edit(
    target: &CoreTarget,
    client: &str,
    input: &Value,
) -> Result<Option<String>, String> {
    if client != "claude-code" {
        return Err(format!("--after-edit is for claude-code, not '{client}'"));
    }
    let project = match project_from_input(input) {
        Some(p) => p,
        None => std::env::current_dir().map_err(|e| e.to_string())?,
    };
    let Ok(token) = std::fs::read_to_string(token_path(target, client)) else {
        return Ok(None);
    };
    let Some(mut core) = target.connect_as(token.trim()).await? else {
        return Ok(None);
    };
    let file = state_file(target);
    let key = format!("{}#edit", state_key(client, &project));
    let seen = load_states(&file).get(&key).cloned().unwrap_or_default();
    let r = core
        .call_raw(
            methods::AGENT_CHECK,
            json!({ "project": project.display().to_string(), "since": 0,
                    "timeout_ms": 0, "probe": true }),
        )
        .await
        .map_err(|e| e.to_string())?;
    if r["status"] != "ok" {
        return Ok(None); // a build runs: its result comes at a later edit or the end of the turn
    }
    let problems = r["problems"].as_array().cloned().unwrap_or_default();
    let sig = signature(&problems);
    if problems.is_empty() || sig == seen.last {
        if problems.is_empty() && !seen.last.is_empty() {
            save_state(&file, &key, HookState::default());
        }
        return Ok(None);
    }
    save_state(
        &file,
        &key,
        HookState {
            last: sig,
            ..HookState::default()
        },
    );
    Ok(Some(format!(
        "While you work: the running app shows these problems from your earlier saves.\n{}",
        report(&problems)
    )))
}

/// The after-edit handler for Claude Code (`PostToolUse`, edit tools only).
#[must_use]
pub fn claude_after_edit_handler(exe: &str) -> Value {
    json!({ "type": "command", "command": exe,
            "args": ["agent", "hook", "claude-code", "--after-edit"],
            "timeout": AFTER_EDIT_TIMEOUT_S })
}

// ---- the hook entry in each client's config -------------------------------------------------

/// The hook handler for Claude Code (exec form: no shell, so the path needs no quoting).
#[must_use]
pub fn claude_handler(exe: &str) -> Value {
    json!({ "type": "command", "command": exe, "args": ["agent", "hook", "claude-code"],
            "timeout": HOOK_TIMEOUT_S })
}

/// The hook handler for Codex (a command line).
#[must_use]
pub fn codex_handler(exe: &str) -> Value {
    json!({ "type": "command", "command": format!("\"{exe}\" agent hook codex"),
            "timeout": HOOK_TIMEOUT_S })
}

/// The hook entry for Cursor (run through PowerShell on Windows: `&` calls a quoted path).
#[must_use]
pub fn cursor_entry(exe: &str) -> Value {
    json!({ "command": format!("& '{}' agent hook cursor", exe.replace('\'', "''")),
            "timeout": HOOK_TIMEOUT_S, "loop_limit": MAX_ROUNDS })
}

/// The Stop handler for Antigravity (a command line run through `cmd.exe` on Windows).
#[must_use]
pub fn antigravity_handler(exe: &str) -> Value {
    json!({ "type": "command", "command": format!("\"{exe}\" agent hook antigravity"),
            "timeout": HOOK_TIMEOUT_S })
}

/// A handler or entry written by setup (its command runs `agent hook`).
fn is_ours(h: &Value) -> bool {
    let in_args = h["args"]
        .as_array()
        .is_some_and(|a| a.len() >= 2 && a[0] == "agent" && a[1] == "hook");
    in_args
        || h["command"]
            .as_str()
            .is_some_and(|c| c.contains(" agent hook "))
}

fn parse(old: Option<&str>, path: &str) -> Result<Value, String> {
    let v: Value = match old {
        Some(t) if !t.trim().is_empty() => {
            serde_json::from_str(t.trim_start_matches('\u{feff}'))
                .map_err(|e| format!("{path} is not valid JSON ({e}); fix it first"))?
        }
        _ => json!({}),
    };
    if v.is_object() {
        Ok(v)
    } else {
        Err(format!("{path} is not a JSON object"))
    }
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default() + "\n"
}

/// The `hooks` object of `v` (created when missing).
fn hooks_of(v: &mut Value) -> &mut Map<String, Value> {
    if !v["hooks"].is_object() {
        v["hooks"] = json!({});
    }
    v["hooks"].as_object_mut().expect("just made an object")
}

/// Removes our handlers from `hooks.<event>`; drops what is left empty. Returns whether the
/// whole file is now empty (only `version` left for Cursor).
fn strip(v: &mut Value, event: &str, grouped: bool) -> bool {
    let hooks = hooks_of(v);
    if let Some(list) = hooks.get_mut(event).and_then(Value::as_array_mut) {
        if grouped {
            for g in list.iter_mut() {
                if let Some(hs) = g["hooks"].as_array_mut() {
                    hs.retain(|h| !is_ours(h));
                }
            }
            list.retain(|g| g["hooks"].as_array().is_none_or(|hs| !hs.is_empty()));
        } else {
            list.retain(|h| !is_ours(h));
        }
        if list.is_empty() {
            hooks.remove(event);
        }
    }
    if hooks.is_empty() {
        if let Some(o) = v.as_object_mut() {
            o.remove("hooks");
        }
    }
    v.as_object()
        .is_some_and(|o| o.keys().all(|k| k == "version"))
}

/// The config of `client` with our stop hook set (any older one replaced; the rest kept).
pub fn hooks_set(client: &str, old: Option<&str>, path: &str, exe: &str) -> Result<String, String> {
    let mut v = parse(old, path)?;
    if client == "antigravity" {
        strip_named(&mut v);
        if let Some(o) = v.as_object_mut() {
            o.insert("umr".into(), json!({ "Stop": [antigravity_handler(exe)] }));
        }
    } else if client == "cursor" {
        strip(&mut v, "stop", false);
        if v.get("version").is_none() {
            if let Some(o) = v.as_object_mut() {
                // `version` first, as in Cursor's own examples
                let mut n = Map::new();
                n.insert("version".into(), json!(1));
                n.extend(std::mem::take(o));
                *o = n;
            }
        }
        let hooks = hooks_of(&mut v);
        let list = hooks.entry("stop").or_insert_with(|| json!([]));
        if let Some(a) = list.as_array_mut() {
            a.push(cursor_entry(exe));
        }
    } else {
        strip(&mut v, "Stop", true);
        strip(&mut v, "PostToolUse", true);
        let handler = if client == "codex" {
            codex_handler(exe)
        } else {
            claude_handler(exe)
        };
        let hooks = hooks_of(&mut v);
        let list = hooks.entry("Stop").or_insert_with(|| json!([]));
        if let Some(a) = list.as_array_mut() {
            a.push(json!({ "hooks": [handler] }));
        }
        // AUD-P1-32: Claude Code also hears about problems during its turn, after each edit
        if client == "claude-code" {
            let list = hooks.entry("PostToolUse").or_insert_with(|| json!([]));
            if let Some(a) = list.as_array_mut() {
                a.push(json!({ "matcher": EDIT_TOOLS, "hooks": [claude_after_edit_handler(exe)] }));
            }
        }
    }
    Ok(pretty(&v))
}

/// Antigravity's `hooks.json`: names at the top level, each `{ "Stop": [handler, …], … }`.
/// Takes our handlers out of every entry and drops entries left with nothing; `true` when the
/// file is now empty.
fn strip_named(v: &mut Value) -> bool {
    let Some(o) = v.as_object_mut() else {
        return false;
    };
    for e in o.values_mut() {
        if let Some(list) = e.get_mut("Stop").and_then(Value::as_array_mut) {
            list.retain(|h| !is_ours(h));
            if list.is_empty() {
                if let Some(eo) = e.as_object_mut() {
                    eo.remove("Stop");
                }
            }
        }
    }
    o.retain(|_, e| {
        e.as_object()
            .is_none_or(|eo| eo.keys().any(|k| k != "enabled"))
    });
    o.is_empty()
}

/// The config without our stop hook; `None` when nothing else is left (the file was ours).
pub fn hooks_remove(client: &str, old: &str, path: &str) -> Result<Option<String>, String> {
    let mut v = parse(Some(old), path)?;
    if client == "antigravity" {
        return Ok(if strip_named(&mut v) {
            None
        } else {
            Some(pretty(&v))
        });
    }
    let (event, grouped) = if client == "cursor" {
        ("stop", false)
    } else {
        ("Stop", true)
    };
    if client == "claude-code" {
        strip(&mut v, "PostToolUse", true);
    }
    Ok(if strip(&mut v, event, grouped) {
        None
    } else {
        Some(pretty(&v))
    })
}

/// Whether `text` (Claude Code settings) has our after-edit hook (AUD-P1-32).
#[must_use]
pub fn has_after_edit_hook(text: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) else {
        return false;
    };
    v["hooks"]["PostToolUse"].as_array().is_some_and(|l| {
        l.iter().any(|g| {
            g["hooks"].as_array().is_some_and(|hs| {
                hs.iter().any(|h| {
                    is_ours(h)
                        && h["args"]
                            .as_array()
                            .is_some_and(|a| a.iter().any(|x| x == "--after-edit"))
                })
            })
        })
    })
}

/// Whether `text` (a hooks config) has our stop hook.
#[must_use]
pub fn has_hook(text: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) else {
        return false;
    };
    let hooks = &v["hooks"];
    // Antigravity: named entries at the top level, each with its own `Stop` list
    let named = v.as_object().is_some_and(|o| {
        o.values()
            .any(|e| e["Stop"].as_array().is_some_and(|l| l.iter().any(is_ours)))
    });
    let grouped = named
        || hooks["Stop"].as_array().is_some_and(|l| {
            l.iter().any(|g| {
                g["hooks"]
                    .as_array()
                    .is_some_and(|hs| hs.iter().any(is_ours))
            })
        });
    grouped
        || hooks["stop"]
            .as_array()
            .is_some_and(|l| l.iter().any(is_ours))
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str = r"C:\Users\u\AppData\Local\AI Mobile Runtime\umr.exe";

    fn build_problem() -> Value {
        json!({ "fp": "ab12", "category": "build", "at": "app/src/main/java/x/MainActivity.kt:12",
                "msg": "Unresolved reference 'foo'.", "count": 1 })
    }

    #[test]
    fn project_folder_comes_from_cwd_or_cursor_roots() {
        assert_eq!(
            project_from_input(&json!({ "cwd": r"D:\work\app" })),
            Some(PathBuf::from(r"D:\work\app"))
        );
        assert_eq!(
            project_from_input(&json!({ "workspace_roots": ["/d:/work/app"] })),
            Some(PathBuf::from(r"d:\work\app"))
        );
        assert_eq!(project_from_input(&json!({})), None);
    }

    #[test]
    fn problems_keep_the_agent_going_a_few_rounds_only() {
        let p = [build_problem()];
        let (v, r) = decide(&p, 0, "");
        let Verdict::Continue(text) = &v else {
            panic!("{v:?}")
        };
        assert!(text.contains("build error at app/src/main/java/x/MainActivity.kt:12"));
        assert!(text.contains("[fp ab12]") && text.contains("get_diagnostic"));
        assert_eq!(r, 1);
        assert_eq!(decide(&p, MAX_ROUNDS, ""), (Verdict::Allow, 0));
        assert_eq!(decide(&[], 2, ""), (Verdict::Allow, 0));
        // the agent left the code alone (declined, or cannot fix it): not asked again
        let same = signature(&p);
        assert_eq!(decide(&p, 1, &same), (Verdict::Allow, 0));
        // it tried, the rebuild failed again (count went up): asked again
        let mut again = build_problem();
        again["count"] = json!(2);
        assert!(matches!(decide(&[again], 1, &same).0, Verdict::Continue(_)));
    }

    #[test]
    fn each_client_gets_its_own_output() {
        let v = Verdict::Continue("fix it".into());
        let claude: Value = serde_json::from_str(&output("claude-code", &v).unwrap()).unwrap();
        assert_eq!(claude, json!({ "decision": "block", "reason": "fix it" }));
        let codex: Value = serde_json::from_str(&output("codex", &v).unwrap()).unwrap();
        assert_eq!(codex["decision"], "block");
        let cursor: Value = serde_json::from_str(&output("cursor", &v).unwrap()).unwrap();
        assert_eq!(cursor, json!({ "followup_message": "fix it" }));
        assert_eq!(output("claude-code", &Verdict::Allow), None);
        assert_eq!(output("cursor", &Verdict::Allow).as_deref(), Some("{}"));
    }

    #[test]
    fn claude_and_codex_hooks_are_added_replaced_and_removed() {
        let old = r#"{"permissions":{"allow":["Bash(ls)"]},"hooks":{"Stop":[{"hooks":[{"type":"command","command":"echo other"}]}]}}"#;
        for client in ["claude-code", "codex"] {
            let once = hooks_set(client, Some(old), "f", EXE).unwrap();
            let twice = hooks_set(client, Some(&once), "f", EXE).unwrap();
            assert_eq!(once, twice, "a re-run changes nothing");
            assert!(has_hook(&once));
            let v: Value = serde_json::from_str(&once).unwrap();
            assert_eq!(
                v["hooks"]["Stop"].as_array().unwrap().len(),
                2,
                "other hook kept"
            );
            assert_eq!(v["permissions"]["allow"][0], "Bash(ls)");
            let back = hooks_remove(client, &once, "f").unwrap().unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&back).unwrap(),
                serde_json::from_str::<Value>(old).unwrap()
            );
            // a file that held only our hook goes away
            let ours = hooks_set(client, None, "f", EXE).unwrap();
            assert_eq!(hooks_remove(client, &ours, "f").unwrap(), None);
        }
        let claude: Value =
            serde_json::from_str(&hooks_set("claude-code", None, "f", EXE).unwrap()).unwrap();
        let h = &claude["hooks"]["Stop"][0]["hooks"][0];
        assert_eq!(h["command"], EXE);
        assert_eq!(h["args"], json!(["agent", "hook", "claude-code"]));
        // AUD-P1-32: Claude Code also gets the after-edit hook; Codex has no such event
        let post = &claude["hooks"]["PostToolUse"][0];
        assert_eq!(post["matcher"], EDIT_TOOLS);
        assert_eq!(
            post["hooks"][0]["args"],
            json!(["agent", "hook", "claude-code", "--after-edit"])
        );
        assert!(has_after_edit_hook(&claude.to_string()));
        let out: Value = serde_json::from_str(&after_edit_output("fix x")).unwrap();
        assert_eq!(out["hookSpecificOutput"]["hookEventName"], "PostToolUse");
        assert_eq!(out["hookSpecificOutput"]["additionalContext"], "fix x");
        let codex: Value =
            serde_json::from_str(&hooks_set("codex", None, "f", EXE).unwrap()).unwrap();
        assert_eq!(
            codex["hooks"]["Stop"][0]["hooks"][0]["command"],
            format!("\"{EXE}\" agent hook codex")
        );
        assert!(codex["hooks"].get("PostToolUse").is_none());
    }

    #[test]
    fn cursor_hook_is_added_replaced_and_removed() {
        let once = hooks_set("cursor", None, "f", EXE).unwrap();
        assert!(
            once.trim_start().starts_with("{\n  \"version\": 1"),
            "{once}"
        );
        assert_eq!(hooks_set("cursor", Some(&once), "f", EXE).unwrap(), once);
        let v: Value = serde_json::from_str(&once).unwrap();
        let e = &v["hooks"]["stop"][0];
        assert_eq!(e["command"], format!("& '{EXE}' agent hook cursor"));
        assert_eq!(e["loop_limit"], MAX_ROUNDS);
        assert_eq!(hooks_remove("cursor", &once, "f").unwrap(), None);
        let other = r#"{"version":1,"hooks":{"afterFileEdit":[{"command":"fmt.ps1"}]}}"#;
        let with = hooks_set("cursor", Some(other), "f", EXE).unwrap();
        let back = hooks_remove("cursor", &with, "f").unwrap().unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&back).unwrap(),
            serde_json::from_str::<Value>(other).unwrap()
        );
        assert!(!has_hook(other));
    }

    #[test]
    fn antigravity_hook_is_added_replaced_and_removed() {
        let once = hooks_set("antigravity", None, "f", EXE).unwrap();
        assert_eq!(
            hooks_set("antigravity", Some(&once), "f", EXE).unwrap(),
            once
        );
        let v: Value = serde_json::from_str(&once).unwrap();
        let h = &v["umr"]["Stop"][0];
        assert_eq!(h["command"], format!("\"{EXE}\" agent hook antigravity"));
        assert_eq!(h["timeout"], HOOK_TIMEOUT_S);
        assert!(has_hook(&once));
        assert_eq!(hooks_remove("antigravity", &once, "f").unwrap(), None);
        // another tool's hooks, one of them sharing an entry with an old copy of ours
        let other = r#"{"lint":{"PostToolUse":[{"matcher":"","hooks":[{"command":"lint.cmd"}]}]},"mine":{"Stop":[{"command":"x.cmd"}]}}"#;
        let with = hooks_set("antigravity", Some(other), "f", EXE).unwrap();
        assert!(has_hook(&with));
        let back = hooks_remove("antigravity", &with, "f").unwrap().unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&back).unwrap(),
            serde_json::from_str::<Value>(other).unwrap()
        );
        assert!(!has_hook(other));
    }

    #[test]
    fn antigravity_gets_a_decision_and_its_workspace() {
        let stop: Value =
            serde_json::from_str(&output("antigravity", &Verdict::Allow).unwrap()).unwrap();
        assert_eq!(stop, json!({ "decision": "stop" }));
        let go: Value = serde_json::from_str(
            &output("antigravity", &Verdict::Continue("fix it".into())).unwrap(),
        )
        .unwrap();
        assert_eq!(go, json!({ "decision": "continue", "reason": "fix it" }));
        // as the Antigravity App sends it (checked live 2026-11-19): empty cwd, forward slashes
        assert_eq!(
            project_from_input(&json!({ "cwd": "", "workspacePaths": ["D:/work/app"] })),
            Some(PathBuf::from(r"D:\work\app"))
        );
        assert_eq!(
            project_from_input(&json!({ "cwd": "", "workspacePaths": [] })),
            None
        );
        assert_eq!(
            project_from_input(&json!({ "workspacePaths": ["file:///d%3A/work/my%20app"] })),
            Some(PathBuf::from(r"d:\work\my app"))
        );
    }
}
