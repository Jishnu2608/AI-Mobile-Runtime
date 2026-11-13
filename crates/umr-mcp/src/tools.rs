//! MCP-002: the tool catalogue and how each tool maps onto the Core API.
//!
//! | Tool | Core method | Scope |
//! |---|---|---|
//! | `runtime_status` | `runtime.digest` (cursor kept by the server) | observe |
//! | `get_diagnostics`, `get_diagnostic` | `diagnostics.list`, `diagnostics.get` | observe |
//! | `get_logs` | `logs.tail` | observe |
//! | `screenshot`, `ui_tree`, `list_device_profiles` | `device.screenshot` (scaled), `device.ui`, `device.profiles` | observe |
//! | `tap`, `swipe`, `type_text`, `press_key`, `rotate` | `device.input`, `device.rotate` | interact |
//! | `reload`, `run_project`, `stop_app`, `switch_device` | `project.reload`, `project.run`, `project.stop`, `session.start` | control (+ user approval for new folders / device switches) |
//!
//! Results are compact text (and a PNG for `screenshot`); errors come back as tool errors
//! (`isError`) with the Core's message, so the agent can read and react.

use std::fmt::Write as _;

use serde_json::{json, Value};

use crate::{Core, Server};

struct Tool {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    schema: fn() -> Value,
    read_only: bool,
}

fn no_args() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

const TOOLS: &[Tool] = &[
    Tool {
        name: "runtime_status",
        title: "Runtime status",
        description: "What runs (device, app, alive) and what changed since your last call: problem counts by category, up to 3 new problems with their fp, the last event and a hint. ≤ 1 KB. Call after every edit.",
        schema: || json!({ "type": "object", "properties": {
            "reset": { "type": "boolean", "description": "Report everything, not only what changed since the last call." } } }),
        read_only: true,
    },
    Tool {
        name: "get_diagnostics",
        title: "Problems",
        description: "The current run's problems, classified (build, runtime, crash, environment, device, network) and deduplicated: fp, category, origin (app = your code, environment = toolchain/device/network), file:line, message, count.",
        schema: || json!({ "type": "object", "properties": {
            "category": { "type": "string", "enum": ["environment", "build", "runtime", "crash", "device", "network"] },
            "limit": { "type": "integer", "minimum": 1, "maximum": 50, "description": "Default 20 (most recently seen)." } } }),
        read_only: true,
    },
    Tool {
        name: "get_diagnostic",
        title: "Problem details",
        description: "One problem by its fp, with the full message and stack.",
        schema: || json!({ "type": "object", "properties": { "fingerprint": { "type": "string" } }, "required": ["fingerprint"] }),
        read_only: true,
    },
    Tool {
        name: "get_logs",
        title: "Logs",
        description: "Recent log lines: the app's device log (logcat), the build/framework tool's output (tool) and runtime progress (umr).",
        schema: || json!({ "type": "object", "properties": {
            "source": { "type": "string", "enum": ["logcat", "tool", "umr"] },
            "level": { "type": "string", "enum": ["V", "D", "I", "W", "E", "F"], "description": "Lowest level shown." },
            "grep": { "type": "string", "description": "Case-insensitive text or tag filter." },
            "limit": { "type": "integer", "minimum": 1, "maximum": 200, "description": "Default 50." } } }),
        read_only: true,
    },
    Tool {
        name: "screenshot",
        title: "Screenshot",
        description: "PNG of the device screen, scaled down (default 540 px wide). The text part gives the device size: tap/swipe take device pixels.",
        schema: || json!({ "type": "object", "properties": {
            "max_width": { "type": "integer", "minimum": 200, "maximum": 1440 } } }),
        read_only: true,
    },
    Tool {
        name: "ui_tree",
        title: "UI tree",
        description: "Visible UI elements of the current screen: class, text, id, description, centre and bounds in device pixels, clickable/scrollable. Takes 1–3 s. Use the centre for tap.",
        schema: no_args,
        read_only: true,
    },
    Tool {
        name: "list_device_profiles",
        title: "Device profiles",
        description: "Device sizes available for switch_device (phone, foldable, tablet …).",
        schema: no_args,
        read_only: true,
    },
    Tool {
        name: "tap",
        title: "Tap",
        description: "Tap at device pixels (x, y).",
        schema: || json!({ "type": "object", "properties": { "x": { "type": "integer" }, "y": { "type": "integer" } }, "required": ["x", "y"] }),
        read_only: false,
    },
    Tool {
        name: "swipe",
        title: "Swipe",
        description: "Swipe from (x, y) to (x2, y2) in device pixels.",
        schema: || json!({ "type": "object", "properties": {
            "x": { "type": "integer" }, "y": { "type": "integer" }, "x2": { "type": "integer" }, "y2": { "type": "integer" },
            "ms": { "type": "integer", "minimum": 50, "maximum": 5000, "description": "Duration, default 300." } },
            "required": ["x", "y", "x2", "y2"] }),
        read_only: false,
    },
    Tool {
        name: "type_text",
        title: "Type text",
        description: "Type text into the focused field.",
        schema: || json!({ "type": "object", "properties": { "text": { "type": "string" } }, "required": ["text"] }),
        read_only: false,
    },
    Tool {
        name: "press_key",
        title: "Press key",
        description: "Press a device key: back, home, recents, enter, delete, arrows …",
        schema: || json!({ "type": "object", "properties": { "key": { "type": "string", "enum": [
            "back", "home", "recents", "enter", "delete", "tab", "escape", "menu", "up", "down", "left", "right"] } },
            "required": ["key"] }),
        read_only: false,
    },
    Tool {
        name: "rotate",
        title: "Rotate",
        description: "Rotate the device by 90°.",
        schema: no_args,
        read_only: false,
    },
    Tool {
        name: "reload",
        title: "Reload",
        description: "Apply code changes: hot (keep state; Flutter / React Native), restart (restart the app's state) or full (rebuild, reinstall, relaunch — needed for Kotlin/Java and native changes).",
        schema: || json!({ "type": "object", "properties": { "kind": { "type": "string", "enum": ["hot", "restart", "full"] } } }),
        read_only: false,
    },
    Tool {
        name: "run_project",
        title: "Run project",
        description: "Build, install and launch a project folder on the device (stops what runs). Takes up to a few minutes. A folder the user has not granted yet waits for their approval.",
        schema: || json!({ "type": "object", "properties": {
            "path": { "type": "string", "description": "Absolute project folder." },
            "profile": { "type": "string", "description": "Device profile; default: the current one." } },
            "required": ["path"] }),
        read_only: false,
    },
    Tool {
        name: "stop_app",
        title: "Stop app",
        description: "Stop the running app and its build tools (the device keeps running).",
        schema: no_args,
        read_only: false,
    },
    Tool {
        name: "switch_device",
        title: "Switch device",
        description: "Switch to (and boot) another device profile, keeping the session. Waits for the user's approval.",
        schema: || json!({ "type": "object", "properties": { "profile": { "type": "string" } }, "required": ["profile"] }),
        read_only: false,
    },
];

/// `tools/list` entries.
#[must_use]
pub fn catalogue() -> Vec<Value> {
    TOOLS
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "title": t.title,
                "description": t.description,
                "inputSchema": (t.schema)(),
                "annotations": { "title": t.title, "readOnlyHint": t.read_only,
                    "destructiveHint": false, "openWorldHint": false },
            })
        })
        .collect()
}

#[must_use]
pub fn exists(name: &str) -> bool {
    TOOLS.iter().any(|t| t.name == name)
}

#[must_use]
pub fn text(s: &str) -> Value {
    json!({ "type": "text", "text": s })
}

fn compact(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_default()
}

fn int(args: &Value, k: &str) -> Result<i64, String> {
    args[k]
        .as_i64()
        .ok_or_else(|| format!("'{k}' (integer) is required"))
}

fn first_line(s: &str, n: usize) -> String {
    let l = s.lines().next().unwrap_or("");
    if l.chars().count() > n {
        format!("{}…", l.chars().take(n).collect::<String>())
    } else {
        l.to_string()
    }
}

fn location(e: &Value) -> Option<String> {
    match (e["file"].as_str(), e["line"].as_u64()) {
        (Some(f), Some(l)) => Some(format!("{f}:{l}")),
        (Some(f), None) => Some(f.to_string()),
        _ => None,
    }
}

fn problems_brief(list: &Value, n: usize) -> Vec<Value> {
    list.as_array()
        .into_iter()
        .flatten()
        .take(n)
        .map(|p| json!({ "at": location(p), "msg": first_line(p["message"].as_str().unwrap_or(""), 160), "source": p["source"] }))
        .collect()
}

/// Runs one tool.
#[allow(clippy::too_many_lines, clippy::many_single_char_names)] // one flat match over the catalogue
pub async fn run<C: Core>(
    s: &mut Server<C>,
    name: &str,
    args: &Value,
) -> Result<Vec<Value>, String> {
    match name {
        "runtime_status" => {
            let since = if args["reset"].as_bool() == Some(true) {
                None
            } else {
                s.cursor.clone()
            };
            let d = s
                .core
                .call("runtime.digest", json!({ "since": since }))
                .await?;
            s.cursor = d["cursor"].as_str().map(String::from);
            Ok(vec![text(&compact(&d))])
        }
        "get_diagnostics" => {
            let limit = args["limit"].as_u64().unwrap_or(20).clamp(1, 50);
            let v = s
                .core
                .call(
                    "diagnostics.list",
                    json!({ "category": args["category"].as_str(), "limit": limit }),
                )
                .await?;
            let entries: Vec<Value> = v["entries"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|e| {
                    json!({ "fp": e["fingerprint"], "category": e["category"], "origin": e["origin"],
                        "at": location(e), "msg": first_line(e["message"].as_str().unwrap_or(""), 240),
                        "count": e["count"], "stack_frames": e["stack_frames"] })
                })
                .collect();
            Ok(vec![text(&compact(
                &json!({ "total": v["total"], "by_category": v["counts"], "problems": entries }),
            ))])
        }
        "get_diagnostic" => {
            let fp = args["fingerprint"]
                .as_str()
                .ok_or("'fingerprint' is required")?;
            let e = s
                .core
                .call("diagnostics.get", json!({ "fingerprint": fp }))
                .await?;
            let mut out = format!(
                "{} · origin {} · {} · seen {}×\n",
                e["category"].as_str().unwrap_or("?"),
                e["origin"].as_str().unwrap_or("?"),
                e["source"].as_str().unwrap_or("?"),
                e["count"]
            );
            if let Some(at) = location(&e) {
                let _ = writeln!(out, "at {at}");
            }
            out.push_str(e["message"].as_str().unwrap_or(""));
            let frames: Vec<&str> = e["stack"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            for f in frames.iter().take(40) {
                let _ = write!(out, "\n    {f}");
            }
            if frames.len() > 40 {
                let _ = write!(out, "\n    … {} more frames", frames.len() - 40);
            }
            Ok(vec![text(&out)])
        }
        "get_logs" => {
            let limit = args["limit"].as_u64().unwrap_or(50).clamp(1, 200);
            let v = s.core
                .call(
                    "logs.tail",
                    json!({ "limit": limit, "source": args["source"].as_str(), "level": args["level"].as_str(), "grep": args["grep"].as_str() }),
                )
                .await?;
            let lines: Vec<String> = v["lines"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|l| {
                    format!(
                        "{} {} {}{}",
                        l["level"].as_str().unwrap_or("I"),
                        l["source"].as_str().unwrap_or(""),
                        l["tag"]
                            .as_str()
                            .map_or_else(String::new, |t| format!("{t}: ")),
                        first_line(l["text"].as_str().unwrap_or(""), 400)
                    )
                })
                .collect();
            Ok(vec![text(&if lines.is_empty() {
                "no log lines".to_string()
            } else {
                lines.join("\n")
            })])
        }
        "screenshot" => {
            let max = args["max_width"].as_u64().unwrap_or(540).clamp(200, 1440);
            let v = s
                .core
                .call("device.screenshot", json!({ "max_width": max }))
                .await?;
            let path = v["path"]
                .as_str()
                .ok_or("the Core returned no screenshot path")?;
            let png = s.core.read_file(path)?;
            let (w, h) = (
                v["width"].as_u64().unwrap_or(0),
                v["height"].as_u64().unwrap_or(0),
            );
            let note = match (v["device_width"].as_u64(), v["device_height"].as_u64()) {
                (Some(dw), Some(dh)) if w > 0 => {
                    #[allow(clippy::cast_precision_loss)]
                    let f = dw as f64 / w as f64;
                    format!("Screenshot {w}×{h} of a {dw}×{dh} screen. tap/swipe take device pixels: multiply image coordinates by {f:.2} (or use ui_tree centres).")
                }
                _ => format!("Screenshot {w}×{h} (device pixels)."),
            };
            Ok(vec![
                json!({ "type": "image", "data": crate::base64(&png), "mimeType": "image/png" }),
                text(&note),
            ])
        }
        "ui_tree" => {
            let v = s.core.call("device.ui", json!({})).await?;
            let nodes: Vec<String> = v["nodes"]
                .as_array()
                .into_iter()
                .flatten()
                .take(150)
                .map(|n| {
                    let mut l = format!(
                        "[{},{}] {}",
                        n["center"][0],
                        n["center"][1],
                        n["class"].as_str().unwrap_or("?")
                    );
                    for (k, f) in [("text", "\"{}\""), ("id", "#{}"), ("desc", "({})")] {
                        if let Some(x) = n[k].as_str().filter(|x| !x.is_empty()) {
                            l.push(' ');
                            l.push_str(&f.replace("{}", &first_line(x, 80)));
                        }
                    }
                    if n["clickable"] == true {
                        l.push_str(" clickable");
                    }
                    if n["scrollable"] == true {
                        l.push_str(" scrollable");
                    }
                    l
                })
                .collect();
            Ok(vec![text(&format!(
                "{} elements (centre in device pixels):\n{}",
                v["count"],
                nodes.join("\n")
            ))])
        }
        "list_device_profiles" => {
            let v = s.core.call("device.profiles", json!({})).await?;
            let p: Vec<Value> = v["profiles"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|p| json!({ "profile": p["profile"]["id"], "class": p["profile"]["class"], "exists": !p["avd"].is_null() }))
                .collect();
            Ok(vec![text(&compact(&json!({ "profiles": p })))])
        }
        "tap" => {
            let (x, y) = (int(args, "x")?, int(args, "y")?);
            s.core
                .call("device.input", json!({ "action": "tap", "x": x, "y": y }))
                .await?;
            Ok(vec![text(&format!("tapped {x},{y}"))])
        }
        "swipe" => {
            let p = json!({ "action": "swipe", "x": int(args, "x")?, "y": int(args, "y")?,
                "x2": int(args, "x2")?, "y2": int(args, "y2")?, "ms": args["ms"].as_u64() });
            s.core.call("device.input", p).await?;
            Ok(vec![text("swiped")])
        }
        "type_text" => {
            let t = args["text"].as_str().ok_or("'text' is required")?;
            s.core
                .call("device.input", json!({ "action": "text", "text": t }))
                .await?;
            Ok(vec![text("typed")])
        }
        "press_key" => {
            let k = args["key"].as_str().ok_or("'key' is required")?;
            s.core
                .call("device.input", json!({ "action": "key", "key": k }))
                .await?;
            Ok(vec![text(&format!("pressed {k}"))])
        }
        "rotate" => {
            let v = s.core.call("device.rotate", json!({})).await?;
            Ok(vec![text(&compact(&v))])
        }
        "reload" => {
            let kind = args["kind"].as_str().unwrap_or("hot");
            let v = s
                .core
                .call("project.reload", json!({ "kind": kind }))
                .await?;
            let mut r = json!({ "status": v["status"], "kind": kind, "ms": v["ms"] });
            if let Some(e) = v["error"].as_str() {
                r["error"] = json!(first_line(e, 300));
                r["problems"] = json!(problems_brief(&v["problems"], 5));
            }
            Ok(vec![text(&compact(&r))])
        }
        "run_project" => {
            let path = args["path"].as_str().ok_or("'path' is required")?;
            let v = s
                .core
                .call(
                    "project.run",
                    json!({ "path": path, "profile": args["profile"].as_str(), "watch": true }),
                )
                .await?;
            let mut r = json!({ "status": v["status"], "adapter": v["adapter"], "package": v["package"],
                "device": v["serial"], "seconds": v["seconds"], "watching": v["watching"] });
            if let Some(e) = v["error"].as_str() {
                r["error"] = json!(first_line(e, 300));
                r["problems"] = json!(problems_brief(&v["problems"], 5));
            }
            Ok(vec![text(&compact(&r))])
        }
        "stop_app" => {
            let v = s.core.call("project.stop", json!({})).await?;
            Ok(vec![text(if v["stopped"].is_null() {
                "nothing was running"
            } else {
                "stopped"
            })])
        }
        "switch_device" => {
            let p = args["profile"].as_str().ok_or("'profile' is required")?;
            let v = s
                .core
                .call("session.start", json!({ "profile": p }))
                .await?;
            let s = &v["session"];
            Ok(vec![text(&compact(
                &json!({ "profile": s["profile"], "state": s["state"], "device": s["serial"], "app": s["app"] }),
            ))])
        }
        other => Err(format!("unknown tool '{other}'")),
    }
}
