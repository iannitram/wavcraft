//! Locked tests for v1 E1 (issue #27, card wav-c-27-b): top-corner waveform fade handle
//! hit-testing and inward-drag range and curve calculation.

use wavcraft_core::dsp::fade::FadeCurve;
use wavcraft_core::fade_handles::{FadeGesture, FadeHandle, HANDLE_HIT_PX, active_fade_range, drag_fade, handle_near};
use wavcraft_core::view::{Selection, Viewport};

#[test]
fn e1_handles_active_range_uses_whole_file_or_clamped_selection() {
    assert_eq!(HANDLE_HIT_PX, 8.0);
    assert_eq!(active_fade_range(&Selection::at(200), 1000), 0..1000);
    assert_eq!(active_fade_range(&Selection { anchor: 800, head: 200 }, 1000), 200..800);
    assert_eq!(active_fade_range(&Selection { anchor: 300, head: 1500 }, 1000), 300..1000);
    assert_eq!(active_fade_range(&Selection { anchor: 1200, head: 1500 }, 1000), 1000..1000);
}

#[test]
fn e1_handles_hit_top_corners_within_eight_pixels() {
    let v = Viewport { start: 0.0, frames_per_px: 10.0 };
    let sel = Selection { anchor: 200, head: 800 }; // px 20..80
    assert_eq!(handle_near(&v, &sel, 1000, 25.0, 4.0), Some(FadeHandle::In));
    assert_eq!(handle_near(&v, &sel, 1000, 74.0, 8.0), Some(FadeHandle::Out));
    assert_eq!(handle_near(&v, &sel, 1000, 20.0, 8.5), None);
    assert_eq!(handle_near(&v, &sel, 1000, 20.0, -0.5), None);
    assert_eq!(handle_near(&v, &sel, 1000, 50.0, 3.0), None);
    assert_eq!(handle_near(&v, &Selection::at(100), 1000, 3.0, 2.0), Some(FadeHandle::In));
    assert_eq!(handle_near(&v, &Selection::at(100), 1000, 96.0, 2.0), Some(FadeHandle::Out));
    let thin = Selection { anchor: 500, head: 540 }; // px 50..54
    assert_eq!(handle_near(&v, &thin, 1000, 51.5, 2.0), Some(FadeHandle::In));
    assert_eq!(handle_near(&v, &thin, 1000, 52.0, 2.0), Some(FadeHandle::In));
    assert_eq!(handle_near(&v, &thin, 1000, 52.5, 2.0), Some(FadeHandle::Out));
    assert_eq!(handle_near(&v, &sel, 0, 20.0, 2.0), None);
    assert_eq!(handle_near(&v, &sel, 1000, f32::NAN, 2.0), None);
    assert_eq!(handle_near(&v, &sel, 1000, 20.0, f32::NAN), None);
}

#[test]
fn e1_handles_drag_fade_in_sets_length_from_start_and_picks_curve() {
    let v = Viewport { start: 0.0, frames_per_px: 10.0 };
    assert_eq!(
        drag_fade(FadeHandle::In, &v, 100..900, 35.0, 0.0, false),
        Some(FadeGesture { handle: FadeHandle::In, range: 100..350, curve: FadeCurve::Linear })
    );
    assert_eq!(
        drag_fade(FadeHandle::In, &v, 100..900, 35.0, -12.0, false),
        Some(FadeGesture { handle: FadeHandle::In, range: 100..350, curve: FadeCurve::Logarithmic })
    );
    assert_eq!(
        drag_fade(FadeHandle::In, &v, 100..900, 35.0, 0.0, true),
        Some(FadeGesture { handle: FadeHandle::In, range: 100..350, curve: FadeCurve::Cosine })
    );
    assert_eq!(
        drag_fade(FadeHandle::In, &v, 100..900, 120.0, 0.0, false),
        Some(FadeGesture { handle: FadeHandle::In, range: 100..900, curve: FadeCurve::Linear })
    );
}

#[test]
fn e1_handles_drag_fade_out_sets_length_to_end_and_picks_curve() {
    let v = Viewport { start: 0.0, frames_per_px: 10.0 };
    assert_eq!(
        drag_fade(FadeHandle::Out, &v, 100..900, 65.0, 10.0, false),
        Some(FadeGesture { handle: FadeHandle::Out, range: 650..900, curve: FadeCurve::Logarithmic })
    );
    assert_eq!(
        drag_fade(FadeHandle::Out, &v, 100..900, 65.0, 15.0, true),
        Some(FadeGesture { handle: FadeHandle::Out, range: 650..900, curve: FadeCurve::Cosine })
    );
    assert_eq!(
        drag_fade(FadeHandle::Out, &v, 100..900, 2.0, 0.0, false),
        Some(FadeGesture { handle: FadeHandle::Out, range: 100..900, curve: FadeCurve::Linear })
    );
}

#[test]
fn e1_handles_zero_length_or_bad_drag_returns_none() {
    let v = Viewport { start: 0.0, frames_per_px: 10.0 };
    assert_eq!(drag_fade(FadeHandle::In, &v, 100..900, 10.0, 0.0, false), None);
    assert_eq!(drag_fade(FadeHandle::In, &v, 100..900, 5.0, 0.0, false), None);
    assert_eq!(drag_fade(FadeHandle::Out, &v, 100..900, 90.0, 0.0, false), None);
    assert_eq!(drag_fade(FadeHandle::Out, &v, 100..900, 95.0, 0.0, false), None);
    assert_eq!(drag_fade(FadeHandle::In, &v, 500..500, 60.0, 0.0, false), None);
    assert_eq!(drag_fade(FadeHandle::In, &v, 100..900, f32::NAN, 0.0, false), None);
}
