//! Edit by transcript (roadmap C251): from word timings and the words the user deleted to the
//! frame ranges to remove. Pure arithmetic, no I/O.

/// `words[i]` is `(start_s, end_s)` of word `i` in seconds, in transcript order. `deleted` holds
/// word indices (any order, duplicates allowed). Returns sorted, non-overlapping, non-touching
/// `(start_frame, end_frame)` ranges, end exclusive, none past `frames`.
pub fn cut_ranges(words: &[(f64, f64)], deleted: &[usize], sample_rate: u32, frames: u64) -> Vec<(u64, u64)> {
    if sample_rate == 0 {
        return vec![];
    }

    // Step 2: Filter and deduplicate valid indices
    let mut valid_indices: Vec<usize> = deleted
        .iter()
        .filter(|&&i| i < words.len())
        .filter(|&&i| {
            let (start, end) = words[i];
            start.is_finite() && end.is_finite() && start >= 0.0 && end > start
        })
        .copied()
        .collect();

    valid_indices.sort_unstable();
    valid_indices.dedup();

    // Step 3: Group into runs
    if valid_indices.is_empty() {
        return vec![];
    }

    let mut runs: Vec<(f64, f64)> = Vec::new();
    let mut current_start = words[valid_indices[0]].0;
    let mut current_end = words[valid_indices[0]].1;
    let mut prev_idx = valid_indices[0];

    for &idx in valid_indices.iter().skip(1) {
        if idx == prev_idx + 1 {
            // Adjacent indices
            current_end = current_end.max(words[idx].1);
        } else {
            runs.push((current_start, current_end));
            current_start = words[idx].0;
            current_end = words[idx].1;
        }
        prev_idx = idx;
    }
    runs.push((current_start, current_end));

    // Step 4: Convert to frames
    let mut ranges: Vec<(u64, u64)> = runs
        .into_iter()
        .map(|(start_s, end_s)| {
            let start_f = (start_s * sample_rate as f64).round();
            let end_f = (end_s * sample_rate as f64).round();

            // Clamp to valid range
            let start_clamped = start_f.clamp(0.0, frames as f64) as u64;
            let end_clamped = end_f.clamp(0.0, frames as f64) as u64;

            (start_clamped, end_clamped)
        })
        .filter(|&(start, end)| start < end)
        .collect();

    // Step 5: Merge overlapping ranges
    if ranges.is_empty() {
        return vec![];
    }

    ranges.sort_unstable_by_key(|&x| x.0);

    let mut merged = Vec::new();
    let mut current_start = ranges[0].0;
    let mut current_end = ranges[0].1;

    for (start, end) in ranges.iter().skip(1) {
        if start <= &current_end {
            // Overlapping or touching
            current_end = current_end.max(*end);
        } else {
            // Non-overlapping - add previous range and start new one
            merged.push((current_start, current_end));
            current_start = *start;
            current_end = *end;
        }
    }

    merged.push((current_start, current_end));
    merged
}

#[cfg(test)]
mod tests {
    use super::cut_ranges;

    const W: [(f64, f64); 4] = [(0.0, 0.5), (0.5, 1.0), (1.2, 1.6), (2.0, 2.4)];

    #[test]
    fn nothing_deleted_cuts_nothing() {
        assert_eq!(cut_ranges(&W, &[], 1000, 3000), vec![]);
    }

    #[test]
    fn single_word() {
        assert_eq!(cut_ranges(&W, &[2], 1000, 3000), vec![(1200, 1600)]);
    }

    #[test]
    fn adjacent_words_merge_into_one_range() {
        assert_eq!(cut_ranges(&W, &[0, 1], 1000, 3000), vec![(0, 1000)]);
    }

    #[test]
    fn duplicates_and_out_of_range_ignored() {
        assert_eq!(cut_ranges(&W, &[1, 3, 3, 9], 1000, 3000), vec![(500, 1000), (2000, 2400)]);
    }

    #[test]
    fn adjacent_run_cuts_the_gap_between_words() {
        assert_eq!(cut_ranges(&W, &[1, 2], 1000, 3000), vec![(500, 1600)]);
    }

    #[test]
    fn ranges_are_clamped_to_frames() {
        assert_eq!(cut_ranges(&W, &[3], 1000, 2200), vec![(2000, 2200)]);
        assert_eq!(cut_ranges(&W, &[3], 1000, 2000), vec![]);
    }

    #[test]
    fn overlapping_ranges_merge() {
        let words = [(0.0, 1.0), (0.2, 0.4), (0.8, 1.5)];
        assert_eq!(cut_ranges(&words, &[0, 2], 1000, 3000), vec![(0, 1500)]);
    }

    #[test]
    fn invalid_word_is_skipped() {
        let words = [(f64::NAN, 1.0), (1.0, 2.0)];
        assert_eq!(cut_ranges(&words, &[0, 1], 1000, 3000), vec![(1000, 2000)]);
        assert_eq!(cut_ranges(&W, &[1], 0, 3000), vec![]);
    }

    #[test]
    fn rate_scales_frames() {
        assert_eq!(cut_ranges(&[(0.01, 0.02)], &[0], 48000, 100000), vec![(480, 960)]);
    }

    #[test]
    fn index_equal_to_len_is_ignored() {
        let words = [(0.0, 0.5), (0.5, 1.0), (1.2, 1.6), (2.0, 2.4)];
        assert_eq!(cut_ranges(&words, &[1, 4], 1000, 3000), vec![(500, 1000)]);
    }

    #[test]
    fn run_of_three_cuts_every_gap() {
        let words = [(0.0, 0.5), (0.5, 1.0), (1.2, 1.6), (2.0, 2.4)];
        assert_eq!(cut_ranges(&words, &[0, 1, 2], 1000, 3000), vec![(0, 1600)]);
    }

    #[test]
    fn duplicates_do_not_split_or_drop_runs() {
        let words = [(0.0, 0.5), (0.5, 1.0), (1.2, 1.6), (2.0, 2.4)];
        assert_eq!(cut_ranges(&words, &[2, 0, 2], 1000, 3000), vec![(0, 500), (1200, 1600)]);
    }
}
