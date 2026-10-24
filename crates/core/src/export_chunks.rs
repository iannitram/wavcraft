//! Streamed export for large files (roadmap C277). Today an export holds the whole mixed file in
//! memory. A streamed export mixes and writes it in chunks instead. This file is the arithmetic
//! of that: how many frames fit a memory budget, and the list of `(start, end)` frame ranges to
//! mix one after another. Mixing, encoding and writing are not here.
//!
//! A frame is one sample of every channel. Ranges are half-open: `end` is not included.

/// Chunk sizes are rounded down to a multiple of this many frames (and never go below it).
pub const ALIGN_FRAMES: u64 = 4096;

/// How many frames of `channels` channels at `bytes_per_sample` bytes fit in `budget_bytes`,
/// rounded down to a multiple of `ALIGN_FRAMES` and never less than `ALIGN_FRAMES`. Returns
/// `ALIGN_FRAMES` when `channels` or `bytes_per_sample` is 0.
pub fn chunk_frames_for_budget(budget_bytes: u64, channels: u32, bytes_per_sample: u32) -> u64 {
    let per_frame = u64::from(channels) * u64::from(bytes_per_sample);
    if per_frame == 0 {
        return ALIGN_FRAMES;
    }
    let frames = budget_bytes / per_frame;
    (frames / ALIGN_FRAMES * ALIGN_FRAMES).max(ALIGN_FRAMES)
}

/// The memory one chunk of `chunk_frames` frames takes, in bytes (saturating at `u64::MAX`).
pub fn chunk_bytes(chunk_frames: u64, channels: u32, bytes_per_sample: u32) -> u64 {
    chunk_frames.saturating_mul(u64::from(channels) * u64::from(bytes_per_sample))
}

/// The `(start, end)` ranges that cover `0..total_frames` in order with no gap and no overlap.
/// Every range is `chunk_frames` long except the last, which may be shorter. A `total_frames` of 0
/// gives no ranges. A `chunk_frames` of 0 gives one range, the whole file.
pub fn chunk_ranges(total_frames: u64, chunk_frames: u64) -> Vec<(u64, u64)> {
    if total_frames == 0 {
        return Vec::new();
    }
    if chunk_frames == 0 {
        return vec![(0, total_frames)];
    }
    (0..chunk_count(total_frames, chunk_frames))
        .map(|index| {
            let start = index * chunk_frames;
            (start, start.saturating_add(chunk_frames).min(total_frames))
        })
        .collect()
}

/// How many ranges `chunk_ranges` gives, without building them.
pub fn chunk_count(total_frames: u64, chunk_frames: u64) -> u64 {
    if total_frames == 0 {
        0
    } else if chunk_frames == 0 {
        1
    } else {
        total_frames.div_ceil(chunk_frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_gives_whole_aligned_frames() {
        assert_eq!(chunk_frames_for_budget(67_108_864, 2, 4), 8_388_608);
        assert_eq!(chunk_frames_for_budget(10_000_000, 6, 3), 552_960);
        assert_eq!(chunk_frames_for_budget(5_368_709_120, 1, 2), 2_684_354_560);
    }

    #[test]
    fn budget_never_goes_below_one_aligned_block() {
        assert_eq!(chunk_frames_for_budget(1000, 2, 4), 4096);
        assert_eq!(chunk_frames_for_budget(0, 2, 4), 4096);
        assert_eq!(chunk_frames_for_budget(1_000_000, 0, 4), 4096);
        assert_eq!(chunk_frames_for_budget(1_000_000, 2, 0), 4096);
    }

    #[test]
    fn chunk_bytes_multiplies_and_saturates() {
        assert_eq!(chunk_bytes(4096, 2, 4), 32_768);
        assert_eq!(chunk_bytes(0, 2, 4), 0);
        assert_eq!(chunk_bytes(u64::MAX, 2, 1), u64::MAX);
    }

    #[test]
    fn ranges_cover_the_file_and_the_last_one_is_shorter() {
        assert_eq!(chunk_ranges(10, 4), vec![(0, 4), (4, 8), (8, 10)]);
        assert_eq!(chunk_ranges(3, 4), vec![(0, 3)]);
    }

    #[test]
    fn ranges_of_an_exact_multiple_have_no_empty_tail() {
        assert_eq!(chunk_ranges(8, 4), vec![(0, 4), (4, 8)]);
    }

    #[test]
    fn ranges_handle_empty_input_and_a_zero_chunk() {
        assert_eq!(chunk_ranges(0, 4), Vec::<(u64, u64)>::new());
        assert_eq!(chunk_ranges(0, 0), Vec::<(u64, u64)>::new());
        assert_eq!(chunk_ranges(5, 0), vec![(0, 5)]);
    }

    #[test]
    fn ranges_do_not_overflow_near_the_top() {
        assert_eq!(chunk_ranges(u64::MAX, u64::MAX - 1), vec![(0, u64::MAX - 1), (u64::MAX - 1, u64::MAX)]);
    }

    #[test]
    fn count_matches_the_number_of_ranges() {
        assert_eq!(chunk_count(10_000_000, 4096), 2442);
        assert_eq!(chunk_ranges(10_000_000, 4096).len(), 2442);
        assert_eq!(chunk_count(8, 4), 2);
        assert_eq!(chunk_count(9, 4), 3);
        assert_eq!(chunk_count(0, 4), 0);
        assert_eq!(chunk_count(5, 0), 1);
        assert_eq!(chunk_count(0, 0), 0);
    }
}
