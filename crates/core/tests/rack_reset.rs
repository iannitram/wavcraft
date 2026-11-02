//! The Effects Rack reset on seek and restart (#931): a new play starts from clean effect state.

use std::f64::consts::TAU;

use wavcraft_core::effects;
use wavcraft_core::rack::{RackConfig, RackProcessor};

const RATE: u32 = 48_000;

fn deesser_rack(args: &str) -> RackConfig {
    let mut c = RackConfig::new();
    let e = effects::find("deesser").unwrap();
    c.set_slot(0, "deesser", Some(effects::parse_params(e, args).unwrap())).unwrap();
    c
}

fn tone(freq: f64, amp: f64, frames: usize) -> Vec<f32> {
    (0..frames).map(|n| (amp * (TAU * freq * n as f64 / f64::from(RATE)).sin()) as f32).collect()
}

fn process(p: &mut RackProcessor, input: &[f32]) -> Vec<f32> {
    let mut buf = input.to_vec();
    p.process_block(&mut [&mut buf[..]]);
    buf
}

/// Charge the effect with a loud, sibilant tone, reset it, then compare what it makes of a quiet
/// tone with what a freshly built rack makes of the same input.
fn reset_matches_fresh(args: &str) {
    let cfg = deesser_rack(args);
    let mut used = RackProcessor::new(&cfg, 1, RATE).unwrap();
    process(&mut used, &tone(9_000.0, 0.9, 4096));
    used.reset();
    let after = process(&mut used, &tone(9_000.0, 0.02, 512));

    let mut fresh = RackProcessor::new(&cfg, 1, RATE).unwrap();
    let expected = process(&mut fresh, &tone(9_000.0, 0.02, 512));
    assert_eq!(after, expected, "{args}: reset state differs from a new rack");
}

#[test]
fn a_reset_multiband_deesser_matches_a_new_rack() {
    reset_matches_fresh("mode=Multiband");
}

#[test]
fn a_reset_broadband_deesser_matches_a_new_rack() {
    reset_matches_fresh("mode=Broadband");
}

#[test]
fn a_reset_multiband_deesser_leaves_exactly_silence_behind() {
    // Its filters write their ringing into the output, so silence after a reset proves they are
    // clear (the Broadband mode's state only changes its gain, which silence cannot show; the
    // compare-with-a-new-rack test covers that mode).
    let cfg = deesser_rack("mode=Multiband");
    let mut p = RackProcessor::new(&cfg, 1, RATE).unwrap();
    process(&mut p, &tone(9_000.0, 0.9, 4096));
    p.reset();
    let out = process(&mut p, &[0.0; 512]);
    assert!(out.iter().all(|v| *v == 0.0), "the old tail still rings: {out:?}");
}

#[test]
fn rack_reset_gives_the_same_output_as_a_new_rack() {
    let mut cfg = deesser_rack("mode=Broadband");
    let e = effects::find("gain").unwrap();
    cfg.set_slot(1, "gain", Some(effects::parse_params(e, "gain_db=-6.0206").unwrap())).unwrap();

    let mut used = RackProcessor::new(&cfg, 1, RATE).unwrap();
    process(&mut used, &tone(9_000.0, 0.9, 3000));
    used.reset();

    let follow = tone(9_000.0, 0.05, 700);
    let after = process(&mut used, &follow);

    let mut fresh = RackProcessor::new(&cfg, 1, RATE).unwrap();
    let expected = process(&mut fresh, &follow);
    assert_eq!(after, expected);
}
