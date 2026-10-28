//! wav-c-116-c (#116, v1 item J4): locked tests for `take_readout::take_readout`. Do not edit.

use wavcraft_core::take_readout::{TakeReadout, TakeStats, take_readout};

/// 48 kHz 24-bit mono, nothing recorded, free space unknown.
fn mono() -> TakeStats {
    TakeStats { frames: 0, rate: 48_000, channels: 1, bits: 24, file_bytes: 44, free_bytes: None, dropped_frames: 0 }
}

#[test]
fn j4_readout_elapsed_is_whole_seconds_as_a_clock() {
    let cases: [(u64, &str); 6] =
        [(0, "0:00"), (47_999, "0:00"), (3_120_000, "1:05"), (172_799_999, "59:59"), (172_800_000, "1:00:00"), (1_733_904_000, "10:02:03")];
    for (frames, want) in cases {
        assert_eq!(take_readout(&TakeStats { frames, ..mono() }).elapsed, want, "frames {frames}");
    }
}

#[test]
fn j4_readout_size_uses_the_app_byte_format() {
    let cases: [(u64, &str); 4] = [(44, "44 bytes"), (144_044, "144 KB"), (518_400_044, "518 MB"), (1_036_800_044, "1.0 GB")];
    for (file_bytes, want) in cases {
        assert_eq!(take_readout(&TakeStats { file_bytes, ..mono() }).size, want, "{file_bytes} bytes");
    }
}

#[test]
fn j4_readout_time_left_from_free_space() {
    // 48 kHz 24-bit mono is 144_000 bytes a second; stereo twice that.
    assert_eq!(take_readout(&TakeStats { free_bytes: Some(100_000_000_000), ..mono() }).left, "192:54:04");
    assert_eq!(take_readout(&TakeStats { free_bytes: Some(1_000_000_000), channels: 2, ..mono() }).left, "57:52");
    assert_eq!(take_readout(&TakeStats { free_bytes: Some(143_999), ..mono() }).left, "0:00");
    assert_eq!(take_readout(&TakeStats { free_bytes: Some(0), ..mono() }).left, "0:00");
    // 44.1 kHz 16-bit stereo is 176_400 bytes a second: 635_040_000 bytes is exactly one hour.
    let cd = TakeStats { rate: 44_100, channels: 2, bits: 16, free_bytes: Some(635_040_000), ..mono() };
    assert_eq!(take_readout(&cd).left, "1:00:00");
}

#[test]
fn j4_readout_unknown_free_space_shows_a_question_mark() {
    assert_eq!(take_readout(&mono()).left, "?");
}

#[test]
fn j4_readout_dropped_count() {
    assert_eq!(take_readout(&mono()).dropped, "0");
    assert_eq!(take_readout(&TakeStats { dropped_frames: 1_234, ..mono() }).dropped, "1234");
}

#[test]
fn j4_readout_whole_value() {
    let stats = TakeStats { frames: 3_120_000, file_bytes: 9_360_044, free_bytes: Some(1_000_000_000), dropped_frames: 7, ..mono() };
    let want = TakeReadout { elapsed: "1:05".to_string(), size: "9.4 MB".to_string(), left: "1:55:44".to_string(), dropped: "7".to_string() };
    assert_eq!(take_readout(&stats), want);
}

#[test]
fn j4_readout_zero_format_does_not_panic() {
    // A rate, channel count or bit depth of 0 counts as 1 so nothing divides by zero.
    let zero = TakeStats { frames: 100, rate: 0, channels: 0, bits: 0, file_bytes: 0, free_bytes: Some(500), dropped_frames: 0 };
    let r = take_readout(&zero);
    assert_eq!(r.elapsed, "1:40");
    assert_eq!(r.left, "8:20");
    assert_eq!(r.size, "0 bytes");
}
