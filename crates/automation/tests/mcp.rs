//! The MCP handler's wire behaviour: one line in, one line out, answered without a window.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use wavcraft_automation::headless::Headless;
use wavcraft_automation::mcp::handle;
use wavcraft_automation::{Backend, Request};

/// A backend that answers nothing. The six tests above `tools/call` never reach it: they send
/// `initialize`, `tools/list`, a notification, bad JSON, an unknown method or a bad id, none of
/// which runs a request.
struct Fake;

impl Backend for Fake {
    fn execute(&mut self, _req: &Request) -> Result<String, String> {
        Err("the fake backend answers nothing".to_string())
    }
}

/// Send one line and parse the reply, which must be there.
fn reply(line: &str) -> Value {
    let out = handle(line, &mut Fake).expect("a reply");
    serde_json::from_str(&out).expect("the reply is JSON")
}

#[test]
fn initialize_answers_as_mcp_expects() {
    let req = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}}});
    let res = reply(&req.to_string());
    assert_eq!(res["jsonrpc"], "2.0");
    assert_eq!(res["id"], 1);
    assert_eq!(res["result"]["protocolVersion"], "2025-06-18");
    assert!(res["result"]["capabilities"]["tools"].is_object());
    assert_eq!(res["result"]["serverInfo"]["name"], "wavcraft");
}

#[test]
fn tools_list_names_the_seven_tools() {
    let res = reply(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
    let tools = res["result"]["tools"].as_array().expect("a tools array");
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().expect("a name")).collect();
    assert_eq!(names, ["open", "select", "action", "effect", "state", "save", "list_effects"]);
    for tool in tools {
        assert_eq!(tool["inputSchema"]["type"], "object", "{}", tool["name"]);
    }
}

#[test]
fn a_notification_gets_no_answer() {
    assert_eq!(handle(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, &mut Fake), None);
}

#[test]
fn bad_json_is_error_minus_32700() {
    let res = reply("{not json");
    assert_eq!(res["id"], Value::Null);
    assert_eq!(res["error"]["code"], -32700);
}

#[test]
fn an_unknown_method_is_error_minus_32601() {
    let res = reply(r#"{"jsonrpc":"2.0","id":7,"method":"nope/nothing"}"#);
    assert_eq!(res["id"], 7);
    assert_eq!(res["error"]["code"], -32601);
}

#[test]
fn the_id_is_echoed() {
    assert_eq!(reply(r#"{"jsonrpc":"2.0","id":42,"method":"tools/list"}"#)["id"], 42);
    assert_eq!(reply(r#"{"jsonrpc":"2.0","id":"abc-1","method":"tools/list"}"#)["id"], "abc-1");
}

const RATE: u32 = 48_000;
const FRAMES: u32 = 20_000;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wavcraft-mcp-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch folder");
    dir
}

/// A 16-bit, 48000 Hz, stereo WAV of silence, `FRAMES` long.
fn write_wav(path: &Path) {
    let data_len = FRAMES * 2 * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.resize(out.len() + data_len as usize, 0);
    std::fs::write(path, out).expect("write the test WAV");
}

/// Send one `tools/call` through `handle` on a headless backend and parse the reply.
fn call(head: &mut Headless, id: u64, name: &str, args: Value) -> Value {
    let line = json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": {"name": name, "arguments": args}})
    .to_string();
    let out = handle(&line, head).expect("a reply");
    serde_json::from_str(&out).expect("the reply is JSON")
}

#[test]
fn a_call_returns_the_backend_reply() {
    let dir = scratch("reply");
    let wav = dir.join("tone.wav");
    write_wav(&wav);
    let mut head = Headless::new(dir.join("sessions"));

    let opened = call(&mut head, 3, "open", json!({"path": wav.to_string_lossy()}));
    assert_eq!(opened["result"]["isError"], false, "{opened}");

    let res = call(&mut head, 4, "select", json!({"start": 10, "end": 20}));
    assert_eq!(res["id"], 4);
    assert_eq!(res["result"]["isError"], false);
    assert_eq!(res["result"]["content"][0]["type"], "text");
    assert_eq!(res["result"]["content"][0]["text"], "selection=10-20");
}

#[test]
fn a_backend_error_is_an_is_error_result() {
    let dir = scratch("error");
    let mut head = Headless::new(dir.join("sessions"));

    let res = call(&mut head, 5, "action", json!({"name": "edit.nope"}));
    assert_eq!(res["result"]["isError"], true, "{res}");
    let text = res["result"]["content"][0]["text"].as_str().expect("a text");
    assert!(text.contains("no action named"), "{text}");
    assert!(res.get("error").is_none(), "a tool failure is not a protocol error: {res}");
}

#[test]
fn list_effects_names_gain() {
    let dir = scratch("effects");
    let mut head = Headless::new(dir.join("sessions"));

    let res = call(&mut head, 6, "list_effects", json!({}));
    assert_eq!(res["result"]["isError"], false, "{res}");
    let text = res["result"]["content"][0]["text"].as_str().expect("a text");
    assert!(text.contains("gain [gain_db="), "{text}");
}

#[test]
fn an_unknown_tool_is_error_minus_32602() {
    let dir = scratch("unknown");
    let mut head = Headless::new(dir.join("sessions"));

    let res = call(&mut head, 8, "nope", json!({}));
    assert_eq!(res["error"]["code"], -32602, "{res}");
    assert!(res.get("result").is_none(), "{res}");
}
