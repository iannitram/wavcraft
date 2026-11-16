//! Tests for roadmap B1.2: the frame-time summary (count, median, worst and frames over 16.7 ms).

use wavcraft_core::frame_stats::{FrameSummary, summarize_frames};

#[test]
fn summary_of_five_frames() {
    let s = summarize_frames(&[10.0, 12.0, 14.0, 16.0, 100.0]);
    assert_eq!(s.count, 5);
    assert_eq!(s.median_ms, 14.0);
    assert_eq!(s.worst_ms, 100.0);
    assert_eq!(s.over_budget, 1);
}

#[test]
fn an_empty_list_is_all_zero() {
    let s = summarize_frames(&[]);
    assert_eq!(s, FrameSummary::default());
    assert_eq!(s.count, 0);
    assert_eq!(s.median_ms, 0.0);
    assert_eq!(s.worst_ms, 0.0);
    assert_eq!(s.over_budget, 0);
}

#[test]
fn an_even_count_takes_the_mean_of_the_middle_two() {
    let s = summarize_frames(&[3.0, 1.0]);
    assert_eq!(s.count, 2);
    assert_eq!(s.median_ms, 2.0);
    assert_eq!(s.worst_ms, 3.0);
    assert_eq!(s.over_budget, 0);
}

#[test]
fn a_frame_of_exactly_16_7_is_not_over() {
    let s = summarize_frames(&[16.7, 16.8]);
    assert_eq!(s.count, 2);
    assert_eq!(s.over_budget, 1);
}

#[test]
fn values_that_are_not_finite_are_skipped() {
    let s = summarize_frames(&[f64::NAN, 5.0, f64::INFINITY]);
    assert_eq!(s.count, 1);
    assert_eq!(s.median_ms, 5.0);
    assert_eq!(s.worst_ms, 5.0);
    assert_eq!(s.over_budget, 0);
}
