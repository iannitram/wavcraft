//! MCP over newline-delimited JSON-RPC 2.0: one message per line in, at most one reply line out.
//!
//! [`handle`] answers `initialize` and `tools/list`. A notification (a message with no `id`)
//! gets no reply. A line that is not JSON gets `-32700` with a null id; an unknown method gets
//! `-32601`. Every reply echoes the request's id (number, string or null). Replies carry no
//! trailing newline: the caller writes one after each.
//!
//! `tools/call` is not answered yet, so it gets `-32601` like any other unknown method. The
//! backend is taken now for that call, which will run [`Request`](crate::Request)s through it.

use serde_json::{Value, json};

use crate::Backend;

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

/// Answer one line of MCP traffic. `None` means there is nothing to send back (a blank line or
/// a notification).
pub fn handle(line: &str, _backend: &mut impl Backend) -> Option<String> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let msg: Value = match serde_json::from_str(line) {
        Ok(msg) => msg,
        Err(_) => return Some(error_reply(Value::Null, PARSE_ERROR, "Parse error")),
    };
    let Value::Object(obj) = &msg else {
        return Some(error_reply(Value::Null, INVALID_REQUEST, "Invalid Request"));
    };
    // A message without an id is a notification: it is never answered, whatever its method.
    let id = obj.get("id")?.clone();
    if !(id.is_null() || id.is_number() || id.is_string()) {
        return Some(error_reply(Value::Null, INVALID_REQUEST, "Invalid Request"));
    }
    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        return Some(error_reply(id, INVALID_REQUEST, "Invalid Request"));
    };
    let params = obj.get("params").unwrap_or(&Value::Null);
    Some(match method {
        "initialize" => match params.get("protocolVersion").and_then(Value::as_str) {
            Some(version) => result_reply(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "wavcraft", "version": env!("CARGO_PKG_VERSION") },
                }),
            ),
            None => error_reply(id, INVALID_PARAMS, "initialize needs protocolVersion"),
        },
        "tools/list" => result_reply(id, json!({ "tools": tools() })),
        _ => error_reply(id, METHOD_NOT_FOUND, "Method not found"),
    })
}

/// The seven tools, with input schemas that follow the request spellings in [`crate::Request`]
/// and [`crate::SaveArgs`].
fn tools() -> Value {
    let no_args = || json!({ "type": "object", "properties": {} });
    json!([
        {
            "name": "open",
            "description": "Open an audio file as the document.",
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string", "description": "The file to open." } },
                "required": ["path"],
            },
        },
        {
            "name": "select",
            "description": "Select a range of frames in the document.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "start": { "type": "integer", "minimum": 0, "description": "First frame." },
                    "end": { "type": "integer", "minimum": 0, "description": "End frame, exclusive." },
                },
                "required": ["start", "end"],
            },
        },
        {
            "name": "action",
            "description": "Run an action from the action table, such as edit.delete.",
            "inputSchema": {
                "type": "object",
                "properties": { "name": { "type": "string", "description": "The action's table name." } },
                "required": ["name"],
            },
        },
        {
            "name": "effect",
            "description": "Run an effect on the selection (or the whole document) with the settings given.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "The effect's id, such as invert." },
                    "args": { "type": "string", "description": "Settings as key=value pairs separated by spaces." },
                },
                "required": ["name"],
            },
        },
        {
            "name": "state",
            "description": "Report the document: frames, channels, rate and selection.",
            "inputSchema": no_args(),
        },
        {
            "name": "save",
            "description": "Save the document. The format follows the path's extension; with no path, the opened file is written in place.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Where to save. Omit to save in place." },
                    "bits": { "type": "integer", "enum": [16, 24, 32], "description": "Integer sample size." },
                    "float": { "type": "boolean", "description": "Write 32-bit float samples (with bits 32 or none)." },
                    "aiff": { "type": "string", "enum": ["aiff", "aifc", "sowt"], "description": "AIFF kind for this save." },
                    "quality": { "type": "number", "minimum": -1, "maximum": 10, "description": "Ogg Vorbis quality." },
                    "meta": { "type": "boolean", "description": "Include markers and other metadata." },
                },
            },
        },
        {
            "name": "list_effects",
            "description": "List the effects, with their ids and settings.",
            "inputSchema": no_args(),
        },
    ])
}

fn result_reply(id: Value, result: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

fn error_reply(id: Value, code: i64, message: &str) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }).to_string()
}
