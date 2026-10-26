//! a287: the Video preferences (timecode overlay, spot frame) kept in the settings file's text.

use wavcraft_core::settings::Settings;
use wavcraft_core::settings::video::{
    COMMENT, DEFAULT_OVERLAY_SIZE, KEY_OVERLAY_CORNER, KEY_OVERLAY_SIZE, KEY_OVERLAY_TIMECODE, KEY_SPOT_FRAME_ON_SCRUB, KEYS, MAX_OVERLAY_SIZE,
    MIN_OVERLAY_SIZE, OverlayCorner, VideoPrefs,
};

const SIZE_ERR: &str = "video_overlay_size is a whole number of percent of the picture height, 2 to 20";
const CORNER_ERR: &str = "video_overlay_corner is top-left, top-right, bottom-left or bottom-right";
const TIMECODE_ERR: &str = "video_overlay_timecode is true or false";
const SPOT_ERR: &str = "video_spot_frame_on_scrub is true or false";

fn custom() -> VideoPrefs {
    let mut p = VideoPrefs::default();
    p.set("video_overlay_timecode", "true").unwrap();
    p.set("video_overlay_corner", "top-right").unwrap();
    p.set("video_overlay_size", "12").unwrap();
    p.set("video_spot_frame_on_scrub", "false").unwrap();
    p
}

fn default_block() -> String {
    format!(
        "{COMMENT}\nvideo_overlay_timecode = false\nvideo_overlay_corner = bottom-left\nvideo_overlay_size = 5\nvideo_spot_frame_on_scrub = true\n"
    )
}

fn custom_block() -> String {
    format!(
        "{COMMENT}\nvideo_overlay_timecode = true\nvideo_overlay_corner = top-right\nvideo_overlay_size = 12\nvideo_spot_frame_on_scrub = false\n"
    )
}

#[test]
fn the_constants_and_defaults_are_as_documented() {
    assert_eq!(MIN_OVERLAY_SIZE, 2);
    assert_eq!(MAX_OVERLAY_SIZE, 20);
    assert_eq!(DEFAULT_OVERLAY_SIZE, 5);
    assert_eq!(KEYS, ["video_overlay_timecode", "video_overlay_corner", "video_overlay_size", "video_spot_frame_on_scrub"]);
    assert_eq!(KEY_OVERLAY_TIMECODE, KEYS[0]);
    assert_eq!(KEY_OVERLAY_CORNER, KEYS[1]);
    assert_eq!(KEY_OVERLAY_SIZE, KEYS[2]);
    assert_eq!(KEY_SPOT_FRAME_ON_SCRUB, KEYS[3]);
    let p = VideoPrefs::default();
    assert!(!p.overlay_timecode());
    assert_eq!(p.overlay_corner(), OverlayCorner::BottomLeft);
    assert_eq!(p.overlay_size_percent(), 5);
    assert!(p.spot_frame_on_scrub());
    assert_eq!(OverlayCorner::default(), OverlayCorner::BottomLeft);
}

#[test]
fn get_reads_each_key_as_text() {
    let d = VideoPrefs::default();
    assert_eq!(d.get("video_overlay_timecode").unwrap(), "false");
    assert_eq!(d.get("video_overlay_corner").unwrap(), "bottom-left");
    assert_eq!(d.get("video_overlay_size").unwrap(), "5");
    assert_eq!(d.get("video_spot_frame_on_scrub").unwrap(), "true");
    let c = custom();
    assert_eq!(c.get("video_overlay_timecode").unwrap(), "true");
    assert_eq!(c.get("video_overlay_corner").unwrap(), "top-right");
    assert_eq!(c.get("video_overlay_size").unwrap(), "12");
    assert_eq!(c.get("video_spot_frame_on_scrub").unwrap(), "false");
}

#[test]
fn set_changes_the_value_the_getters_and_get_report() {
    let mut p = VideoPrefs::default();
    p.set("video_overlay_timecode", "true").unwrap();
    assert!(p.overlay_timecode());
    assert_eq!(p.get("video_overlay_timecode").unwrap(), "true");
    p.set("video_overlay_corner", "bottom-right").unwrap();
    assert_eq!(p.overlay_corner(), OverlayCorner::BottomRight);
    assert_eq!(p.get("video_overlay_corner").unwrap(), "bottom-right");
    p.set("video_overlay_size", "9").unwrap();
    assert_eq!(p.overlay_size_percent(), 9);
    assert_eq!(p.get("video_overlay_size").unwrap(), "9");
    p.set("video_spot_frame_on_scrub", "false").unwrap();
    assert!(!p.spot_frame_on_scrub());
    assert_eq!(p.get("video_spot_frame_on_scrub").unwrap(), "false");
    // Each key changed only itself.
    assert_eq!(p.get("video_overlay_timecode").unwrap(), "true");
    assert_eq!(p.get("video_overlay_corner").unwrap(), "bottom-right");
}

#[test]
fn each_corner_word_round_trips_and_is_spelled_as_documented() {
    let all = [
        (OverlayCorner::TopLeft, "top-left"),
        (OverlayCorner::TopRight, "top-right"),
        (OverlayCorner::BottomLeft, "bottom-left"),
        (OverlayCorner::BottomRight, "bottom-right"),
    ];
    for (corner, word) in all {
        assert_eq!(corner.as_str(), word);
        assert_eq!(OverlayCorner::from_text(word), Some(corner));
    }
}

#[test]
fn corner_text_ignores_case_and_spaces_and_refuses_anything_else() {
    assert_eq!(OverlayCorner::from_text("  Top-RIGHT "), Some(OverlayCorner::TopRight));
    assert_eq!(OverlayCorner::from_text("BOTTOM-LEFT"), Some(OverlayCorner::BottomLeft));
    for bad in ["", "top", "left", "center", "bottom_left", "top left", "top-left-ish", "1"] {
        assert_eq!(OverlayCorner::from_text(bad), None, "{bad:?}");
    }
}

#[test]
fn a_bad_corner_is_refused_with_its_message_and_changes_nothing() {
    let mut p = custom();
    let before = p.clone();
    let err = p.set("video_overlay_corner", "center").unwrap_err();
    assert!(err.contains(CORNER_ERR), "{err}");
    let err = p.set("video_overlay_corner", "").unwrap_err();
    assert!(err.contains(CORNER_ERR), "{err}");
    assert_eq!(p, before);
}

#[test]
fn booleans_accept_true_and_false_in_any_case_with_spaces() {
    let mut p = VideoPrefs::default();
    p.set("video_overlay_timecode", " TRUE ").unwrap();
    assert!(p.overlay_timecode());
    p.set("video_overlay_timecode", "False").unwrap();
    assert!(!p.overlay_timecode());
    p.set("video_spot_frame_on_scrub", "false").unwrap();
    assert!(!p.spot_frame_on_scrub());
    p.set("video_spot_frame_on_scrub", " True").unwrap();
    assert!(p.spot_frame_on_scrub());
}

#[test]
fn a_bad_boolean_is_refused_with_its_message_and_changes_nothing() {
    let mut p = custom();
    let before = p.clone();
    for bad in ["yes", "no", "1", "0", "on", "", "truee", "tru"] {
        let err = p.set("video_overlay_timecode", bad).unwrap_err();
        assert!(err.contains(TIMECODE_ERR), "{bad:?}: {err}");
        let err = p.set("video_spot_frame_on_scrub", bad).unwrap_err();
        assert!(err.contains(SPOT_ERR), "{bad:?}: {err}");
    }
    assert_eq!(p, before);
}

#[test]
fn the_size_accepts_2_to_20_and_refuses_one_step_outside() {
    let mut p = VideoPrefs::default();
    // Lower bound at n-1, n, n+1.
    let err = p.set("video_overlay_size", "1").unwrap_err();
    assert!(err.contains(SIZE_ERR), "{err}");
    assert_eq!(p.overlay_size_percent(), 5);
    p.set("video_overlay_size", "2").unwrap();
    assert_eq!(p.overlay_size_percent(), 2);
    p.set("video_overlay_size", "3").unwrap();
    assert_eq!(p.overlay_size_percent(), 3);
    // Upper bound at n-1, n, n+1.
    p.set("video_overlay_size", "19").unwrap();
    assert_eq!(p.overlay_size_percent(), 19);
    p.set("video_overlay_size", "20").unwrap();
    assert_eq!(p.overlay_size_percent(), 20);
    let err = p.set("video_overlay_size", "21").unwrap_err();
    assert!(err.contains(SIZE_ERR), "{err}");
    assert_eq!(p.overlay_size_percent(), 20);
}

#[test]
fn a_bad_size_is_refused_with_its_message_and_changes_nothing() {
    let mut p = custom();
    let before = p.clone();
    for bad in ["0", "255", "256", "300", "-1", "-5", "abc", "", "5.5", "1e1", "ten", "5 %"] {
        let err = p.set("video_overlay_size", bad).unwrap_err();
        assert!(err.contains(SIZE_ERR), "{bad:?}: {err}");
    }
    assert_eq!(p, before);
}

#[test]
fn set_ignores_spaces_around_the_value() {
    let mut p = VideoPrefs::default();
    p.set("video_overlay_size", "  7 ").unwrap();
    assert_eq!(p.overlay_size_percent(), 7);
    p.set("video_overlay_corner", " top-left ").unwrap();
    assert_eq!(p.overlay_corner(), OverlayCorner::TopLeft);
}

#[test]
fn an_unknown_key_is_an_error_for_get_and_set() {
    let mut p = custom();
    let before = p.clone();
    let keys = "the keys are video_overlay_timecode, video_overlay_corner, video_overlay_size, video_spot_frame_on_scrub";
    for key in ["video_overlay_colour", "session_folder", "crossfade_edge_ms", "", "VIDEO_OVERLAY_SIZE"] {
        let err = p.set(key, "5").unwrap_err();
        assert!(err.contains("unknown video preference"), "{key:?}: {err}");
        assert!(err.contains(keys), "{key:?}: {err}");
        let err = p.get(key).unwrap_err();
        assert!(err.contains("unknown video preference"), "{key:?}: {err}");
        assert!(err.contains(keys), "{key:?}: {err}");
    }
    assert_eq!(p, before);
}

#[test]
fn load_from_nothing_gives_the_defaults() {
    assert_eq!(VideoPrefs::load_from(""), VideoPrefs::default());
    assert_eq!(VideoPrefs::load_from("\n\n   \n"), VideoPrefs::default());
}

#[test]
fn load_from_reads_all_four_keys() {
    let text = "video_overlay_timecode = true\nvideo_overlay_corner = top-right\nvideo_overlay_size = 12\nvideo_spot_frame_on_scrub = false\n";
    assert_eq!(VideoPrefs::load_from(text), custom());
}

#[test]
fn load_from_keeps_the_default_for_each_bad_value() {
    let text = "video_overlay_timecode = maybe\nvideo_overlay_corner = center\nvideo_overlay_size = 99\nvideo_spot_frame_on_scrub = 7\n";
    assert_eq!(VideoPrefs::load_from(text), VideoPrefs::default());
}

#[test]
fn load_from_keeps_good_values_next_to_bad_ones() {
    let text = "video_overlay_timecode = true\nvideo_overlay_size = abc\nvideo_overlay_corner = top-left\nvideo_spot_frame_on_scrub =\n";
    let p = VideoPrefs::load_from(text);
    assert!(p.overlay_timecode());
    assert_eq!(p.overlay_corner(), OverlayCorner::TopLeft);
    assert_eq!(p.overlay_size_percent(), 5);
    assert!(p.spot_frame_on_scrub());
}

#[test]
fn load_from_ignores_comments_unknown_keys_and_lines_without_equals() {
    // Every odd line comes before the one good line, so a loader that stops at one would miss it.
    let text = "# video_overlay_size = 9\n\nvideo_overlay_size 9\nsession_folder = /home/user/sessions\ncrossfade_edge_ms = 9\nvideo_unknown = 1\nrandom words\nvideo_overlay_corner = top-right\n";
    let mut expected = VideoPrefs::default();
    expected.set("video_overlay_corner", "top-right").unwrap();
    assert_eq!(VideoPrefs::load_from(text), expected);
    assert_eq!(VideoPrefs::load_from(text).overlay_size_percent(), 5);
}

#[test]
fn load_from_ignores_spaces_around_keys_and_values_and_windows_line_ends() {
    let p = VideoPrefs::load_from("  video_overlay_size  =  9  \r\nvideo_overlay_timecode=true\r\n");
    assert_eq!(p.overlay_size_percent(), 9);
    assert!(p.overlay_timecode());
}

#[test]
fn load_from_takes_the_last_good_value_of_a_repeated_key() {
    assert_eq!(VideoPrefs::load_from("video_overlay_size = 7\nvideo_overlay_size = 9\n").overlay_size_percent(), 9);
    assert_eq!(VideoPrefs::load_from("video_overlay_size = 7\nvideo_overlay_size = 99\n").overlay_size_percent(), 7);
}

#[test]
fn to_text_writes_the_comment_and_four_lines() {
    assert_eq!(VideoPrefs::default().to_text(), default_block());
    assert_eq!(custom().to_text(), custom_block());
    assert!(COMMENT.starts_with("# video_*"));
}

#[test]
fn serialize_into_nothing_is_just_the_block() {
    assert_eq!(VideoPrefs::default().serialize(""), default_block());
    assert_eq!(custom().serialize(""), custom_block());
}

#[test]
fn serialize_keeps_every_other_line_unchanged_and_in_order() {
    let existing = "# WavCraft settings.\nsession_folder = /home/user/sessions\n\ncrossfade_cut_join_ms = 7\n  output_device = Speakers (USB)  \nmystery = 1\nnot a setting\n";
    let out = custom().serialize(existing);
    assert_eq!(out, format!("{existing}{}", custom_block()));
}

#[test]
fn serialize_replaces_its_own_lines_and_old_comment_wherever_they_are() {
    let existing = format!(
        "# WavCraft settings.\nsession_folder = /home/user/sessions\nvideo_overlay_size = 9\n{COMMENT}\ncrossfade_edge_ms = 7\n   video_overlay_corner   =   top-left  \nvideo_spot_frame_on_scrub = true\nvideo_overlay_timecode = nonsense\nmystery = 1\n"
    );
    let out = custom().serialize(&existing);
    let expected = format!("# WavCraft settings.\nsession_folder = /home/user/sessions\ncrossfade_edge_ms = 7\nmystery = 1\n{}", custom_block());
    assert_eq!(out, expected);
    assert_eq!(out.matches("video_overlay_size").count(), 1);
}

#[test]
fn serialize_removes_every_repeat_of_its_keys() {
    let out = custom().serialize("video_overlay_size = 3\nvideo_overlay_size = 4\nvideo_overlay_size = 5\n");
    assert_eq!(out, custom_block());
}

#[test]
fn serialize_keeps_commented_out_and_look_alike_lines() {
    let existing = "# video_overlay_size = 9\nvideo_overlay_size_extra = 1\nmy_video_overlay_size = 2\nvideo_overlay = 3\n";
    let out = custom().serialize(existing);
    assert_eq!(out, format!("{existing}{}", custom_block()));
}

#[test]
fn serialize_adds_a_newline_when_the_last_line_has_none() {
    let out = custom().serialize("session_folder = /home/user/sessions");
    assert_eq!(out, format!("session_folder = /home/user/sessions\n{}", custom_block()));
}

#[test]
fn serialize_keeps_windows_line_ends_of_the_other_lines() {
    let out = custom().serialize("a = 1\r\nvideo_overlay_size = 9\r\nb = 2\r\n");
    assert_eq!(out, format!("a = 1\r\nb = 2\r\n{}", custom_block()));
}

#[test]
fn serialize_twice_gives_the_same_text() {
    let existing = "session_folder = /home/user/sessions\nvideo_overlay_size = 9\nmystery = 1\n";
    let once = custom().serialize(existing);
    let twice = custom().serialize(&once);
    assert_eq!(once, twice);
    // A different second set of values replaces the first, not adds to it.
    let other = VideoPrefs::default().serialize(&once);
    assert_eq!(other, format!("session_folder = /home/user/sessions\nmystery = 1\n{}", default_block()));
}

#[test]
fn serialize_then_load_from_gives_the_same_preferences() {
    for p in [VideoPrefs::default(), custom()] {
        assert_eq!(VideoPrefs::load_from(&p.serialize("")), p);
        assert_eq!(VideoPrefs::load_from(&p.serialize("mystery = 1\nvideo_overlay_size = 3\n")), p);
    }
}

#[test]
fn the_settings_file_keeps_its_own_lines_and_ignores_ours() {
    let settings_text = Settings::default().to_text();
    let out = custom().serialize(&settings_text);
    assert!(out.starts_with(&settings_text));
    assert_eq!(Settings::parse(&out), Settings::default());
    assert_eq!(VideoPrefs::load_from(&out), custom());
    // The Settings keys and ours never overlap.
    for key in KEYS {
        assert!(!wavcraft_core::settings::KEYS.contains(&key), "{key}");
    }
}

#[test]
fn settings_text_with_our_keys_still_loads_as_settings() {
    let mut s = Settings::default();
    s.set("crossfade_edge_ms", "9").unwrap();
    let out = custom().serialize(&s.to_text());
    assert_eq!(Settings::parse(&out), s);
}
