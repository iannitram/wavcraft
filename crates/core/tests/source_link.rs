//! Roadmap A326: the link from a mixed-down file back to its session, kept in one INFO tag of
//! the file's metadata. Core only: no file is written here.

use wavcraft_core::meta::source_link::{LINK_FIELD, MAX_ID_BYTES, MAX_NAME_BYTES, MAX_PATH_BYTES, SourceLink, read_source_link, write_source_link};
use wavcraft_core::meta::{Bext, InfoTags, Metadata};

const AT: u64 = 1_700_000_000;

fn link() -> SourceLink {
    SourceLink::new("/home/user/Music/Song Mix.wcs", "Song Mix", Some("clip-7"), AT).unwrap()
}

fn busy_meta() -> Metadata {
    Metadata {
        bext: Some(Bext {
            description: "take 3".to_string(),
            originator: "wavcraft".to_string(),
            time_reference: 480_000,
            tail: vec![1, 2, 3],
            coding_history: "A=PCM\r\n".to_string(),
            ..Bext::default()
        }),
        info: InfoTags(vec![(*b"INAM", b"Song".to_vec()), (*b"ICMT", b"first".to_vec()), (*b"ISFT", b"WavCraft".to_vec())]),
        info_edited: false,
        ixml: Some(b"<BWFXML></BWFXML>\0\0".to_vec()),
        chnames: vec!["L".to_string(), "R".to_string()],
        chnames_edited: false,
        xmp: None,
        xmp_problem: None,
        xmp_edited: false,
    }
}

// ---- new ----

#[test]
fn new_keeps_what_it_is_given() {
    let l = link();
    assert_eq!(l.session_path(), "/home/user/Music/Song Mix.wcs");
    assert_eq!(l.session_name(), "Song Mix");
    assert_eq!(l.item_id(), Some("clip-7"));
    assert_eq!(l.created_at(), AT);
    let none = SourceLink::new("/home/user/a.wcs", "A", None, 0).unwrap();
    assert_eq!(none.item_id(), None);
    assert_eq!(none.created_at(), 0);
}

#[test]
fn new_refuses_empty_fields() {
    assert_eq!(SourceLink::new("", "A", None, 0), Err("session_path cannot be empty".to_string()));
    assert_eq!(SourceLink::new("/home/user/a.wcs", "", None, 0), Err("session_name cannot be empty".to_string()));
    assert_eq!(SourceLink::new("/home/user/a.wcs", "A", Some(""), 0), Err("item_id cannot be empty".to_string()));
}

#[test]
fn new_refuses_a_zero_byte() {
    assert_eq!(SourceLink::new("/home/user/a\0.wcs", "A", None, 0), Err("session_path cannot hold a zero byte".to_string()));
    assert_eq!(SourceLink::new("/home/user/a.wcs", "A\0", None, 0), Err("session_name cannot hold a zero byte".to_string()));
    assert_eq!(SourceLink::new("/home/user/a.wcs", "A", Some("\0"), 0), Err("item_id cannot hold a zero byte".to_string()));
}

#[test]
fn path_limit_is_exact() {
    assert_eq!(MAX_PATH_BYTES, 512);
    let at = |n: usize| SourceLink::new(&"a".repeat(n), "A", None, 0);
    assert!(at(MAX_PATH_BYTES - 1).is_ok());
    assert!(at(MAX_PATH_BYTES).is_ok());
    assert_eq!(at(MAX_PATH_BYTES + 1), Err("session_path takes at most 512 bytes".to_string()));
}

#[test]
fn name_limit_is_exact() {
    assert_eq!(MAX_NAME_BYTES, 128);
    let at = |n: usize| SourceLink::new("/home/user/a.wcs", &"n".repeat(n), None, 0);
    assert!(at(MAX_NAME_BYTES - 1).is_ok());
    assert!(at(MAX_NAME_BYTES).is_ok());
    assert_eq!(at(MAX_NAME_BYTES + 1), Err("session_name takes at most 128 bytes".to_string()));
}

#[test]
fn id_limit_is_exact() {
    assert_eq!(MAX_ID_BYTES, 64);
    let at = |n: usize| SourceLink::new("/home/user/a.wcs", "A", Some(&"i".repeat(n)), 0);
    assert!(at(MAX_ID_BYTES - 1).is_ok());
    assert!(at(MAX_ID_BYTES).is_ok());
    assert_eq!(at(MAX_ID_BYTES + 1), Err("item_id takes at most 64 bytes".to_string()));
}

#[test]
fn limits_count_bytes_not_characters() {
    // 256 two-byte characters are 512 bytes; one more byte is too many.
    assert!(SourceLink::new(&"\u{e9}".repeat(256), "A", None, 0).is_ok());
    assert!(SourceLink::new(&format!("{}\u{e9}", "a".repeat(511)), "A", None, 0).is_err());
    assert!(SourceLink::new("/home/user/a.wcs", &"\u{e9}".repeat(64), None, 0).is_ok());
    assert!(SourceLink::new("/home/user/a.wcs", &"\u{e9}".repeat(65), None, 0).is_err());
    assert!(SourceLink::new("/home/user/a.wcs", "A", Some(&"\u{e9}".repeat(32)), 0).is_ok());
    assert!(SourceLink::new("/home/user/a.wcs", "A", Some(&"\u{e9}".repeat(33)), 0).is_err());
}

#[test]
fn checks_run_path_then_name_then_id() {
    assert_eq!(SourceLink::new("", "", Some(""), 0), Err("session_path cannot be empty".to_string()));
    assert_eq!(SourceLink::new("/home/user/a.wcs", "", Some(""), 0), Err("session_name cannot be empty".to_string()));
}

// ---- encode ----

#[test]
fn encode_is_one_known_line() {
    assert_eq!(link().encode(), "WCLINK1;path=/home/user/Music/Song Mix.wcs;name=Song Mix;id=clip-7;at=1700000000");
}

#[test]
fn encode_leaves_out_a_missing_id() {
    let l = SourceLink::new("/home/user/a.wcs", "A", None, 0).unwrap();
    assert_eq!(l.encode(), "WCLINK1;path=/home/user/a.wcs;name=A;at=0");
}

#[test]
fn encode_escapes_separators_controls_and_unicode() {
    let l = SourceLink::new("/home/user/a;b%c.wcs", "x\ny\tz\u{7f}", Some("\u{e9}"), u64::MAX).unwrap();
    assert_eq!(l.encode(), "WCLINK1;path=/home/user/a%3Bb%25c.wcs;name=x%0Ay%09z%7F;id=%C3%A9;at=18446744073709551615");
    let j = SourceLink::new("/home/user/\u{65e5}\u{672c}.wcs", "A", None, 1).unwrap();
    assert_eq!(j.encode(), "WCLINK1;path=/home/user/%E6%97%A5%E6%9C%AC.wcs;name=A;at=1");
    assert!(j.encode().is_ascii());
    // The edges of the plain range: 0x1f and 0x7f are escaped, 0x20 and 0x7e are not.
    let e = SourceLink::new("/home/user/a~b.wcs", "\u{1f} x", None, 1).unwrap();
    assert_eq!(e.encode(), "WCLINK1;path=/home/user/a~b.wcs;name=%1F x;at=1");
}

// ---- parse ----

#[test]
fn parse_round_trips() {
    let cases = [
        link(),
        SourceLink::new("/home/user/a.wcs", "A", None, 0).unwrap(),
        SourceLink::new("/home/user/a;b%c=d.wcs", "x\ny\tz", Some("id;1%"), u64::MAX).unwrap(),
        SourceLink::new("/home/user/\u{65e5}\u{672c}/\u{43c}\u{443}\u{437}.wcs", "\u{e9}t\u{e9}", Some("\u{1f3b5}"), 42).unwrap(),
        SourceLink::new(&"a".repeat(MAX_PATH_BYTES), &"n".repeat(MAX_NAME_BYTES), Some(&"i".repeat(MAX_ID_BYTES)), 7).unwrap(),
        SourceLink::new(&"\u{e9}".repeat(256), "A", None, 7).unwrap(),
    ];
    for l in cases {
        assert_eq!(SourceLink::parse(&l.encode()), Some(l.clone()), "{}", l.encode());
    }
}

#[test]
fn parse_reads_a_hand_written_line() {
    let got = SourceLink::parse("WCLINK1;path=/home/user/a%3Bb.wcs;name=A;id=t1;at=12").unwrap();
    assert_eq!(got.session_path(), "/home/user/a;b.wcs");
    assert_eq!(got.item_id(), Some("t1"));
    assert_eq!(got.created_at(), 12);
    // Lower-case hex is accepted.
    assert_eq!(SourceLink::parse("WCLINK1;path=/home/user/%c3%a9.wcs;name=A;at=1").unwrap().session_path(), "/home/user/\u{e9}.wcs");
}

#[test]
fn parse_gives_none_for_junk() {
    for v in ["", " ", "garbage", "WCLINK1", "WCLINK1;", ";;;", "path=/home/user/a.wcs;name=A;at=1", "\u{0}"] {
        assert_eq!(SourceLink::parse(v), None, "{v:?}");
    }
}

#[test]
fn parse_gives_none_for_another_version() {
    for v in [
        "WCLINK2;path=/home/user/a.wcs;name=A;at=1",
        "wclink1;path=/home/user/a.wcs;name=A;at=1",
        " WCLINK1;path=/home/user/a.wcs;name=A;at=1",
        "WCLINK;path=/home/user/a.wcs;name=A;at=1",
    ] {
        assert_eq!(SourceLink::parse(v), None, "{v:?}");
    }
    assert!(SourceLink::parse("WCLINK1;path=/home/user/a.wcs;name=A;at=1").is_some());
}

#[test]
fn parse_gives_none_when_a_field_is_missing() {
    assert_eq!(SourceLink::parse("WCLINK1;name=A;at=1"), None);
    assert_eq!(SourceLink::parse("WCLINK1;path=/home/user/a.wcs;at=1"), None);
    assert_eq!(SourceLink::parse("WCLINK1;path=/home/user/a.wcs;name=A"), None);
}

#[test]
fn parse_gives_none_for_a_bad_timestamp() {
    for at in ["", "x", "+5", "-1", "1.5", " 1", "1 ", "18446744073709551616"] {
        assert_eq!(SourceLink::parse(&format!("WCLINK1;path=/home/user/a.wcs;name=A;at={at}")), None, "{at:?}");
    }
    assert!(SourceLink::parse("WCLINK1;path=/home/user/a.wcs;name=A;at=18446744073709551615").is_some());
}

#[test]
fn parse_gives_none_for_repeated_unknown_or_malformed_parts() {
    assert_eq!(SourceLink::parse("WCLINK1;path=/home/user/a.wcs;path=/home/user/b.wcs;name=A;at=1"), None);
    assert_eq!(SourceLink::parse("WCLINK1;path=/home/user/a.wcs;name=A;name=B;at=1"), None);
    assert_eq!(SourceLink::parse("WCLINK1;path=/home/user/a.wcs;name=A;id=a;id=b;at=1"), None);
    assert_eq!(SourceLink::parse("WCLINK1;path=/home/user/a.wcs;name=A;at=1;at=2"), None);
    assert_eq!(SourceLink::parse("WCLINK1;path=/home/user/a.wcs;name=A;at=1;extra=1"), None);
    assert_eq!(SourceLink::parse("WCLINK1;path=/home/user/a.wcs;name=A;at=1;"), None);
    assert_eq!(SourceLink::parse("WCLINK1;path=/home/user/a.wcs;;name=A;at=1"), None);
    assert_eq!(SourceLink::parse("WCLINK1;path=/home/user/a.wcs;name;at=1"), None);
}

#[test]
fn parse_gives_none_for_empty_values() {
    assert_eq!(SourceLink::parse("WCLINK1;path=;name=A;at=1"), None);
    assert_eq!(SourceLink::parse("WCLINK1;path=/home/user/a.wcs;name=;at=1"), None);
    assert_eq!(SourceLink::parse("WCLINK1;path=/home/user/a.wcs;name=A;id=;at=1"), None);
}

#[test]
fn parse_gives_none_for_a_bad_escape() {
    for bad in ["%", "a%", "a%4", "a%ZZ", "a%G1", "a%1G", "a%+1", "a%-1", "%FF", "%C3", "%00"] {
        assert_eq!(SourceLink::parse(&format!("WCLINK1;path=/home/user/{bad};name=A;at=1")), None, "{bad:?}");
        assert_eq!(SourceLink::parse(&format!("WCLINK1;path=/home/user/a.wcs;name={bad};at=1")), None, "{bad:?}");
        assert_eq!(SourceLink::parse(&format!("WCLINK1;path=/home/user/a.wcs;name=A;id={bad};at=1")), None, "{bad:?}");
    }
}

#[test]
fn parse_gives_none_for_raw_non_ascii() {
    assert_eq!(SourceLink::parse("WCLINK1;path=/home/user/\u{e9}.wcs;name=A;at=1"), None);
}

#[test]
fn parse_keeps_the_limits() {
    let line = |p: usize, n: usize, i: usize| format!("WCLINK1;path={};name={};id={};at=1", "a".repeat(p), "n".repeat(n), "i".repeat(i));
    assert!(SourceLink::parse(&line(512, 128, 64)).is_some());
    assert_eq!(SourceLink::parse(&line(513, 128, 64)), None);
    assert_eq!(SourceLink::parse(&line(512, 129, 64)), None);
    assert_eq!(SourceLink::parse(&line(512, 128, 65)), None);
    // The limit is on the decoded bytes, not the escaped text.
    assert!(SourceLink::parse(&format!("WCLINK1;path={};name=A;at=1", "%C3%A9".repeat(256))).is_some());
    assert_eq!(SourceLink::parse(&format!("WCLINK1;path={};name=A;at=1", "%C3%A9".repeat(257))), None);
}

// ---- write and read in the metadata ----

#[test]
fn link_field_is_an_info_tag() {
    assert_eq!(LINK_FIELD, "INFO.WCSL");
}

#[test]
fn write_then_read_gives_the_link_back() {
    let mut m = Metadata::default();
    write_source_link(&mut m, &link()).unwrap();
    assert_eq!(read_source_link(&m), Some(link()));
    assert_eq!(m.get(LINK_FIELD), Some(link().encode()));
    assert_eq!(m.info.0, vec![(*b"WCSL", link().encode().into_bytes())]);
    assert!(m.info_edited);
}

#[test]
fn nothing_to_read_is_none() {
    assert_eq!(read_source_link(&Metadata::default()), None);
    assert_eq!(read_source_link(&busy_meta()), None);
}

#[test]
fn write_touches_only_its_own_field() {
    let before = busy_meta();
    let mut m = before.clone();
    write_source_link(&mut m, &link()).unwrap();
    assert_eq!(m.bext, before.bext);
    assert_eq!(m.ixml, before.ixml);
    assert_eq!(m.chnames, before.chnames);
    assert!(!m.chnames_edited);
    assert_eq!(m.info.0.len(), before.info.0.len() + 1);
    assert_eq!(&m.info.0[..3], &before.info.0[..]);
    assert_eq!(m.info.0[3].0, *b"WCSL");
    assert_eq!(read_source_link(&m), Some(link()));
    for field in ["title", "comment", "software", "description", "originator", "time_reference", "coding_history", "chname.1", "chname.2"] {
        assert_eq!(m.get(field), before.get(field), "{field}");
    }
}

#[test]
fn write_replaces_an_earlier_link_and_keeps_the_rest() {
    let mut m = busy_meta();
    write_source_link(&mut m, &link()).unwrap();
    let second = SourceLink::new("/home/user/other.wcs", "Other", None, 5).unwrap();
    write_source_link(&mut m, &second).unwrap();
    assert_eq!(read_source_link(&m), Some(second));
    assert_eq!(m.info.0.iter().filter(|(id, _)| id == b"WCSL").count(), 1);
    assert_eq!(&m.info.0[..3], &busy_meta().info.0[..]);
}

#[test]
fn write_replaces_an_unreadable_tag() {
    let mut m = Metadata { info: InfoTags(vec![(*b"WCSL", b"junk".to_vec()), (*b"INAM", b"Song".to_vec())]), ..Metadata::default() };
    assert_eq!(read_source_link(&m), None);
    write_source_link(&mut m, &link()).unwrap();
    assert_eq!(read_source_link(&m), Some(link()));
    assert_eq!(m.info.0.len(), 2);
    assert_eq!(m.get("title"), Some("Song".to_string()));
}

#[test]
fn read_ignores_unreadable_values_and_other_tags() {
    let mut m = busy_meta();
    m.info.0.push((*b"WCSL", b"WCLINK1;path=/home/user/a.wcs;name=A;at=x".to_vec()));
    assert_eq!(read_source_link(&m), None);
    // A good link line in some other tag is not the link.
    let mut other = busy_meta();
    other.info.0.push((*b"ICMT", link().encode().into_bytes()));
    assert_eq!(read_source_link(&other), None);
    // Spaces or a line end around the value make it unreadable, not trimmed.
    for padded in [format!(" {}", link().encode()), format!("{} ", link().encode()), format!("{}\n", link().encode())] {
        let m = Metadata { info: InfoTags(vec![(*b"WCSL", padded.into_bytes())]), ..Metadata::default() };
        assert_eq!(read_source_link(&m), None);
    }
}

#[test]
fn the_link_survives_the_info_chunk_bytes() {
    let unicode = SourceLink::new("/home/user/\u{65e5}\u{672c}/s;1.wcs", "\u{e9}t\u{e9}", Some("t%1"), 9).unwrap();
    let mut m = busy_meta();
    write_source_link(&mut m, &unicode).unwrap();
    let again = Metadata { info: InfoTags::parse(&m.info.to_bytes()).unwrap(), ..Metadata::default() };
    assert_eq!(read_source_link(&again), Some(unicode));
    assert_eq!(again.get("title"), Some("Song".to_string()));
    assert_eq!(again.get("comment"), Some("first".to_string()));
}
