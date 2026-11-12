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
//! - [`mcp::handle`] answers MCP clients (JSON-RPC 2.0, one message per line): `initialize` and
//!   `tools/list` so far.
//!
//! Only the verbs above are here. The window still parses the rest of the channel itself, and
//! its own copy of these (`parse_save_args`, `take_quality`, `take_meta` in `app.rs` are the
//! model for [`SaveArgs::parse`]; N1.6 moves the window onto this parser).

pub mod headless;
pub mod mcp;
pub mod remote;

use std::path::PathBuf;

use wavcraft_core::save::{AiffKind, SampleType};

/// The options a `SAVE` line may end with, as the window reads them: `bits=<16|24|32>` and
/// `float=<0|1>` (together, the sample type), `aiff=<aiff|aifc|sowt>`, `quality=<-1..10>` (Ogg
/// Vorbis) and `meta=<0|1>` ("Include markers and other metadata"). `None` is "not given": the
/// save keeps what it would use anyway.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SaveArgs {
    /// From `bits=` and `float=`.
    pub ty: Option<SampleType>,
    /// From `aiff=`; for this save only.
    pub aiff: Option<AiffKind>,
    /// From `quality=`.
    pub quality: Option<f32>,
    /// From `meta=`.
    pub meta: Option<bool>,
}

impl SaveArgs {
    /// `SAVE`'s arguments: a path (may hold spaces, may be empty) then the options in any order.
    /// The errors and their order are the window's (`take_meta`, `take_quality`, then
    /// `parse_save_args` in `apps/wavcraft/src/app.rs`).
    pub fn parse(rest: &str) -> Result<(Option<PathBuf>, SaveArgs), String> {
        let meta = take_meta(rest)?;
        let quality = take_quality(rest)?;
        let (path, ty, aiff) = parse_save_args(rest)?;
        let path = (!path.is_empty()).then(|| PathBuf::from(path));
        Ok((path, SaveArgs { ty, aiff, quality, meta }))
    }

    /// The options as they end a `SAVE` line, each with a space before it; empty when none.
    /// A 64-bit float type is written `bits=64 float=1`, which no save accepts from a line.
    fn line(&self) -> String {
        let mut out = String::new();
        match self.ty {
            None => {}
            Some(SampleType::Int16) => out.push_str(" bits=16"),
            Some(SampleType::Int24) => out.push_str(" bits=24"),
            Some(SampleType::Int32) => out.push_str(" bits=32"),
            Some(SampleType::Float32) => out.push_str(" float=1"),
            Some(SampleType::Float64) => out.push_str(" bits=64 float=1"),
        }
        if let Some(kind) = self.aiff {
            out.push_str(&format!(" aiff={}", kind.key()));
        }
        if let Some(q) = self.quality {
            out.push_str(&format!(" quality={q}"));
        }
        if let Some(m) = self.meta {
            out.push_str(&format!(" meta={}", u8::from(m)));
        }
        out
    }
}

/// `quality=<-1..10>` (Ogg Vorbis VBR) out of `SAVE`'s arguments, if given (the window's
/// `take_quality`). [`parse_save_args`] skips that token.
fn take_quality(rest: &str) -> Result<Option<f32>, String> {
    let mut quality = None;
    for tok in rest.split_whitespace() {
        if let Some(v) = tok.strip_prefix("quality=") {
            let q: f32 = v.parse().ok().filter(|q: &f32| (-1.0..=10.0).contains(q)).ok_or_else(|| format!("quality=<-1..10>, not {v}"))?;
            quality = Some(q);
        }
    }
    Ok(quality)
}

/// `meta=<0|1>` out of `SAVE`'s arguments, if given (the window's `take_meta`).
fn take_meta(rest: &str) -> Result<Option<bool>, String> {
    let mut meta = None;
    for tok in rest.split_whitespace() {
        if let Some(v) = tok.strip_prefix("meta=") {
            meta = Some(match v {
                "0" => false,
                "1" => true,
                _ => return Err(format!("meta=<0|1>, not {v}")),
            });
        }
    }
    Ok(meta)
}

/// `SAVE`'s path and the trailing `bits=`, `float=` and `aiff=` options (the window's
/// `parse_save_args`): options are read from the end of the line until a token is not one.
fn parse_save_args(rest: &str) -> Result<(&str, Option<SampleType>, Option<AiffKind>), String> {
    let (mut path, mut bits, mut float, mut aiff) = (rest.trim(), None, None, None);
    while let Some((head, last)) = path.rsplit_once(char::is_whitespace).or(if path.contains('=') { Some(("", path)) } else { None }) {
        if let Some(v) = last.strip_prefix("bits=") {
            bits = Some(v.parse::<u32>().map_err(|_| format!("bits=<16|24|32>, not {v}"))?);
        } else if let Some(v) = last.strip_prefix("aiff=") {
            aiff = Some(AiffKind::parse(v).ok_or_else(|| format!("aiff=<aiff|aifc|sowt>, not {v}"))?);
        } else if let Some(v) = last.strip_prefix("float=") {
            float = Some(match v {
                "0" => false,
                "1" => true,
                _ => return Err(format!("float=<0|1>, not {v}")),
            });
        } else if last.starts_with("quality=") || last.starts_with("meta=") {
            // Read by `take_quality` and `take_meta`.
        } else {
            break;
        }
        path = head.trim();
    }
    let ty = match (bits, float) {
        (None, None) => None,
        (Some(16), None | Some(false)) => Some(SampleType::Int16),
        (Some(24), None | Some(false)) => Some(SampleType::Int24),
        (Some(32), None | Some(false)) => Some(SampleType::Int32),
        (None | Some(32), Some(true)) => Some(SampleType::Float32),
        (None, Some(false)) => None,
        (b, f) => return Err(format!("no sample type for bits={b:?} float={f:?}: 16, 24, 32 bit integer or float=1 (32 bit)")),
    };
    Ok((path, ty, aiff))
}

/// One request on the control channel, in the form both backends act on.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// `OPEN <path>`: open a file as the document (the path runs to the end of the line).
    Open(PathBuf),
    /// `SELECT <start> <end>` (frames).
    Select { start: u64, end: u64 },
    /// `ACTION <name>`: an action table name such as `edit.delete` or `effect.invert`.
    Action(String),
    /// `EFFECT <name> key=value ...`: an effect by id, run with the settings given (none: defaults).
    Effect { name: String, args: String },
    /// `SAVE [<path>] [bits=..] [float=..] [aiff=..] [quality=..] [meta=..]`: save the document
    /// to `path`, the format from its extension; with no path, to the file that was opened (the
    /// window saves in place).
    Save { path: Option<PathBuf>, args: SaveArgs },
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
            "SAVE" => SaveArgs::parse(rest).map(|(path, args)| Request::Save { path, args }),
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
            Request::Save { path: Some(p), args } => format!("SAVE {}{}", p.display(), args.line()),
            Request::Save { path: None, args } => format!("SAVE{}", args.line()),
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
