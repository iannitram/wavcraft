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
