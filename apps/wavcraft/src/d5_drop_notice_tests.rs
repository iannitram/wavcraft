//! Tests for D5.2: the History panel's drop notice speaks only when the dropped count has risen.
use super::*;

#[test]
fn drop_notice_speaks_only_when_the_count_rises() {
    assert_eq!(drop_notice(0, 3, 4_000_000_000).as_deref(), Some("Oldest 3 steps removed to stay under 4.0 GB"));
    assert_eq!(drop_notice(3, 3, 4_000_000_000), None);
    assert_eq!(drop_notice(0, 0, 4_000_000_000), None);
}
