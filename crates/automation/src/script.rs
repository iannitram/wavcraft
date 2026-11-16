//! The script reader: turns a script text into the requests it holds, one per line, each with
//! its 1-based line number. It runs nothing; running a script is N4.2.

use crate::Request;

/// Read a script text into its requests, each with its 1-based line number.
///
/// Blank lines and lines that start with `#` are skipped, but their numbers still count. CRLF
/// line endings are accepted. An `OPEN` or `SAVE` line is refused, since a script does not open
/// or save files. The first bad line stops the reading and is reported as `line N: reason`.
pub fn parse(text: &str) -> Result<Vec<(usize, Request)>, String> {
    let mut requests = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let n = index + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let verb = line.split_whitespace().next().unwrap_or("");
        if matches!(verb, "OPEN" | "SAVE") {
            return Err(format!("line {n}: {verb} is not allowed in a script"));
        }
        match Request::parse(line) {
            Ok(request) => requests.push((n, request)),
            Err(why) => return Err(format!("line {n}: {why}")),
        }
    }
    Ok(requests)
}
