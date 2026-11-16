//! Fade handle drag to effect call (v1 E1.1, issue #27).

use std::ops::Range;

use wavcraft_core::dsp::fade::FadeCurve;
use wavcraft_core::fade_handles::{FadeGesture, FadeHandle};

/// The effect call a fade handle drag turns into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FadeCall {
    /// The effect id: `fade_in` or `fade_out`.
    pub id: &'static str,
    /// The value of the effect's `curve` parameter (0 Linear, 1 Logarithmic, 2 Cosine).
    pub curve_param: u32,
    /// The frame range to apply the effect over.
    pub range: Range<u64>,
}

/// Turn a fade handle gesture into the effect id, curve parameter and frame range to apply.
pub fn effect_call(gesture: &FadeGesture) -> FadeCall {
    let id = match gesture.handle {
        FadeHandle::In => "fade_in",
        FadeHandle::Out => "fade_out",
    };
    let curve_param = match gesture.curve {
        FadeCurve::Linear => 0,
        FadeCurve::Logarithmic => 1,
        FadeCurve::Cosine => 2,
    };
    FadeCall { id, curve_param, range: gesture.range.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wavcraft_core::effects::Effect;
    use wavcraft_core::effects::fade::{FadeIn, FadeOut, curve_from_param};

    #[test]
    fn e1_gesture_maps_to_effect_call() {
        let fade_in = effect_call(&FadeGesture { handle: FadeHandle::In, range: 0..48000, curve: FadeCurve::Cosine });
        assert_eq!(fade_in, FadeCall { id: "fade_in", curve_param: 2, range: 0..48000 });

        let fade_out = effect_call(&FadeGesture { handle: FadeHandle::Out, range: 24000..48000, curve: FadeCurve::Linear });
        assert_eq!(fade_out, FadeCall { id: "fade_out", curve_param: 0, range: 24000..48000 });

        let log_in = effect_call(&FadeGesture { handle: FadeHandle::In, range: 0..10, curve: FadeCurve::Logarithmic });
        let log_out = effect_call(&FadeGesture { handle: FadeHandle::Out, range: 0..10, curve: FadeCurve::Logarithmic });
        assert_eq!(log_in.curve_param, 1);
        assert_eq!(log_out.curve_param, 1);

        // Agreement with the core: the parameter decodes back to the same curve, and the ids match the effects.
        for curve in [FadeCurve::Linear, FadeCurve::Logarithmic, FadeCurve::Cosine] {
            let call = effect_call(&FadeGesture { handle: FadeHandle::In, range: 0..10, curve });
            assert_eq!(curve_from_param(&[call.curve_param as f64]), curve);
        }
        let in_call = effect_call(&FadeGesture { handle: FadeHandle::In, range: 0..10, curve: FadeCurve::Linear });
        let out_call = effect_call(&FadeGesture { handle: FadeHandle::Out, range: 0..10, curve: FadeCurve::Linear });
        assert_eq!(in_call.id, FadeIn.id());
        assert_eq!(out_call.id, FadeOut.id());
    }
}
