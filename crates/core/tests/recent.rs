//! f63: the Open Recent list.

use std::path::{Path, PathBuf};

use wavcraft_core::recent::{MAX_RECENT, Recent};

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

#[test]
fn the_newest_file_is_first_and_a_repeat_moves_to_the_front() {
    let mut r = Recent::new();
    r.push(Path::new("/a.wav"));
    r.push(Path::new("/b.wav"));
    r.push(Path::new("/c.wav"));
    r.push(Path::new("/a.wav"));
    assert_eq!(r.paths(), [p("/a.wav"), p("/c.wav"), p("/b.wav")]);
}

#[test]
fn only_the_last_ten_are_kept() {
    let mut r = Recent::new();
    for i in 0..15 {
        r.push(Path::new(&format!("/f{i}.wav")));
    }
    assert_eq!(r.paths().len(), MAX_RECENT);
    assert_eq!(r.paths()[0], p("/f14.wav"));
    assert_eq!(r.paths()[9], p("/f5.wav"));
}

#[test]
fn it_survives_a_restart_through_the_file() {
    let dir = std::env::temp_dir().join(format!("wavcraft-recent-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("nested").join("recent.txt");
    assert_eq!(Recent::load(&file), Recent::new(), "a missing file is an empty list");
    let mut r = Recent::new();
    r.push(Path::new("/with space/one.wav"));
    r.push(Path::new("/two.flac"));
    r.save(&file).unwrap();
    assert_eq!(Recent::load(&file), r);
    assert!(!file.with_extension("txt.new").exists(), "the temporary file is renamed away");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn parse_skips_comments_blanks_and_repeats_and_caps_the_list() {
    let text = "# note\n\n/a.wav\r\n/b.wav\n/a.wav\n";
    assert_eq!(Recent::parse(text).paths(), [p("/a.wav"), p("/b.wav")]);
    let many: String = (0..20).map(|i| format!("/f{i}.wav\n")).collect();
    assert_eq!(Recent::parse(&many).paths().len(), MAX_RECENT);
}

#[test]
fn remove_and_clear() {
    let mut r = Recent::new();
    r.push(Path::new("/a.wav"));
    r.push(Path::new("/b.wav"));
    assert_eq!(r.remove(5), None);
    assert_eq!(r.remove(0), Some(p("/b.wav")));
    assert_eq!(r.paths(), [p("/a.wav")]);
    r.clear();
    assert!(r.paths().is_empty());
}

#[test]
fn a_path_that_cannot_live_on_one_line_is_not_listed() {
    let mut r = Recent::new();
    r.push(Path::new("/odd\nname.wav"));
    r.push(Path::new(""));
    assert!(r.paths().is_empty());
}
