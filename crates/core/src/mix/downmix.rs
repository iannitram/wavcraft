//! Downmixing functions for audio processing.

pub const MIX_GAIN: f32 = std::f32::consts::FRAC_1_SQRT_2;
pub const NORMALISE_GAIN: f32 = 1.0 / (1.0 + std::f32::consts::SQRT_2);

/// Downmix 5.1 to stereo without normalization
///
/// Channel order is WAV order [L, R, C, LFE, Ls, Rs]
/// Centre and both surrounds are mixed in at FRAC_1_SQRT_2 (0.7071, -3 dB)
/// LFE is dropped
///
/// # Arguments
///
/// * `frame` - A 6-element array representing the 5.1 channel frame
///
/// # Returns
///
/// A 2-element array with the left and right stereo channels
pub fn downmix_51_to_stereo_raw(frame: &[f32; 6]) -> [f32; 2] {
    let [l, r, c, _lfe, ls, rs] = *frame;
    [l + MIX_GAIN * c + MIX_GAIN * ls, r + MIX_GAIN * c + MIX_GAIN * rs]
}

/// Downmix 5.1 to stereo with normalization
///
/// Channel order is WAV order [L, R, C, LFE, Ls, Rs]
/// Centre and both surrounds are mixed in at FRAC_1_SQRT_2 (0.7071, -3 dB)
/// LFE is dropped
/// Normalized variant scales by 1/(1+SQRT_2) so a full-scale signal on every channel stays at full scale
///
/// # Arguments
///
/// * `frame` - A 6-element array representing the 5.1 channel frame
///
/// # Returns
///
/// A 2-element array with the left and right stereo channels
pub fn downmix_51_to_stereo(frame: &[f32; 6]) -> [f32; 2] {
    let raw = downmix_51_to_stereo_raw(frame);
    [raw[0] * NORMALISE_GAIN, raw[1] * NORMALISE_GAIN]
}

/// Downmix interleaved 5.1 audio to stereo
///
/// # Arguments
///
/// * `input` - Interleaved 6-channel frames
/// * `output` - Output vector to write stereo pairs to
///
/// # Returns
///
/// None, but writes to the output vector
pub fn downmix_51_interleaved(input: &[f32], output: &mut Vec<f32>) {
    output.clear();

    // Process complete frames only
    let frame_count = input.len() / 6;
    for i in 0..frame_count {
        let start = i * 6;
        let frame = [input[start], input[start + 1], input[start + 2], input[start + 3], input[start + 4], input[start + 5]];
        let stereo = downmix_51_to_stereo(&frame);
        output.push(stereo[0]);
        output.push(stereo[1]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper for approximate floating point comparisons
    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn raw_unit_vectors() {
        // [1,0,0,0,0,0] -> [1.0, 0.0]
        assert_eq!(downmix_51_to_stereo_raw(&[1.0, 0.0, 0.0, 0.0, 0.0, 0.0]), [1.0, 0.0]);

        // [0,1,0,0,0,0] -> [0.0, 1.0]
        assert_eq!(downmix_51_to_stereo_raw(&[0.0, 1.0, 0.0, 0.0, 0.0, 0.0]), [0.0, 1.0]);

        // [0,0,1,0,0,0] -> [MIX_GAIN, MIX_GAIN]
        assert!(approx_eq(downmix_51_to_stereo_raw(&[0.0, 0.0, 1.0, 0.0, 0.0, 0.0])[0], MIX_GAIN));
        assert!(approx_eq(downmix_51_to_stereo_raw(&[0.0, 0.0, 1.0, 0.0, 0.0, 0.0])[1], MIX_GAIN));

        // [0,0,0,1,0,0] -> [0.0, 0.0]
        assert_eq!(downmix_51_to_stereo_raw(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0]), [0.0, 0.0]);

        // [0,0,0,0,1,0] -> [MIX_GAIN, 0.0]
        assert!(approx_eq(downmix_51_to_stereo_raw(&[0.0, 0.0, 0.0, 0.0, 1.0, 0.0])[0], MIX_GAIN));
        assert_eq!(downmix_51_to_stereo_raw(&[0.0, 0.0, 0.0, 0.0, 1.0, 0.0])[1], 0.0);

        // [0,0,0,0,0,1] -> [0.0, MIX_GAIN]
        assert_eq!(downmix_51_to_stereo_raw(&[0.0, 0.0, 0.0, 0.0, 0.0, 1.0])[0], 0.0);
        assert!(approx_eq(downmix_51_to_stereo_raw(&[0.0, 0.0, 0.0, 0.0, 0.0, 1.0])[1], MIX_GAIN));

        // [1,1,1,1,1,1] -> [2.414_213_6, 2.414_213_6]
        let result = downmix_51_to_stereo_raw(&[1.0, 1.0, 1.0, 1.0, 1.0, 1.0]);
        assert!(approx_eq(result[0], 2.414_213_6));
        assert!(approx_eq(result[1], 2.414_213_6));
    }

    #[test]
    fn raw_all_ones() {
        let result = downmix_51_to_stereo_raw(&[1.0, 1.0, 1.0, 1.0, 1.0, 1.0]);
        assert!(approx_eq(result[0], 2.414_213_6));
        assert!(approx_eq(result[1], 2.414_213_6));
    }

    #[test]
    fn normalised_unit_vectors() {
        // [1,0,0,0,0,0] -> [0.414_213_57, 0.0]
        assert!(approx_eq(downmix_51_to_stereo(&[1.0, 0.0, 0.0, 0.0, 0.0, 0.0])[0], 0.414_213_57));
        assert_eq!(downmix_51_to_stereo(&[1.0, 0.0, 0.0, 0.0, 0.0, 0.0])[1], 0.0);

        // [0,1,0,0,0,0] -> [0.0, 0.414_213_57]
        assert_eq!(downmix_51_to_stereo(&[0.0, 1.0, 0.0, 0.0, 0.0, 0.0])[0], 0.0);
        assert!(approx_eq(downmix_51_to_stereo(&[0.0, 1.0, 0.0, 0.0, 0.0, 0.0])[1], 0.414_213_57));

        // [0,0,1,0,0,0] -> [0.292_893_2, 0.292_893_2]
        let result = downmix_51_to_stereo(&[0.0, 0.0, 1.0, 0.0, 0.0, 0.0]);
        assert!(approx_eq(result[0], 0.292_893_2));
        assert!(approx_eq(result[1], 0.292_893_2));

        // [0,0,0,1,0,0] -> [0.0, 0.0]
        assert_eq!(downmix_51_to_stereo(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0])[0], 0.0);
        assert_eq!(downmix_51_to_stereo(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0])[1], 0.0);

        // [0,0,0,0,1,0] -> [0.292_893_2, 0.0]
        assert!(approx_eq(downmix_51_to_stereo(&[0.0, 0.0, 0.0, 0.0, 1.0, 0.0])[0], 0.292_893_2));
        assert_eq!(downmix_51_to_stereo(&[0.0, 0.0, 0.0, 0.0, 1.0, 0.0])[1], 0.0);

        // [0,0,0,0,0,1] -> [0.0, 0.292_893_2]
        assert_eq!(downmix_51_to_stereo(&[0.0, 0.0, 0.0, 0.0, 0.0, 1.0])[0], 0.0);
        assert!(approx_eq(downmix_51_to_stereo(&[0.0, 0.0, 0.0, 0.0, 0.0, 1.0])[1], 0.292_893_2));

        // [1,1,1,1,1,1] -> [1.0, 1.0]
        let result = downmix_51_to_stereo(&[1.0, 1.0, 1.0, 1.0, 1.0, 1.0]);
        assert!(approx_eq(result[0], 1.0));
        assert!(approx_eq(result[1], 1.0));
    }

    #[test]
    fn normalised_all_ones() {
        let result = downmix_51_to_stereo(&[1.0, 1.0, 1.0, 1.0, 1.0, 1.0]);
        assert!(approx_eq(result[0], 1.0));
        assert!(approx_eq(result[1], 1.0));
    }

    #[test]
    fn normalised_silence_and_negative() {
        // [0;6] -> [0.0, 0.0]
        assert_eq!(downmix_51_to_stereo(&[0.0, 0.0, 0.0, 0.0, 0.0, 0.0]), [0.0, 0.0]);

        // [-1,-1,-1,-1,-1,-1] -> [-1.0, -1.0]
        let result = downmix_51_to_stereo(&[-1.0, -1.0, -1.0, -1.0, -1.0, -1.0]);
        assert!(approx_eq(result[0], -1.0));
        assert!(approx_eq(result[1], -1.0));
    }

    #[test]
    fn lfe_only_is_silent() {
        // [0,0,0,1,0,0] -> [0.0, 0.0]
        assert_eq!(downmix_51_to_stereo_raw(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0]), [0.0, 0.0]);
    }

    #[test]
    fn centre_only_is_minus_3db_to_both() {
        // [0,0,1,0,0,0] -> [MIX_GAIN, MIX_GAIN]
        let result = downmix_51_to_stereo_raw(&[0.0, 0.0, 1.0, 0.0, 0.0, 0.0]);
        assert!(approx_eq(result[0], MIX_GAIN));
        assert!(approx_eq(result[1], MIX_GAIN));
    }

    #[test]
    fn interleaved_two_frames() {
        // input [1,0,0,0,0,0, 1,1,1,1,1,1] -> output [0.414_213_57, 0.0, 1.0, 1.0]
        let mut output = Vec::new();
        downmix_51_interleaved(&[1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0], &mut output);
        assert_eq!(output.len(), 4);
        assert!(approx_eq(output[0], 0.414_213_57));
        assert_eq!(output[1], 0.0);
        assert!(approx_eq(output[2], 1.0));
        assert!(approx_eq(output[3], 1.0));
    }

    #[test]
    fn interleaved_ignores_partial_frame() {
        // Input of 13 samples = the 12 above plus 0.5 -> same 4 output values
        let mut output = Vec::new();
        downmix_51_interleaved(&[1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.5], &mut output);
        assert_eq!(output.len(), 4);
        assert!(approx_eq(output[0], 0.414_213_57));
        assert_eq!(output[1], 0.0);
        assert!(approx_eq(output[2], 1.0));
        assert!(approx_eq(output[3], 1.0));
    }

    #[test]
    fn interleaved_empty_and_clears_output() {
        // empty input -> empty output
        let mut output = Vec::new();
        downmix_51_interleaved(&[], &mut output);
        assert_eq!(output.len(), 0);

        // output passed in non-empty is cleared first
        output = vec![1.0, 2.0];
        downmix_51_interleaved(&[], &mut output);
        assert_eq!(output.len(), 0);
    }
}
