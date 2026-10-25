//! Track colour and notes (C105): the validated colour setter, the named palette and the track
//! notes on `Multitrack`.

use wavcraft_core::multitrack::soundcraft_model::{BitDepthSetting, MAX_COMMENT_CHARS, Session, TrackId};
use wavcraft_core::multitrack::track_info::{TRACK_NOTES_MAX_CHARS, clean_track_notes, default_color, format_color, palette, parse_color};
use wavcraft_core::multitrack::{MtError, Multitrack};

fn two_tracks() -> (Multitrack, TrackId, TrackId) {
    let mut m = Multitrack::new("Demo", 48_000, BitDepthSetting::Int24).expect("new session");
    let a = m.add_track(Some("Guitar"), 2).expect("track a");
    let b = m.add_track(Some("Voice"), 1).expect("track b");
    (m, a, b)
}

fn gone_id(m: &mut Multitrack) -> TrackId {
    let id = m.add_track(Some("Temp"), 1).expect("temp track");
    m.remove_track(id).expect("remove temp");
    id
}

fn msg(r: Result<impl std::fmt::Debug, MtError>) -> String {
    r.expect_err("should be refused").0
}

// ---- palette -------------------------------------------------------------------------------

#[test]
fn the_palette_is_eight_distinct_named_colours() {
    let p = palette();
    assert_eq!(p.len(), 8);
    assert_eq!(p[0], ("Red", [220, 60, 60]));
    assert_eq!(p[7], ("Pink", [220, 100, 160]));
    for (i, (name, color)) in p.iter().enumerate() {
        assert!(!name.is_empty());
        for (j, (n2, c2)) in p.iter().enumerate() {
            if i != j {
                assert_ne!(name, n2, "names {i} and {j}");
                assert_ne!(color, c2, "colours {i} and {j}");
            }
        }
    }
}

#[test]
fn default_color_walks_the_palette_in_order() {
    assert_eq!(palette().len(), 8);
    for (n, (_, color)) in palette().iter().enumerate() {
        assert_eq!(default_color(n), *color, "n = {n}");
    }
}

#[test]
fn default_color_cycles_after_the_palette_length() {
    assert_eq!(default_color(8), default_color(0));
    assert_eq!(default_color(9), default_color(1));
    assert_eq!(default_color(15), default_color(7));
    assert_eq!(default_color(16), default_color(0));
    assert_ne!(default_color(7), default_color(8));
    assert_eq!(default_color(usize::MAX), palette()[7].1);
}

#[test]
fn default_color_never_repeats_within_a_palette_length() {
    for start in 0..20 {
        let run: Vec<[u8; 3]> = (start..start + 8).map(default_color).collect();
        for i in 0..run.len() {
            for j in (i + 1)..run.len() {
                assert_ne!(run[i], run[j], "start {start}: {i} and {j}");
            }
        }
    }
}

// ---- colour text ---------------------------------------------------------------------------

#[test]
fn parse_color_reads_six_hex_digits() {
    assert_eq!(parse_color("#DC3C3C"), Ok([220, 60, 60]));
    assert_eq!(parse_color("#000000"), Ok([0, 0, 0]));
    assert_eq!(parse_color("#FFFFFF"), Ok([255, 255, 255]));
    assert_eq!(parse_color("#010203"), Ok([1, 2, 3]));
    assert_eq!(parse_color("#0a0B0c"), Ok([10, 11, 12]));
}

#[test]
fn parse_color_allows_no_hash_and_surrounding_space() {
    assert_eq!(parse_color("dc3c3c"), Ok([220, 60, 60]));
    assert_eq!(parse_color("  #dc3c3c \t"), Ok([220, 60, 60]));
    assert_eq!(parse_color("\n#FF8000\n"), Ok([255, 128, 0]));
}

#[test]
fn parse_color_refuses_malformed_text() {
    let bad = [
        "",
        "   ",
        "#",
        "#fff",
        "#12345",
        "#1234567",
        "#GG0000",
        "#12 456",
        "##123456",
        "0x123456",
        "#+1+2+3",
        "-1-2-3",
        "#12345é",
        "#ééé",
        "rgb(1,2,3)",
        "red",
        "#12345\u{0}",
    ];
    for text in bad {
        assert!(msg(parse_color(text)).contains("#RRGGBB"), "{text:?}");
    }
}

#[test]
fn format_color_is_upper_case_hex_with_padding() {
    assert_eq!(format_color([220, 60, 60]), "#DC3C3C");
    assert_eq!(format_color([0, 0, 0]), "#000000");
    assert_eq!(format_color([1, 2, 3]), "#010203");
    assert_eq!(format_color([255, 255, 255]), "#FFFFFF");
    assert_eq!(format_color([10, 11, 12]), "#0A0B0C");
}

#[test]
fn format_then_parse_gives_the_colour_back() {
    for (_, color) in palette() {
        assert_eq!(parse_color(&format_color(*color)), Ok(*color));
    }
    for color in [[0, 0, 0], [255, 255, 255], [0, 255, 0], [16, 1, 160]] {
        assert_eq!(parse_color(&format_color(color)), Ok(color));
    }
}

// ---- track colour --------------------------------------------------------------------------

#[test]
fn track_color_reads_the_models_colour() {
    let (m, a, b) = two_tracks();
    assert_eq!(m.track_color(a), Ok(m.session.track(a).expect("track a").color));
    assert_eq!(m.track_color(b), Ok(m.session.track(b).expect("track b").color));
    assert_ne!(m.track_color(a), m.track_color(b));
}

#[test]
fn set_track_color_changes_one_track_and_returns_the_old_colour() {
    let (mut m, a, b) = two_tracks();
    let old_a = m.track_color(a).expect("colour a");
    let old_b = m.track_color(b).expect("colour b");
    assert_eq!(m.set_track_color(a, [1, 2, 3]), Ok(old_a));
    assert_eq!(m.track_color(a), Ok([1, 2, 3]));
    assert_eq!(m.track_color(b), Ok(old_b));
    assert_eq!(m.session.track(a).expect("track a").color, [1, 2, 3]);
    assert_eq!(m.set_track_color(a, [9, 9, 9]), Ok([1, 2, 3]));
}

#[test]
fn set_track_color_to_the_same_colour_is_accepted() {
    let (mut m, a, _) = two_tracks();
    let old = m.track_color(a).expect("colour a");
    assert_eq!(m.set_track_color(a, old), Ok(old));
    assert_eq!(m.track_color(a), Ok(old));
}

#[test]
fn set_track_color_leaves_the_rest_of_the_track_alone() {
    let (mut m, a, _) = two_tracks();
    m.set_mute(a, true).expect("mute");
    let before = m.session.track(a).expect("track a").clone();
    m.set_track_color(a, [7, 8, 9]).expect("colour");
    let mut after = m.session.track(a).expect("track a").clone();
    assert_eq!(after.color, [7, 8, 9]);
    after.color = before.color;
    assert_eq!(after, before);
}

#[test]
fn an_unknown_track_is_refused_for_colour() {
    let (mut m, a, b) = two_tracks();
    let gone = gone_id(&mut m);
    let (ca, cb) = (m.track_color(a).expect("colour a"), m.track_color(b).expect("colour b"));
    assert_eq!(msg(m.track_color(gone)), "no such track");
    assert_eq!(msg(m.set_track_color(gone, [1, 2, 3])), "no such track");
    assert_eq!(msg(m.set_track_color_hex(gone, "#010203")), "no such track");
    assert_eq!(m.track_color(a), Ok(ca));
    assert_eq!(m.track_color(b), Ok(cb));
}

#[test]
fn set_track_color_hex_sets_and_returns_the_old_colour() {
    let (mut m, a, b) = two_tracks();
    let old_a = m.track_color(a).expect("colour a");
    let old_b = m.track_color(b).expect("colour b");
    assert_eq!(m.set_track_color_hex(a, "#dc3c3c"), Ok(old_a));
    assert_eq!(m.track_color(a), Ok([220, 60, 60]));
    assert_eq!(m.track_color(b), Ok(old_b));
    assert_eq!(m.set_track_color_hex(a, " 0A0b0C "), Ok([220, 60, 60]));
    assert_eq!(m.track_color(a), Ok([10, 11, 12]));
}

#[test]
fn set_track_color_hex_refuses_malformed_text_and_changes_nothing() {
    let (mut m, a, _) = two_tracks();
    let old = m.track_color(a).expect("colour a");
    for text in ["", "#fff", "#12345", "#1234567", "#GG0000", "#+1+2+3", "red"] {
        assert!(msg(m.set_track_color_hex(a, text)).contains("#RRGGBB"), "{text:?}");
        assert_eq!(m.track_color(a), Ok(old), "{text:?}");
    }
}

#[test]
fn the_colour_survives_the_models_json_round_trip() {
    let (mut m, a, b) = two_tracks();
    m.set_track_color(a, [11, 22, 33]).expect("colour a");
    let text = m.to_json().expect("to json");
    let back = Session::from_json(&text).expect("from json");
    assert_eq!(back.track(a).expect("track a").color, [11, 22, 33]);
    assert_eq!(back.track(b).expect("track b").color, m.track_color(b).expect("colour b"));
}

// ---- notes text ----------------------------------------------------------------------------

#[test]
fn the_notes_limit_is_the_models() {
    assert_eq!(TRACK_NOTES_MAX_CHARS, 4096);
    assert_eq!(TRACK_NOTES_MAX_CHARS, MAX_COMMENT_CHARS);
}

#[test]
fn clean_track_notes_trims_and_keeps_inner_text() {
    assert_eq!(clean_track_notes("  take 3, keep  "), Ok(String::from("take 3, keep")));
    assert_eq!(clean_track_notes("line one\nline two\n\tindented"), Ok(String::from("line one\nline two\n\tindented")));
    assert_eq!(clean_track_notes("\n\n  a b  \n"), Ok(String::from("a b")));
}

#[test]
fn clean_track_notes_blank_text_is_empty() {
    assert_eq!(clean_track_notes(""), Ok(String::new()));
    assert_eq!(clean_track_notes("   "), Ok(String::new()));
    assert_eq!(clean_track_notes(" \n\t \n "), Ok(String::new()));
}

#[test]
fn clean_track_notes_keeps_unicode_as_typed() {
    let text = "Caf\u{e9} \u{65e5}\u{672c}\u{8a9e} \u{1f3b5} na\u{ef}ve";
    assert_eq!(clean_track_notes(text), Ok(text.to_string()));
}

#[test]
fn clean_track_notes_refuses_control_characters_but_not_newline_or_tab() {
    for c in ['\u{0}', '\u{7}', '\u{8}', '\r', '\u{1b}', '\u{7f}', '\u{85}'] {
        let text = format!("a{c}b");
        assert!(msg(clean_track_notes(&text)).contains("control characters"), "U+{:04X}", c as u32);
    }
    assert_eq!(clean_track_notes("a\nb\tc"), Ok(String::from("a\nb\tc")));
    assert!(msg(clean_track_notes("a\r\nb")).contains("U+000D"));
}

#[test]
fn clean_track_notes_accepts_exactly_the_limit_and_refuses_one_more() {
    let at = "a".repeat(4095);
    let on = "a".repeat(4096);
    let over = "a".repeat(4097);
    assert_eq!(clean_track_notes(&at), Ok(at.clone()));
    assert_eq!(clean_track_notes(&on), Ok(on.clone()));
    let m = msg(clean_track_notes(&over));
    assert!(m.contains("at most 4096 characters"), "{m}");
    assert!(m.contains("has 4097"), "{m}");
}

#[test]
fn clean_track_notes_counts_characters_not_bytes() {
    let wide = "\u{65e5}".repeat(4096);
    assert_eq!(wide.len(), 12_288);
    assert_eq!(clean_track_notes(&wide), Ok(wide.clone()));
    assert!(clean_track_notes(&"\u{65e5}".repeat(4097)).is_err());
    let emoji = "\u{1f3b5}".repeat(4096);
    assert_eq!(clean_track_notes(&emoji), Ok(emoji.clone()));
    assert!(clean_track_notes(&"\u{1f3b5}".repeat(4097)).is_err());
}

#[test]
fn clean_track_notes_counts_after_trimming() {
    let padded = format!("  {}  \n", "a".repeat(4096));
    assert_eq!(clean_track_notes(&padded), Ok("a".repeat(4096)));
    let padded_over = format!("  {}  \n", "a".repeat(4097));
    assert!(clean_track_notes(&padded_over).is_err());
}

// ---- track notes ---------------------------------------------------------------------------

#[test]
fn a_new_track_has_no_notes() {
    let (m, a, b) = two_tracks();
    assert_eq!(m.track_notes(a), Ok(""));
    assert_eq!(m.track_notes(b), Ok(""));
}

#[test]
fn set_track_notes_sets_one_track_and_returns_the_old_notes() {
    let (mut m, a, b) = two_tracks();
    assert_eq!(m.set_track_notes(a, "  DI, no amp  "), Ok(String::new()));
    assert_eq!(m.track_notes(a), Ok("DI, no amp"));
    assert_eq!(m.track_notes(b), Ok(""));
    assert_eq!(m.session.track(a).expect("track a").comments, "DI, no amp");
    assert_eq!(m.set_track_notes(a, "second\nline"), Ok(String::from("DI, no amp")));
    assert_eq!(m.track_notes(a), Ok("second\nline"));
}

#[test]
fn blank_text_clears_the_track_notes() {
    let (mut m, a, _) = two_tracks();
    m.set_track_notes(a, "keep me").expect("set");
    assert_eq!(m.set_track_notes(a, " \n\t "), Ok(String::from("keep me")));
    assert_eq!(m.track_notes(a), Ok(""));
    m.set_track_notes(a, "again").expect("set again");
    assert_eq!(m.set_track_notes(a, ""), Ok(String::from("again")));
    assert_eq!(m.track_notes(a), Ok(""));
}

#[test]
fn refused_track_notes_change_nothing() {
    let (mut m, a, _) = two_tracks();
    m.set_track_notes(a, "original").expect("set");
    assert!(msg(m.set_track_notes(a, "bad\u{0}text")).contains("control characters"));
    assert_eq!(m.track_notes(a), Ok("original"));
    let over = "x".repeat(4097);
    assert!(msg(m.set_track_notes(a, &over)).contains("4096"));
    assert_eq!(m.track_notes(a), Ok("original"));
    let on = "y".repeat(4096);
    assert_eq!(m.set_track_notes(a, &on), Ok(String::from("original")));
    assert_eq!(m.track_notes(a).map(str::len), Ok(4096));
}

#[test]
fn track_notes_are_per_track() {
    let (mut m, a, b) = two_tracks();
    m.set_track_notes(a, "for a").expect("a");
    m.set_track_notes(b, "for b").expect("b");
    assert_eq!(m.track_notes(a), Ok("for a"));
    assert_eq!(m.track_notes(b), Ok("for b"));
    m.set_track_notes(a, "").expect("clear a");
    assert_eq!(m.track_notes(b), Ok("for b"));
}

#[test]
fn an_unknown_track_is_refused_for_notes() {
    let (mut m, a, _) = two_tracks();
    m.set_track_notes(a, "mine").expect("set");
    let gone = gone_id(&mut m);
    assert_eq!(msg(m.track_notes(gone)), "no such track");
    assert_eq!(msg(m.set_track_notes(gone, "x")), "no such track");
    assert_eq!(msg(m.set_track_notes(gone, "")), "no such track");
    assert_eq!(m.track_notes(a), Ok("mine"));
}

#[test]
fn set_track_notes_leaves_the_rest_of_the_track_alone() {
    let (mut m, a, _) = two_tracks();
    m.set_track_color(a, [4, 5, 6]).expect("colour");
    let before = m.session.track(a).expect("track a").clone();
    m.set_track_notes(a, "hello").expect("notes");
    let mut after = m.session.track(a).expect("track a").clone();
    assert_eq!(after.comments, "hello");
    after.comments = before.comments.clone();
    assert_eq!(after, before);
}

#[test]
fn track_notes_survive_the_models_json_round_trip_with_unicode() {
    let (mut m, a, b) = two_tracks();
    let text = "Caf\u{e9} \u{65e5}\u{672c}\u{8a9e} \u{1f3b5}\nline two\tTabbed";
    m.set_track_notes(a, text).expect("notes");
    let json = m.to_json().expect("to json");
    let back = Session::from_json(&json).expect("from json");
    assert_eq!(back.track(a).expect("track a").comments, text);
    assert_eq!(back.track(b).expect("track b").comments, "");
}

#[test]
fn a_full_length_note_survives_the_round_trip_uncut() {
    let (mut m, a, _) = two_tracks();
    let text = "\u{65e5}".repeat(4096);
    m.set_track_notes(a, &text).expect("notes");
    let back = Session::from_json(&m.to_json().expect("to json")).expect("from json");
    assert_eq!(back.track(a).expect("track a").comments.chars().count(), 4096);
}
