# C085-a: Marker list CSV export and import

Refs #321 (the roadmap row has no issue; #321 is the issue of the base PR #323). Never write "Closes". Files you may touch: `crates/core/src/multitrack/marker_export.rs` and ONE new line in `crates/core/src/multitrack.rs`. Nothing else.
Local only: do not push. The harness checks, commits and opens the PR.

## Step 0: check the base first (before you edit anything)
Run, from the repo root:
- `ls crates/core/src/multitrack/marker_export.rs`
- `grep -n "fn marker_rows\|fn markers_csv\|fn import_markers" crates/core/src/multitrack.rs`
- `grep -n "pub fn add_marker\|^fn err" crates/core/src/multitrack.rs`
`ls` must say the file does not exist, the first `grep` must find nothing, and the second must find `pub fn add_marker` and `fn err`. If a check fails, STOP and report it; write nothing.

## Rules
- Run no git or gh commands and use no network: the harness checks, commits, pushes and opens the PR. You only edit files.
- Use no `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` or `unreachable!` in production code (the crate denies them). Test code may use `.unwrap_or_else(|e| panic!("{e})")` exactly as written below. Use no `assert_eq!(x, true)`.
- Do not edit, rename, split or reformat any existing test or any existing line (except the one new line stated). Add no dependencies, no other files and no `debug_*.rs` scratch files.
- If a stated literal or a stated line cannot be matched, stop and report it. Do not change it.
- Type the file text below exactly (the design is fixed; your job is the typing). Run `rustfmt --edition 2024` on the one new file only, never the crate.
- Tests and checks verify correctness; they are not the goal. Implement the requested behavior for all valid inputs, not just the checked cases.
- At any failure, hang or odd result, read the tool's own output (the compiler or test log) and quote it in your report.

## Why
The marker list can be seen and renamed in the app but not saved or loaded as text. This job adds the pure text side: the rows, CSV out, CSV in, and an import through `add_marker`. No UI.

## Behaviour
- `Multitrack::marker_rows` returns `MarkerRow { name, start_s, end_s }` in time order (the session keeps markers sorted by start). `end_s` is `Some` only for a range marker (`MarkerKind::Selection`).
- `markers_csv(rows)` returns the header `name,start_s,end_s` then one line per row, every line ending `\r\n`. Numbers are printed with `f64::to_string` (so 2.0 prints `2`). A point marker has an empty last field. A name containing `,` `"` `\n` or `\r` is wrapped in `"` with each inner `"` doubled; other names are written bare.
- `parse_markers_csv(text)` is the inverse: it accepts `\r\n` or `\n`, skips blank lines, accepts a missing final line end, and reads quoted fields that hold commas, doubled quotes and line breaks. Errors are `MtError` with the exact texts in the code and the 1-based line where the row starts.
- `import_markers(rows)` checks all rows first (`row N: ...`), then adds each through `add_marker` in order and returns the count; a bad row means nothing is added.
- Queue tie-check: nothing in the queue text is ambiguous. One decision made here: CRLF line ends (RFC 4180) and an up-front check so an import is all-or-nothing.

## Where it goes and the code around it
- Base branch: `feat/ap-f110-session-markers`.
- Map: none for multitrack (the maps describe main, which has no multitrack.rs): read `crates/core/src/multitrack.rs` on this base. The new folder `crates/core/src/multitrack/` does not exist yet: create it with the one new file. Rust finds it from the `pub mod` line in `multitrack.rs`.
- Registration: Put `pub mod marker_export;` on its own line directly after the line `pub use soundcraft_model;` (about line 23), away from other lines.
- One existing function from this base, as the pattern to follow (do not edit it):

```rust
    /// Add a marker at `start` seconds, or a range when `end` is given and later than `start`.
    /// `None` names it "Marker N" / "Range N" from its number. Returns its 1-based position.
    pub fn add_marker(&mut self, name: Option<&str>, start: f64, end: Option<f64>) -> Result<usize, MtError> {
        if !start.is_finite() || start < 0.0 || end.is_some_and(|e| !e.is_finite() || e < 0.0) {
            return err("marker times are seconds, 0 or more");
        }
        let rate = self.session.sample_rate;
        let (s0, s1) = (rate.samples(start), rate.samples(end.unwrap_or(start)));
        if s1 < s0 {
            return err("a range ends after it starts");
        }
        let kind = if s1 > s0 { MarkerKind::Selection } else { MarkerKind::Marker };
        let name = match name {
            Some(n) => clean_name(n)?,
            None => {
                let n = self.session.markers.iter().map(|m| m.number).max().unwrap_or(0) + 1;
                format!("{} {n}", if kind == MarkerKind::Selection { "Range" } else { "Marker" })
            }
        };
        let id = self.session.add_marker(&name, kind, s0, s1);
        Ok(self.session.markers.iter().position(|m| m.id == id).map_or(0, |i| i + 1))
    }
```

```rust
fn clean_name(name: &str) -> Result<String, MtError> {
    let mut n = name.trim().to_string();
    if n.is_empty() {
        return err("a name cannot be empty");
    }
    truncate_chars(&mut n, MAX_NAME_CHARS);
    Ok(n)
}
```

## The queue row, verbatim
| 6 | C085-a | C085 | T9 | Marker/cue list panel (Markers) | easy | feat/ap-f110-session-markers (#323) | new -> #323 -> #318 -> main | `crates/core/src/multitrack/marker_export.rs` | crates/core/src/multitrack.rs | partly |

### 6. C085-a (Marker/cue list panel (Markers)), easy
- Base: feat/ap-f110-session-markers (#323). Chain: new -> #323 -> #318 -> main. Depends-on: C084.
- Scope: Export and import of the session marker list as text: `struct MarkerRow {name, start_s, end_s: Option<f64>}`; `Multitrack::marker_rows(&self) -> Vec<MarkerRow>` (time order); `markers_csv(rows) -> String` (header, RFC-4180 quoting); `parse_markers_csv(text) -> Result<Vec<MarkerRow>, MtError>`; `Multitrack::import_markers(&mut self, rows) -> Result<usize, MtError>` through `add_marker`.
- Tests: quoting of commas, quotes and newlines in a name; round trip; point marker has empty end; bad number gives a line number; empty list is only the header; import adds in order.
- Where: new `crates/core/src/multitrack/marker_export.rs`; module line in `crates/core/src/multitrack.rs`; pattern function: `Multitrack::markers_json` (:434) and `add_marker` (:344) in #323 `multitrack.rs`; map: none for multitrack: read #323 `crates/core/src/multitrack.rs`.
- Screen: partly: #323 has the marker list (rename by double-click, delete, jump, next/prev) as UI, so list/rename/jump are built; its own text says export (C085) is not done.

## The complete new file
Type it exactly. It is rustfmt-formatted; run `rustfmt --edition 2024` on the file afterwards anyway. There are exactly 13 new tests and all 13 must exist and pass.

```rust
//! Marker list export and import (C085): the session's markers as CSV text, and back.
//!
//! The text has the header `name,start_s,end_s` and one line per marker, ending in CRLF
//! (RFC 4180). A name that holds a comma, a quote or a line break is wrapped in quotes with
//! inner quotes doubled. `end_s` is empty for a point marker. Times are seconds.

use soundcraft_model::MarkerKind;

use super::{MtError, Multitrack, err};

/// The header line, without its line end.
const HEADER: &str = "name,start_s,end_s";

/// One marker as a row of the list: a point marker has no end.
#[derive(Debug, Clone, PartialEq)]
pub struct MarkerRow {
    pub name: String,
    pub start_s: f64,
    pub end_s: Option<f64>,
}

impl Multitrack {
    /// The session's markers as rows, in time order (the session keeps them sorted by start).
    pub fn marker_rows(&self) -> Vec<MarkerRow> {
        let rate = self.session.sample_rate;
        self.session
            .markers
            .iter()
            .map(|m| MarkerRow {
                name: m.name.clone(),
                start_s: rate.seconds(m.start),
                end_s: (m.kind == MarkerKind::Selection).then(|| rate.seconds(m.end)),
            })
            .collect()
    }

    /// Add every row through `add_marker`, in the order given. All rows are checked first: when
    /// one is bad nothing is added and the error names its row (1-based). Returns how many
    /// markers were added.
    pub fn import_markers(&mut self, rows: &[MarkerRow]) -> Result<usize, MtError> {
        for (i, row) in rows.iter().enumerate() {
            if let Err(why) = check_row(row) {
                return err(format!("row {}: {why}", i + 1));
            }
        }
        for row in rows {
            self.add_marker(Some(&row.name), row.start_s, row.end_s)?;
        }
        Ok(rows.len())
    }
}

/// Why a row cannot become a marker, or `Ok` when it can.
fn check_row(row: &MarkerRow) -> Result<(), &'static str> {
    if row.name.trim().is_empty() {
        return Err("the name is empty");
    }
    if !row.start_s.is_finite() || row.start_s < 0.0 {
        return Err("start_s is not a number of seconds, 0 or more");
    }
    if let Some(end) = row.end_s {
        if !end.is_finite() || end < 0.0 {
            return Err("end_s is not a number of seconds, 0 or more");
        }
        if end < row.start_s {
            return Err("a range ends after it starts");
        }
    }
    Ok(())
}

/// The rows as CSV text: the header, then one CRLF-ended line per row, in the order given.
pub fn markers_csv(rows: &[MarkerRow]) -> String {
    let mut out = String::from(HEADER);
    out.push_str("\r\n");
    for row in rows {
        out.push_str(&quote(&row.name));
        out.push(',');
        out.push_str(&row.start_s.to_string());
        out.push(',');
        if let Some(end) = row.end_s {
            out.push_str(&end.to_string());
        }
        out.push_str("\r\n");
    }
    out
}

/// The name as one CSV field: quoted only when it holds a comma, a quote or a line break.
fn quote(name: &str) -> String {
    if name.contains([',', '"', '\n', '\r']) { format!("\"{}\"", name.replace('"', "\"\"")) } else { name.to_string() }
}

/// Rows from CSV text (CRLF or LF line ends, blank lines skipped, a missing final line end
/// allowed). Every error names the line (1-based) where the row starts.
pub fn parse_markers_csv(text: &str) -> Result<Vec<MarkerRow>, MtError> {
    let mut records = split_records(text)?.into_iter();
    match records.next() {
        None => return err("the text has no header line"),
        Some((line, fields)) => {
            if fields.join(",") != HEADER {
                return err(format!("line {line}: expected the header {HEADER}"));
            }
        }
    }
    let mut rows = Vec::new();
    for (line, fields) in records {
        if fields.len() == 1 && fields[0].trim().is_empty() {
            continue;
        }
        if fields.len() != 3 {
            return err(format!("line {line}: expected 3 fields, found {}", fields.len()));
        }
        let Some(start_s) = parse_seconds(&fields[1]) else {
            return err(format!("line {line}: start_s is not a number of seconds, 0 or more"));
        };
        let end_s = if fields[2].trim().is_empty() {
            None
        } else {
            let Some(end) = parse_seconds(&fields[2]) else {
                return err(format!("line {line}: end_s is not a number of seconds, 0 or more"));
            };
            Some(end)
        };
        let row = MarkerRow { name: fields[0].clone(), start_s, end_s };
        if let Err(why) = check_row(&row) {
            return err(format!("line {line}: {why}"));
        }
        rows.push(row);
    }
    Ok(rows)
}

fn parse_seconds(text: &str) -> Option<f64> {
    text.trim().parse::<f64>().ok()
}

/// Split CSV text into records of fields, each with the line where it starts. A quoted field
/// may hold commas, doubled quotes and line breaks. A `\r` outside quotes is dropped.
fn split_records(text: &str) -> Result<Vec<(usize, Vec<String>)>, MtError> {
    let mut records: Vec<(usize, Vec<String>)> = Vec::new();
    let mut fields: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut line = 1usize;
    let mut record_line = 1usize;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                if c == '\n' {
                    line += 1;
                }
                field.push(c);
            }
        } else if c == '"' && field.is_empty() {
            in_quotes = true;
        } else if c == ',' {
            fields.push(std::mem::take(&mut field));
        } else if c == '\n' {
            fields.push(std::mem::take(&mut field));
            records.push((record_line, std::mem::take(&mut fields)));
            line += 1;
            record_line = line;
        } else if c != '\r' {
            field.push(c);
        }
    }
    if in_quotes {
        return err(format!("line {record_line}: a quoted name is never closed"));
    }
    if !field.is_empty() || !fields.is_empty() {
        fields.push(field);
        records.push((record_line, fields));
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use soundcraft_model::BitDepthSetting;

    fn session() -> Multitrack {
        Multitrack::new("s", 48_000, BitDepthSetting::Int24).unwrap_or_else(|e| panic!("{e}"))
    }

    fn row(name: &str, start_s: f64, end_s: Option<f64>) -> MarkerRow {
        MarkerRow { name: name.to_string(), start_s, end_s }
    }

    fn names(mt: &Multitrack) -> Vec<String> {
        mt.marker_rows().into_iter().map(|r| r.name).collect()
    }

    fn fail(text: &str) -> Result<Vec<MarkerRow>, MtError> {
        parse_markers_csv(text)
    }

    fn msg(text: &str) -> String {
        fail(text).err().map(|e| e.0).unwrap_or_default()
    }

    #[test]
    fn csv_quotes_commas_quotes_and_newlines_in_a_name() {
        let rows = [row("a,b \"c\"\nd", 1.5, None)];
        assert_eq!(markers_csv(&rows), "name,start_s,end_s\r\n\"a,b \"\"c\"\"\nd\",1.5,\r\n");
    }

    #[test]
    fn point_marker_has_an_empty_end() {
        let rows = [row("A", 2.0, None), row("B", 1.0, Some(3.25))];
        assert_eq!(markers_csv(&rows), "name,start_s,end_s\r\nA,2,\r\nB,1,3.25\r\n");
    }

    #[test]
    fn csv_round_trips_through_parse() {
        let rows = vec![row("Intro, \"v1\"", 0.5, None), row("Line\nbreak", 1.0, Some(2.5)), row("Verse", 2.0, Some(4.5))];
        assert_eq!(parse_markers_csv(&markers_csv(&rows)), Ok(rows));
    }

    #[test]
    fn bad_number_names_its_line() {
        assert_eq!(msg("name,start_s,end_s\r\nA,1,\r\nB,x,\r\n"), "line 3: start_s is not a number of seconds, 0 or more");
    }

    #[test]
    fn line_numbers_count_newlines_inside_quotes() {
        assert_eq!(msg("name,start_s,end_s\n\"a\nb\",1,\nC,-1,\n"), "line 4: start_s is not a number of seconds, 0 or more");
    }

    #[test]
    fn empty_list_is_only_the_header() {
        assert_eq!(markers_csv(&[]), "name,start_s,end_s\r\n");
        assert_eq!(parse_markers_csv("name,start_s,end_s\r\n"), Ok(Vec::new()));
        assert!(session().marker_rows().is_empty());
    }

    #[test]
    fn parse_accepts_lf_blank_lines_and_a_missing_final_newline() {
        assert_eq!(parse_markers_csv("name,start_s,end_s\n\nA,1,2"), Ok(vec![row("A", 1.0, Some(2.0))]));
    }

    #[test]
    fn parse_refuses_a_bad_header_short_row_and_backwards_range() {
        assert_eq!(msg("x,y,z\n"), "line 1: expected the header name,start_s,end_s");
        assert_eq!(msg(""), "the text has no header line");
        assert_eq!(msg("name,start_s,end_s\nA,1\n"), "line 2: expected 3 fields, found 2");
        assert_eq!(msg("name,start_s,end_s\nA,5,2\n"), "line 2: a range ends after it starts");
        assert_eq!(msg("name,start_s,end_s\n,1,\n"), "line 2: the name is empty");
        assert_eq!(msg("name,start_s,end_s\nA,1,z\n"), "line 2: end_s is not a number of seconds, 0 or more");
    }

    #[test]
    fn an_unclosed_quote_is_reported_at_its_line() {
        assert_eq!(msg("name,start_s,end_s\n\"abc,1,\n"), "line 2: a quoted name is never closed");
    }

    #[test]
    fn marker_rows_are_in_time_order_with_point_and_range() {
        let mut mt = session();
        mt.add_marker(Some("Verse"), 2.0, Some(4.5)).unwrap_or_else(|e| panic!("{e}"));
        mt.add_marker(Some("Intro"), 0.5, None).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(mt.marker_rows(), vec![row("Intro", 0.5, None), row("Verse", 2.0, Some(4.5))]);
    }

    #[test]
    fn import_adds_in_order() {
        let mut mt = session();
        let rows = [row("First", 1.0, None), row("Second", 1.0, None)];
        assert_eq!(mt.import_markers(&rows), Ok(2));
        assert_eq!(names(&mt), vec!["First".to_string(), "Second".to_string()]);
    }

    #[test]
    fn import_refuses_a_bad_row_and_adds_nothing() {
        let mut mt = session();
        let rows = [row("Good", 1.0, None), row("Bad", -1.0, None)];
        assert_eq!(mt.import_markers(&rows), Err(MtError("row 2: start_s is not a number of seconds, 0 or more".to_string())));
        assert_eq!(mt.marker_count(), 0);
    }

    #[test]
    fn import_then_export_round_trips() {
        let mut mt = session();
        let rows = vec![row("A", 1.0, None), row("B", 2.0, Some(3.0))];
        assert_eq!(mt.import_markers(&rows), Ok(2));
        assert_eq!(mt.marker_rows(), rows);
    }
}
```

## Expected values (computed beforehand by an independent Python reference or by hand where trivial)
| test | expected |
|---|---|
| csv_quotes_commas_quotes_and_newlines_in_a_name | `name,start_s,end_s\r\n"a,b ""c""\nd",1.5,\r\n` |
| point_marker_has_an_empty_end | `name,start_s,end_s\r\nA,2,\r\nB,1,3.25\r\n` |
| csv_round_trips_through_parse | parse(csv(rows)) == Ok(rows) for 3 rows (quote+comma name, line-break name, ranges) |
| bad_number_names_its_line | `line 3: start_s is not a number of seconds, 0 or more` |
| line_numbers_count_newlines_inside_quotes | `line 4: start_s is not a number of seconds, 0 or more` |
| empty_list_is_only_the_header | `name,start_s,end_s\r\n`; parse gives `Ok([])`; empty session gives no rows |
| parse_accepts_lf_blank_lines_and_a_missing_final_newline | `Ok([A, 1, Some(2)])` |
| parse_refuses_a_bad_header_short_row_and_backwards_range | six exact messages (see the test) |
| an_unclosed_quote_is_reported_at_its_line | `line 2: a quoted name is never closed` |
| marker_rows_are_in_time_order_with_point_and_range | `[Intro 0.5 None, Verse 2 Some(4.5)]` (48000 Hz: 24000 and 216000 samples) |
| import_adds_in_order | `Ok(2)`, names `First`, `Second` (equal starts keep insertion order) |
| import_refuses_a_bad_row_and_adds_nothing | `row 2: start_s is not a number of seconds, 0 or more`, marker_count 0 |
| import_then_export_round_trips | rows come back equal |
All values are exact strings or short decimals (0.5, 2.5, 4.5, 3.25 are exact in binary); no python needed beyond that check.

## Mutation checks (the harness runs these, not you)
Each change is applied to the production line named, one at a time, and the named test must FAIL with a real `test result: FAILED` line; the file is restored afterwards.
- `crates/core/src/multitrack/marker_export.rs`: change `name.replace('"', "\"\"")` to `name.replace('"', "\"")`: `csv_quotes_commas_quotes_and_newlines_in_a_name` must FAIL.
- `crates/core/src/multitrack/marker_export.rs`: change `out.push_str(&end.to_string());` to `out.push_str("0");`: `point_marker_has_an_empty_end` must FAIL.
- `crates/core/src/multitrack/marker_export.rs`: change `field.push('"');` to `field.push('x');`: `csv_round_trips_through_parse` must FAIL.
- `crates/core/src/multitrack/marker_export.rs`: change `return err(format!("line {line}: start_s is not` to `return err(format!("row {line}: start_s is not`: `bad_number_names_its_line` must FAIL.
- `crates/core/src/multitrack/marker_export.rs`: change `line += 1;` to `line += 0;`: `line_numbers_count_newlines_inside_quotes` must FAIL.
- `crates/core/src/multitrack/marker_export.rs`: change `let mut out = String::from(HEADER);` to `let mut out = String::new();`: `empty_list_is_only_the_header` must FAIL.
- `crates/core/src/multitrack/marker_export.rs`: change `if fields.len() == 1 && fields[0].trim().is_empty() {` to `if false {`: `parse_accepts_lf_blank_lines_and_a_missing_final_newline` must FAIL.
- `crates/core/src/multitrack/marker_export.rs`: change `if end < row.start_s {` to `if end < row.start_s - 100.0 {`: `parse_refuses_a_bad_header_short_row_and_backwards_range` must FAIL.
- `crates/core/src/multitrack/marker_export.rs`: change `err(format!("line {record_line}: a quoted` to `err(format!("line {record_line}: an open`: `an_unclosed_quote_is_reported_at_its_line` must FAIL.
- `crates/core/src/multitrack/marker_export.rs`: change `.then(|| rate.seconds(m.end))` to `.then(|| rate.seconds(m.start))`: `marker_rows_are_in_time_order_with_point_and_range` must FAIL.
- `crates/core/src/multitrack/marker_export.rs`: change `for row in rows {` to `for row in rows.iter().rev() {`: `import_adds_in_order` must FAIL.
- `crates/core/src/multitrack/marker_export.rs`: change `return err(format!("row {}: {why}", i + 1));` to `break;`: `import_refuses_a_bad_row_and_adds_nothing` must FAIL.
- `crates/core/src/multitrack/marker_export.rs`: change `self.add_marker(Some(&row.name), row.start_s, row.end_s)?;` to `self.add_marker(None, row.start_s, row.end_s)?;`: `import_then_export_round_trips` must FAIL.

## Commands (run from the repo root, plain `cargo`; this job runs locally)
```sh
cargo test -p wavcraft-core marker_export
cargo clippy -p wavcraft-core --all-targets -- -D warnings
rustfmt --edition 2024 crates/core/src/multitrack/marker_export.rs
```
rustfmt that one file only. Stop when the marker_export tests pass (13 of 13), clippy is clean and rustfmt has been run.
