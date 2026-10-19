//! Timestamps and the two line formats (text and JSON).

use std::borrow::Cow;
use std::fmt::Write as _;
use std::time::SystemTime;

/// `2026-10-13T17:47:32.123Z` (UTC, milliseconds). A clock before 1970 reads as the epoch.
pub fn timestamp(t: SystemTime) -> String {
    let since = t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = since.as_secs();
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3600, rem % 3600 / 60, rem % 60, since.subsec_millis())
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian (Howard Hinnant's algorithm),
/// saturating so an absurd clock cannot overflow.
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let z = days.saturating_add(719_468);
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = era.saturating_mul(400).saturating_add(yoe).saturating_add(u64::from(m <= 2));
    (y, m, d)
}

/// A message longer than this is cut, ending "…(N bytes cut)".
pub const MAX_MESSAGE_BYTES: usize = 8 * 1024;

/// `message` without one trailing newline, cut to [`MAX_MESSAGE_BYTES`] at a character boundary.
fn bounded(message: &str) -> Cow<'_, str> {
    let message = message.strip_suffix('\n').unwrap_or(message);
    if message.len() <= MAX_MESSAGE_BYTES {
        return Cow::Borrowed(message);
    }
    let mut cut = MAX_MESSAGE_BYTES;
    while !message.is_char_boundary(cut) {
        cut -= 1;
    }
    Cow::Owned(format!("{}…({} bytes cut)", &message[..cut], message.len() - cut))
}

/// Text-line escaping, so that one record is one line and nothing logged (an argument, a file
/// name) can forge a line: newline and carriage return as `\n` and `\r`; every other control
/// character (C0, DEL, C1) and the Unicode line and paragraph separators as `\u{..}`. Tabs and
/// everything else are kept as they are (a backslash too, so Windows paths stay readable).
pub fn escape_text(s: &str) -> Cow<'_, str> {
    let special = |c: char| c != '\t' && (c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'));
    if !s.chars().any(special) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if special(c) => {
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
    Cow::Owned(out)
}

/// One record, ready to format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line<'a> {
    pub ts: &'a str,
    pub level: log::Level,
    pub pid: Option<u32>,
    pub thread: &'a str,
    pub target: &'a str,
    pub message: &'a str,
}

impl Line<'_> {
    /// `<ts> <LEVEL> [<thread>] <target>: <message>\n`, the same shape as the apps' own loggers;
    /// with a process id, `[<pid> <thread>]`. Always exactly one line: the message is cut to
    /// [`MAX_MESSAGE_BYTES`] and control characters are escaped ([`escape_text`]).
    pub fn text(&self) -> String {
        let message = escape_text(&bounded(self.message)).into_owned();
        let (thread, target) = (escape_text(self.thread), escape_text(self.target));
        match self.pid {
            Some(pid) => format!("{} {:<5} [{pid} {thread}] {target}: {message}\n", self.ts, self.level),
            None => format!("{} {:<5} [{thread}] {target}: {message}\n", self.ts, self.level),
        }
    }

    /// One JSON object and a newline: `ts`, `level`, `pid` (when known), `thread`, `target`, `msg`.
    pub fn json(&self) -> String {
        let message = bounded(self.message);
        let pid = self.pid.map(|p| format!(",\"pid\":{p}")).unwrap_or_default();
        format!(
            "{{\"ts\":\"{}\",\"level\":\"{}\"{pid},\"thread\":\"{}\",\"target\":\"{}\",\"msg\":\"{}\"}}\n",
            escape(self.ts),
            self.level,
            escape(self.thread),
            escape(self.target),
            escape(&message)
        )
    }
}

/// JSON string escaping (quotes, backslashes, control characters).
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn timestamps_are_utc_with_milliseconds() {
        assert_eq!(timestamp(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        // 2026-10-13 17:47:32.123 UTC
        assert_eq!(timestamp(UNIX_EPOCH + Duration::from_millis(1_791_913_652_123)), "2026-10-13T17:47:32.123Z");
        assert_eq!(timestamp(UNIX_EPOCH + Duration::from_secs(951_782_400)), "2000-02-29T00:00:00.000Z");
    }

    #[test]
    fn text_and_json_lines() {
        let l = Line { ts: "T", level: log::Level::Warn, pid: Some(42), thread: "main", target: "a::b", message: "say \"hi\"\n" };
        assert_eq!(l.text(), "T WARN  [42 main] a::b: say \"hi\"\n");
        assert_eq!(l.json(), "{\"ts\":\"T\",\"level\":\"WARN\",\"pid\":42,\"thread\":\"main\",\"target\":\"a::b\",\"msg\":\"say \\\"hi\\\"\"}\n");
        let l = Line { pid: None, message: "tab\there\u{1}", ..l };
        assert!(l.json().contains("\"msg\":\"tab\\there\\u0001\""));
    }

    #[test]
    fn control_characters_cannot_forge_a_line() {
        let forged = "open x\n2026-10-13T00:00:00.000Z ERROR [1 main] app: forged\r\u{1}\u{7f}\u{85}\u{2028}tab\tkept";
        let l = Line { ts: "T", level: log::Level::Warn, pid: Some(42), thread: "main", target: "app", message: forged };
        let text = l.text();
        assert_eq!(text.matches('\n').count(), 1, "one record, one line: {text:?}");
        assert_eq!(text, "T WARN  [42 main] app: open x\\n2026-10-13T00:00:00.000Z ERROR [1 main] app: forged\\r\\u{1}\\u{7f}\\u{85}\\u{2028}tab\tkept\n");
        // the thread and target names are escaped too
        let l = Line { thread: "a\nb", target: "t\n", message: "m", ..l };
        assert_eq!(l.text(), "T WARN  [42 a\\nb] t\\n: m\n");
    }

    #[test]
    fn a_long_message_is_cut_with_a_count() {
        const CUT: usize = 8 * 1024; // MAX_MESSAGE_BYTES
        let long = format!("{}é{}", "a".repeat(CUT - 1), "b".repeat(2000));
        let l = Line { ts: "T", level: log::Level::Info, pid: None, thread: "main", target: "app", message: &long };
        let text = l.text();
        // the cut falls inside "é" (bytes 8191..8193), so it moves back to the character's start
        let kept = text.strip_prefix("T INFO  [main] app: ").and_then(|t| t.strip_suffix("…(2002 bytes cut)\n"));
        assert!(kept.is_some_and(|k| k.len() == CUT - 1 && k.bytes().all(|b| b == b'a')), "{} bytes: …{:?}", text.len(), &text[text.len() - 40..]);
        assert!(l.json().ends_with("…(2002 bytes cut)\"}\n"));
        let short = Line { message: "short", ..l };
        assert_eq!(short.text(), "T INFO  [main] app: short\n");
    }
}
