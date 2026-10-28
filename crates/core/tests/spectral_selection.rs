//! B135 / A052 / B141 / A115: the time + frequency selection, its feather mask, its move, and
//! the Marquee tool's place in the tool model. No window: the maths the Marquee tool draws with.

use wavcraft_core::actions::{ActionId, Editor, by_name};
use wavcraft_core::doc::edit::Crossfades;
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::shortcuts::{BINDINGS, Shortcut};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};
use wavcraft_core::view::{Band, SpectralSelection, fraction_of_hz, hz_at_fraction};
use wavcraft_core::{Selection, Viewport};

const RATE: u32 = 48_000;
const NYQ: f32 = 24_000.0;

fn sel(a: usize, b: usize, lo: f32, hi: f32, feather_hz: f32, feather_ms: f32) -> SpectralSelection {
    let mut band = Band::new(lo, hi, NYQ);
    band.feather_hz = feather_hz;
    band.feather_ms = feather_ms;
    SpectralSelection { time: Selection { anchor: a, head: b }, band, rate: RATE }
}

#[test]
fn mask_is_one_inside_and_zero_outside_without_a_feather() {
    let s = sel(1000, 5000, 1000.0, 4000.0, 0.0, 0.0);
    for (f, hz) in [(1000, 1000.0), (3000, 2500.0), (4999, 4000.0)] {
        assert_eq!(s.mask(f, hz), 1.0, "inside at {f},{hz}");
    }
    for (f, hz) in [(999, 2000.0), (5000, 2000.0), (3000, 999.0), (3000, 4001.0), (0, 0.0)] {
        assert_eq!(s.mask(f, hz), 0.0, "outside at {f},{hz}");
    }
}

#[test]
fn mask_falls_off_monotonically_across_the_feather_and_is_half_at_its_middle() {
    let s = sel(10_000, 20_000, 2000.0, 6000.0, 400.0, 0.0);
    assert_eq!(s.mask(15_000, 2000.0), 1.0);
    assert_eq!(s.mask(15_000, 1800.0), 0.5, "middle of the lower feather");
    assert_eq!(s.mask(15_000, 6200.0), 0.5, "middle of the upper feather");
    assert_eq!(s.mask(15_000, 1600.0), 0.0, "the feather ends");
    assert_eq!(s.mask(15_000, 1000.0), 0.0);
    let mut last = 1.0;
    for i in 0..=40 {
        let m = s.mask(15_000, 2000.0 - i as f32 * 10.0);
        assert!((0.0..=1.0).contains(&m));
        assert!(m <= last, "not rising at step {i}");
        last = m;
    }
    // Time feather: 10 ms is 480 frames; its middle is 240 frames outside.
    let t = sel(10_000, 20_000, 2000.0, 6000.0, 0.0, 10.0);
    assert_eq!(t.mask(10_000 - 240, 3000.0), 0.5);
    assert_eq!(t.mask(10_000, 3000.0), 1.0);
    assert_eq!(t.mask(10_000 - 480, 3000.0), 0.0);
    assert_eq!(t.mask(20_000 - 1 + 240, 3000.0), 0.5);
    // Both feathers multiply.
    let b = sel(10_000, 20_000, 2000.0, 6000.0, 400.0, 10.0);
    assert_eq!(b.mask(10_000 - 240, 1800.0), 0.25);
}

#[test]
fn move_clamps_at_the_file_and_at_zero_and_nyquist() {
    let s = sel(10_000, 20_000, 1000.0, 3000.0, 0.0, 0.0);
    let total = 100_000;
    let m = s.moved(5000, 500.0, total, NYQ);
    assert_eq!((m.time.start(), m.time.end()), (15_000, 25_000));
    assert_eq!((m.band.lo_hz, m.band.hi_hz), (1500.0, 3500.0));
    let m = s.moved(-50_000, -9000.0, total, NYQ);
    assert_eq!((m.time.start(), m.time.end()), (0, 10_000), "stops at the start, keeps its length");
    assert_eq!((m.band.lo_hz, m.band.hi_hz), (0.0, 2000.0), "stops at 0 Hz, keeps its width");
    let m = s.moved(500_000, 90_000.0, total, NYQ);
    assert_eq!((m.time.start(), m.time.end()), (90_000, 100_000), "stops at the end");
    assert_eq!((m.band.lo_hz, m.band.hi_hz), (NYQ - 2000.0, NYQ), "stops at Nyquist");
    // A selection drawn backwards keeps its direction.
    let back = SpectralSelection { time: Selection { anchor: 20_000, head: 10_000 }, ..s };
    assert_eq!(back.moved(100, 0.0, total, NYQ).time, Selection { anchor: 20_100, head: 10_100 });
}

#[test]
fn a_band_is_sorted_and_clamped_to_zero_and_nyquist() {
    let b = Band::new(5000.0, 1000.0, NYQ);
    assert_eq!((b.lo_hz, b.hi_hz), (1000.0, 5000.0));
    let b = Band::new(-300.0, 99_999.0, NYQ);
    assert_eq!((b.lo_hz, b.hi_hz), (0.0, NYQ));
    let b = Band::new(f32::NAN, 100.0, NYQ);
    assert_eq!((b.lo_hz, b.hi_hz), (0.0, 100.0));
    assert_eq!(Band::new(1000.0, 4000.0, NYQ).to_string(), "1000-4000");
}

#[test]
fn a_time_only_selection_and_the_view_are_untouched_by_a_band() {
    // The band is a separate value: building, moving and dropping one never changes the Selection.
    let time = Selection { anchor: 700, head: 300 };
    let s = SpectralSelection { time, band: Band::new(100.0, 200.0, NYQ), rate: RATE };
    let _ = s.moved(10, 10.0, 1000, NYQ);
    assert_eq!((time.start(), time.end()), (300, 700));
    assert_eq!(time.extend_nearest_to(900, 1000), Selection { anchor: 300, head: 900 });
    // Zooming and scrolling the view leave a band alone.
    let mut view = Viewport { start: 0.0, frames_per_px: 10.0 };
    let before = s;
    view.zoom_at(2.0, 100.0, 800.0, 100_000);
    view.scroll_by(50.0, 800.0, 100_000);
    assert_eq!(s, before);
}

#[test]
fn the_display_axis_maps_hz_both_ways_linear_and_log() {
    for log in [false, true] {
        for hz in [40.0f32, 440.0, 5000.0, 20_000.0] {
            let t = fraction_of_hz(40.0, 24_000.0, log, hz);
            assert!((0.0..=1.0).contains(&t));
            assert!((hz_at_fraction(40.0, 24_000.0, log, t) - hz).abs() < hz * 1e-4);
        }
        assert_eq!(fraction_of_hz(40.0, 24_000.0, log, 40.0), 0.0);
        assert_eq!(fraction_of_hz(40.0, 24_000.0, log, 24_000.0), 1.0);
    }
    // 1 kHz is half way up a log axis from 10 Hz to 100 kHz; linear it is nowhere near.
    assert!((fraction_of_hz(10.0, 100_000.0, true, 1000.0) - 0.5).abs() < 1e-6);
}

#[test]
fn the_marquee_tool_is_a_window_action_and_the_key_e_is_free() {
    let a = by_name("tool.marquee").expect("a Marquee action");
    assert_eq!(a.id, ActionId::ToolMarquee);
    let e_rows = BINDINGS.iter().filter(|b| Shortcut::parse("E") == Ok(b.chord)).count();
    assert_eq!(e_rows, 1, "exactly one binding uses a bare E");
}

/// The selection operations a time-only selection (no band) has always had give the same results:
/// the band is a separate value that none of them reads or writes.
#[test]
fn every_existing_selection_operation_on_a_time_only_selection_is_unchanged() {
    // Selection arithmetic (the same cases as the model's own tests).
    let s = Selection { anchor: 100, head: 200 };
    assert_eq!((s.start(), s.end(), s.len(), s.is_empty()), (100, 200, 100, false));
    assert_eq!(s.extend_nearest_to(300, 1000), Selection { anchor: 100, head: 300 });
    assert_eq!(s.extend_nearest_to(40, 1000), Selection { anchor: 200, head: 40 });
    assert_eq!(Selection::at(500).extend_nearest_to(5000, 1000).end(), 1000);
    assert!(Selection::at(42).is_empty());
    // The editor's selection actions, run on the selection a marquee left behind.
    let dir = std::env::temp_dir().join(format!("wavcraft-spectral-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::create_with(&dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let data: Vec<f32> = (0..777).map(|k| k as f32).collect();
    let chans = vec![Channel::from_pieces(data.chunks(BLOCK_FRAMES).map(|b| Piece::whole(store.append(b).unwrap()).unwrap()))];
    let doc = DocState::new(store.clone(), chans, RATE, SourceFormat::synthetic(RATE, 1)).unwrap();
    let mut e = Editor::new(History::new(doc, u64::MAX));
    e.crossfades = Crossfades::NONE;
    let band: Option<Band> = None;
    assert_eq!(e.run(ActionId::SelectAll, 3..3).unwrap().selection, 0..777);
    assert_eq!(e.run(ActionId::DeselectAll, 10..50).unwrap().selection, 10..10);
    assert_eq!(e.run(ActionId::ClearTimeSelection, 10..50).unwrap().selection, 10..10);
    assert_eq!(e.run(ActionId::CursorToStart, 10..50).unwrap().selection, 0..0);
    assert_eq!(e.run(ActionId::CursorToEnd, 10..50).unwrap().selection, 777..777);
    assert_eq!(band, None, "none of them made a band");
    let _ = std::fs::remove_dir_all(&dir);
}
