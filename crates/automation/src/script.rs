//! The script reader and runner. `parse` turns a script text into the requests it holds, one per
//! line, each with its 1-based line number. `run` executes them in order on any [`Backend`].

use crate::{Backend, Request};

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

/// Run a script text on a backend: parse it, then execute each line in order.
///
/// A script that does not parse runs nothing. Otherwise the first line the backend refuses stops
/// the run, and the error is reported as `line N: reason`; no later line runs. The replies of
/// the lines that pass are discarded.
pub fn run(backend: &mut dyn Backend, script: &str) -> Result<(), String> {
    for (n, request) in parse(script)? {
        backend.execute(&request).map_err(|why| format!("line {n}: {why}"))?;
    }
    Ok(())
}
