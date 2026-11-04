//! WavCraft automation: the control channel's requests, answered either by the window or by
//! a headless engine, so one script works with or without the window.
//!
//! The shape (a `Backend` trait with a `Headless` and a `Remote` implementation) follows
//! SoundCraft's `crates/automation` (MIT OR Apache-2.0, our fork); the code here is written for
//! WavCraft's own request names, which are the ones `apps/wavcraft/src/control.rs` answers:
//!
//! - [`Request`] is one parsed request line (`OPEN`, `SELECT`, `ACTION`, `EFFECT`, `SAVE`, `STATE`),
//!   with [`Request::parse`] and [`Request::line`] as the one spelling both sides use.
//! - [`Backend`] answers a request with the reply text after `OK` (or the reason for `ERR`).
//! - [`headless::Headless`] runs `wavcraft-core` in this process, with no window.
//! - [`remote::Remote`] talks to a running app over the control channel (loopback, token first).
//!
//! Only the verbs above are here. The window still parses the rest of the channel itself.

pub mod headless;
pub mod remote;

use std::path::PathBuf;

/// One request on the control channel, in the form both backends act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// `OPEN <path>`: open a file as the document (the path runs to the end of the line).
    Open(PathBuf),
    /// `SELECT <start> <end>` (frames).
    Select { start: u64, end: u64 },
    /// `ACTION <name>`: an action table name such as `edit.delete` or `effect.invert`.
    Action(String),
    /// `EFFECT <name> key=value ...`: an effect by id, run with the settings given (none: defaults).
    Effect { name: String, args: String },
    /// `SAVE <path>`: save the document to `path`, the format from its extension.
    Save(PathBuf),
    /// `STATE`: what the document is (frames, channels, rate, selection).
    State,
}

impl Request {
    /// Parse one request line. Verbs are case-sensitive, as the window's are.
    pub fn parse(line: &str) -> Result<Request, String> {
        let line = line.trim();
        let (verb, rest) = line.split_once(' ').map_or((line, ""), |(v, r)| (v, r.trim()));
        match verb {
            "OPEN" if !rest.is_empty() => Ok(Request::Open(PathBuf::from(rest))),
            "OPEN" => Err("OPEN <path>".to_string()),
            "SELECT" => {
                let mut it = rest.split_whitespace().map(str::parse::<u64>);
                match (it.next(), it.next(), it.next()) {
                    (Some(Ok(start)), Some(Ok(end)), None) => Ok(Request::Select { start, end }),
                    _ => Err("SELECT <start> <end>, in frames".to_string()),
                }
            }
            "ACTION" if !rest.is_empty() => Ok(Request::Action(rest.to_string())),
            "ACTION" => Err("ACTION <name>".to_string()),
            "EFFECT" => {
                let (name, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                if name.is_empty() {
                    return Err("EFFECT <name> key=value ...".to_string());
                }
                let name = name.strip_prefix("effect.").unwrap_or(name);
                Ok(Request::Effect { name: name.to_string(), args: args.trim().to_string() })
            }
            "SAVE" if !rest.is_empty() => Ok(Request::Save(PathBuf::from(rest))),
            "SAVE" => Err("SAVE <path>".to_string()),
            "STATE" if rest.is_empty() => Ok(Request::State),
            _ => Err(format!("no request named {line:?}")),
        }
    }

    /// The request as one line on the wire. `parse(line())` gives the same request back.
    pub fn line(&self) -> String {
        match self {
            Request::Open(p) => format!("OPEN {}", p.display()),
            Request::Select { start, end } => format!("SELECT {start} {end}"),
            Request::Action(name) => format!("ACTION {name}"),
            Request::Effect { name, args } if args.is_empty() => format!("EFFECT {name}"),
            Request::Effect { name, args } => format!("EFFECT {name} {args}"),
            Request::Save(p) => format!("SAVE {}", p.display()),
            Request::State => "STATE".to_string(),
        }
    }
}

/// Answers requests. The reply is the text after `OK`; a failure is the reason, which the
/// control channel sends as `ERR <reason>`.
pub trait Backend {
    fn execute(&mut self, req: &Request) -> Result<String, String>;

    /// Answer one raw request line as the control channel would: `OK <text>` or `ERR <reason>`.
    fn answer_line(&mut self, line: &str) -> String {
        match Request::parse(line).and_then(|req| self.execute(&req)) {
            Ok(text) if text.is_empty() => "OK".to_string(),
            Ok(text) => format!("OK {text}"),
            Err(why) => format!("ERR {why}"),
        }
    }
}
