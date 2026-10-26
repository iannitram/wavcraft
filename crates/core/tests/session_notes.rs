//! Roadmap C094: a session's title and notes, validated, and kept through the model's JSON.
//!
//! The model's `Session` already stores both (`name`, `comments`); these tests cover the rules
//! the editing side adds (length, trimming, control characters, empty, Unicode) and prove that
//! a session file written before notes existed still loads.

use wavcraft_core::multitrack::notes::{NOTES_MAX_CHARS, TITLE_MAX_CHARS, clean_notes, clean_title};
use wavcraft_core::multitrack::soundcraft_model::{self, BitDepthSetting, Session};
use wavcraft_core::multitrack::{MtError, Multitrack};

fn session() -> Multitrack {
    Multitrack::new("Demo", 48_000, BitDepthSetting::Int24).unwrap_or_else(|e| panic!("{e}"))
}

fn err_text<T>(r: Result<T, MtError>) -> String {
    match r {
        Ok(_) => String::from("(no error)"),
        Err(e) => e.0,
    }
}

#[test]
fn the_limits_match_what_the_model_keeps_on_load() {
    assert_eq!(NOTES_MAX_CHARS, 4096);
    assert_eq!(TITLE_MAX_CHARS, 256);
    assert_eq!(NOTES_MAX_CHARS, soundcraft_model::MAX_COMMENT_CHARS);
    assert_eq!(TITLE_MAX_CHARS, soundcraft_model::MAX_NAME_CHARS);
}

#[test]
fn a_new_session_has_no_notes_and_its_title_is_its_name() {
    let m = session();
    assert_eq!(m.notes(), "");
    assert_eq!(m.title(), "Demo");
    assert_eq!(m.session.comments, "");
}

#[test]
fn notes_are_trimmed_and_kept_in_the_models_comments_field() {
    let mut m = session();
    m.set_notes("  Take 3: keep the intro \n ").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(m.notes(), "Take 3: keep the intro");
    assert_eq!(m.session.comments, "Take 3: keep the intro");
}

#[test]
fn newlines_and_tabs_inside_notes_are_kept() {
    let mut m = session();
    m.set_notes("\n line one\n\tline two\n\n  line four  ").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(m.notes(), "line one\n\tline two\n\n  line four");
}

#[test]
fn blank_notes_clear_them() {
    let mut m = session();
    m.set_notes("something").unwrap_or_else(|e| panic!("{e}"));
    m.set_notes("").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(m.notes(), "");
    m.set_notes("something else").unwrap_or_else(|e| panic!("{e}"));
    m.set_notes("  \n\t  ").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(m.notes(), "");
    assert_eq!(m.session.comments, "");
}

#[test]
fn notes_may_be_exactly_the_maximum_and_not_one_more() {
    let mut m = session();
    let full = "a".repeat(NOTES_MAX_CHARS);
    m.set_notes(&full).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(m.notes().chars().count(), 4096);
    let padded = format!("  {full}\n");
    m.set_notes(&padded).unwrap_or_else(|e| panic!("the padding is not counted: {e}"));
    assert_eq!(m.notes(), full);
    let over = "a".repeat(NOTES_MAX_CHARS + 1);
    let msg = err_text(m.set_notes(&over));
    assert!(msg.contains("at most 4096 characters"), "message was: {msg}");
    assert_eq!(m.notes(), full, "a refused call changes nothing");
}

#[test]
fn the_notes_limit_counts_characters_not_bytes() {
    let mut m = session();
    let accents = "é".repeat(NOTES_MAX_CHARS);
    assert_eq!(accents.len(), 8192);
    m.set_notes(&accents).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(m.notes(), accents);
    let emoji = "😀".repeat(NOTES_MAX_CHARS);
    m.set_notes(&emoji).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(m.notes().chars().count(), 4096);
    let too_many = "😀".repeat(NOTES_MAX_CHARS + 1);
    assert!(m.set_notes(&too_many).is_err());
    assert_eq!(m.notes(), emoji);
}

#[test]
fn unicode_notes_are_kept_exactly_as_typed() {
    let mut m = session();
    let text = "Café 日本語 🎹 mix\ne\u{301} (combining) Привет";
    m.set_notes(text).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(m.notes(), text);
    assert_eq!(m.notes().chars().filter(|c| *c == '\u{301}').count(), 1, "not normalised");
}

#[test]
fn control_characters_in_notes_are_refused_not_stripped() {
    let mut m = session();
    m.set_notes("keep me").unwrap_or_else(|e| panic!("{e}"));
    for bad in ["bad\u{0}x", "\u{0}start", "a\u{7}b", "a\u{1b}[0mb", "a\u{7f}b", "a\rb", "a\u{9b}b"] {
        let msg = err_text(m.set_notes(bad));
        assert!(msg.contains("control character"), "{bad:?} gave: {msg}");
        assert_eq!(m.notes(), "keep me", "{bad:?} changed the notes");
    }
}

#[test]
fn a_trailing_line_ending_is_trimmed_before_the_control_check() {
    let mut m = session();
    m.set_notes("note\r\n").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(m.notes(), "note");
}

#[test]
fn clean_notes_gives_the_text_that_would_be_stored() {
    assert_eq!(clean_notes("  \n "), Ok(String::new()));
    assert_eq!(clean_notes(" a b "), Ok(String::from("a b")));
    assert!(clean_notes("a\u{1}b").is_err());
    assert!(clean_notes(&"x".repeat(NOTES_MAX_CHARS + 1)).is_err());
    assert!(clean_notes(&"x".repeat(NOTES_MAX_CHARS)).is_ok());
}

#[test]
fn a_title_is_trimmed_and_is_the_sessions_name() {
    let mut m = session();
    m.set_title("  Mix v2  ").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(m.title(), "Mix v2");
    assert_eq!(m.name(), "Mix v2");
    assert_eq!(m.session.name, "Mix v2");
}

#[test]
fn an_empty_title_is_refused_and_changes_nothing() {
    let mut m = session();
    for blank in ["", "   ", " \t\n"] {
        let msg = err_text(m.set_title(blank));
        assert!(msg.contains("cannot be empty"), "{blank:?} gave: {msg}");
        assert_eq!(m.title(), "Demo");
    }
}

#[test]
fn a_title_is_one_line_with_no_control_characters() {
    let mut m = session();
    for bad in ["A\nB", "A\tB", "A\u{0}B", "A\rB", "A\u{7f}B"] {
        let msg = err_text(m.set_title(bad));
        assert!(msg.contains("control character"), "{bad:?} gave: {msg}");
        assert_eq!(m.title(), "Demo");
    }
}

#[test]
fn a_title_may_be_exactly_the_maximum_and_not_one_more() {
    let mut m = session();
    let full = "t".repeat(TITLE_MAX_CHARS);
    m.set_title(&full).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(m.title().chars().count(), 256);
    let msg = err_text(m.set_title(&"t".repeat(TITLE_MAX_CHARS + 1)));
    assert!(msg.contains("at most 256 characters"), "message was: {msg}");
    assert_eq!(m.title(), full);
    let wide = "日".repeat(TITLE_MAX_CHARS);
    m.set_title(&wide).unwrap_or_else(|e| panic!("characters, not bytes: {e}"));
    assert_eq!(m.title(), wide);
}

#[test]
fn unicode_titles_are_kept_and_clean_title_matches() {
    let mut m = session();
    m.set_title("Café 日本 🎹").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(m.title(), "Café 日本 🎹");
    assert_eq!(clean_title(" a "), Ok(String::from("a")));
    assert!(clean_title("").is_err());
    assert!(clean_title("a\nb").is_err());
}

#[test]
fn notes_and_title_do_not_touch_each_other_or_the_tracks() {
    let mut m = session();
    m.add_track(Some("Voice"), 1).unwrap_or_else(|e| panic!("{e}"));
    m.set_notes("n").unwrap_or_else(|e| panic!("{e}"));
    let tracks = m.session.tracks.clone();
    m.set_title("T").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((m.title(), m.notes()), ("T", "n"));
    m.set_notes("n2").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((m.title(), m.notes()), ("T", "n2"));
    assert_eq!(m.session.tracks, tracks);
    assert_eq!(m.track_count(), 1);
}

#[test]
fn title_and_notes_round_trip_through_the_session_json() {
    let mut m = session();
    m.add_track(None, 2).unwrap_or_else(|e| panic!("{e}"));
    let notes = format!("Café 日本語 🎹\n\tsecond line\n{}", "é".repeat(NOTES_MAX_CHARS - 40));
    m.set_title("Mix 日本 🎹").unwrap_or_else(|e| panic!("{e}"));
    m.set_notes(&notes).unwrap_or_else(|e| panic!("{e}"));
    let text = m.to_json().unwrap_or_else(|e| panic!("{e}"));
    let value: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(value["comments"].as_str(), Some(m.notes()));
    assert_eq!(value["name"].as_str(), Some("Mix 日本 🎹"));
    let back = Session::from_json(&text).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(back.comments, m.notes());
    assert_eq!(back.name, "Mix 日本 🎹");
    assert_eq!(back.tracks, m.session.tracks);
    let mut reopened = session();
    reopened.session = back;
    assert_eq!((reopened.title(), reopened.notes()), (m.title(), m.notes()));
}

#[test]
fn cleared_notes_round_trip_as_empty() {
    let mut m = session();
    m.set_notes("something").unwrap_or_else(|e| panic!("{e}"));
    m.set_notes("").unwrap_or_else(|e| panic!("{e}"));
    let back = Session::from_json(&m.to_json().unwrap_or_else(|e| panic!("{e}"))).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(back.comments, "");
}

#[test]
fn a_session_file_without_the_notes_field_still_loads() {
    let mut m = session();
    m.add_track(Some("Voice"), 1).unwrap_or_else(|e| panic!("{e}"));
    m.add_track(None, 2).unwrap_or_else(|e| panic!("{e}"));
    m.set_notes("will be removed from the file").unwrap_or_else(|e| panic!("{e}"));
    let text = m.to_json().unwrap_or_else(|e| panic!("{e}"));
    let mut value: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}"));
    let removed = value.as_object_mut().and_then(|o| o.remove("comments"));
    assert!(removed.is_some(), "the file really had the field before it was removed");
    assert!(value.get("comments").is_none());
    let old_file = value.to_string();
    let back = Session::from_json(&old_file).unwrap_or_else(|e| panic!("an older file must load: {e}"));
    assert_eq!(back.comments, "");
    assert_eq!(back.name, "Demo");
    assert_eq!(back.tracks, m.session.tracks);
    let mut reopened = session();
    reopened.session = back;
    assert_eq!(reopened.notes(), "");
    assert_eq!(reopened.track_count(), 2);
    reopened.set_notes("added later").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(reopened.notes(), "added later");
}

#[test]
fn a_hand_edited_file_with_oversize_notes_is_cut_by_the_model_which_is_why_longer_notes_are_refused() {
    let m = session();
    let text = m.to_json().unwrap_or_else(|e| panic!("{e}"));
    let mut value: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}"));
    value["comments"] = serde_json::Value::String("z".repeat(NOTES_MAX_CHARS + 904));
    let back = Session::from_json(&value.to_string()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(back.comments.chars().count(), NOTES_MAX_CHARS);
}
