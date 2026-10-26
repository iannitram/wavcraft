//! Session properties (C078): change a multitrack session's name, tempo, time signature, sample
//! rate and bit depth after it was made. A new file beside `multitrack.rs`; the multitrack model
//! has no undo, so every setter returns the value it replaced and a caller can build an undo.
//!
//! Rules (all of them are tested in `crates/core/tests/session_properties.rs`):
//! - A refused change leaves the session exactly as it was.
//! - The name is trimmed, may not be empty and is cut to `MAX_NAME_CHARS` (the same rules as a
//!   track name). The title and notes belong to another module.
//! - The tempo is the session's opening tempo (the tempo event at tick 0), 5 to 1000 BPM, the
//!   model's own range. Tempo events later in the session are not touched. The time signature is
//!   the opening meter in the same way: 1 to 64 beats over a note value of 1, 2, 4, 8, 16, 32 or
//!   64. Neither depends on the sample rate, so both may change while the session holds audio.
//! - The sample rate (8000 to 768000 Hz) and the bit depth (16, 24 or 32 for float) change only
//!   while the session holds no audio, that is no source and no clip on any playlist of any
//!   track. Changing the rate under clips would silently retime them. Asking for the value the
//!   session already has is not a change and is always accepted.

use super::{MtError, Multitrack, bit_depth_from_bits, bits_of, clean_name, err};
use soundcraft_model::time::SampleRate;

impl Multitrack {
    /// True when the session holds any source or any clip (on any playlist of any track).
    pub fn holds_audio(&self) -> bool {
        !self.session.sources.is_empty() || self.session.tracks.iter().any(|track| track.playlists.iter().any(|playlist| !playlist.clips.is_empty()))
    }

    /// Rename the session; returns the old name.
    pub fn set_name(&mut self, name: &str) -> Result<String, MtError> {
        let name = clean_name(name)?;
        Ok(std::mem::replace(&mut self.session.name, name))
    }

    /// The opening tempo in BPM.
    pub fn tempo_bpm(&self) -> f64 {
        self.session.tempo.tempo_at_tick(0)
    }

    /// Set the opening tempo; returns the old one.
    pub fn set_tempo_bpm(&mut self, bpm: f64) -> Result<f64, MtError> {
        let old = self.tempo_bpm();
        self.session.tempo.set_tempo(0, bpm).map_err(|_| MtError(format!("a tempo must be 5 to 1000 BPM, not {bpm}")))?;
        Ok(old)
    }

    /// The opening time signature as (beats, note value).
    pub fn time_signature(&self) -> (u32, u32) {
        let m = self.session.tempo.meter_at_tick(0);
        (m.numerator, m.denominator)
    }

    /// Set the opening time signature; returns the old one.
    pub fn set_time_signature(&mut self, numerator: u32, denominator: u32) -> Result<(u32, u32), MtError> {
        let old = self.time_signature();
        self.session.tempo.set_meter(0, numerator, denominator).map_err(|_| {
            MtError(format!("a time signature needs 1 to 64 beats over a note value of 1, 2, 4, 8, 16, 32 or 64, not {numerator}/{denominator}"))
        })?;
        Ok(old)
    }

    /// Set the sample rate in Hz; returns the old one.
    pub fn set_rate(&mut self, rate_hz: u32) -> Result<u32, MtError> {
        let rate = SampleRate::new(rate_hz).map_err(|_| MtError(format!("a session's sample rate must be 8000 to 768000 Hz, not {rate_hz}")))?;
        let old = self.rate();
        if rate_hz == old {
            return Ok(old);
        }
        if self.holds_audio() {
            return err(format!(
                "the session already holds audio, so its sample rate cannot change from {old} Hz to {rate_hz} Hz without retiming it; remove every clip and source first"
            ));
        }
        self.session.sample_rate = rate;
        Ok(old)
    }

    /// Set the bit depth (16, 24 or 32 for float); returns the old one.
    pub fn set_bit_depth(&mut self, bits: u32) -> Result<u32, MtError> {
        let Some(depth) = bit_depth_from_bits(bits) else {
            return err(format!("a bit depth must be 16, 24 or 32 (float), not {bits}"));
        };
        let old = bits_of(self.session.bit_depth);
        if bits == old {
            return Ok(old);
        }
        if self.holds_audio() {
            return err(format!(
                "the session already holds audio, so its bit depth cannot change from {old} to {bits} bits; remove every clip and source first"
            ));
        }
        self.session.bit_depth = depth;
        Ok(old)
    }
}
