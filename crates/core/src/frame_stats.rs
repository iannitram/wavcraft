//! Sums up a list of frame times in milliseconds (roadmap B1.2, B1.3): count, median, p95, worst,
//! and how many frames are over the 16.7 ms budget. The app's FRAMETIMES verb calls it.

/// The frame budget in milliseconds: one frame at 60 Hz takes about 16.7 ms.
pub const FRAME_BUDGET_MS: f64 = 16.7;

/// The summary of a list of frame times. Every field is zero for an empty list.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FrameSummary {
    /// How many finite frame times were counted. Values that are not finite are not counted.
    pub count: usize,
    /// The median frame time in milliseconds: the middle value, or the mean of the two middle
    /// values when the count is even.
    pub median_ms: f64,
    /// The 95th percentile frame time in milliseconds by nearest rank: the value at rank
    /// `ceil(0.95 * count)` in the sorted list. Zero for an empty list.
    pub p95_ms: f64,
    /// The longest frame time in milliseconds.
    pub worst_ms: f64,
    /// How many frames took strictly longer than `FRAME_BUDGET_MS`.
    pub over_budget: usize,
}

/// Sums up `times_ms`, a list of frame times in milliseconds. Values that are not finite (NaN or
/// infinity) are skipped. The input does not need to be sorted. An empty list gives
/// `FrameSummary::default()`.
pub fn summarize_frames(times_ms: &[f64]) -> FrameSummary {
    let mut times: Vec<f64> = times_ms.iter().copied().filter(|t| t.is_finite()).collect();
    times.sort_by(|a, b| a.total_cmp(b));
    let count = times.len();
    if count == 0 {
        return FrameSummary::default();
    }
    let mid = count / 2;
    let median_ms = if count % 2 == 1 {
        times.get(mid).copied().unwrap_or(0.0)
    } else {
        let lower = times.get(mid.saturating_sub(1)).copied().unwrap_or(0.0);
        let upper = times.get(mid).copied().unwrap_or(0.0);
        (lower + upper) / 2.0
    };
    let rank = ((count as f64) * 0.95).ceil() as usize;
    let rank = rank.clamp(1, count);
    let p95_ms = times.get(rank - 1).copied().unwrap_or(0.0);
    let worst_ms = times.last().copied().unwrap_or(0.0);
    let over_budget = times.iter().filter(|&&t| t > FRAME_BUDGET_MS).count();
    FrameSummary { count, median_ms, p95_ms, worst_ms, over_budget }
}
