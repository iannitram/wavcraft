/// Counts RMS levels (in dB) into equal-width bins covering `min_db..max_db`.
///
/// Bin `i` covers `[min_db + i * bin_db, min_db + (i + 1) * bin_db)`, so a level
/// exactly on a bin edge belongs to the upper bin, and the bin count is rounded
/// up for uneven ranges. Non-finite levels, levels below `min_db` and levels at
/// or above `max_db` are dropped. Invalid parameters (non-finite values,
/// `bin_db <= 0.0`, or `max_db <= min_db`) return an empty vector.
pub fn rms_level_histogram(levels_db: &[f64], min_db: f64, max_db: f64, bin_db: f64) -> Vec<u64> {
    if !min_db.is_finite() || !max_db.is_finite() || !bin_db.is_finite() || bin_db <= 0.0 || max_db <= min_db {
        return Vec::new();
    }
    let n = ((max_db - min_db) / bin_db).ceil() as usize;
    let mut counts = vec![0u64; n];
    if n == 0 {
        return counts;
    }
    for &v in levels_db {
        if !v.is_finite() {
            continue;
        }
        if min_db <= v && v < max_db {
            let idx = (((v - min_db) / bin_db).floor() as usize).min(n - 1);
            counts[idx] += 1;
        }
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAN: f64 = f64::NAN;

    #[test]
    fn counts_levels_into_bins_from_min() {
        let levels = [-65.0, -60.0, -59.9, -50.0, -10.0, -0.1, 0.0, 3.0, NAN, f64::INFINITY, f64::NEG_INFINITY, -45.0, -45.0];
        assert_eq!(rms_level_histogram(&levels, -60.0, 0.0, 10.0), vec![2, 3, 0, 0, 0, 2]);
    }

    #[test]
    fn exact_bin_edge_belongs_to_the_upper_bin() {
        assert_eq!(rms_level_histogram(&[-50.0], -60.0, 0.0, 10.0), vec![0, 1, 0, 0, 0, 0]);
    }

    #[test]
    fn levels_below_min_and_at_or_above_max_are_dropped() {
        assert_eq!(rms_level_histogram(&[-61.0, 0.0, 5.0], -60.0, 0.0, 10.0), vec![0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn non_finite_levels_are_skipped() {
        assert_eq!(rms_level_histogram(&[NAN, f64::INFINITY, f64::NEG_INFINITY, -30.0], -60.0, 0.0, 10.0), vec![0, 0, 0, 1, 0, 0]);
    }

    #[test]
    fn uneven_range_rounds_bin_count_up() {
        assert_eq!(rms_level_histogram(&[-59.0, -57.0, -55.5, -55.0, -56.5], -60.0, -55.0, 2.0), vec![1, 2, 1]);
    }

    #[test]
    fn invalid_parameters_return_no_bins() {
        let levels = [-30.0];
        assert!(rms_level_histogram(&levels, -60.0, 0.0, 0.0).is_empty());
        assert!(rms_level_histogram(&levels, -60.0, 0.0, -1.0).is_empty());
        assert!(rms_level_histogram(&levels, -60.0, 0.0, NAN).is_empty());
        assert!(rms_level_histogram(&levels, -60.0, 0.0, f64::INFINITY).is_empty());
        assert!(rms_level_histogram(&levels, 0.0, -60.0, 10.0).is_empty());
        assert!(rms_level_histogram(&levels, NAN, 0.0, 10.0).is_empty());
    }

    #[test]
    fn empty_levels_give_zero_bins_of_the_right_length() {
        assert_eq!(rms_level_histogram(&[], -60.0, 0.0, 10.0), vec![0, 0, 0, 0, 0, 0]);
    }
}
