//! f67: the preference pages' settings (Data, Markers and Metadata, Playback and Recording):
//! defaults, typed getters, round trip through the file, and bad values refused.

use std::path::PathBuf;
use std::time::Duration;

use wavcraft_core::settings::{AutoScroll, KEYS, Settings, SrcQuality};

struct TempDir(PathBuf);
impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-pp-test-{}-{name}", std::process::id()));
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

const NEW_KEYS: [(&str, &str); 8] = [
    ("src_quality", "high"),
    ("save_markers", "true"),
    ("save_metadata", "true"),
    ("marker_name_pattern", "Marker {n}"),
    ("autoscroll", "pagewise"),
    ("jkl_speed_steps", "0.5,1,2,4"),
    ("preroll_s", "2"),
    ("postroll_s", "2"),
];

#[test]
fn defaults_and_typed_getters() {
    let s = Settings::default();
    for (k, v) in NEW_KEYS {
        assert_eq!(s.get(k).unwrap(), v, "default of {k}");
        assert!(KEYS.contains(&k), "{k} is listed in KEYS");
    }
    assert_eq!(s.get("record_offset_ms").unwrap(), "0");
    assert!(KEYS.contains(&"record_offset_ms"));
    assert_eq!(s.src_quality, SrcQuality::High);
    assert_eq!(s.autoscroll, AutoScroll::Pagewise);
    assert_eq!(s.jkl_speed_steps(), &[0.5, 1.0, 2.0, 4.0]);
    assert_eq!(s.preroll(), Duration::from_secs(2));
    assert_eq!(s.postroll(), Duration::from_secs(2));
    assert!(s.save_markers && s.save_metadata);
    assert_eq!(s.record_offset_ms, 0);
}

#[test]
fn every_new_key_round_trips_through_the_file() {
    let dir = TempDir::new("rt");
    let file = dir.0.join("settings.txt");
    let mut s = Settings::default();
    for (k, v) in [
        ("src_quality", "best"),
        ("save_markers", "false"),
        ("save_metadata", "false"),
        ("marker_name_pattern", "Cue = {n} (take)"),
        ("autoscroll", "centred"),
        ("jkl_speed_steps", "0.25,1,3.5,8"),
        ("preroll_s", "1.5"),
        ("postroll_s", "0"),
        ("record_offset_ms", "-35"),
    ] {
        s.set(k, v).unwrap();
        assert_eq!(s.get(k).unwrap(), v, "{k}");
    }
    s.save(&file).unwrap();
    let back = Settings::load(&file);
    assert_eq!(back, s);
    assert_eq!(back.src_quality, SrcQuality::Best);
    assert_eq!(back.autoscroll, AutoScroll::Centred);
    assert_eq!(back.jkl_speed_steps(), &[0.25, 1.0, 3.5, 8.0]);
    assert_eq!(back.preroll(), Duration::from_millis(1500));
    assert_eq!(back.postroll(), Duration::ZERO);
    assert_eq!(back.record_offset_ms, -35);
    assert_eq!(back.marker_name_pattern, "Cue = {n} (take)");
}

#[test]
fn an_older_file_loads_with_the_new_defaults() {
    let s = Settings::parse("session_folder = /x\ncrossfade_edge_ms = 7\n");
    let d = Settings::default();
    assert_eq!(s.edge_ms, 7);
    assert_eq!(s.autoscroll, d.autoscroll);
    assert_eq!(s.jkl_speed_steps(), d.jkl_speed_steps());
    assert_eq!(s.marker_name_pattern, d.marker_name_pattern);
}

#[test]
fn bad_values_are_refused_and_change_nothing() {
    let mut s = Settings::default();
    for (k, v) in [
        ("src_quality", "ultra"),
        ("save_markers", "maybe"),
        ("save_metadata", "2"),
        ("marker_name_pattern", ""),
        ("marker_name_pattern", "a\nb"),
        ("marker_name_pattern", &"x".repeat(200)),
        ("autoscroll", "sideways"),
        ("jkl_speed_steps", ""),
        ("jkl_speed_steps", "1,abc"),
        ("jkl_speed_steps", "2,1"),
        ("jkl_speed_steps", "0,1"),
        ("jkl_speed_steps", "1,100"),
        ("jkl_speed_steps", "1,1,2"),
        ("jkl_speed_steps", "1,2,3,4,5,6,7,8,9"),
        ("jkl_speed_steps", "nan,1"),
        ("preroll_s", "-1"),
        ("preroll_s", "61"),
        ("preroll_s", "nan"),
        ("postroll_s", "inf"),
        ("postroll_s", "x"),
        ("record_offset_ms", "5000"),
        ("record_offset_ms", "1.5"),
    ] {
        assert!(s.set(k, v).is_err(), "{k} = {v:?} should be refused");
    }
    assert_eq!(s, Settings::default(), "refused values changed nothing");
    // A bad line in a file leaves that key's default.
    let p = Settings::parse("autoscroll = nope\npreroll_s = 99\nsave_markers = false\n");
    assert_eq!(p.autoscroll, AutoScroll::Pagewise);
    assert_eq!(p.preroll(), Duration::from_secs(2));
    assert!(!p.save_markers);
}

#[test]
fn marker_names_come_from_the_pattern() {
    let mut s = Settings::default();
    assert_eq!(s.marker_name(3), "Marker 3");
    s.set("marker_name_pattern", "Cue {n}!").unwrap();
    assert_eq!(s.marker_name(12), "Cue 12!");
    s.set("marker_name_pattern", "Take").unwrap();
    assert_eq!(s.marker_name(2), "Take 2", "no placeholder: the number is appended");
}
