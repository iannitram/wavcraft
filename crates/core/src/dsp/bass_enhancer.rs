//! One-pole bass exciter: split off the low band, saturate it, add it back.

/// For each sample: low = alpha * sample + (1 - alpha) * previous_low;
/// saturated = tanh(drive * low); output = sample + amount * (saturated - low).
pub fn enhance(samples: &[f32], alpha: f32, drive: f32, amount: f32) -> Vec<f32> {
    let mut low = 0.0_f32;
    let mut out = Vec::with_capacity(samples.len());
    for &sample in samples {
        low = alpha * sample + (1.0 - alpha) * low;
        let saturated = (drive * low).tanh();
        out.push(sample + amount * (saturated - low));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::enhance;

    #[test]
    fn one_pole_exciter_has_literal_fixture() {
        let out = enhance(&[0.0, 1.0, 0.0, 0.0], 0.5, 2.0, 1.0);
        let expected = [0.0_f32, 1.261594, 0.212117, 0.119919];
        assert_eq!(out.len(), expected.len());
        for (got, want) in out.iter().zip(expected.iter()) {
            assert!((got - want).abs() < 1e-6, "got {got}, want {want}");
        }
    }
}
