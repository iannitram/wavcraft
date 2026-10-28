//! a283: the Multitrack and Multitrack Clips preferences kept in the settings file.

use std::path::PathBuf;

use wavcraft_core::settings::Settings;
use wavcraft_core::settings::multitrack::{
    COMMENT, ClipColorMode, DEFAULT_CLIP_CROSSFADE_MS, DEFAULT_TRACK_HEIGHT, KEYS, MAX_CLIP_CROSSFADE_MS, MAX_TRACK_HEIGHT, MIN_TRACK_HEIGHT,
    MultitrackPrefs,
};
use wavcraft_core::settings::video::VideoPrefs;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-mtprefs-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn custom() -> MultitrackPrefs {
    let mut p = MultitrackPrefs::default();
    p.set("multitrack_track_height", "200").unwrap();
    p.set("multitrack_clip_crossfade_ms", "40").unwrap();
    p.set("multitrack_clip_color", "clip").unwrap();
    p
}

const DEFAULT_BLOCK: &str = "multitrack_track_height = 88\nmultitrack_clip_crossfade_ms = 5\nmultitrack_clip_color = track\n";
const CUSTOM_BLOCK: &str = "multitrack_track_height = 200\nmultitrack_clip_crossfade_ms = 40\nmultitrack_clip_color = clip\n";

#[test]
fn the_constants_and_defaults_are_as_documented() {
    assert_eq!(MIN_TRACK_HEIGHT, 16);
    assert_eq!(MAX_TRACK_HEIGHT, 440);
    assert_eq!(DEFAULT_TRACK_HEIGHT, 88);
    assert_eq!(DEFAULT_CLIP_CROSSFADE_MS, 5);
    assert_eq!(MAX_CLIP_CROSSFADE_MS, 1000);
    assert_eq!(KEYS, ["multitrack_track_height", "multitrack_clip_crossfade_ms", "multitrack_clip_color"]);
    let d = MultitrackPrefs::default();
    assert_eq!(d.track_height(), 88);
    assert_eq!(d.clip_crossfade_ms(), 5);
    assert_eq!(d.clip_color(), ClipColorMode::Track);
}

#[test]
fn get_reads_each_key_as_text_and_an_unknown_key_is_an_error() {
    let d = MultitrackPrefs::default();
    assert_eq!(d.get("multitrack_track_height").unwrap(), "88");
    assert_eq!(d.get("multitrack_clip_crossfade_ms").unwrap(), "5");
    assert_eq!(d.get("multitrack_clip_color").unwrap(), "track");
    let c = custom();
    assert_eq!(c.get("multitrack_track_height").unwrap(), "200");
    assert_eq!(c.get("multitrack_clip_crossfade_ms").unwrap(), "40");
    assert_eq!(c.get("multitrack_clip_color").unwrap(), "clip");
    let err = d.get("crossfade_edge_ms").unwrap_err();
    assert!(err.contains("unknown"), "{err}");
    assert!(err.contains("multitrack_track_height, multitrack_clip_crossfade_ms, multitrack_clip_color"), "{err}");
}

#[test]
fn track_height_accepts_16_to_440_points_and_nothing_else() {
    for (text, ok) in [("15", false), ("16", true), ("17", true), ("439", true), ("440", true), ("441", false)] {
        let mut p = MultitrackPrefs::default();
        let r = p.set("multitrack_track_height", text);
        assert_eq!(r.is_ok(), ok, "{text}");
        if ok {
            assert_eq!(p.track_height().to_string(), text);
            assert_eq!(p.get("multitrack_track_height").unwrap(), text);
        } else {
            assert!(r.unwrap_err().contains("16 to 440"), "{text}");
            assert_eq!(p.track_height(), 88, "{text}");
        }
    }
    for text in ["", "abc", "-1", "88.5", "65536", "70000", "1e2", "0x20"] {
        let mut p = MultitrackPrefs::default();
        let err = p.set("multitrack_track_height", text).unwrap_err();
        assert!(err.contains("16 to 440"), "{text:?}: {err}");
        assert_eq!(p.track_height(), 88, "{text:?}");
    }
}

#[test]
fn clip_crossfade_accepts_0_to_1000_ms_and_nothing_else() {
    for (text, ok) in [("-1", false), ("0", true), ("1", true), ("999", true), ("1000", true), ("1001", false)] {
        let mut p = MultitrackPrefs::default();
        let r = p.set("multitrack_clip_crossfade_ms", text);
        assert_eq!(r.is_ok(), ok, "{text}");
        if ok {
            assert_eq!(p.clip_crossfade_ms().to_string(), text);
            assert_eq!(p.get("multitrack_clip_crossfade_ms").unwrap(), text);
        } else {
            assert!(r.unwrap_err().contains("0 to 1000"), "{text}");
            assert_eq!(p.clip_crossfade_ms(), 5, "{text}");
        }
    }
    for text in ["", "abc", "1.5", "4294967296", "1e2"] {
        let mut p = MultitrackPrefs::default();
        let err = p.set("multitrack_clip_crossfade_ms", text).unwrap_err();
        assert!(err.contains("0 to 1000"), "{text:?}: {err}");
        assert_eq!(p.clip_crossfade_ms(), 5, "{text:?}");
    }
}

#[test]
fn clip_color_takes_track_or_clip_in_any_letter_case() {
    for (text, want) in
        [("track", ClipColorMode::Track), ("clip", ClipColorMode::Clip), ("Clip", ClipColorMode::Clip), (" TRACK ", ClipColorMode::Track)]
    {
        let mut p = custom();
        p.set("multitrack_clip_color", text).unwrap();
        assert_eq!(p.clip_color(), want, "{text:?}");
    }
    for text in ["", "rainbow", "tracks", "clips", "cl ip"] {
        let mut p = custom();
        let err = p.set("multitrack_clip_color", text).unwrap_err();
        assert!(err.contains("track or clip"), "{text:?}: {err}");
        assert_eq!(p.clip_color(), ClipColorMode::Clip, "{text:?}");
    }
}

#[test]
fn clip_color_mode_words() {
    assert_eq!(ClipColorMode::Track.as_str(), "track");
    assert_eq!(ClipColorMode::Clip.as_str(), "clip");
    assert_eq!(ClipColorMode::from_text("track"), Some(ClipColorMode::Track));
    assert_eq!(ClipColorMode::from_text("CLIP"), Some(ClipColorMode::Clip));
    assert_eq!(ClipColorMode::from_text("  clip\t"), Some(ClipColorMode::Clip));
    assert_eq!(ClipColorMode::from_text("trackclip"), None);
    assert_eq!(ClipColorMode::from_text(""), None);
}

#[test]
fn a_bad_value_or_unknown_key_changes_nothing() {
    let mut p = custom();
    let before = p.clone();
    assert!(p.set("multitrack_track_height", "9999").is_err());
    assert!(p.set("multitrack_clip_crossfade_ms", "9999").is_err());
    assert!(p.set("multitrack_clip_color", "purple").is_err());
    let err = p.set("crossfade_edge_ms", "7").unwrap_err();
    assert!(err.contains("unknown"), "{err}");
    assert!(err.contains("multitrack_clip_crossfade_ms"), "{err}");
    assert!(p.set("", "7").is_err());
    assert_eq!(p, before);
}

#[test]
fn set_trims_the_value() {
    let mut p = MultitrackPrefs::default();
    p.set("multitrack_track_height", "  120 ").unwrap();
    p.set("multitrack_clip_crossfade_ms", "\t30\t").unwrap();
    assert_eq!(p.track_height(), 120);
    assert_eq!(p.clip_crossfade_ms(), 30);
}

#[test]
fn to_text_writes_the_comment_and_three_lines() {
    assert_eq!(MultitrackPrefs::default().to_text(), format!("{COMMENT}\n{DEFAULT_BLOCK}"));
    assert_eq!(custom().to_text(), format!("{COMMENT}\n{CUSTOM_BLOCK}"));
    assert!(COMMENT.starts_with("# multitrack_*:"));
}

#[test]
fn parse_of_empty_or_unrelated_text_gives_the_defaults() {
    assert_eq!(MultitrackPrefs::parse(""), MultitrackPrefs::default());
    assert_eq!(MultitrackPrefs::parse("\n\n   \n"), MultitrackPrefs::default());
    let other = "# WavCraft settings.\nsession_folder = /home/user/sessions\ncrossfade_edge_ms = 9\nno equals sign here\n";
    assert_eq!(MultitrackPrefs::parse(other), MultitrackPrefs::default());
}

#[test]
fn parse_reads_all_three_keys_and_ignores_the_others() {
    let text = format!("# WavCraft settings.\nsession_folder = /home/user/sessions\n{CUSTOM_BLOCK}output_device = Speakers\n");
    assert_eq!(MultitrackPrefs::parse(&text), custom());
    let spaced = "  multitrack_track_height   =   120  \n\tmultitrack_clip_crossfade_ms=0\nmultitrack_clip_color = CLIP\n";
    let p = MultitrackPrefs::parse(spaced);
    assert_eq!((p.track_height(), p.clip_crossfade_ms(), p.clip_color()), (120, 0, ClipColorMode::Clip));
}

#[test]
fn parse_with_bad_values_keeps_the_default_for_just_that_key() {
    let bad_all = "multitrack_track_height = abc\nmultitrack_clip_crossfade_ms = 99999\nmultitrack_clip_color = purple\n";
    assert_eq!(MultitrackPrefs::parse(bad_all), MultitrackPrefs::default());
    let one_good = "multitrack_track_height = 300\nmultitrack_clip_crossfade_ms = -4\nmultitrack_clip_color = \n";
    let p = MultitrackPrefs::parse(one_good);
    assert_eq!((p.track_height(), p.clip_crossfade_ms(), p.clip_color()), (300, 5, ClipColorMode::Track));
    let out_of_range = "multitrack_track_height = 441\nmultitrack_clip_crossfade_ms = 1001\n";
    assert_eq!(MultitrackPrefs::parse(out_of_range), MultitrackPrefs::default());
}

#[test]
fn parse_ignores_comments_and_the_last_good_line_wins() {
    let text = "# multitrack_track_height = 300\nmultitrack_track_height = 100\nmultitrack_track_height = 150\nmultitrack_track_height = oops\n";
    assert_eq!(MultitrackPrefs::parse(text).track_height(), 150);
    let after_junk = "no equals sign here\n\n# a comment\nmultitrack_track_height = 120\n";
    assert_eq!(MultitrackPrefs::parse(after_junk).track_height(), 120);
}

#[test]
fn to_text_then_parse_is_a_round_trip() {
    for p in [MultitrackPrefs::default(), custom()] {
        assert_eq!(MultitrackPrefs::parse(&p.to_text()), p);
    }
    let mut edge = MultitrackPrefs::default();
    edge.set("multitrack_track_height", "16").unwrap();
    edge.set("multitrack_clip_crossfade_ms", "1000").unwrap();
    assert_eq!(MultitrackPrefs::parse(&edge.to_text()), edge);
    let mut edge = MultitrackPrefs::default();
    edge.set("multitrack_track_height", "440").unwrap();
    edge.set("multitrack_clip_crossfade_ms", "0").unwrap();
    assert_eq!(MultitrackPrefs::parse(&edge.to_text()), edge);
}

#[test]
fn merge_into_keeps_other_lines_and_replaces_ours() {
    let existing = "session_folder = /home/user/s\nmultitrack_track_height = 300\n# note\nmultitrack_clip_color = clip\ncrossfade_edge_ms = 9\n";
    let merged = MultitrackPrefs::default().merge_into(existing);
    assert_eq!(merged, format!("session_folder = /home/user/s\n# note\ncrossfade_edge_ms = 9\n{COMMENT}\n{DEFAULT_BLOCK}"));
    let merged = custom().merge_into(existing);
    assert_eq!(merged, format!("session_folder = /home/user/s\n# note\ncrossfade_edge_ms = 9\n{COMMENT}\n{CUSTOM_BLOCK}"));
}

#[test]
fn merge_into_keeps_the_other_lines_exactly_as_they_were() {
    let merged = custom().merge_into("  indented = 2  \n\n\t# tabbed comment\nlast = 3");
    assert_eq!(merged, format!("  indented = 2  \n\n\t# tabbed comment\nlast = 3\n{COMMENT}\n{CUSTOM_BLOCK}"));
}

#[test]
fn merge_into_handles_empty_and_unterminated_text_and_is_repeatable() {
    let p = custom();
    assert_eq!(p.merge_into(""), p.to_text());
    assert_eq!(p.merge_into("a = 1"), format!("a = 1\n{}", p.to_text()));
    assert_eq!(p.merge_into("a = 1\n"), format!("a = 1\n{}", p.to_text()));
    let once = p.merge_into("a = 1\n");
    assert_eq!(p.merge_into(&once), once);
    let mut q = p.clone();
    q.set("multitrack_track_height", "50").unwrap();
    let again = q.merge_into(&once);
    assert_eq!(again.matches("multitrack_track_height").count(), 1);
    assert_eq!(again.matches(COMMENT).count(), 1);
    assert_eq!(MultitrackPrefs::parse(&again), q);
}

#[test]
fn the_existing_settings_ignore_our_lines_and_do_not_write_them() {
    let text = format!("session_folder = /home/user/s\n{COMMENT}\n{CUSTOM_BLOCK}");
    let s = Settings::parse(&text);
    let mut want = Settings::default();
    want.set("session_folder", "/home/user/s").unwrap();
    assert_eq!(s, want);
    assert!(!Settings::default().to_text().contains("multitrack"));
    assert!(Settings::default().get("multitrack_track_height").is_err());
}

#[test]
fn load_of_a_missing_or_garbage_file_gives_the_defaults() {
    let dir = TempDir::new("load");
    assert_eq!(MultitrackPrefs::load(&dir.0.join("nope.txt")), MultitrackPrefs::default());
    assert_eq!(MultitrackPrefs::load(&dir.0.join("no-folder").join("settings.txt")), MultitrackPrefs::default());
    let garbage = dir.0.join("garbage.txt");
    std::fs::write(&garbage, [0xff, 0xfe, 0x00, 0x9f]).unwrap();
    assert_eq!(MultitrackPrefs::load(&garbage), MultitrackPrefs::default());
    assert_eq!(MultitrackPrefs::load(&dir.0), MultitrackPrefs::default());
    let good = dir.0.join("good.txt");
    std::fs::write(&good, format!("session_folder = /home/user/s\n{CUSTOM_BLOCK}")).unwrap();
    assert_eq!(MultitrackPrefs::load(&good), custom());
}

#[test]
fn load_of_a_half_corrupt_file_keeps_what_is_good() {
    let dir = TempDir::new("half");
    let file = dir.0.join("settings.txt");
    std::fs::write(&file, "multitrack_track_height = 250\nmultitrack_clip_crossfade_ms = banana\nmultitrack_clip_color = clip\n").unwrap();
    let p = MultitrackPrefs::load(&file);
    assert_eq!((p.track_height(), p.clip_crossfade_ms(), p.clip_color()), (250, 5, ClipColorMode::Clip));
}

#[test]
fn save_makes_the_folder_and_load_reads_it_back() {
    let dir = TempDir::new("save");
    let file = dir.0.join("deeper").join("wavcraft").join("settings.txt");
    custom().save(&file).unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), format!("{COMMENT}\n{CUSTOM_BLOCK}"));
    assert_eq!(MultitrackPrefs::load(&file), custom());
    assert!(!file.with_extension("txt.new").exists());
}

#[test]
fn save_keeps_the_settings_lines_and_replaces_its_own() {
    let dir = TempDir::new("keep");
    let file = dir.0.join("settings.txt");
    let mut settings = Settings::default();
    settings.set("session_folder", "/home/user/s").unwrap();
    settings.set("crossfade_edge_ms", "11").unwrap();
    settings.save(&file).unwrap();
    let before = Settings::load(&file);
    custom().save(&file).unwrap();
    assert_eq!(Settings::load(&file), before);
    assert_eq!(MultitrackPrefs::load(&file), custom());
    MultitrackPrefs::default().save(&file).unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    assert_eq!(text.matches("multitrack_track_height").count(), 1);
    assert_eq!(text.matches(COMMENT).count(), 1);
    assert!(text.contains("session_folder = /home/user/s\n"));
    assert_eq!(MultitrackPrefs::load(&file), MultitrackPrefs::default());
    assert_eq!(Settings::load(&file), before);
}

#[test]
fn settings_save_keeps_multitrack_and_video_preferences() {
    let dir = TempDir::new("settings-keep-lines");
    let file = dir.0.join("settings.txt");
    custom().save(&file).unwrap();
    let mut video = VideoPrefs::default();
    video.set("video_overlay_size", "12").unwrap();
    std::fs::write(&file, video.serialize(&std::fs::read_to_string(&file).unwrap())).unwrap();

    Settings::default().save(&file).unwrap();

    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("multitrack_track_height = 200\n"));
    assert!(text.contains("multitrack_clip_crossfade_ms = 40\n"));
    assert!(text.contains("multitrack_clip_color = clip\n"));
    assert!(text.contains("video_overlay_size = 12\n"));
}

#[test]
fn save_over_an_unreadable_file_just_writes_ours() {
    let dir = TempDir::new("over");
    let file = dir.0.join("settings.txt");
    std::fs::write(&file, [0xff, 0xfe, 0x00]).unwrap();
    custom().save(&file).unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), format!("{COMMENT}\n{CUSTOM_BLOCK}"));
}
