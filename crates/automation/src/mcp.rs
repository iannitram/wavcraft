//! MCP over newline-delimited JSON-RPC 2.0: one message per line in, at most one reply line out.
//!
//! [`handle`] answers `initialize` and `tools/list`. A notification (a message with no `id`)
//! gets no reply. A line that is not JSON gets `-32700` with a null id; an unknown method gets
//! `-32601`. Every reply echoes the request's id (number, string or null). Replies carry no
//! trailing newline: the caller writes one after each.
//!
//! `tools/call` runs the tool's [`Request`](crate::Request) on the backend and answers with the
//! reply as text; a backend error is an `isError` result, not a protocol error. An unknown tool
//! name gets `-32602`.

use std::path::PathBuf;

use serde_json::{Map, Value, json};
use wavcraft_core::effects;

use crate::{Backend, Request, SaveArgs};

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

/// Answer one line of MCP traffic. `None` means there is nothing to send back (a blank line or
/// a notification).
pub fn handle(line: &str, backend: &mut impl Backend) -> Option<String> {
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
        "tools/call" => call(id, params, backend),
        _ => error_reply(id, METHOD_NOT_FOUND, "Method not found"),
    })
}

/// `tools/call`: run one tool. A missing or wrongly typed argument, and a backend error, are
/// tool failures (an `isError` result); only a missing tool name or an unknown tool is a protocol
/// error.
fn call(id: Value, params: &Value, backend: &mut impl Backend) -> String {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return error_reply(id, INVALID_PARAMS, "tools/call needs a tool name");
    };
    let empty = Map::new();
    let args = match params.get("arguments") {
        None | Some(Value::Null) => &empty,
        Some(Value::Object(args)) => args,
        Some(_) => return error_reply(id, INVALID_PARAMS, "tools/call arguments must be an object"),
    };
    match name {
        "list_effects" => tool_reply(id, &list_effects(), false),
        "open" | "select" | "action" | "effect" | "state" | "save" => {
            let req = match request_for(name, args) {
                Ok(req) => req,
                Err(why) => return tool_reply(id, &why, true),
            };
            match backend.execute(&req) {
                Ok(text) if text.is_empty() => tool_reply(id, "ok", false),
                Ok(text) => tool_reply(id, &text, false),
                Err(text) => tool_reply(id, &text, true),
            }
        }
        _ => error_reply(id, INVALID_PARAMS, "Unknown tool"),
    }
}

/// The request a tool's arguments stand for. Each argument is checked before it is used.
fn request_for(name: &str, args: &Map<String, Value>) -> Result<Request, String> {
    match name {
        "open" => {
            let path = str_arg(args, "path")?.ok_or("open needs a string path")?;
            Ok(Request::Open(PathBuf::from(path)))
        }
        "select" => match (args.get("start").and_then(Value::as_u64), args.get("end").and_then(Value::as_u64)) {
            (Some(start), Some(end)) => Ok(Request::Select { start, end }),
            _ => Err("select needs integer start and end".to_string()),
        },
        "action" => {
            let name = str_arg(args, "name")?.ok_or("action needs a string name")?;
            Ok(Request::Action(name.to_string()))
        }
        "effect" => {
            let name = str_arg(args, "name")?.ok_or("effect needs a string name")?;
            let name = name.strip_prefix("effect.").unwrap_or(name);
            let args = str_arg(args, "args")?.unwrap_or("");
            Ok(Request::Effect { name: name.to_string(), args: args.to_string() })
        }
        "state" => Ok(Request::State),
        "save" => save_request(args),
        _ => Err("Unknown tool".to_string()),
    }
}

/// `save`: every argument is optional. They become the same trailing options a `SAVE` line has,
/// then [`SaveArgs::parse`] reads them, so the checks are the window's.
fn save_request(args: &Map<String, Value>) -> Result<Request, String> {
    let mut rest = String::new();
    if let Some(path) = str_arg(args, "path")? {
        rest.push_str(path);
    }
    if let Some(bits) = args.get("bits") {
        let bits = bits.as_i64().ok_or("save bits must be an integer")?;
        rest.push_str(&format!(" bits={bits}"));
    }
    if let Some(float) = bool_arg(args, "float")? {
        rest.push_str(&format!(" float={}", u8::from(float)));
    }
    if let Some(aiff) = str_arg(args, "aiff")? {
        rest.push_str(&format!(" aiff={aiff}"));
    }
    if let Some(quality) = args.get("quality") {
        let quality = quality.as_f64().ok_or("save quality must be a number")?;
        rest.push_str(&format!(" quality={quality}"));
    }
    if let Some(meta) = bool_arg(args, "meta")? {
        rest.push_str(&format!(" meta={}", u8::from(meta)));
    }
    let (path, args) = SaveArgs::parse(rest.trim())?;
    Ok(Request::Save { path, args })
}

/// A string argument: `None` when absent or null, an error when it is not a string.
fn str_arg<'a>(args: &'a Map<String, Value>, key: &str) -> Result<Option<&'a str>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value.as_str().map(Some).ok_or_else(|| format!("{key} must be a string")),
    }
}

/// A boolean argument: `None` when absent or null, an error when it is not a boolean.
fn bool_arg(args: &Map<String, Value>, key: &str) -> Result<Option<bool>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value.as_bool().map(Some).ok_or_else(|| format!("{key} must be true or false")),
    }
}

/// `list_effects`: each effect's id and its settings, as the window's effect list spells them.
fn list_effects() -> String {
    effects::all().iter().map(|e| format!("{} [{}]", e.id(), effects::describe(*e))).collect::<Vec<_>>().join("; ")
}

/// A tool's answer: its text, and whether it is a failure.
fn tool_reply(id: Value, text: &str, is_error: bool) -> String {
    result_reply(id, json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))
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
