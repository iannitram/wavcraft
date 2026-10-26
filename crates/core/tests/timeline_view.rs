//! C082-a: the multitrack timeline view model (zoom, scroll, fit, row height), in seconds and
//! pixels. Every expected value is a power-of-two fraction worked out by hand, so the checks are
//! exact. Session end 64 s across 1024 px fits at 0.0625 s/px.

use wavcraft_core::multitrack::timeline::{
    DEFAULT_TRACK_HEIGHT_PX, MAX_TRACK_HEIGHT_PX, MIN_SECONDS_PER_PX, MIN_TRACK_HEIGHT_PX, TimelineView, fit_seconds_per_px, max_start_s,
};

const END: f64 = 64.0;
const W: f64 = 1024.0;

/// The fit view zoomed in 2x around the middle: start 16 s, 0.03125 s/px.
fn zoomed2() -> TimelineView {
    let mut v = TimelineView::fit(END, W);
    v.zoom_at(2.0, 512.0, W, END);
    v
}

#[test]
fn fit_puts_the_whole_session_in_the_width() {
    let v = TimelineView::fit(END, W);
    assert_eq!(v.start_s, 0.0);
    assert_eq!(v.seconds_per_px, 0.0625);
    assert_eq!(v.track_height_px, DEFAULT_TRACK_HEIGHT_PX);
    assert_eq!(v.time_at(W), END);
    assert_eq!(v.time_at(512.0), 32.0);
    assert_eq!(v.x_of(32.0), 512.0);
}

#[test]
fn fit_of_an_empty_or_bad_session_uses_the_closest_zoom() {
    for end in [0.0, -5.0, f64::NAN, f64::INFINITY, 0.1] {
        let v = TimelineView::fit(end, 1000.0);
        assert_eq!(v.seconds_per_px, MIN_SECONDS_PER_PX, "end {end}");
        assert_eq!(v.start_s, 0.0, "end {end}");
    }
}

#[test]
fn fit_of_a_zero_or_bad_width_counts_one_pixel() {
    for width in [0.0, -10.0, f64::NAN, f64::INFINITY] {
        let v = TimelineView::fit(END, width);
        assert_eq!(v.seconds_per_px, 64.0, "width {width}");
        assert_eq!(v.start_s, 0.0, "width {width}");
    }
}

#[test]
fn the_fit_level_values() {
    assert_eq!(fit_seconds_per_px(64.0, 1024.0), 0.0625);
    assert_eq!(fit_seconds_per_px(100.0, 2.0), 50.0);
    assert_eq!(fit_seconds_per_px(0.0, 1024.0), MIN_SECONDS_PER_PX);
    assert_eq!(fit_seconds_per_px(-5.0, 1024.0), MIN_SECONDS_PER_PX);
    assert_eq!(fit_seconds_per_px(f64::NAN, 1024.0), MIN_SECONDS_PER_PX);
    assert_eq!(fit_seconds_per_px(0.1, 1000.0), MIN_SECONDS_PER_PX);
    assert_eq!(fit_seconds_per_px(64.0, 0.0), 64.0);
    assert_eq!(fit_seconds_per_px(64.0, f64::NAN), 64.0);
}

#[test]
fn the_furthest_start_values() {
    // Whole session fits (visible 64 s, session 64 s): no scrolling.
    assert_eq!(max_start_s(0.0625, 1024.0, 64.0), 0.0);
    assert_eq!(max_start_s(0.0625, 1024.0, 10.0), 0.0);
    // Visible exactly the session: still none; a little longer: end - visible + 40 px.
    assert_eq!(max_start_s(0.5, 100.0, 50.0), 0.0);
    assert_eq!(max_start_s(0.5, 100.0, 50.5), 20.5);
    // Zoomed in: 64 - 32 + 40 * 0.03125.
    assert_eq!(max_start_s(0.03125, 1024.0, 64.0), 33.25);
    assert_eq!(max_start_s(0.5, 100.0, 1000.0), 970.0);
    // A zero width counts one pixel: 1000 - 0.5 + 40 * 0.5.
    assert_eq!(max_start_s(0.5, 0.0, 1000.0), 1019.5);
    assert_eq!(max_start_s(0.5, 100.0, f64::NAN), 0.0);
    assert_eq!(max_start_s(0.5, 100.0, -5.0), 0.0);
}

#[test]
fn the_furthest_start_repairs_bad_inputs() {
    assert_eq!(max_start_s(0.5, 100.0, f64::INFINITY), 0.0);
    assert_eq!(max_start_s(0.5, f64::INFINITY, 1000.0), 1019.5);
    assert_eq!(max_start_s(0.5, f64::NAN, 1000.0), 1019.5);
    // A bad zoom counts as the closest zoom: 1000 - 100 * 0.0005 + 40 * 0.0005.
    for spp in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!((max_start_s(spp, 100.0, 1000.0) - 999.97).abs() < 1e-9, "spp {spp}");
    }
}

#[test]
fn a_fit_view_never_scrolls_even_when_the_float_product_is_a_hair_short() {
    // For these sizes `(end / width) * width` comes out just below `end`.
    for (end, width) in [(123.0, 1920.0), (123.0, 777.0), (61.0, 7.0)] {
        let mut v = TimelineView::fit(end, width);
        assert_eq!(max_start_s(v.seconds_per_px, width, end), 0.0, "end {end} width {width}");
        v.scroll_by(100.0, width, end);
        assert_eq!(v.start_s, 0.0, "end {end} width {width}");
    }
}

#[test]
fn a_bad_width_counts_one_pixel_in_zoom() {
    for width in [0.0, f64::NAN, f64::INFINITY, -3.0] {
        let mut v = TimelineView::fit(END, width);
        v.zoom_at(2.0, 50.0, width, END);
        assert_eq!(v.seconds_per_px, 32.0, "width {width}");
        assert_eq!(v.start_s, 32.0, "width {width}");
    }
}

#[test]
fn zoom_in_around_the_middle_keeps_the_time_under_the_anchor() {
    let v = zoomed2();
    assert_eq!(v.seconds_per_px, 0.03125);
    assert_eq!(v.start_s, 16.0);
    assert_eq!(v.time_at(512.0), 32.0);
}

#[test]
fn zoom_in_at_the_edges_keeps_the_time_under_the_anchor() {
    let mut left = zoomed2();
    left.zoom_at(2.0, 0.0, W, END);
    assert_eq!(left.seconds_per_px, 0.015625);
    assert_eq!(left.start_s, 16.0);
    assert_eq!(left.time_at(0.0), 16.0);

    let mut right = TimelineView::fit(END, W);
    right.zoom_at(2.0, W, W, END);
    assert_eq!(right.seconds_per_px, 0.03125);
    assert_eq!(right.start_s, 32.0);
    assert_eq!(right.time_at(W), END);
}

#[test]
fn three_zoom_ins_hold_the_anchor() {
    let mut v = TimelineView::fit(END, W);
    for (n, start) in [8.0, 12.0, 14.0].into_iter().enumerate() {
        v.zoom_at(2.0, 256.0, W, END);
        assert_eq!(v.start_s, start, "step {n}");
        assert_eq!(v.time_at(256.0), 16.0, "step {n}");
    }
    assert_eq!(v.seconds_per_px, 0.0078125);
}

#[test]
fn zooming_out_undoes_zooming_in() {
    let mut v = zoomed2();
    v.zoom_at(0.5, 512.0, W, END);
    assert_eq!(v, TimelineView::fit(END, W));
}

#[test]
fn zoom_in_stops_at_the_closest_zoom() {
    let mut v = TimelineView::fit(END, W);
    v.zoom_at(1e6, 512.0, W, END);
    assert_eq!(v.seconds_per_px, MIN_SECONDS_PER_PX);
    assert!((v.time_at(512.0) - 32.0).abs() < 1e-9);
}

#[test]
fn zoom_out_stops_at_the_fit_level_and_scrolls_back_to_the_start() {
    let mut v = zoomed2();
    v.zoom_at(0.001, 256.0, W, END);
    assert_eq!(v.seconds_per_px, 0.0625);
    assert_eq!(v.start_s, 0.0);
}

#[test]
fn zoom_keeps_the_row_height() {
    let mut v = TimelineView::fit(END, W);
    v.track_height_px = 200.0;
    v.zoom_at(2.0, 100.0, W, END);
    v.scroll_by(10.0, W, END);
    assert_eq!(v.track_height_px, 200.0);
}

#[test]
fn a_bad_factor_changes_nothing() {
    for factor in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut v = zoomed2();
        v.zoom_at(factor, 300.0, W, END);
        assert_eq!(v, zoomed2(), "factor {factor}");
    }
}

#[test]
fn an_anchor_outside_the_width_is_pulled_in() {
    let mut before = zoomed2();
    before.zoom_at(2.0, -100.0, W, END);
    assert_eq!(before.start_s, 16.0);
    let mut after = zoomed2();
    after.zoom_at(2.0, 5000.0, W, END);
    assert_eq!(after.start_s, 32.0);
    for anchor in [f64::NAN, f64::INFINITY] {
        let mut v = zoomed2();
        v.zoom_at(2.0, anchor, W, END);
        assert_eq!(v.start_s, 16.0, "anchor {anchor}");
    }
}

#[test]
fn an_empty_session_stays_at_the_closest_zoom() {
    let mut v = TimelineView::fit(0.0, W);
    v.zoom_at(2.0, 100.0, W, 0.0);
    assert_eq!(v.seconds_per_px, MIN_SECONDS_PER_PX);
    assert_eq!(v.start_s, 0.0);
    v.scroll_by(500.0, W, 0.0);
    assert_eq!(v.start_s, 0.0);
}

#[test]
fn a_zero_width_does_not_break_zoom() {
    let mut v = TimelineView::fit(END, 0.0);
    v.zoom_at(2.0, 0.0, 0.0, END);
    assert_eq!(v.seconds_per_px, 32.0);
    assert_eq!(v.start_s, 0.0);
    let mut w = TimelineView::fit(END, 0.0);
    w.zoom_at(2.0, 50.0, 0.0, END);
    assert_eq!(w.start_s, 32.0);
    let mut s = TimelineView::fit(END, 0.0);
    s.scroll_by(10.0, 0.0, END);
    assert_eq!(s.start_s, 0.0);
}

#[test]
fn scroll_moves_by_pixels() {
    let mut v = zoomed2();
    v.scroll_by(100.0, W, END);
    assert_eq!(v.start_s, 19.125);
    v.scroll_by(-100.0, W, END);
    assert_eq!(v.start_s, 16.0);
    v.scroll_by(-512.0, W, END);
    assert_eq!(v.start_s, 0.0);
}

#[test]
fn scroll_stops_at_zero() {
    let mut v = zoomed2();
    v.scroll_by(-1000.0, W, END);
    assert_eq!(v.start_s, 0.0);
}

#[test]
fn scroll_stops_a_little_past_the_end() {
    let mut v = zoomed2();
    v.scroll_by(1e6, W, END);
    assert_eq!(v.start_s, 33.25);
    assert_eq!(v.time_at(W), 65.25);
    let mut deeper = zoomed2();
    deeper.zoom_at(2.0, 0.0, W, END);
    deeper.scroll_by(1e6, W, END);
    assert_eq!(deeper.start_s, 48.625);
}

#[test]
fn scroll_ignores_a_bad_amount() {
    for dx in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut v = zoomed2();
        v.scroll_by(dx, W, END);
        assert_eq!(v, zoomed2(), "dx {dx}");
    }
}

#[test]
fn nothing_scrolls_while_the_whole_session_fits() {
    let mut v = TimelineView::fit(END, W);
    v.scroll_by(500.0, W, END);
    assert_eq!(v.start_s, 0.0);
    v.scroll_by(-500.0, W, END);
    assert_eq!(v.start_s, 0.0);
    let mut short = TimelineView { start_s: 0.0, seconds_per_px: 0.0625, track_height_px: 80.0 };
    short.scroll_by(100.0, W, 10.0);
    assert_eq!(short.start_s, 0.0);
}

#[test]
fn clamp_repairs_a_bad_view() {
    let fit = 0.0625;
    let cases = [
        (f64::NAN, 5.0, fit, 0.0),
        (0.0, 5.0, fit, 0.0),
        (-1.0, 5.0, fit, 0.0),
        (f64::INFINITY, 5.0, fit, 0.0),
        (10.0, 0.0, fit, 0.0),
        (0.0001, 0.0, MIN_SECONDS_PER_PX, 0.0),
        (0.03125, f64::NAN, 0.03125, 0.0),
        (0.03125, f64::INFINITY, 0.03125, 0.0),
        (0.03125, -5.0, 0.03125, 0.0),
        (0.03125, 1000.0, 0.03125, 33.25),
        (0.03125, 16.0, 0.03125, 16.0),
    ];
    for (n, (spp, start, want_spp, want_start)) in cases.into_iter().enumerate() {
        let mut v = TimelineView { start_s: start, seconds_per_px: spp, track_height_px: 300.0 };
        v.clamp(W, END);
        assert_eq!(v.seconds_per_px, want_spp, "case {n}");
        assert_eq!(v.start_s, want_start, "case {n}");
        assert_eq!(v.track_height_px, 300.0, "case {n}");
    }
}

#[test]
fn vertical_zoom_steps_by_sixteen_pixels() {
    let mut v = TimelineView::default();
    assert_eq!(v.track_height_px, 80.0);
    v.zoom_vertical(1);
    assert_eq!(v.track_height_px, 96.0);
    v.zoom_vertical(2);
    assert_eq!(v.track_height_px, 128.0);
    v.zoom_vertical(-3);
    assert_eq!(v.track_height_px, 80.0);
    v.zoom_vertical(0);
    assert_eq!(v.track_height_px, 80.0);
    v.zoom_vertical(19);
    assert_eq!(v.track_height_px, 384.0);
}

#[test]
fn vertical_zoom_stops_at_the_limits() {
    let mut v = TimelineView::default();
    v.zoom_vertical(-4);
    assert_eq!(v.track_height_px, MIN_TRACK_HEIGHT_PX);
    v.zoom_vertical(-1);
    assert_eq!(v.track_height_px, 24.0);
    v.zoom_vertical(20);
    assert_eq!(v.track_height_px, 344.0);
    v.zoom_vertical(21);
    assert_eq!(v.track_height_px, MAX_TRACK_HEIGHT_PX);
    v.zoom_vertical(1);
    assert_eq!(v.track_height_px, 400.0);
    v.zoom_vertical(i32::MIN);
    assert_eq!(v.track_height_px, 24.0);
    v.zoom_vertical(i32::MAX);
    assert_eq!(v.track_height_px, 400.0);
}

#[test]
fn vertical_zoom_repairs_a_bad_height() {
    for (height, steps, want) in
        [(f64::NAN, 1, 96.0), (f64::INFINITY, 1, 96.0), (f64::NEG_INFINITY, -1, 64.0), (1000.0, 0, 400.0), (5.0, 0, 24.0), (100.0, 0, 100.0)]
    {
        let mut v = TimelineView { start_s: 0.0, seconds_per_px: 0.01, track_height_px: height };
        v.zoom_vertical(steps);
        assert_eq!(v.track_height_px, want, "height {height} steps {steps}");
    }
}

#[test]
fn time_and_pixel_convert_both_ways() {
    let v = TimelineView { start_s: 10.0, seconds_per_px: 0.5, track_height_px: 80.0 };
    assert_eq!(v.time_at(0.0), 10.0);
    assert_eq!(v.time_at(40.0), 30.0);
    assert_eq!(v.time_at(-4.0), 8.0);
    assert_eq!(v.x_of(10.0), 0.0);
    assert_eq!(v.x_of(30.0), 40.0);
    assert_eq!(v.x_of(5.0), -10.0);
    for x in [0.0, 17.0, 512.0, -30.0] {
        assert_eq!(v.x_of(v.time_at(x)), x);
    }
}

#[test]
fn a_zero_zoom_converts_with_the_closest_zoom() {
    let v = TimelineView { start_s: 0.0, seconds_per_px: 0.0, track_height_px: 80.0 };
    assert!((v.time_at(1000.0) - 0.5).abs() < 1e-9);
    assert!((v.x_of(1.0) - 2000.0).abs() < 1e-6);
    let nan = TimelineView { start_s: 0.0, seconds_per_px: f64::NAN, track_height_px: 80.0 };
    assert!((nan.time_at(1000.0) - 0.5).abs() < 1e-9);
}

#[test]
fn a_new_view_has_the_defaults() {
    let v = TimelineView::default();
    assert_eq!(v.start_s, 0.0);
    assert_eq!(v.seconds_per_px, 0.01);
    assert_eq!(v.track_height_px, 80.0);
}
