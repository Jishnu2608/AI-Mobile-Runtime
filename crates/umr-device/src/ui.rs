//! DEV-006: UI tree (`uiautomator dump`) in a compact form for agents, plus input that works on
//! any device through adb (`input tap|swipe|text|keyevent`). Emulators use gRPC for touch and
//! text ([`crate::grpc`]); adb is the fallback.

use std::time::Duration;

use serde::Serialize;

use crate::adb;

/// One meaningful node: it has text, a description, an id, or can be clicked / scrolled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UiNode {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub desc: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub id: String,
    /// Class without the package (`TextView`).
    pub class: String,
    /// `[left, top, right, bottom]` in device pixels.
    pub bounds: [i32; 4],
    pub center: [i32; 2],
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub clickable: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub scrollable: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub focused: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub package: String,
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&#10;", "\n")
        .replace("&amp;", "&")
}

/// Value of `name="…"` inside one tag.
fn attr<'a>(tag: &'a str, name: &str) -> &'a str {
    let key = format!(" {name}=\"");
    let Some(at) = tag.find(&key) else { return "" };
    let start = at + key.len();
    tag[start..]
        .find('"')
        .map_or("", |end| &tag[start..start + end])
}

/// `[x1,y1][x2,y2]`
fn parse_bounds(b: &str) -> Option<[i32; 4]> {
    let nums: Vec<i32> = b
        .split(|c: char| !c.is_ascii_digit() && c != '-')
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    (nums.len() == 4).then(|| [nums[0], nums[1], nums[2], nums[3]])
}

/// Parses `uiautomator dump` XML into the meaningful nodes, in document order.
#[must_use]
pub fn parse_dump(xml: &str) -> Vec<UiNode> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find("<node ") {
        let tail = &rest[at..];
        let end = tail.find('>').unwrap_or(tail.len());
        let tag = &tail[..end];
        rest = &tail[end.min(tail.len())..];
        let text = unescape(attr(tag, "text"));
        let desc = unescape(attr(tag, "content-desc"));
        let id = attr(tag, "resource-id").to_string();
        let clickable = attr(tag, "clickable") == "true";
        let scrollable = attr(tag, "scrollable") == "true";
        if text.is_empty() && desc.is_empty() && id.is_empty() && !clickable && !scrollable {
            continue;
        }
        let Some(b) = parse_bounds(attr(tag, "bounds")) else {
            continue;
        };
        let class = attr(tag, "class");
        out.push(UiNode {
            text,
            desc,
            id,
            class: class.rsplit('.').next().unwrap_or(class).to_string(),
            bounds: b,
            center: [i32::midpoint(b[0], b[2]), i32::midpoint(b[1], b[3])],
            clickable,
            scrollable,
            focused: attr(tag, "focused") == "true",
            package: attr(tag, "package").to_string(),
        });
    }
    out
}

const DUMP_PATH: &str = "/data/local/tmp/umr_ui.xml";

/// Dumps the current UI tree (1–3 s; fails while the screen animates, then retried once).
pub fn dump(port: u16, serial: &str) -> Result<Vec<UiNode>, String> {
    let mut last = String::new();
    for _ in 0..2 {
        let out = adb::shell_bytes(
            port,
            serial,
            &format!(
                "uiautomator dump {DUMP_PATH} >/dev/null 2>&1; cat {DUMP_PATH}; rm -f {DUMP_PATH}"
            ),
            Duration::from_secs(30),
        )
        .map_err(|e| e.to_string())?;
        let xml = String::from_utf8_lossy(&out).into_owned();
        if xml.contains("<hierarchy") {
            return Ok(parse_dump(&xml));
        }
        last = xml;
        std::thread::sleep(Duration::from_millis(500));
    }
    Err(format!(
        "uiautomator could not dump the screen: {}",
        last.trim().chars().take(200).collect::<String>()
    ))
}

/// Android keys by friendly name → `KEYCODE_*`.
#[must_use]
pub fn keycode(name: &str) -> Option<&'static str> {
    Some(match name.to_ascii_lowercase().as_str() {
        "home" => "KEYCODE_HOME",
        "back" => "KEYCODE_BACK",
        "recents" | "app_switch" => "KEYCODE_APP_SWITCH",
        "enter" => "KEYCODE_ENTER",
        "delete" | "backspace" => "KEYCODE_DEL",
        "tab" => "KEYCODE_TAB",
        "escape" | "esc" => "KEYCODE_ESCAPE",
        "menu" => "KEYCODE_MENU",
        "power" => "KEYCODE_POWER",
        "volume_up" => "KEYCODE_VOLUME_UP",
        "volume_down" => "KEYCODE_VOLUME_DOWN",
        "up" => "KEYCODE_DPAD_UP",
        "down" => "KEYCODE_DPAD_DOWN",
        "left" => "KEYCODE_DPAD_LEFT",
        "right" => "KEYCODE_DPAD_RIGHT",
        "wakeup" => "KEYCODE_WAKEUP",
        _ => return None,
    })
}

pub fn key(port: u16, serial: &str, name: &str) -> Result<(), String> {
    let code = keycode(name).ok_or_else(|| format!("unknown key '{name}'"))?;
    adb::shell(port, serial, &format!("input keyevent {code}"))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub fn tap(port: u16, serial: &str, x: i32, y: i32) -> Result<(), String> {
    adb::shell(port, serial, &format!("input tap {x} {y}"))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub fn swipe(
    port: u16,
    serial: &str,
    from: (i32, i32),
    to: (i32, i32),
    ms: u64,
) -> Result<(), String> {
    adb::shell(
        port,
        serial,
        &format!("input swipe {} {} {} {} {ms}", from.0, from.1, to.0, to.1),
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// `input text` needs spaces as `%s` and shell metacharacters escaped.
#[must_use]
pub fn shell_text(text: &str) -> String {
    let mut s = String::new();
    for c in text.chars() {
        match c {
            ' ' => s.push_str("%s"),
            c if c.is_ascii_alphanumeric() || "._-,@:/".contains(c) => s.push(c),
            c => {
                s.push('\\');
                s.push(c);
            }
        }
    }
    s
}

pub fn text(port: u16, serial: &str, text: &str) -> Result<(), String> {
    if text.chars().any(|c| !c.is_ascii() || c.is_ascii_control()) {
        return Err(
            "adb input supports printable ASCII only; use an emulator (gRPC) for other text".into(),
        );
    }
    adb::shell(port, serial, &format!("input text {}", shell_text(text)))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUMP: &str = r#"<?xml version='1.0' encoding='UTF-8' standalone='yes' ?><hierarchy rotation="0"><node index="0" text="" resource-id="" class="android.widget.FrameLayout" package="com.x" content-desc="" checkable="false" checked="false" clickable="false" enabled="true" focusable="false" focused="false" scrollable="false" long-clickable="false" password="false" selected="false" bounds="[0,0][1080,2400]"><node index="0" text="Hello &amp; welcome" resource-id="com.x:id/title" class="android.widget.TextView" package="com.x" content-desc="" checkable="false" checked="false" clickable="false" enabled="true" focusable="false" focused="false" scrollable="false" long-clickable="false" password="false" selected="false" bounds="[42,200][1038,290]" /><node index="1" text="" resource-id="" class="android.widget.Button" package="com.x" content-desc="Increment" checkable="false" checked="false" clickable="true" enabled="true" focusable="true" focused="true" scrollable="false" long-clickable="false" password="false" selected="false" bounds="[900,2100][1040,2240]" /></node></hierarchy>"#;

    #[test]
    fn compact_tree_keeps_meaningful_nodes() {
        let n = parse_dump(DUMP);
        assert_eq!(n.len(), 2, "plain layout containers are dropped");
        assert_eq!(n[0].text, "Hello & welcome");
        assert_eq!(n[0].id, "com.x:id/title");
        assert_eq!(n[0].class, "TextView");
        assert_eq!(n[0].center, [540, 245]);
        assert!(n[1].clickable && n[1].focused);
        assert_eq!(n[1].desc, "Increment");
        let json = serde_json::to_string(&n[0]).unwrap();
        assert!(
            !json.contains("clickable"),
            "false flags are left out: {json}"
        );
    }

    #[test]
    fn keys_and_text_escaping() {
        assert_eq!(keycode("Home"), Some("KEYCODE_HOME"));
        assert_eq!(keycode("recents"), Some("KEYCODE_APP_SWITCH"));
        assert_eq!(keycode("reboot"), None);
        assert_eq!(shell_text("hi there"), "hi%sthere");
        assert_eq!(shell_text("a;b&c"), "a\\;b\\&c");
        assert_eq!(shell_text("me@x.io"), "me@x.io");
    }

    #[test]
    fn bounds() {
        assert_eq!(parse_bounds("[0,10][20,30]"), Some([0, 10, 20, 30]));
        assert_eq!(parse_bounds("bogus"), None);
    }
}
