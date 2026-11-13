#![allow(clippy::unused_async_trait_impl, clippy::needless_pass_by_value)]

use std::collections::VecDeque;

use serde_json::{json, Value};

use crate::{base64, tools, Core, Server, PROTOCOL_VERSIONS};

/// Records calls and answers from a queue.
#[derive(Default)]
struct Fake {
    calls: Vec<(String, Value)>,
    answers: VecDeque<Result<Value, String>>,
}

impl Core for Fake {
    async fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.calls.push((method.to_string(), params));
        self.answers.pop_front().unwrap_or(Ok(json!({})))
    }

    fn read_file(&self, _path: &str) -> Result<Vec<u8>, String> {
        Ok(b"\x89PNG".to_vec())
    }
}

fn server(answers: Vec<Result<Value, String>>) -> Server<Fake> {
    Server::new(Fake {
        calls: Vec::new(),
        answers: answers.into(),
    })
}

fn call(tool: &str, args: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": 9, "method": "tools/call", "params": { "name": tool, "arguments": args } })
}

#[tokio::test]
async fn initialize_negotiates_the_version_and_sends_instructions() {
    let mut s = server(vec![]);
    let r = s
        .handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": { "protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": { "name": "t", "version": "1" } } }))
        .await
        .unwrap();
    assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
    assert_eq!(r["result"]["serverInfo"]["name"], "umr");
    assert!(r["result"]["capabilities"]["tools"].is_object());
    assert!(r["result"]["instructions"]
        .as_str()
        .unwrap()
        .contains("runtime_status"));
    let r = s
        .handle(&json!({ "jsonrpc": "2.0", "id": 2, "method": "initialize", "params": { "protocolVersion": "1999-01-01" } }))
        .await
        .unwrap();
    assert_eq!(
        r["result"]["protocolVersion"], PROTOCOL_VERSIONS[0],
        "unknown version → ours"
    );
    // notifications get no answer; unknown methods an error
    assert!(s
        .handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
        .await
        .is_none());
    let r = s
        .handle(&json!({ "jsonrpc": "2.0", "id": 3, "method": "resources/list" }))
        .await
        .unwrap();
    assert_eq!(r["error"]["code"], -32601);
    let r = s
        .handle(&json!({ "jsonrpc": "2.0", "id": 4, "method": "ping" }))
        .await
        .unwrap();
    assert_eq!(r["result"], json!({}));
}

#[tokio::test]
async fn the_catalogue_is_well_formed() {
    let mut s = server(vec![]);
    let r = s
        .handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }))
        .await
        .unwrap();
    let list = r["result"]["tools"].as_array().unwrap();
    assert_eq!(list.len(), 16);
    for t in list {
        let name = t["name"].as_str().unwrap();
        assert!(
            name.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "{name}"
        );
        assert_eq!(t["inputSchema"]["type"], "object", "{name}");
        assert!(t["description"].as_str().unwrap().len() > 15, "{name}");
        assert!(t["annotations"]["readOnlyHint"].is_boolean());
    }
    let r = s.handle(&call("no_such_tool", json!({}))).await.unwrap();
    assert_eq!(r["error"]["code"], -32602);
}

#[tokio::test]
async fn runtime_status_keeps_its_cursor() {
    let mut s = server(vec![
        Ok(json!({ "cursor": "3.7", "hint": "x" })),
        Ok(json!({ "cursor": "4.9", "hint": "y" })),
        Ok(json!({ "cursor": "4.9", "hint": "z" })),
    ]);
    s.handle(&call("runtime_status", json!({}))).await.unwrap();
    let r = s.handle(&call("runtime_status", json!({}))).await.unwrap();
    assert_eq!(r["result"]["isError"], false);
    s.handle(&call("runtime_status", json!({ "reset": true })))
        .await
        .unwrap();
    let since: Vec<Value> = s
        .core
        .calls
        .iter()
        .map(|(_, p)| p["since"].clone())
        .collect();
    assert_eq!(since, [Value::Null, json!("3.7"), Value::Null]);
    assert_eq!(s.cursor.as_deref(), Some("4.9"));
}

#[tokio::test]
async fn tools_map_onto_core_methods() {
    let cases: Vec<(&str, Value, &str, Value)> = vec![
        (
            "tap",
            json!({"x": 10, "y": 20}),
            "device.input",
            json!({"action": "tap", "x": 10, "y": 20}),
        ),
        (
            "type_text",
            json!({"text": "hi"}),
            "device.input",
            json!({"action": "text", "text": "hi"}),
        ),
        (
            "press_key",
            json!({"key": "back"}),
            "device.input",
            json!({"action": "key", "key": "back"}),
        ),
        (
            "reload",
            json!({}),
            "project.reload",
            json!({"kind": "hot"}),
        ),
        (
            "reload",
            json!({"kind": "full"}),
            "project.reload",
            json!({"kind": "full"}),
        ),
        (
            "run_project",
            json!({"path": "C:/p"}),
            "project.run",
            json!({"path": "C:/p", "profile": null, "watch": true}),
        ),
        (
            "switch_device",
            json!({"profile": "large"}),
            "session.start",
            json!({"profile": "large"}),
        ),
        (
            "get_diagnostic",
            json!({"fingerprint": "ab"}),
            "diagnostics.get",
            json!({"fingerprint": "ab"}),
        ),
        ("stop_app", json!({}), "project.stop", json!({})),
    ];
    for (tool, args, method, params) in cases {
        let mut s = server(vec![]);
        s.handle(&call(tool, args)).await.unwrap();
        assert_eq!(s.core.calls[0], (method.to_string(), params), "{tool}");
    }
}

#[tokio::test]
async fn errors_and_missing_arguments_are_tool_errors() {
    let mut s = server(vec![Err("needs the user's approval".into())]);
    let r = s
        .handle(&call("run_project", json!({"path": "C:/p"})))
        .await
        .unwrap();
    assert_eq!(r["result"]["isError"], true);
    assert_eq!(
        r["result"]["content"][0]["text"],
        "needs the user's approval"
    );
    let r = s.handle(&call("tap", json!({"x": 1}))).await.unwrap();
    assert_eq!(r["result"]["isError"], true);
    assert!(r["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("'y'"));
}

#[tokio::test]
async fn screenshot_is_an_image_with_the_scale() {
    let mut s = server(vec![Ok(
        json!({ "path": "x.png", "width": 540, "height": 1200, "device_width": 1080, "device_height": 2400 }),
    )]);
    let r = s.handle(&call("screenshot", json!({}))).await.unwrap();
    let c = &r["result"]["content"];
    assert_eq!(c[0]["type"], "image");
    assert_eq!(c[0]["mimeType"], "image/png");
    assert_eq!(c[0]["data"], base64(b"\x89PNG"));
    assert!(c[1]["text"]
        .as_str()
        .unwrap()
        .contains("multiply image coordinates by 2.00"));
    assert_eq!(s.core.calls[0].1, json!({ "max_width": 540 }));
}

#[tokio::test]
async fn diagnostics_and_logs_are_compact() {
    let mut s = server(vec![
        Ok(
            json!({ "total": 1, "counts": {"build": 1}, "entries": [{ "fingerprint": "f1", "category": "build",
            "origin": "app", "file": "lib/main.dart", "line": 3, "message": "Expected ';'\nmore", "count": 2, "stack_frames": 0,
            "first_seen_ms": 1, "seq": 1 }] }),
        ),
        Ok(
            json!({ "lines": [{ "level": "E", "source": "logcat", "tag": "flutter", "text": "boom" }], "cursor": 1 }),
        ),
    ]);
    let r = s.handle(&call("get_diagnostics", json!({}))).await.unwrap();
    let t = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        t.contains("\"at\":\"lib/main.dart:3\"")
            && !t.contains("more")
            && !t.contains("first_seen"),
        "{t}"
    );
    let r = s.handle(&call("get_logs", json!({}))).await.unwrap();
    assert_eq!(r["result"]["content"][0]["text"], "E logcat flutter: boom");
}

#[test]
fn base64_matches_rfc_4648_vectors() {
    for (i, o) in [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ] {
        assert_eq!(base64(i.as_bytes()), o);
    }
    assert!(tools::exists("runtime_status") && !tools::exists("shell"));
}
