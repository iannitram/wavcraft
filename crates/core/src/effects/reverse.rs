//! Reverse (E4, #30): play a range backwards, bit for bit.
//!
//! [`mirror_range`] and [`reverse_frames`] are the pure arithmetic and slice reversal;
//! [`Reverse`] reads each output chunk's mirror range from the document in [`Effect::begin`]
//! and reverses its samples chunk by chunk.

use std::io;
use std::ops::Range;

use super::{Effect, ParamDesc, Run, RunContext};

/// The document frame range that supplies the output chunk `[chunk_first_frame, chunk_first_frame + chunk_len)`
/// when `range` (`start..end`) is reversed.
///
/// An output chunk at offset `chunk_first_frame - range.start` from the start of `range` comes
/// from the mirror slice of length `chunk_len` ending `chunk_first_frame - range.start` frames
/// before `range.end`: `(range.end - (chunk_first_frame - range.start) - chunk_len)..(range.end - (chunk_first_frame - range.start))`
/// (with saturating subtraction).
pub fn mirror_range(range: Range<u64>, chunk_first_frame: u64, chunk_len: usize) -> Range<u64> {
    let _ = (range, chunk_first_frame, chunk_len);
    0..0
}

/// Copy `input` into `output` in reverse frame order (`output[k] = input[n - 1 - k]` for
/// `n = input.len().min(output.len())`), bit for bit without changing sign or rounding.
pub fn reverse_frames(input: &[f32], output: &mut [f32]) {
    let _ = (input, output);
}

/// Time reversal: every active channel's samples in the range in reverse order, bit for bit.
pub struct Reverse;

impl Effect for Reverse {
    fn id(&self) -> &'static str {
        "reverse"
    }
    fn name(&self) -> &'static str {
        "Reverse"
    }
    fn parameters(&self) -> &'static [ParamDesc] {
        &[]
    }
    fn begin(&self, _params: &[f64], ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        let _ = ctx;
        Err(io::Error::new(io::ErrorKind::Unsupported, "Reverse is not built yet"))
    }
    fn process(&self, _params: &[f64], _first_frame: u64, _input: &[&[f32]], _output: &mut [&mut [f32]]) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "Reverse reads mirror chunks from the document: run it through effects::apply"))
    }
}

#[cfg(test)]
mod tests {
    use super::{mirror_range, reverse_frames};

    #[test]
    fn mirror_single_chunk_and_subrange() {
        assert_eq!(mirror_range(0..100, 0, 100), 0..100);
        assert_eq!(mirror_range(100..250, 100, 150), 100..250);
        assert_eq!(mirror_range(10..50, 10, 15), 35..50);
        assert_eq!(mirror_range(10..50, 25, 25), 10..35);
    }

    #[test]
    fn mirror_three_chunks_partition_range_back_to_front() {
        let range = 1_000..140_000;
        assert_eq!(mirror_range(range.clone(), 1_000, 65_536), 74_464..140_000);
        assert_eq!(mirror_range(range.clone(), 66_536, 65_536), 8_928..74_464);
        assert_eq!(mirror_range(range, 132_072, 7_928), 1_000..8_928);
    }

    #[test]
    fn mirror_empty_chunk_at_range_edges() {
        assert_eq!(mirror_range(50..100, 50, 0), 100..100);
        assert_eq!(mirror_range(50..100, 100, 0), 50..50);
        assert_eq!(mirror_range(0..0, 0, 0), 0..0);
    }

    #[test]
    fn frames_reversed_bit_for_bit_including_signed_zero() {
        let input = [0.0_f32, -0.0_f32, 0.125_f32, -0.875_f32];
        let mut out = [1.0_f32; 4];
        reverse_frames(&input, &mut out);
        let got: Vec<u32> = out.iter().map(|x| x.to_bits()).collect();
        let want = [(-0.875_f32).to_bits(), 0.125_f32.to_bits(), (-0.0_f32).to_bits(), 0.0_f32.to_bits()];
        assert_eq!(got, want);
    }

    #[test]
    fn frames_reversed_odd_length_single_and_round_trip() {
        let input = [-1.0_f32, -0.5_f32, 0.25_f32, 0.5_f32, 1.0_f32];
        let mut out = [0.0_f32; 5];
        reverse_frames(&input, &mut out);
        assert_eq!(out, [1.0_f32, 0.5_f32, 0.25_f32, -0.5_f32, -1.0_f32]);

        let mut back = [0.0_f32; 5];
        reverse_frames(&out, &mut back);
        assert_eq!(back, input);

        let mut one = [0.0_f32; 1];
        reverse_frames(&[0.75_f32], &mut one);
        assert_eq!(one, [0.75_f32]);

        let mut empty: [f32; 0] = [];
        reverse_frames(&[], &mut empty);
        assert_eq!(empty.len(), 0);
    }
}
