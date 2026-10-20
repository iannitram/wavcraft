//! The Files panel's table: one row per open document, sorted and filtered. This module holds
//! only the data and the rules (sort, filter, the Reveal command), so it is tested without a
//! window; `app.rs` fills the rows and draws them.

use std::cmp::Ordering;

/// What a row shows about one open document.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// The document's position in the order opened (what `SWITCH` takes).
    pub doc: usize,
    pub name: String,
    pub unsaved: bool,
    pub frames: u64,
    pub rate: u32,
    pub channels: usize,
    /// "16", "24", "32f" ... or "-" for a lossy file.
    pub bits: String,
    pub format: String,
    /// Empty for an untitled document.
    pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Col {
    Name,
    Unsaved,
    Duration,
    Rate,
    Channels,
    Bits,
    Format,
    Path,
}

impl Col {
    pub const ALL: [Col; 8] = [Col::Name, Col::Unsaved, Col::Duration, Col::Rate, Col::Channels, Col::Bits, Col::Format, Col::Path];

    /// The column's name in the control channel.
    pub fn key(self) -> &'static str {
        match self {
            Col::Name => "name",
            Col::Unsaved => "unsaved",
            Col::Duration => "duration",
            Col::Rate => "rate",
            Col::Channels => "channels",
            Col::Bits => "bits",
            Col::Format => "format",
            Col::Path => "path",
        }
    }

    /// The header text.
    pub fn title(self) -> &'static str {
        match self {
            Col::Name => "Name",
            Col::Unsaved => "*",
            Col::Duration => "Duration",
            Col::Rate => "Rate",
            Col::Channels => "Ch",
            Col::Bits => "Bits",
            Col::Format => "Format",
            Col::Path => "Path",
        }
    }

    pub fn parse(s: &str) -> Option<Col> {
        Col::ALL.into_iter().find(|c| c.key().eq_ignore_ascii_case(s.trim()))
    }

    fn compare(self, a: &Row, b: &Row) -> Ordering {
        match self {
            Col::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            Col::Unsaved => a.unsaved.cmp(&b.unsaved),
            // Lengths compare as time, not as frames: a 48 kHz and a 44.1 kHz file differ.
            Col::Duration => (u128::from(a.frames) * u128::from(b.rate.max(1))).cmp(&(u128::from(b.frames) * u128::from(a.rate.max(1)))),
            Col::Rate => a.rate.cmp(&b.rate),
            Col::Channels => a.channels.cmp(&b.channels),
            Col::Bits => a.bits.len().cmp(&b.bits.len()).then_with(|| a.bits.cmp(&b.bits)),
            Col::Format => a.format.cmp(&b.format),
            Col::Path => a.path.to_lowercase().cmp(&b.path.to_lowercase()),
        }
    }
}

/// How the table is ordered and narrowed. Not saved with a session.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct View {
    /// The sort column and whether it ascends; `None` is the order opened.
    pub sort: Option<(Col, bool)>,
    pub filter: String,
}

impl View {
    /// A header click: sort by `col` ascending; again, descending; again, back to the order opened.
    pub fn click_header(&mut self, col: Col) {
        self.sort = match self.sort {
            Some((c, true)) if c == col => Some((col, false)),
            Some((c, false)) if c == col => None,
            _ => Some((col, true)),
        };
    }

    /// The rows to show, in order: those matching the filter (case-insensitive text anywhere in
    /// the name, path or format), sorted. Ties keep the order opened.
    pub fn arrange<'a>(&self, rows: &'a [Row]) -> Vec<&'a Row> {
        let needle = self.filter.trim().to_lowercase();
        let mut out: Vec<&Row> =
            rows.iter().filter(|r| needle.is_empty() || [&r.name, &r.path, &r.format].iter().any(|s| s.to_lowercase().contains(&needle))).collect();
        if let Some((col, asc)) = self.sort {
            out.sort_by(|a, b| {
                let o = col.compare(a, b);
                (if asc { o } else { o.reverse() }).then(a.doc.cmp(&b.doc))
            });
        }
        out
    }

    /// `name:asc`, `rate:desc` or `none`, for the control channel.
    pub fn sort_text(&self) -> String {
        self.sort.map_or_else(|| "none".to_string(), |(c, asc)| format!("{}:{}", c.key(), if asc { "asc" } else { "desc" }))
    }
}

/// The program and arguments that show `path` in the system file manager: macOS `open -R`,
/// Windows `explorer /select,`, anywhere else `xdg-open` on the containing folder.
pub fn reveal_command(os: &str, path: &str) -> Option<(String, Vec<String>)> {
    if path.is_empty() {
        return None;
    }
    Some(match os {
        "macos" => ("open".to_string(), vec!["-R".to_string(), path.to_string()]),
        "windows" => ("explorer".to_string(), vec![format!("/select,{path}")]),
        _ => {
            let dir = std::path::Path::new(path)
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map_or_else(|| ".".to_string(), |p| p.to_string_lossy().into_owned());
            ("xdg-open".to_string(), vec![dir])
        }
    })
}

/// The command for this system, as one line for the control channel.
pub fn reveal_text(path: &str) -> Option<String> {
    let (prog, args) = reveal_command(std::env::consts::OS, path)?;
    Some(std::iter::once(prog).chain(args).collect::<Vec<_>>().join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(doc: usize, name: &str, frames: u64, rate: u32, channels: usize, path: &str) -> Row {
        Row { doc, name: name.into(), unsaved: false, frames, rate, channels, bits: "16".into(), format: "Wav".into(), path: path.into() }
    }

    fn three() -> Vec<Row> {
        vec![
            row(0, "b.wav", 48_000, 48_000, 2, "/x/b.wav"),
            row(1, "A.wav", 44_100 * 3, 44_100, 1, "/y/A.wav"),
            row(2, "c.wav", 96_000, 48_000, 2, "/x/c.wav"),
        ]
    }

    fn docs(v: &View, rows: &[Row]) -> Vec<usize> {
        v.arrange(rows).iter().map(|r| r.doc).collect()
    }

    #[test]
    fn no_sort_keeps_the_order_opened() {
        assert_eq!(docs(&View::default(), &three()), [0, 1, 2]);
    }

    #[test]
    fn sorts_by_name_ignoring_case_and_reverses() {
        let mut v = View { sort: Some((Col::Name, true)), filter: String::new() };
        assert_eq!(docs(&v, &three()), [1, 0, 2]);
        v.sort = Some((Col::Name, false));
        assert_eq!(docs(&v, &three()), [2, 0, 1]);
    }

    #[test]
    fn duration_compares_time_not_frames() {
        // 44.1k*3 frames is 3 s; 96000 at 48k is 2 s; 48000 at 48k is 1 s.
        let v = View { sort: Some((Col::Duration, true)), filter: String::new() };
        assert_eq!(docs(&v, &three()), [0, 2, 1]);
    }

    #[test]
    fn ties_keep_the_order_opened() {
        let v = View { sort: Some((Col::Channels, false)), filter: String::new() };
        assert_eq!(docs(&v, &three()), [0, 2, 1]);
    }

    #[test]
    fn bits_sort_by_number_not_text() {
        let mut rows = three();
        rows[0].bits = "24".into();
        rows[1].bits = "8".into();
        rows[2].bits = "32f".into();
        let v = View { sort: Some((Col::Bits, true)), filter: String::new() };
        assert_eq!(docs(&v, &rows), [1, 0, 2]);
    }

    #[test]
    fn the_filter_matches_name_path_or_format_in_any_case() {
        let mut v = View { sort: None, filter: "A.W".into() };
        assert_eq!(docs(&v, &three()), [1]);
        v.filter = "/x/".into();
        assert_eq!(docs(&v, &three()), [0, 2]);
        v.filter = "  WAV ".into();
        assert_eq!(docs(&v, &three()), [0, 1, 2]);
        v.filter = "nothing".into();
        assert!(docs(&v, &three()).is_empty());
    }

    #[test]
    fn header_clicks_cycle_ascending_descending_none() {
        let mut v = View::default();
        v.click_header(Col::Rate);
        assert_eq!(v.sort, Some((Col::Rate, true)));
        v.click_header(Col::Rate);
        assert_eq!(v.sort, Some((Col::Rate, false)));
        v.click_header(Col::Rate);
        assert_eq!(v.sort, None);
        v.click_header(Col::Rate);
        v.click_header(Col::Name);
        assert_eq!(v.sort_text(), "name:asc");
    }

    #[test]
    fn column_names_parse() {
        assert_eq!(Col::parse("Duration"), Some(Col::Duration));
        assert_eq!(Col::parse("nope"), None);
        assert!(Col::ALL.iter().all(|c| Col::parse(c.key()) == Some(*c)));
    }

    #[test]
    fn reveal_commands_per_system() {
        assert_eq!(reveal_command("macos", "/a/b.wav"), Some(("open".into(), vec!["-R".into(), "/a/b.wav".into()])));
        assert_eq!(reveal_command("windows", "C:\\a\\b.wav"), Some(("explorer".into(), vec!["/select,C:\\a\\b.wav".into()])));
        assert_eq!(reveal_command("linux", "/a/b.wav"), Some(("xdg-open".into(), vec!["/a".into()])));
        assert_eq!(reveal_command("macos", ""), None);
    }
}
