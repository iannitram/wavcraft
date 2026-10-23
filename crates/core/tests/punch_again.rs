//! Punch Again (C041): the last completed punch range is remembered and re-armed on a `Punch`.

use soundcraft_model::time::Range;
use wavcraft_core::mtpunch::PunchAgain;
use wavcraft_core::mtrecord::Punch;

const PREROLL: i64 = 96_000;

fn punch() -> Punch {
    Punch::new(PREROLL)
}

fn remembered(from: i64, to: i64) -> PunchAgain {
    let mut m = PunchAgain::new();
    m.record(from, to).unwrap();
    m
}

#[test]
fn new_memory_has_no_last() {
    assert_eq!(PunchAgain::new().last(), None);
}

#[test]
fn default_memory_is_empty() {
    assert_eq!(PunchAgain::default().last(), None);
}

#[test]
fn record_stores_the_range() {
    let m = remembered(48_000, 96_000);
    assert_eq!(m.last(), Some(Range::new(48_000, 96_000)));
}

#[test]
fn record_replaces_the_previous_range() {
    let mut m = remembered(100, 200);
    m.record(300, 450).unwrap();
    assert_eq!(m.last(), Some(Range::new(300, 450)));
}

#[test]
fn record_accepts_a_range_from_zero() {
    assert_eq!(remembered(0, 10).last(), Some(Range::new(0, 10)));
}

#[test]
fn record_accepts_a_one_frame_range() {
    assert_eq!(remembered(5, 6).last(), Some(Range::new(5, 6)));
}

#[test]
fn record_refuses_a_negative_start() {
    let mut m = PunchAgain::new();
    let e = m.record(-1, 10).unwrap_err();
    assert!(e.0.contains("0 <= in < out"), "{}", e.0);
    assert_eq!(m.last(), None);
}

#[test]
fn record_refuses_an_empty_range() {
    let mut m = PunchAgain::new();
    let e = m.record(10, 10).unwrap_err();
    assert!(e.0.contains("0 <= in < out"), "{}", e.0);
    assert_eq!(m.last(), None);
}

#[test]
fn record_refuses_a_reversed_range() {
    let mut m = PunchAgain::new();
    assert!(m.record(20, 10).is_err());
    assert_eq!(m.last(), None);
}

#[test]
fn a_refused_record_keeps_the_previous_range() {
    let mut m = remembered(100, 200);
    assert!(m.record(50, 40).is_err());
    assert!(m.record(-5, 40).is_err());
    assert!(m.record(60, 60).is_err());
    assert_eq!(m.last(), Some(Range::new(100, 200)));
}

#[test]
fn rearm_without_a_remembered_punch_is_refused_and_changes_nothing() {
    let m = PunchAgain::new();
    let mut p = punch();
    let e = m.rearm(&mut p, 1_000_000).unwrap_err();
    assert!(e.0.contains("no punch to repeat"), "{}", e.0);
    assert_eq!(p.range, None);
    assert!(!p.on);
    assert_eq!(p.preroll, PREROLL);
}

#[test]
fn rearm_sets_the_range_turns_punch_on_and_returns_the_range() {
    let m = remembered(48_000, 96_000);
    let mut p = punch();
    let r = m.rearm(&mut p, 1_000_000).unwrap();
    assert_eq!(r, Range::new(48_000, 96_000));
    assert_eq!(p.range, Some(Range::new(48_000, 96_000)));
    assert!(p.on);
    assert_eq!(p.active(), Some(Range::new(48_000, 96_000)));
}

#[test]
fn rearm_leaves_the_preroll_alone() {
    let m = remembered(480_000, 960_000);
    let mut p = Punch::new(12_345);
    m.rearm(&mut p, 2_000_000).unwrap();
    assert_eq!(p.preroll, 12_345);
    assert_eq!(p.record_start(), Some(480_000 - 12_345));
}

#[test]
fn rearm_record_start_does_not_go_before_zero() {
    let m = remembered(1_000, 2_000);
    let mut p = punch();
    m.rearm(&mut p, 10_000).unwrap();
    assert_eq!(p.record_start(), Some(0));
}

#[test]
fn rearm_replaces_a_range_already_on_the_punch() {
    let m = remembered(300, 400);
    let mut p = punch();
    p.set(10, 20).unwrap();
    p.on = true;
    m.rearm(&mut p, 1_000).unwrap();
    assert_eq!(p.range, Some(Range::new(300, 400)));
    assert!(p.on);
}

#[test]
fn rearm_keeps_the_memory_so_it_can_repeat() {
    let m = remembered(300, 400);
    let mut a = punch();
    let mut b = punch();
    assert_eq!(m.rearm(&mut a, 1_000).unwrap(), Range::new(300, 400));
    assert_eq!(m.rearm(&mut b, 1_000).unwrap(), Range::new(300, 400));
    assert_eq!(m.last(), Some(Range::new(300, 400)));
}

#[test]
fn rearm_accepts_a_range_ending_exactly_at_the_session_end() {
    let m = remembered(100, 200);
    let mut p = punch();
    assert_eq!(m.rearm(&mut p, 200).unwrap(), Range::new(100, 200));
    assert!(p.on);
}

#[test]
fn rearm_refuses_a_range_one_frame_past_the_session_end() {
    let m = remembered(100, 200);
    let mut p = punch();
    let e = m.rearm(&mut p, 199).unwrap_err();
    assert!(e.0.contains("outside the session"), "{}", e.0);
    assert_eq!(p.range, None);
    assert!(!p.on);
}

#[test]
fn rearm_refuses_when_the_session_is_empty() {
    let m = remembered(0, 10);
    let mut p = punch();
    assert!(m.rearm(&mut p, 0).is_err());
    assert!(!p.on);
}

#[test]
fn a_refused_rearm_leaves_an_existing_punch_as_it_was() {
    let m = remembered(100, 200);
    let mut p = punch();
    p.set(1, 2).unwrap();
    assert!(m.rearm(&mut p, 150).is_err());
    assert_eq!(p.range, Some(Range::new(1, 2)));
    assert!(!p.on);
}

#[test]
fn clear_forgets_the_range() {
    let mut m = remembered(100, 200);
    m.clear();
    assert_eq!(m.last(), None);
    let mut p = punch();
    assert!(m.rearm(&mut p, 1_000).is_err());
    assert_eq!(p.range, None);
    assert!(!p.on);
}

#[test]
fn clear_on_an_empty_memory_is_harmless() {
    let mut m = PunchAgain::new();
    m.clear();
    assert_eq!(m, PunchAgain::new());
}

#[test]
fn a_range_can_be_remembered_again_after_clear() {
    let mut m = remembered(100, 200);
    m.clear();
    m.record(7, 9).unwrap();
    assert_eq!(m.last(), Some(Range::new(7, 9)));
    let mut p = punch();
    assert_eq!(m.rearm(&mut p, 100).unwrap(), Range::new(7, 9));
}
