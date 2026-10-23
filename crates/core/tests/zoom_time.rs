//! Zoom (time navigator, vertical zoom, amplitude ruler) and the time display (formats, typed
//! times, settings, actions), through the public API only. None of this existed before PR 525, so
//! on the old source these tests do not compile, and each one pins a behaviour a person sees.

use wavcraft_core::actions::{ActionId, by_name};
use wavcraft_core::settings::Settings;
use wavcraft_core::view::{NavPart, nav_hit};
use wavcraft_core::{AmpScale, AmpZoom, Selection, TimeFormat, Viewport};

const RATE: u32 = 48_000;
const TOTAL: usize = RATE as usize * 600; // ten minutes
const W: f32 = 1000.0;

#[test]
fn time_display_writes_the_same_instant_in_four_ways() {
    let at = 3723 * RATE as usize + RATE as usize / 2; // 1:02:03.5
    assert_eq!(TimeFormat::Decimal.format(at, RATE), "1:02:03.500");
    assert_eq!(TimeFormat::Compact.format(at, RATE), "1:02:03.5");
    assert_eq!(TimeFormat::Compact.format(3 * RATE as usize, RATE), "0:03");
    assert_eq!(TimeFormat::Samples.format(at, RATE), at.to_string());
    assert_eq!(TimeFormat::Smpte30.format(at, RATE), "01:02:03:15");
    // No file open (rate 0) never panics.
    for f in TimeFormat::ALL {
        let _ = f.format(12_345, 0);
    }
}

#[test]
fn a_typed_time_in_the_chosen_format_round_trips_at_any_rate() {
    for rate in [8000u32, 22_050, 44_100, 48_000, 96_000] {
        for frame in [0usize, 1, 441, 999, rate as usize - 1, rate as usize, 12_345_678, 3600 * rate as usize + 17] {
            for f in TimeFormat::ALL {
                let text = f.format(frame, rate);
                let back = f.parse(&text, rate).unwrap_or_else(|| panic!("{f:?} {text:?} did not parse at {rate}"));
                assert_eq!(f.format(back, rate), text, "{f:?} at {rate} Hz, frame {frame}");
            }
        }
    }
    assert_eq!(TimeFormat::Decimal.parse("1:02.5", RATE), Some(62 * 48_000 + 24_000));
    assert_eq!(TimeFormat::Smpte30.parse("1:15", RATE), Some(48_000 + 24_000));
    for bad in ["", "abc", "1:75.0", "-3", "1.", "1::2"] {
        assert_eq!(TimeFormat::Decimal.parse(bad, RATE), None, "{bad:?}");
    }
    assert_eq!(TimeFormat::Smpte30.parse("00:00:01:30", RATE), None, "no frame 30 at 30 fps");
    assert_eq!(TimeFormat::Decimal.parse("5", 0), None, "no rate, no time");
}

#[test]
fn ruler_ticks_are_exact_whatever_the_rate() {
    // 3 ms at 44.1 kHz is not a whole frame; the tick must still read 3 ms, not 2.
    assert_eq!(TimeFormat::Decimal.format_tick(0.003, 44_100), "0:00.003");
    assert_eq!(TimeFormat::Samples.format_tick(0.5, RATE), "24000");
    assert_eq!(TimeFormat::Smpte30.format_tick(1.0, RATE), "00:00:01:00");
}

#[test]
fn the_time_display_choice_is_a_menu_action_and_is_kept_in_the_settings() {
    for f in TimeFormat::ALL {
        let name = format!("view.time_display.{}", f.name());
        assert!(by_name(&name).is_some(), "{name} is an action");
        assert_eq!(TimeFormat::from_name(f.name()), Some(f));
    }
    assert_eq!(by_name("view.time_display.smpte30").map(|a| a.id), Some(ActionId::TimeSmpte30));
    let s = Settings { time_display: TimeFormat::Smpte30, amp_scale: AmpScale::Db, ..Settings::default() };
    let back = Settings::parse(&s.to_text());
    assert_eq!((back.time_display, back.amp_scale), (TimeFormat::Smpte30, AmpScale::Db));
}

#[test]
fn typing_selection_boxes_moves_the_selection_inside_the_file() {
    let c = Selection::at(1000);
    assert_eq!(c.with_end(5000, TOTAL), Selection { anchor: 1000, head: 5000 }, "a cursor becomes a range");
    let r = Selection { anchor: 1000, head: 5000 };
    assert_eq!((r.with_start(2000, TOTAL).start(), r.with_start(2000, TOTAL).end()), (2000, 5000));
    assert_eq!((r.with_start(9000, TOTAL).start(), r.with_start(9000, TOTAL).end()), (5000, 9000), "passing the end swaps");
    assert_eq!(r.with_len(300, TOTAL).end(), 1300);
    assert_eq!(r.with_len(usize::MAX, TOTAL).end(), TOTAL, "kept inside the file");
    assert_eq!(r.with_end(TOTAL + 99, TOTAL).end(), TOTAL);
}

#[test]
fn typed_view_values_and_set_range_show_exactly_what_was_asked() {
    let mut v = Viewport::fit(TOTAL, W);
    v.set_range(48_000.0, 96_000.0, W, TOTAL);
    assert_eq!((v.start, v.end(W)), (48_000.0, 96_000.0));
    let before = v;
    v.set_range(f64::NAN, 10.0, W, TOTAL);
    v.set_range(100.0, 100.0, W, TOTAL);
    v.set_range(200.0, 100.0, W, TOTAL);
    assert_eq!(v, before, "bad numbers leave the view alone");
    v.set_start(120_000, W, TOTAL);
    assert_eq!(v.start, 120_000.0);
    v.set_end(240_000, W, TOTAL);
    assert!((v.end(W) - 240_000.0).abs() < 1e-6);
    v.set_duration(20_000, W, TOTAL);
    assert!((v.end(W) - v.start - 20_000.0).abs() < 1e-6);
    v.set_start(usize::MAX / 2, W, TOTAL);
    assert!(v.end(W) <= TOTAL as f64 + 1e-6, "never scrolls past the end");
}

#[test]
fn the_zoom_navigator_handle_is_the_visible_part_and_drags_scroll_and_zoom() {
    let nav = 500.0;
    let mut v = Viewport::fit(TOTAL, W);
    assert_eq!(v.nav_handle(W, nav, TOTAL), (0.0, nav), "zoomed out: the handle fills the bar");
    v.set_range(TOTAL as f64 * 0.25, TOTAL as f64 * 0.5, W, TOTAL);
    let (l, r) = v.nav_handle(W, nav, TOTAL);
    assert!((l - 125.0).abs() < 0.01 && (r - 250.0).abs() < 0.01, "{l} {r}");
    // Click outside the handle: centre there.
    v.nav_jump(400.0, W, nav, TOTAL);
    let mid = (v.start + v.end(W)) / 2.0;
    assert!((mid - TOTAL as f64 * 0.8).abs() < 1.0);
    // Body drag scrolls and keeps the zoom; an edge drag zooms and keeps the other edge.
    let (fpp, end) = (v.frames_per_px, v.end(W));
    v.nav_drag(NavPart::Body, 100.0, 0.0, W, nav, TOTAL);
    assert_eq!((v.frames_per_px, v.start), (fpp, TOTAL as f64 * 0.2));
    let end2 = v.end(W);
    v.nav_drag(NavPart::Left, 50.0, 0.0, W, nav, TOTAL);
    assert!((v.end(W) - end2).abs() < 1e-3 && v.start == TOTAL as f64 * 0.1 && v.frames_per_px > fpp);
    assert!(end > 0.0);
    // Hit testing.
    let h = (100.0, 200.0);
    assert_eq!(nav_hit(h, 101.0, 6.0), Some(NavPart::Left));
    assert_eq!(nav_hit(h, 150.0, 6.0), Some(NavPart::Body));
    assert_eq!(nav_hit(h, 199.0, 6.0), Some(NavPart::Right));
    assert_eq!(nav_hit(h, 20.0, 6.0), None);
}

#[test]
fn vertical_zoom_doubles_halves_and_stops_at_its_limits() {
    let mut z = AmpZoom::full();
    z.by(2.0);
    z.by(2.0);
    assert_eq!(z.get(), 4.0);
    z.by(0.5);
    assert_eq!(z.get(), 2.0);
    for _ in 0..40 {
        z.by(0.5);
    }
    assert_eq!(z.get(), AmpZoom::MIN, "cannot zoom out past full scale");
    for _ in 0..40 {
        z.by(2.0);
    }
    assert_eq!(z.get(), AmpZoom::MAX);
    z.by(f32::NAN);
    z.by(-3.0);
    z.by(0.0);
    assert_eq!(z.get(), AmpZoom::MAX, "bad factors change nothing");
    for (name, id) in
        [("view.zoom.ampIn", ActionId::ZoomAmpIn), ("view.zoom.ampOut", ActionId::ZoomAmpOut), ("view.zoom.ampReset", ActionId::ZoomAmpReset)]
    {
        assert_eq!(by_name(name).map(|a| a.id), Some(id));
    }
}

#[test]
fn the_amplitude_ruler_labels_follow_the_scale_and_zoom() {
    assert_eq!(AmpScale::Percent.label(0.5), "50");
    assert_eq!(AmpScale::Percent.label(-1.0), "-100");
    assert_eq!(AmpScale::Samples.label(1.0), "32768");
    assert_eq!(AmpScale::Normalized.label(0.25), "0.25");
    assert_eq!(AmpScale::Db.label(0.5), "-6");
    assert_eq!(AmpScale::Db.label(0.0), "-inf");
    for s in AmpScale::ALL {
        assert_eq!(AmpScale::parse(s.name()), Some(s));
    }
    assert_eq!(AmpScale::parse("furlongs"), None);
    // Zoomed out the dB ruler starts at 0 dB; zoomed in 8x it shows only levels under -18 dB.
    let full = AmpScale::Db.ticks(AmpZoom::full(), 100.0, 16.0);
    assert_eq!(full[0].label, "0");
    let mut z = AmpZoom::full();
    z.by(8.0);
    let zoomed = AmpScale::Db.ticks(z, 100.0, 16.0);
    assert!(zoomed.iter().all(|t| t.amp <= 0.125 * 1.001), "only levels in view");
    // Linear ticks never crowd: at least the gap apart, and always from zero up.
    let ticks = AmpScale::Percent.ticks(AmpZoom::full(), 100.0, 16.0);
    assert_eq!(ticks[0].amp, 0.0);
    for w in ticks.windows(2) {
        assert!((w[1].amp - w[0].amp) * 100.0 >= 16.0 * 0.99);
    }
}
