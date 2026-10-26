//! wav-c-116-a (#116, v1 item J3): locked tests for `take_repair::repair_header`. Do not edit.

use wavcraft_core::take_repair::{HEADER_BYTES, HeaderRepair, repair_header};

/// A take's 44-byte header, written out field by field (format 1 = PCM, 3 = float).
fn header(format: u16, channels: u16, rate: u32, bits: u16, riff: u32, data: u32) -> [u8; 44] {
    let block = channels * (bits / 8);
    let mut h = [0u8; 44];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&riff.to_le_bytes());
    h[8..16].copy_from_slice(b"WAVEfmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&format.to_le_bytes());
    h[22..24].copy_from_slice(&channels.to_le_bytes());
    h[24..28].copy_from_slice(&rate.to_le_bytes());
    h[28..32].copy_from_slice(&(rate * u32::from(block)).to_le_bytes());
    h[32..34].copy_from_slice(&block.to_le_bytes());
    h[34..36].copy_from_slice(&bits.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data.to_le_bytes());
    h
}

#[test]
fn j3_repair_uses_only_the_first_44_header_bytes() {
    // 44.1 kHz 16-bit stereo (4 bytes a frame), one second of samples; sample bytes after HEADER_BYTES in the slice are ignored.
    let mut with_samples = header(1, 2, 44_100, 16, 36, 0).to_vec();
    with_samples.extend_from_slice(&[9u8; 20]);
    let want = HeaderRepair::Fixed(header(1, 2, 44_100, 16, 176_436, 176_400));
    assert_eq!(repair_header(&with_samples, HEADER_BYTES as u64 + 176_400), want);
}

#[test]
fn j3_repair_fixes_a_take_killed_before_its_first_update() {
    // 48 kHz 24-bit mono, one second of samples after a header still saying "no data".
    let stale = header(1, 1, 48_000, 24, 36, 0);
    assert_eq!(repair_header(&stale, 144_044), HeaderRepair::Fixed(header(1, 1, 48_000, 24, 144_036, 144_000)));
}

#[test]
fn j3_repair_drops_a_partial_last_frame() {
    // 24-bit stereo: 6 bytes a frame; 1000 whole frames and 4 bytes of a frame cut short.
    let stale = header(1, 2, 48_000, 24, 36, 0);
    assert_eq!(repair_header(&stale, 6_048), HeaderRepair::Fixed(header(1, 2, 48_000, 24, 6_036, 6_000)));
}

#[test]
fn j3_repair_shrinks_sizes_that_claim_more_than_the_file_holds() {
    let long = header(1, 2, 48_000, 24, 12_036, 12_000);
    assert_eq!(repair_header(&long, 6_044), HeaderRepair::Fixed(header(1, 2, 48_000, 24, 6_036, 6_000)));
    // Only one of the two sizes is wrong: still fixed.
    let riff_only = header(1, 2, 48_000, 24, 36, 6_000);
    assert_eq!(repair_header(&riff_only, 6_044), HeaderRepair::Fixed(header(1, 2, 48_000, 24, 6_036, 6_000)));
}

#[test]
fn j3_repair_reports_an_intact_header() {
    let good = header(1, 2, 48_000, 24, 6_036, 6_000);
    assert_eq!(repair_header(&good, 6_044), HeaderRepair::Intact);
    // 5 trailing bytes are less than a frame: nothing to add.
    assert_eq!(repair_header(&good, 6_049), HeaderRepair::Intact);
    // A slice longer than 44 bytes (the start of the samples follows the header): only the first 44 count.
    let mut with_samples = good.to_vec();
    with_samples.extend_from_slice(&[7u8; 20]);
    assert_eq!(repair_header(&with_samples, 6_044), HeaderRepair::Intact);
    // An empty take that was stopped cleanly.
    assert_eq!(repair_header(&header(1, 1, 48_000, 24, 36, 0), 44), HeaderRepair::Intact);
}

#[test]
fn j3_repair_handles_float_takes() {
    // 32-bit float stereo: 8 bytes a frame, 10 frames.
    let stale = header(3, 2, 48_000, 32, 36, 0);
    assert_eq!(repair_header(&stale, 124), HeaderRepair::Fixed(header(3, 2, 48_000, 32, 116, 80)));
}

#[test]
fn j3_repair_caps_sizes_at_the_32_bit_limit() {
    // A 5 GB file: the data size stops at the largest whole number of frames
    // with RIFF size = data + 36 <= 4_294_967_295.
    let mono16 = header(1, 1, 48_000, 16, 36, 0);
    assert_eq!(repair_header(&mono16, 5_000_000_044), HeaderRepair::Fixed(header(1, 1, 48_000, 16, 4_294_967_294, 4_294_967_258)));
    let stereo24 = header(1, 2, 48_000, 24, 36, 0);
    assert_eq!(repair_header(&stereo24, 5_000_000_044), HeaderRepair::Fixed(header(1, 2, 48_000, 24, 4_294_967_292, 4_294_967_256)));
}

#[test]
fn j3_repair_refuses_what_is_not_a_take() {
    let good = header(1, 2, 48_000, 24, 36, 0);
    // Too short a header slice, and a file shorter than a header.
    assert_eq!(repair_header(&good[..43], 6_044), HeaderRepair::NotATake);
    assert_eq!(repair_header(&good, 43), HeaderRepair::NotATake);
    // One wrong field at a time: (byte offset, bytes written there).
    let cases: [(usize, &[u8]); 9] = [
        (0, b"RIFX"),
        (8, b"AVI "),
        (12, b"LIST"),
        (16, &[18, 0, 0, 0]),
        (20, &[2, 0]),
        (20, &[0xFE, 0xFF]),
        (22, &[0, 0]),
        (32, &[0, 0]),
        (36, b"fact"),
    ];
    for (at, bytes) in cases {
        let mut bad = good;
        bad[at..at + bytes.len()].copy_from_slice(bytes);
        assert_eq!(repair_header(&bad, 6_044), HeaderRepair::NotATake, "byte {at} set to {bytes:?}");
    }
}
