//! Locked tests for B2.2 (v1 item B2): one step along the frequency scale, stopping at each end.

use wavcraft_core::freq_axis::Scale;

#[test]
fn the_scale_steps_and_stops() {
    assert_eq!(Scale::Linear.more_log(), Scale::LogPartial);
    assert_eq!(Scale::LogPartial.more_log(), Scale::LogFull);
    assert_eq!(Scale::LogFull.more_log(), Scale::LogFull);
    assert_eq!(Scale::LogFull.more_linear(), Scale::LogPartial);
    assert_eq!(Scale::LogPartial.more_linear(), Scale::Linear);
    assert_eq!(Scale::Linear.more_linear(), Scale::Linear);
}
