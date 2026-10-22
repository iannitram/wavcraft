//! Which blocks of a spectrogram `Overview` an edit makes stale (roadmap B147). Pure integer
//! arithmetic; it mirrors the grid in `spectrogram.rs` (hop = FFT length / 2, 16 FFTs a block).

use std::ops::Range;

/// Blocks that must be recomputed after an edit. The edit replaced `removed` frames at frame
/// `start` of the old audio with `inserted` frames; the new audio has `frames_after` frames; the
/// analysis uses `fft_len` frames per FFT. Returns the first dirty block up to one past the last,
/// or exactly `0..0` when no block is dirty.
pub fn dirty_blocks(fft_len: usize, start: usize, removed: usize, inserted: usize, frames_after: usize) -> Range<usize> {
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
}
