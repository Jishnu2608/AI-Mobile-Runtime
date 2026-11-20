//! React Native JavaScript errors from the app's device log (AUD-P0-2).
//!
//! Since React Native 0.77 the app's `console.*` output no longer reaches Metro's terminal, so
//! a render error or an uncaught exception in a handler shows up only in the device log, under
//! the `ReactNativeJS` tag at level `E`. Logcat splits one multi-line message into several
//! entries with the same timestamp; the collector joins them and hands the text here.
//!
//! Shapes seen on React Native 0.88 (bridgeless, Hermes):
//!
//! ```text
//! { [Error: audit: runtime error in render]
//!   componentStack: '\n    at App (http://10.0.2.2:8081/App.bundle//&platform=android…:92:40)\n    at RCTView (<anonymous>)',
//!   isComponentError: true }
//! ```
//! ```text
//! TypeError: Cannot read property 'length' of undefined
//!
//! This error is located at:
//!     in Calculator (created by App)
//!     in App
//! ```
//! ```text
//! Warning: Each child in a list should have a unique "key" prop.
//! ```

use serde::Serialize;

/// One JavaScript error the app logged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JsError {
    /// `Error: message`, `TypeError: message`, or the logged text's first line.
    pub message: String,
    /// React's own warnings (`Warning: …`) are not errors.
    pub warning: bool,
    /// Component names from the component stack, innermost first (`App`, `Calculator`).
    pub components: Vec<String>,
    /// The raw component stack / location lines, as logged.
    pub stack: Vec<String>,
    /// Source location after symbolication through Metro ([`bundle_frames`],
    /// [`parse_symbolicated`]): project-relative path, 1-based line and column.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
}

/// A stack frame at a position in a Metro bundle: what Metro's `/symbolicate` maps to source.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JsFrame {
    pub method: String,
    pub url: String,
    pub line: u32,
    pub column: u32,
}

/// Components React Native itself renders around the app's own (never the app's code).
const HOST_COMPONENTS: &[&str] = &[
    "RCTView",
    "View",
    "View_withRef",
    "AppContainer",
    "RCTText",
    "Text",
    "ScrollView",
    "RCTScrollView",
    "LogBoxStateSubscription",
    "ErrorToastContainer",
];

fn is_host(name: &str) -> bool {
    HOST_COMPONENTS.contains(&name) || name.contains("(RootComponent)") || name.starts_with("RCT")
}

/// `at App (http://…)`, `in App (created by X)`, `in App` → `App`.
fn component_of(frame: &str) -> Option<String> {
    let f = frame.trim();
    let rest = f.strip_prefix("at ").or_else(|| f.strip_prefix("in "))?;
    if rest.contains("(RootComponent)") {
        return None; // the registered root: React Native's wrapper, not a component in the code
    }
    let name = rest.split([' ', '(']).next()?.trim();
    let ok = !name.is_empty()
        && name.chars().next().is_some_and(char::is_alphabetic)
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '$' || c == '.');
    (ok && !is_host(name)).then(|| name.to_string())
}

/// The headline: `{ [Error: x]` → `Error: x`; otherwise the first line. `None` for text that is
/// only the tail of a message split across log reads (`  isComponentError: true }`).
fn headline(text: &str) -> Option<String> {
    let first = text.lines().next()?;
    if first.starts_with(char::is_whitespace) || first.starts_with('}') {
        return None;
    }
    let t = first.trim();
    if let Some(inner) = t.strip_prefix("{ [") {
        let end = inner.rfind(']').unwrap_or(inner.len());
        let h = inner[..end].trim();
        return (!h.is_empty()).then(|| h.to_string());
    }
    let lower = t.to_ascii_lowercase();
    // property lines of an object that started in an earlier read
    if ["componentstack:", "iscomponenterror:", "stack:", "message:"]
        .iter()
        .any(|p| lower.starts_with(p))
    {
        return None;
    }
    (!t.is_empty()).then(|| t.to_string())
}

/// Parses one `ReactNativeJS` error-level message (its logcat entries joined with newlines).
#[must_use]
pub fn parse_js_error(text: &str) -> Option<JsError> {
    let message = headline(text)?;
    // the component stack: either escaped inside `componentStack: '…'` or real lines
    let unescaped = text.replace("\\n", "\n");
    let stack: Vec<String> = unescaped
        .lines()
        .skip(1)
        .map(str::trim)
        .filter(|l| l.starts_with("at ") || l.starts_with("in "))
        .map(|l| l.trim_end_matches(['\'', ',']).to_string())
        .collect();
    let mut components: Vec<String> = Vec::new();
    for c in stack.iter().filter_map(|f| component_of(f)) {
        if !components.contains(&c) {
            components.push(c);
        }
    }
    let warning = message.starts_with("Warning:");
    Some(JsError {
        message,
        warning,
        components,
        stack,
        file: None,
        line: None,
        column: None,
    })
}

/// The frames of the app's own components that point into a Metro bundle, innermost first:
/// `at App (http://10.0.2.2:8081/App.bundle//&platform=android…:92:40)`. Host components and
/// the registered root are left out (they are React Native's, and their frames point into the
/// whole `index.bundle`, which Metro would have to build again to map).
#[must_use]
pub fn bundle_frames(e: &JsError) -> Vec<JsFrame> {
    e.stack
        .iter()
        .filter_map(|f| {
            let method = component_of(f)?;
            let open = f.find("(http")?;
            let inner = f[open + 1..].trim_end().strip_suffix(')')?;
            let (rest, col) = inner.rsplit_once(':')?;
            let (url, line) = rest.rsplit_once(':')?;
            if !url.contains(".bundle") {
                return None;
            }
            Some(JsFrame {
                method,
                url: url.to_string(),
                line: line.parse().ok()?,
                column: col.parse().ok()?,
            })
        })
        .collect()
}

/// Metro's port in a bundle URL (`http://10.0.2.2:8081/…` → 8081). On the PC Metro listens on
/// the same port on localhost (the device reaches it through the emulator's host alias or
/// `adb reverse`).
#[must_use]
pub fn metro_port(url: &str) -> Option<u16> {
    let host = url.strip_prefix("http://")?.split('/').next()?;
    host.rsplit_once(':')?.1.parse().ok()
}

/// The `/symbolicate` request body for one frame.
#[must_use]
pub fn symbolicate_body(f: &JsFrame) -> String {
    serde_json::json!({ "stack": [{
        "file": f.url, "lineNumber": f.line, "column": f.column, "methodName": f.method
    }] })
    .to_string()
}

/// Metro's `/symbolicate` answer → the first frame's source file (absolute, as Metro gives it),
/// line and column.
#[must_use]
pub fn parse_symbolicated(body: &str) -> Option<(String, u32, Option<u32>)> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let f = v["stack"].as_array()?.first()?;
    let file = f["file"].as_str()?;
    // an unmapped frame comes back unchanged
    if file.starts_with("http") {
        return None;
    }
    let line = u32::try_from(f["lineNumber"].as_u64()?).ok()?;
    let column = f["column"]
        .as_u64()
        .and_then(|c| u32::try_from(c).ok())
        .map(|c| c + 1); // Metro's column is 0-based
    Some((file.to_string(), line, column))
}

/// Joins logcat lines into messages: consecutive `ReactNativeJS` error lines with the same
/// timestamp are one message. Input: `(epoch ms, level, tag, text)`.
#[must_use]
pub fn group_js_errors(lines: &[(u64, char, String, String)]) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    let mut cur_ms = 0;
    for (ms, level, tag, text) in lines {
        let js = tag == "ReactNativeJS" && matches!(level, 'E' | 'F');
        if js && !cur.is_empty() && *ms == cur_ms {
            cur.push(text);
            continue;
        }
        if !cur.is_empty() {
            out.push(cur.join("\n"));
            cur.clear();
        }
        if js {
            cur.push(text);
            cur_ms = *ms;
        }
    }
    if !cur.is_empty() {
        out.push(cur.join("\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Captured live on React Native 0.88 (Hermes, bridgeless) during the 2026-11-16 audit:
    /// `throw new Error(...)` in `App()`. The real log repeats the bundle URL arguments.
    const LIVE_RENDER_ERROR: &str = "{ [Error: audit: runtime error in render]\n  componentStack: '\\n    at App (http://10.0.2.2:8081/App.bundle//&platform=android&dev=true&lazy=true&minify=false&app=com.tumr.samples.rncalc&modulesOnly=true&runModule=true&sourcePaths=url-server&shallow=true:92:40)\\n    at RCTView (<anonymous>)\\n    at View_withRef (address at http://10.0.2.2:8081/index.bundle?platform=android:1:302458)\\n    at RCTView (<anonymous>)\\n    at AppContainer (http://10.0.2.2:8081/index.bundle//&platform=android:53073:22)\\n    at CalculatorRN(RootComponent) (http://10.0.2.2:8081/index.bundle//&platform=android:69474:34)',\n  isComponentError: true }";

    #[test]
    fn live_render_error_is_parsed() {
        let e = parse_js_error(LIVE_RENDER_ERROR).unwrap();
        assert_eq!(e.message, "Error: audit: runtime error in render");
        assert!(!e.warning);
        assert_eq!(e.components, ["App"]);
        assert_eq!(e.stack.len(), 6);
        assert!(e.stack[0].starts_with("at App (http://10.0.2.2:8081/App.bundle"));
    }

    #[test]
    fn located_at_format_and_type_errors() {
        let text = "TypeError: Cannot read property 'length' of undefined\n\nThis error is located at:\n    in Keypad (created by App)\n    in RCTView\n    in App";
        let e = parse_js_error(text).unwrap();
        assert_eq!(
            e.message,
            "TypeError: Cannot read property 'length' of undefined"
        );
        assert_eq!(e.components, ["Keypad", "App"]);
    }

    #[test]
    fn react_warnings_are_marked() {
        let e = parse_js_error("Warning: Each child in a list should have a unique \"key\" prop.")
            .unwrap();
        assert!(e.warning);
        assert_eq!(e.components, Vec::<String>::new());
    }

    #[test]
    fn tails_of_split_messages_are_not_errors() {
        assert_eq!(parse_js_error("  isComponentError: true }"), None);
        assert_eq!(parse_js_error("componentStack: '\\n at App'"), None);
        assert_eq!(parse_js_error(""), None);
    }

    #[test]
    fn logcat_entries_with_one_timestamp_form_one_message() {
        let l = |ms, lv, tag: &str, t: &str| (ms, lv, tag.to_string(), t.to_string());
        let lines = vec![
            l(1, 'I', "ReactNativeJS", "Running \"CalculatorRN\""),
            l(5, 'E', "ReactNativeJS", "{ [Error: boom]"),
            l(5, 'E', "ReactNativeJS", "  isComponentError: true }"),
            l(5, 'D', "FlutterJNI", "noise"),
            l(9, 'E', "ReactNativeJS", "TypeError: x is not a function"),
            l(12, 'E', "ReactNativeJS", "Warning: something"),
        ];
        assert_eq!(
            group_js_errors(&lines),
            [
                "{ [Error: boom]\n  isComponentError: true }",
                "TypeError: x is not a function",
                "Warning: something"
            ]
        );
    }

    #[test]
    fn js_errors_become_runtime_problems_in_the_apps_code() {
        use crate::{from_js_error, Category, Origin, Severity};
        let d = from_js_error(&parse_js_error(LIVE_RENDER_ERROR).unwrap());
        assert_eq!(d.category, Category::Runtime);
        assert_eq!(d.origin, Origin::App);
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.source, "react-native");
        assert_eq!(
            d.message,
            "Error: audit: runtime error in render (in component <App>)"
        );
        let w = from_js_error(&parse_js_error("Warning: key prop").unwrap());
        assert_eq!((w.severity, w.origin), (Severity::Warning, Origin::Unknown));
        // a JS error that is really a network failure keeps its network category
        let n = from_js_error(&parse_js_error("TypeError: Network request failed").unwrap());
        assert_eq!(n.category, Category::Network);
    }

    /// Metro's real answer for the live render error's `App` frame (2026-11-16).
    const LIVE_SYMBOLICATED: &str = r#"{"codeFrame":{"content":"…","location":{"row":80,"column":38},"fileName":"D:\\Projects\\Mobile Runtime Companion\\sample-app\\sample-react-native\\App.tsx"},"stack":[{"file":"D:\\Projects\\Mobile Runtime Companion\\sample-app\\sample-react-native\\App.tsx","lineNumber":80,"column":38,"methodName":"App","collapse":false}]}"#;

    #[test]
    fn only_the_apps_own_bundle_frames_are_sent_to_metro() {
        let e = parse_js_error(LIVE_RENDER_ERROR).unwrap();
        let frames = bundle_frames(&e);
        assert_eq!(frames.len(), 1, "host components and the root are left out");
        assert_eq!(frames[0].method, "App");
        assert_eq!((frames[0].line, frames[0].column), (92, 40));
        assert!(frames[0]
            .url
            .starts_with("http://10.0.2.2:8081/App.bundle//&platform=android"));
        assert_eq!(metro_port(&frames[0].url), Some(8081));
        assert_eq!(
            metro_port("http://localhost:19000/index.bundle"),
            Some(19000)
        );
        assert_eq!(metro_port("address at x"), None);
        let body: serde_json::Value = serde_json::from_str(&symbolicate_body(&frames[0])).unwrap();
        assert_eq!(body["stack"][0]["lineNumber"], 92);
        assert_eq!(body["stack"][0]["methodName"], "App");
        // the old "located at" format has no positions: nothing to map
        let old = parse_js_error("TypeError: x\n    in Keypad (created by App)").unwrap();
        assert_eq!(bundle_frames(&old), Vec::<JsFrame>::new());
    }

    #[test]
    fn metros_answer_gives_file_line_and_1_based_column() {
        assert_eq!(
            parse_symbolicated(LIVE_SYMBOLICATED),
            Some((
                r"D:\Projects\Mobile Runtime Companion\sample-app\sample-react-native\App.tsx"
                    .into(),
                80,
                Some(39)
            ))
        );
        // an unmapped frame comes back as the bundle URL; garbage is no answer
        assert_eq!(
            parse_symbolicated(
                r#"{"stack":[{"file":"http://x/index.bundle","lineNumber":1,"column":2}]}"#
            ),
            None
        );
        assert_eq!(parse_symbolicated("<html>"), None);
    }

    #[test]
    fn a_mapped_error_points_at_the_source_line() {
        use crate::{from_js_error, Origin};
        let mut e = parse_js_error(LIVE_RENDER_ERROR).unwrap();
        e.file = Some("App.tsx".into());
        e.line = Some(80);
        e.column = Some(39);
        let d = from_js_error(&e);
        assert_eq!(
            (d.file.as_deref(), d.line, d.column),
            (Some("App.tsx"), Some(80), Some(39))
        );
        assert_eq!(d.origin, Origin::App);
    }
}
