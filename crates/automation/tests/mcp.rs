//! The MCP handler's wire behaviour: one line in, one line out, answered without a window.

use serde_json::{Value, json};
use wavcraft_automation::mcp::handle;
use wavcraft_automation::{Backend, Request};

/// A backend that answers nothing: `handle` does not reach it until `tools/call` exists.
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
