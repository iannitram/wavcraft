//! B4.8: the RMS reading against the dsp crate's RMS meter, on a steady 1 kHz tone.

use wavcraft_core::levels::rms::RmsLevels;

fn sine(amp: f32, period: usize, n: usize) -> Vec<f32> {
    (0..n).map(|i| amp * (std::f32::consts::TAU * i as f32 / period as f32).sin()).collect()
}

#[test]
fn rms_matches_the_dsp_meter_on_a_tone() {
    // 3 s of a 1 kHz sine of amplitude 0.1 at 48 kHz: a period is 48 samples.
    let tone = sine(0.1, 48, 144_000);

    let mut ours = RmsLevels::new(48000.0, 300.0);
    ours.feed(0, &tone);
    let a = ours.reading(0).rms_db;

    let mut theirs = soundcraft_dsp::meter::RmsMeter::new(48000.0);
    theirs.process(&tone);
    let b = theirs.rms_db();

    assert!((a - (-23.01)).abs() <= 0.1, "RmsLevels read {a} dB, expected -23.01 within 0.1 (dsp meter read {b} dB)");
    assert!((b - (-23.01)).abs() <= 0.1, "dsp RmsMeter read {b} dB, expected -23.01 within 0.1 (RmsLevels read {a} dB)");
    assert!((a - b).abs() <= 0.1, "RmsLevels read {a} dB and the dsp RmsMeter read {b} dB; they differ by more than 0.1");
}
