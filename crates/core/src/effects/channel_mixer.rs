//! Channel Mixer: each output channel is a weighted sum of the left and right inputs, with an
//! optional polarity flip. Swaps channels, sums to mono, or rebalances a stereo pair.
//!
//! The seam hands over one slice per ACTIVE channel. With fewer than two (a mono file, or a
//! channel mask selecting one channel) there is nothing to mix, so the input is copied to the
//! output unchanged. With more than two slices only the first two are mixed; the rest pass
//! through unchanged.

use std::io;

use super::{Effect, ParamDesc};

pub struct ChannelMixer;

const OFF_ON: &[&str] = &["off", "on"];

impl Effect for ChannelMixer {
    fn id(&self) -> &'static str {
        "channel_mixer"
    }
    fn name(&self) -> &'static str {
        "Channel Mixer"
    }
    fn parameters(&self) -> &'static [ParamDesc] {
        const P: &[ParamDesc] = &[
            ParamDesc::percent("l_from_l", "Left from left", -100.0, 100.0, 100.0),
            ParamDesc::percent("l_from_r", "Left from right", -100.0, 100.0, 0.0),
            ParamDesc::percent("r_from_l", "Right from left", -100.0, 100.0, 0.0),
            ParamDesc::percent("r_from_r", "Right from right", -100.0, 100.0, 100.0),
            ParamDesc::choice("invert_l", "Invert left", OFF_ON, 0),
            ParamDesc::choice("invert_r", "Invert right", OFF_ON, 0),
        ];
        P
    }
    fn process(&self, params: &[f64], _first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        // Anything the mixer does not touch is copied through.
        for (i, o) in input.iter().zip(output.iter_mut()) {
            o.copy_from_slice(i);
        }
        if input.len() < 2 || output.len() < 2 {
            return Ok(());
        }
        let (ll, lr, rl, rr) = (params[0] / 100.0, params[1] / 100.0, params[2] / 100.0, params[3] / 100.0);
        let (sl, sr) = (if params[4] != 0.0 { -1.0 } else { 1.0 }, if params[5] != 0.0 { -1.0 } else { 1.0 });
        if (ll, lr, rl, rr, sl, sr) == (1.0, 0.0, 0.0, 1.0, 1.0, 1.0) {
            return Ok(());
        }
        let (left, right) = (input[0], input[1]);
        let [out_l, out_r, ..] = output else { return Ok(()) };
        for n in 0..left.len() {
            let (l, r) = (f64::from(left[n]), f64::from(right[n]));
            out_l[n] = (sl * (ll * l + lr * r)) as f32;
            out_r[n] = (sr * (rl * l + rr * r)) as f32;
        }
        Ok(())
    }
}
