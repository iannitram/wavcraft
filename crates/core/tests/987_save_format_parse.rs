//! #987: `SaveFormat::parse` is the inverse of `SaveFormat::key` for all four formats (Vorbis
//! included), and Cargo.toml keeps the Task 03 comment directly above the `symphonia` line.

use wavcraft_core::save::SaveFormat;

const ALL: [SaveFormat; 4] = [SaveFormat::Wav, SaveFormat::Aiff, SaveFormat::Flac, SaveFormat::Vorbis];

#[test]
fn parse_gives_back_every_format_from_its_key() {
    for f in ALL {
        assert_eq!(SaveFormat::parse(f.key()), Some(f), "key {:?}", f.key());
    }
}

#[test]
fn ogg_parses_as_vorbis() {
    assert_eq!(SaveFormat::parse("ogg"), Some(SaveFormat::Vorbis));
}

#[test]
fn oga_parses_as_vorbis() {
    assert_eq!(SaveFormat::parse("oga"), Some(SaveFormat::Vorbis));
}

#[test]
fn parse_still_accepts_aif_and_ignores_case_and_spaces() {
    assert_eq!(SaveFormat::parse("aif"), Some(SaveFormat::Aiff));
    assert_eq!(SaveFormat::parse(" OGG "), Some(SaveFormat::Vorbis));
    assert_eq!(SaveFormat::parse("mp3"), None);
}

#[test]
fn the_key_doc_comment_lists_ogg() {
    let src = include_str!("../src/save.rs");
    let imp = src.find("impl SaveFormat {").expect("impl SaveFormat is in save.rs");
    let at = imp + src[imp..].find("pub fn key(self)").expect("SaveFormat::key is in save.rs");
    let before = &src[..src[..at].rfind('\n').expect("a line break before key") + 1];
    let doc = before.lines().rev().take_while(|l| l.trim_start().starts_with("///")).collect::<Vec<_>>().join("\n");
    assert!(doc.contains("ogg"), "the doc comment on key must list ogg: {doc:?}");
}

fn lines_of_cargo_toml() -> Vec<&'static str> {
    include_str!("../../../Cargo.toml").lines().collect()
}

#[test]
fn the_task_03_comment_sits_directly_above_the_symphonia_line() {
    let l = lines_of_cargo_toml();
    let sym = l.iter().position(|x| x.starts_with("symphonia = ")).expect("a symphonia line");
    let task = l.iter().position(|x| x.starts_with("# Task 03 ")).expect("the Task 03 comment");
    assert!(task < sym, "the Task 03 comment starts above symphonia");
    assert!(l[task..sym].iter().all(|x| x.starts_with('#')), "only comment lines sit between the Task 03 comment and symphonia: {:?}", &l[task..sym]);
    assert!(!l[task..sym].iter().any(|x| x.contains("vorbis_rs") || x.starts_with("# f93")), "the f93 comment is not inside the Task 03 comment");
}

#[test]
fn the_f93_comment_and_vorbis_rs_form_their_own_block_after_the_task_03_block() {
    let l = lines_of_cargo_toml();
    let sym = l.iter().position(|x| x.starts_with("symphonia = ")).expect("a symphonia line");
    let proptest = l.iter().position(|x| x.starts_with("proptest = ")).expect("a proptest line");
    let f93 = l.iter().position(|x| x.starts_with("# f93")).expect("the f93 comment");
    let vorbis = l.iter().position(|x| x.starts_with("vorbis_rs = ")).expect("a vorbis_rs line");
    assert!(f93 > proptest && f93 > sym, "the f93 comment comes after the Task 03 block");
    assert_eq!(vorbis, f93 + l[f93..].iter().take_while(|x| x.starts_with('#')).count(), "vorbis_rs follows its comment directly");
    assert_eq!(l[vorbis], "vorbis_rs = \"0.5.6\"");
}

#[test]
fn the_task_03_dependency_lines_are_unchanged() {
    let l = lines_of_cargo_toml();
    let sym = l.iter().position(|x| x.starts_with("symphonia = ")).expect("a symphonia line");
    assert_eq!(
        &l[sym + 1..sym + 5],
        [
            "bytemuck = \"1.25\"",
            "dirs = \"6\"",
            "sysinfo = { version = \"0.37\", default-features = false, features = [\"system\", \"disk\"] }",
            "proptest = \"1.11\""
        ]
    );
}
