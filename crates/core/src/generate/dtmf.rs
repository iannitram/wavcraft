//! Dual-tone multi-frequency signal generation.

/// Returns the row and column frequencies for a DTMF key.
pub fn key_frequencies(key: char) -> Option<(f32, f32)> {
    match key {
        '1' => Some((697.0, 1209.0)),
        '2' => Some((697.0, 1336.0)),
        '3' => Some((697.0, 1477.0)),
        'A' => Some((697.0, 1633.0)),
        '4' => Some((770.0, 1209.0)),
        '5' => Some((770.0, 1336.0)),
        '6' => Some((770.0, 1477.0)),
        'B' => Some((770.0, 1633.0)),
        '7' => Some((852.0, 1209.0)),
        '8' => Some((852.0, 1336.0)),
        '9' => Some((852.0, 1477.0)),
        'C' => Some((852.0, 1633.0)),
        '*' => Some((941.0, 1209.0)),
        '0' => Some((941.0, 1336.0)),
        '#' => Some((941.0, 1477.0)),
        'D' => Some((941.0, 1633.0)),
        _ => None,
    }
}

/// Generates one DTMF tone, or `None` for an unknown key or zero sample rate.
pub fn tone(key: char, tone_ms: u32, sample_rate: u32) -> Option<Vec<f32>> {
    let (row_hz, col_hz) = key_frequencies(key)?;
    if sample_rate == 0 {
        return None;
    }
    let sample_count = u64::from(tone_ms) * u64::from(sample_rate) / 1_000;
    let sample_count = usize::try_from(sample_count).ok()?;
    let sample_rate = sample_rate as f32;
    let samples = (0..sample_count)
        .map(|n| {
            let phase = n as f32 / sample_rate * std::f32::consts::TAU;
            0.5 * (phase * row_hz).sin() + 0.5 * (phase * col_hz).sin()
        })
        .collect();
    Some(samples)
}

/// Generates DTMF tones separated by silent gaps, or `None` for invalid input.
pub fn sequence(keys: &str, tone_ms: u32, gap_ms: u32, sample_rate: u32) -> Option<Vec<f32>> {
    if sample_rate == 0 {
        return None;
    }
    let gap_count = usize::try_from(u64::from(gap_ms) * u64::from(sample_rate) / 1_000).ok()?;
    let mut output = Vec::new();
    for (index, key) in keys.chars().enumerate() {
        output.extend(tone(key, tone_ms, sample_rate)?);
        if index + 1 < keys.chars().count() {
            output.resize(output.len().checked_add(gap_count)?, 0.0);
        }
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn goertzel_power(samples: &[f32], frequency: f32, sample_rate: f32) -> f32 {
        let coefficient = 2.0 * (std::f32::consts::TAU * frequency / sample_rate).cos();
        let (mut s1, mut s2) = (0.0, 0.0);
        for sample in samples {
            let s0 = *sample + coefficient * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        (s1 * s1 + s2 * s2 - coefficient * s1 * s2) / (samples.len() as f32 / 2.0).powi(2)
    }

    #[test]
    fn key_frequency_table_is_complete() {
        let expected = [
            ('1', (697.0, 1209.0)),
            ('2', (697.0, 1336.0)),
            ('3', (697.0, 1477.0)),
            ('A', (697.0, 1633.0)),
            ('4', (770.0, 1209.0)),
            ('5', (770.0, 1336.0)),
            ('6', (770.0, 1477.0)),
            ('B', (770.0, 1633.0)),
            ('7', (852.0, 1209.0)),
            ('8', (852.0, 1336.0)),
            ('9', (852.0, 1477.0)),
            ('C', (852.0, 1633.0)),
            ('*', (941.0, 1209.0)),
            ('0', (941.0, 1336.0)),
            ('#', (941.0, 1477.0)),
            ('D', (941.0, 1633.0)),
        ];
        for (key, frequencies) in expected {
            assert_eq!(key_frequencies(key), Some(frequencies));
        }
    }

    #[test]
    fn goertzel_detects_each_key_at_both_sample_rates() {
        let keys = "123A456B789C*0#D";
        let frequencies = [697.0, 770.0, 852.0, 941.0, 1209.0, 1336.0, 1477.0, 1633.0];
        for sample_rate in [8000, 48000] {
            for key in keys.chars() {
                let samples = tone(key, 100, sample_rate).unwrap();
                let (row, column) = key_frequencies(key).unwrap();
                for frequency in frequencies {
                    let power = goertzel_power(&samples, frequency, sample_rate as f32);
                    if frequency == row || frequency == column {
                        assert!(power > 0.2, "{key} at {frequency}: {power}");
                    } else {
                        assert!(power < 0.01, "{key} at {frequency}: {power}");
                    }
                }
            }
        }
    }

    #[test]
    fn tone_lengths_and_first_samples_are_exact() {
        assert_eq!(tone('5', 100, 8000).unwrap()[0], 0.0);
        assert_eq!(tone('5', 100, 48000).unwrap()[0], 0.0);
        assert_eq!(tone('1', 100, 8000).unwrap().len(), 800);
        assert_eq!(tone('1', 100, 48000).unwrap().len(), 4800);
        assert_eq!(tone('1', 25, 8000).unwrap().len(), 200);
    }

    #[test]
    fn sequence_lengths_are_exact() {
        assert_eq!(sequence("123", 100, 50, 8000).unwrap().len(), 3200);
        assert_eq!(sequence("123", 100, 50, 48000).unwrap().len(), 19200);
    }

    #[test]
    fn peak_amplitude_stays_within_unit_range() {
        for key in "123A456B789C*0#D".chars() {
            assert!(tone(key, 100, 8000).unwrap().iter().all(|sample| sample.abs() <= 1.0));
        }
    }

    #[test]
    fn invalid_keys_rates_and_empty_sequence_are_handled() {
        assert_eq!(tone('X', 100, 8000), None);
        assert_eq!(sequence("12X", 100, 50, 8000), None);
        assert_eq!(sequence("a", 100, 50, 8000), None);
        assert_eq!(tone('1', 100, 0), None);
        assert_eq!(sequence("1", 100, 50, 0), None);
        assert_eq!(sequence("", 100, 50, 8000), Some(Vec::new()));
    }
}
