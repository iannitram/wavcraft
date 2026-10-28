//! Which blocks of a spectrogram `Overview` an edit makes stale (roadmap B147). Pure integer
//! arithmetic; it mirrors the grid in `spectrogram.rs` (hop = FFT length / 2, 16 FFTs a block).

use std::ops::Range;

/// Blocks that must be recomputed after an edit. The edit replaced `removed` frames at frame
/// `start` of the old audio with `inserted` frames; the new audio has `frames_after` frames; the
/// analysis uses `fft_len` frames per FFT. Returns the first dirty block up to one past the last,
/// or exactly `0..0` when no block is dirty.
pub fn dirty_blocks(fft_len: usize, start: usize, removed: usize, inserted: usize, frames_after: usize) -> Range<usize> {
    let fft_len = fft_len as i128;
    let hop = (fft_len / 2).max(1);
    let blocks = ((frames_after as i128 / hop + 2) / 16) as usize;
    if blocks == 0 || (removed == 0 && inserted == 0) {
        return 0..0;
    }
    // Block b reads [16*b*hop - hop/2, (16*b+15)*hop - hop/2 + fft_len).
    // Solve the two strict overlap inequalities, without walking every block.
    let first = ((start as i128 + hop / 2 - fft_len - 15 * hop).div_euclid(16 * hop) + 1).max(0) as usize;
    let last = if removed == inserted { ((start as i128 + inserted as i128 + hop / 2 + 16 * hop - 1) / (16 * hop)).max(0) as usize } else { blocks };
    let first = first.min(blocks);
    let last = last.min(blocks);
    if first < last { first..last } else { 0..0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_length_edit_dirties_few_blocks() {
        assert_eq!(dirty_blocks(2048, 100000, 500, 500, 200000), 6..7);
    }

    #[test]
    fn length_change_dirties_everything_after() {
        assert_eq!(dirty_blocks(2048, 50000, 0, 1000, 200000), 3..12);
    }

    #[test]
    fn empty_edit_is_clean() {
        assert_eq!(dirty_blocks(2048, 70000, 0, 0, 200000), 0..0);
    }

    #[test]
    fn edit_at_start_dirties_first_block_only() {
        assert_eq!(dirty_blocks(2048, 0, 10, 10, 200000), 0..1);
    }

    #[test]
    fn small_fft_has_finer_blocks() {
        assert_eq!(dirty_blocks(64, 1000, 40, 40, 5000), 1..3);
    }

    #[test]
    fn empty_audio_has_no_blocks() {
        assert_eq!(dirty_blocks(2048, 100, 10, 10, 0), 0..0);
        assert_eq!(dirty_blocks(2048, 10, 10, 10, 100), 0..0);
    }

    #[test]
    fn tail_deletion_dirties_blocks_reaching_the_cut() {
        assert_eq!(dirty_blocks(2048, 190000, 1000, 0, 196000), 11..12);
    }

    #[test]
    fn tail_deletion_past_last_whole_block_is_clean() {
        assert_eq!(dirty_blocks(2048, 199000, 1000, 0, 199000), 0..0);
    }

    /// The previous build of `dirty_blocks`, copied verbatim from `origin/main`: it walks every
    /// block. The closed form must agree with it everywhere.
    fn brute_force(fft_len: usize, start: usize, removed: usize, inserted: usize, frames_after: usize) -> Range<usize> {
        // Step 1: Calculate hop and blocks
        let fft_len = i64::try_from(fft_len).unwrap_or(i64::MAX);
        let start = i64::try_from(start).unwrap_or(i64::MAX);
        let removed = i64::try_from(removed).unwrap_or(i64::MAX);
        let inserted = i64::try_from(inserted).unwrap_or(i64::MAX);
        let frames_after = i64::try_from(frames_after).unwrap_or(i64::MAX);

        let hop = (fft_len / 2).max(1);
        let blocks = (frames_after / hop + 2) / 16;

        // If no blocks, return empty range
        if blocks == 0 {
            return 0..0;
        }

        // If no change, return empty range
        if removed == 0 && inserted == 0 {
            return 0..0;
        }

        // Determine the affected span
        let end = if removed == inserted { start + inserted } else { i64::MAX };

        // Find dirty blocks
        let mut lowest = blocks;
        let mut highest = -1;

        for b in 0..blocks {
            let first = 16 * b * hop - hop / 2;
            let last = (16 * b + 15) * hop - hop / 2 + fft_len;

            // A block is dirty if it overlaps with the edit
            if first < end && last > start {
                lowest = lowest.min(b);
                highest = highest.max(b);
            }
        }

        // Return the range of dirty blocks
        if highest >= 0 { (lowest as usize)..(highest as usize + 1) } else { 0..0 }
    }

    /// Seed for the random comparison. Fixed, so a failing case reproduces.
    const SEED: u64 = 0x1036_D1A7;
    /// Number of random cases generated from `SEED` in the random comparison.
    const RANDOM_CASES: usize = 20_000;
    const FFT_LENS: [usize; 12] = [1, 2, 3, 16, 255, 256, 512, 1024, 2048, 4096, 8192, 32768];

    /// Small deterministic generator (64-bit LCG with Knuth's MMIX constants). No dependencies.
    struct Lcg(u64);

    impl Lcg {
        fn draw(&mut self) -> u64 {
            self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 32
        }

        /// A value in `0..n`, for `1 <= n < 2^32`.
        fn below(&mut self, n: u64) -> u64 {
            self.draw() % n
        }

        /// An edit length: 1..=200 or 1..=200,000, half the time each.
        fn edit_len(&mut self) -> usize {
            if self.below(2) == 0 { 1 + self.below(200) as usize } else { 1 + self.below(200_000) as usize }
        }
    }

    /// Compares the closed form with the brute force, and names the inputs if they disagree.
    fn check(fft_len: usize, start: usize, removed: usize, inserted: usize, frames_after: usize) {
        let closed = dirty_blocks(fft_len, start, removed, inserted, frames_after);
        let brute = brute_force(fft_len, start, removed, inserted, frames_after);
        assert_eq!(
            closed, brute,
            "disagreement: fft_len={fft_len} start={start} removed={removed} inserted={inserted} frames_after={frames_after} (seed {SEED:#x})"
        );
    }

    /// Runs `RANDOM_CASES` (20,000) cases drawn from the fixed seed `SEED`: `fft_len` from a
    /// list of 12 sizes, `frames_after` up to about 3,000,000, `start` up to past the end, and
    /// edits of 0 or 1..=200,000 frames. At least a quarter are same-length edits, and some
    /// have no edit at all.
    #[test]
    fn closed_form_matches_the_brute_force_on_random_cases() {
        let mut rng = Lcg(SEED);
        let (mut same_length, mut no_edit) = (0usize, 0usize);
        for _ in 0..RANDOM_CASES {
            let fft_len = FFT_LENS[rng.below(FFT_LENS.len() as u64) as usize];
            let frames_after = rng.below(3_000_001) as usize;
            let start = rng.below((frames_after + 10_001) as u64) as usize;
            let (removed, inserted) = match rng.below(8) {
                0 => (0, 0),
                1..=3 => {
                    let n = rng.edit_len();
                    (n, n)
                }
                4 => (0, rng.edit_len()),
                5 => (rng.edit_len(), 0),
                _ => (rng.edit_len(), rng.edit_len()),
            };
            same_length += usize::from(removed == inserted);
            no_edit += usize::from(removed == 0 && inserted == 0);
            check(fft_len, start, removed, inserted, frames_after);
        }
        assert!(same_length >= RANDOM_CASES / 4, "only {same_length} same-length cases");
        assert!(no_edit > 0, "no case without an edit");
    }

    /// Hand-picked edges, each compared with the brute force: `fft_len` 1 and 2 (hop 1), empty
    /// and one-block documents, `frames_after` just below, at and above block boundaries,
    /// `start` at 0, at a block's first and last frame, and past the end.
    #[test]
    fn closed_form_matches_the_brute_force_on_edge_cases() {
        for fft_len in [1, 2, 3, 16, 255, 256, 2048, 32768] {
            let hop = (fft_len / 2).max(1);
            let block = 16 * hop;

            let mut frames = vec![0, 1, 2, hop * 14, hop * 29, hop * 30];
            for k in [1, 2, 3, 7] {
                for d in [-2i64, -1, 0, 1] {
                    frames.push((block as i64 * k + d).max(0) as usize);
                }
            }

            let mut edits = vec![(0, 0), (0, 1), (1, 0), (1, 1), (5, 5), (5, 7), (7, 5), (1, block), (block, block)];
            edits.push((block, 0));

            for frames_after in frames {
                let mut starts = vec![0, 1, frames_after, frames_after + 1, frames_after + 10_000];
                for b in 0..=8 {
                    let base = block * b;
                    starts.extend([base.saturating_sub(hop / 2), base.saturating_sub(1), base, base + block - 1]);
                }
                for &start in &starts {
                    for &(removed, inserted) in &edits {
                        check(fft_len, start, removed, inserted, frames_after);
                    }
                }
            }
        }
    }

    /// Maximum length: 3 hours at 192 kHz = 2,073,600,000 frames. The brute force loops over
    /// every block, so this runs only a handful of calls.
    #[test]
    fn closed_form_matches_the_brute_force_at_three_hours_of_192khz() {
        const THREE_HOURS_AT_192K: usize = 3 * 60 * 60 * 192_000;
        check(1, 0, 1000, 1000, THREE_HOURS_AT_192K);
        check(1, 1_000_000_000, 0, 5, THREE_HOURS_AT_192K);
        check(1, THREE_HOURS_AT_192K + 10, 3, 0, THREE_HOURS_AT_192K);
        check(32768, 0, 0, 1, THREE_HOURS_AT_192K);
        check(32768, 1_000_000_000, 4096, 4096, THREE_HOURS_AT_192K);
    }
}
