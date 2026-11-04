//! #922: `Settings::to_text` writes its newlines as `\n` escapes, never as real line breaks inside
//! string literals (a CRLF checkout would otherwise change the text), and its output is unchanged.

use wavcraft_core::settings::Settings;

const SETTINGS_SOURCE: &str = include_str!("../src/settings.rs");

/// The source text of `to_text` (from its `fn` line up to the next function, `save`).
fn to_text_source() -> &'static str {
    let start = SETTINGS_SOURCE.find("pub fn to_text").expect("to_text is in settings.rs");
    let rest = &SETTINGS_SOURCE[start..];
    let end = rest.find("pub fn save(").expect("save follows to_text");
    &rest[..end]
}

/// How many unescaped double quotes the line holds.
fn unescaped_quotes(line: &str) -> usize {
    let mut count = 0;
    let mut escaped = false;
    for c in line.chars() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '"' {
            count += 1;
        }
    }
    count
}

#[test]
fn no_string_literal_in_to_text_spans_a_line_break() {
    let source = to_text_source();
    let open: Vec<(usize, &str)> = source.lines().enumerate().filter(|(_, line)| unescaped_quotes(line) % 2 == 1).collect();
    assert!(open.is_empty(), "string literal left open at a line end (line offset, text): {open:?}");
}

#[test]
fn to_text_source_holds_no_carriage_return() {
    assert!(!to_text_source().contains('\r'));
}

#[test]
fn to_text_of_a_fully_set_file_is_unchanged() {
    let mut s = Settings::default();
    s.set("session_folder", "/x/y").unwrap();
    s.hidden_panels = vec!["a".to_string(), "b".to_string()];
    s.set("crossfade_cut_join_ms", "3").unwrap();
    s.set("crossfade_edge_ms", "7").unwrap();
    s.set("output_device", "Out").unwrap();
    s.set("input_device", "In").unwrap();
    s.set("time_display", "smpte_25").unwrap();
    let expected = concat!(
        "# WavCraft settings. One `key = value` per line.\n",
        "# session_folder: where opened files are kept while you work on them (empty: the default).\n",
        "session_folder = /x/y\n",
        "# hidden_panels: ids of panels hidden in the Window menu, comma separated (empty: all shown).\n",
        "hidden_panels = a,b\n",
        "# crossfade_*_ms: edit crossfades in milliseconds (Delete/Cut join, every other boundary); 0 turns one off.\n",
        "crossfade_cut_join_ms = 3\n",
        "crossfade_edge_ms = 7\n",
        "output_device = Out\n",
        "input_device = In\n",
        "# time_display: decimal, compact, samples, smpte_24, smpte_25, smpte_2997df, smpte_2997ndf, smpte_30, smpte_5994, custom, cd75 or bars_beats (View > Time Display).\n",
        "time_display = smpte_25\n",
    );
    assert_eq!(s.to_text(), expected);
}

#[test]
fn to_text_of_the_defaults_is_unchanged() {
    let expected = concat!(
        "# WavCraft settings. One `key = value` per line.\n",
        "# session_folder: where opened files are kept while you work on them (empty: the default).\n",
        "session_folder = \n",
    );
    assert_eq!(Settings::default().to_text(), expected);
}
