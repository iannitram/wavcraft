//! Roadmap C080: the recent sessions list (newest first, capped, deduped, prunable, JSON).

use std::cell::RefCell;

use wavcraft_core::recent_sessions::{DEFAULT_MAX_RECENT_SESSIONS, RecentSessions, normalise_path};

fn list(max: usize, newest_last: &[&str]) -> RecentSessions {
    let mut r = RecentSessions::with_max(max);
    for p in newest_last {
        assert!(r.promote(p));
    }
    r
}

fn strs(r: &RecentSessions) -> Vec<&str> {
    r.paths().iter().map(String::as_str).collect()
}

#[test]
fn normalise_turns_backslashes_into_slashes_and_drops_trailing_ones() {
    assert_eq!(normalise_path("C:\\m\\a.wcsession"), "C:/m/a.wcsession");
    assert_eq!(normalise_path("C:/m/a/"), "C:/m/a");
    assert_eq!(normalise_path("a//"), "a");
    assert_eq!(normalise_path("C:\\m\\a\\"), "C:/m/a");
}

#[test]
fn normalise_keeps_a_lone_root_trims_and_keeps_case() {
    assert_eq!(normalise_path("/"), "/");
    assert_eq!(normalise_path("//"), "/");
    assert_eq!(normalise_path("  /x/Y  "), "/x/Y");
    assert_eq!(normalise_path(""), "");
}

#[test]
fn a_new_list_is_empty_with_the_default_maximum_of_ten() {
    let r = RecentSessions::new();
    assert!(r.paths().is_empty());
    assert_eq!(r.max(), 10);
    assert_eq!(DEFAULT_MAX_RECENT_SESSIONS, 10);
    assert_eq!(RecentSessions::default(), r);
    assert_eq!(RecentSessions::with_max(4).max(), 4);
}

#[test]
fn promote_puts_the_newest_first() {
    let mut r = RecentSessions::new();
    assert!(r.promote("a.wcsession"));
    assert!(r.promote("b.wcsession"));
    assert_eq!(strs(&r), ["b.wcsession", "a.wcsession"]);
}

#[test]
fn promoting_a_listed_session_moves_it_to_the_front_once() {
    let mut r = list(10, &["a", "b", "c"]);
    assert_eq!(strs(&r), ["c", "b", "a"]);
    assert!(r.promote("a"));
    assert_eq!(strs(&r), ["a", "c", "b"]);
    assert!(r.promote("a"));
    assert_eq!(strs(&r), ["a", "c", "b"]);
}

#[test]
fn slash_and_backslash_and_a_trailing_slash_are_one_entry_and_the_newest_text_is_kept() {
    let mut r = RecentSessions::new();
    assert!(r.promote("C:\\music\\a.wcsession"));
    assert!(r.promote("C:/music/a.wcsession"));
    assert_eq!(strs(&r), ["C:/music/a.wcsession"]);
    assert!(r.promote("C:/music/a.wcsession/"));
    assert_eq!(strs(&r), ["C:/music/a.wcsession/"]);
}

#[test]
fn paths_that_differ_only_in_case_are_two_entries() {
    let r = list(10, &["C:/m/a.wcsession", "C:/m/A.wcsession"]);
    assert_eq!(strs(&r), ["C:/m/A.wcsession", "C:/m/a.wcsession"]);
}

#[test]
fn a_path_is_stored_trimmed_and_matches_its_trimmed_twin() {
    let mut r = RecentSessions::new();
    assert!(r.promote("  a  "));
    assert_eq!(strs(&r), ["a"]);
    assert!(r.promote("b"));
    assert!(r.promote("a"));
    assert_eq!(strs(&r), ["a", "b"]);
}

#[test]
fn blank_paths_and_paths_with_a_line_break_are_not_listed() {
    let mut r = list(10, &["a", "b"]);
    for bad in ["", "   ", "\t", "x\ny", "x\ry", "x\r\ny"] {
        assert!(!r.promote(bad), "{bad:?}");
    }
    assert_eq!(strs(&r), ["b", "a"]);
}

#[test]
fn the_list_is_capped_at_the_maximum_one_below_at_and_one_past() {
    let mut r = RecentSessions::with_max(3);
    r.promote("1");
    r.promote("2");
    assert_eq!(strs(&r), ["2", "1"]);
    r.promote("3");
    assert_eq!(strs(&r), ["3", "2", "1"]);
    r.promote("4");
    assert_eq!(strs(&r), ["4", "3", "2"]);
    r.promote("5");
    assert_eq!(strs(&r), ["5", "4", "3"]);
}

#[test]
fn promoting_a_listed_session_in_a_full_list_drops_nothing() {
    let mut r = list(3, &["a", "b", "c"]);
    assert!(r.promote("a"));
    assert_eq!(strs(&r), ["a", "c", "b"]);
    assert!(r.promote("b"));
    assert_eq!(strs(&r), ["b", "a", "c"]);
}

#[test]
fn a_maximum_of_one_keeps_only_the_newest_and_zero_keeps_none() {
    let mut one = RecentSessions::with_max(1);
    assert!(one.promote("a"));
    assert!(one.promote("b"));
    assert_eq!(strs(&one), ["b"]);
    let mut zero = RecentSessions::with_max(0);
    assert!(!zero.promote("a"));
    assert!(zero.paths().is_empty());
    assert_eq!(zero.to_json(), "[]");
}

#[test]
fn a_smaller_maximum_drops_the_oldest_now_and_a_larger_one_keeps_everything() {
    let mut r = list(10, &["a", "b", "c", "d"]);
    r.set_max(5);
    assert_eq!(r.max(), 5);
    assert_eq!(strs(&r), ["d", "c", "b", "a"]);
    r.set_max(4);
    assert_eq!(strs(&r), ["d", "c", "b", "a"]);
    r.set_max(3);
    assert_eq!(r.max(), 3);
    assert_eq!(strs(&r), ["d", "c", "b"]);
    assert!(r.promote("e"));
    assert_eq!(strs(&r), ["e", "d", "c"]);
    r.set_max(0);
    assert!(r.paths().is_empty());
}

#[test]
fn remove_takes_out_one_entry_by_normalised_path_and_keeps_the_order() {
    let mut r = list(10, &["C:/m/a", "C:/m/b", "C:/m/c"]);
    assert!(r.remove("C:\\m\\b\\"));
    assert_eq!(strs(&r), ["C:/m/c", "C:/m/a"]);
    assert!(!r.remove("C:/m/b"));
    assert!(!r.remove("C:/m/B"));
    assert!(!r.remove("C:/m/A"));
    assert!(!r.remove(""));
    assert_eq!(strs(&r), ["C:/m/c", "C:/m/a"]);
    assert!(r.remove("C:/m/c"));
    assert!(r.remove("C:/m/a"));
    assert!(r.paths().is_empty());
}

#[test]
fn clear_empties_the_list_and_keeps_the_maximum() {
    let mut r = list(4, &["a", "b"]);
    r.clear();
    assert!(r.paths().is_empty());
    assert_eq!(r.max(), 4);
    assert!(r.promote("c"));
    assert_eq!(strs(&r), ["c"]);
}

#[test]
fn prune_drops_the_entries_whose_file_is_gone_and_counts_them() {
    let mut r = list(10, &["a", "b", "c", "d"]);
    assert_eq!(strs(&r), ["d", "c", "b", "a"]);
    let gone = r.prune(|p| p != "c" && p != "a");
    assert_eq!(gone, 2);
    assert_eq!(strs(&r), ["d", "b"]);
}

#[test]
fn prune_with_every_file_present_changes_nothing_and_with_none_empties_the_list() {
    let mut r = list(10, &["a", "b"]);
    assert_eq!(r.prune(|_| true), 0);
    assert_eq!(strs(&r), ["b", "a"]);
    assert_eq!(r.prune(|_| false), 2);
    assert!(r.paths().is_empty());
    assert_eq!(r.max(), 10);
    assert_eq!(r.prune(|_| false), 0);
}

#[test]
fn prune_asks_about_each_stored_path_once_newest_first() {
    let mut r = RecentSessions::new();
    r.promote("  C:\\x\\a  ");
    r.promote("b");
    let asked = RefCell::new(Vec::new());
    r.prune(|p| {
        asked.borrow_mut().push(p.to_owned());
        true
    });
    assert_eq!(*asked.borrow(), ["b", "C:\\x\\a"]);
}

#[test]
fn to_json_is_an_array_of_strings_newest_first() {
    assert_eq!(RecentSessions::new().to_json(), "[]");
    let r = list(10, &["a.wcsession", "b.wcsession"]);
    assert_eq!(r.to_json(), "[\"b.wcsession\",\"a.wcsession\"]");
}

#[test]
fn the_json_round_trip_keeps_odd_paths_and_the_order() {
    let r = list(10, &["C:\\music\\one.wcsession", "/home/user/caf\u{e9} \"x\".wcsession", "two"]);
    let back = RecentSessions::from_json(&r.to_json(), 10);
    assert_eq!(back, r);
    assert_eq!(strs(&back), ["two", "/home/user/caf\u{e9} \"x\".wcsession", "C:\\music\\one.wcsession"]);
}

#[test]
fn from_json_gives_an_empty_list_for_missing_or_corrupt_text() {
    for bad in ["", "   ", "not json", "{}", "null", "42", "\"a\"", "{\"a\":[\"x\"]}", "[1,2", "[\"a\",", "\u{0}"] {
        let r = RecentSessions::from_json(bad, 7);
        assert!(r.paths().is_empty(), "{bad:?}");
        assert_eq!(r.max(), 7, "{bad:?}");
    }
    assert!(RecentSessions::from_json("[]", 7).paths().is_empty());
}

#[test]
fn from_json_skips_elements_that_are_not_usable_paths() {
    let r = RecentSessions::from_json("[\"a\", 5, null, \"\", \"  \", \"b\", \"a/\", [\"x\"], {\"y\":1}, \"c\\nd\", \"f\\rg\", \"e\"]", 10);
    assert_eq!(strs(&r), ["a", "b", "e"]);
}

#[test]
fn from_json_keeps_the_first_usable_entries_up_to_the_maximum() {
    let text = "[\"a\",\"b\",\"c\",\"d\"]";
    let r = RecentSessions::from_json(text, 2);
    assert_eq!(strs(&r), ["a", "b"]);
    assert_eq!(r.max(), 2);
    assert_eq!(strs(&RecentSessions::from_json(text, 3)), ["a", "b", "c"]);
    assert_eq!(strs(&RecentSessions::from_json(text, 4)), ["a", "b", "c", "d"]);
    assert_eq!(strs(&RecentSessions::from_json(text, 5)), ["a", "b", "c", "d"]);
    assert!(RecentSessions::from_json(text, 0).paths().is_empty());
}

#[test]
fn from_json_skipped_elements_do_not_use_up_the_maximum() {
    let r = RecentSessions::from_json("[1, \"a\", \"a\", \"\", \"b\", \"c\"]", 2);
    assert_eq!(strs(&r), ["a", "b"]);
}

#[test]
fn from_json_trims_paths() {
    let r = RecentSessions::from_json("[\"  a \", \"a\", \"b\"]", 10);
    assert_eq!(strs(&r), ["a", "b"]);
}
