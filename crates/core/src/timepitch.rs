//! Stretch and Pitch, and the pitch readout (f127, roadmap B108 and B103), built on
//! `soundcraft-dsp` (MIT OR Apache-2.0): `offline::time_stretch` (WSOLA), `offline::pitch_shift`
//! (WSOLA plus resampling) and `pitch_detect::yin`.
//!
//! Stretching changes the length, so it is not an effect on the fixed-length `replace_range`
//! path: the selection is processed whole in memory (capped at [`MAX_FRAMES`] a channel) and
//! the result replaces the range as a paste, a normal length-changing edit with one undo step.

use std::io;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

use soundcraft_dsp::{offline, pitch_detect};

use crate::doc::DocState;
use crate::doc::edit::{Change, ChannelMask, Clip, Crossfades, Edit, EditError, PasteAt, paste};

pub mod fit;

/// The undo step's name.
pub const LABEL: &str = "Stretch and Pitch";

/// The most frames a channel Stretch and Pitch will hold in memory (10 minutes at 48 kHz).
pub const MAX_FRAMES: u64 = 48_000 * 600;

fn invalid(why: String) -> EditError {
    EditError::Io(io::Error::new(io::ErrorKind::InvalidInput, why))
}

/// Stretch `range` of every channel to `ratio` times its length (0.1 to 10, pitch kept), then
/// shift it by `semitones` (-48 to 48, length kept). One edit named [`LABEL`].
pub fn stretch_pitch(doc: &DocState, range: Range<u64>, ratio: f64, semitones: f64, xf: Crossfades, cancel: &AtomicBool) -> Result<Edit, EditError> {
    if !(0.1..=10.0).contains(&ratio) || !(-48.0..=48.0).contains(&semitones) {
        return Err(invalid(format!("ratio must be 0.1 to 10 and pitch -48 to 48 semitones (got {ratio} and {semitones})")));
    }
    if range.start >= range.end || range.end > doc.len_frames() {
        return Ok(Edit { state: doc.clone(), label: LABEL.to_string(), change: Change::None });
    }
    let frames = range.end - range.start;
    if frames > MAX_FRAMES {
        return Err(invalid(format!(
            "select at most {} s to stretch (this is {} s)",
            MAX_FRAMES / u64::from(doc.sample_rate()),
            frames / u64::from(doc.sample_rate())
        )));
    }
    let mut channels = Vec::with_capacity(doc.channel_count());
    for c in 0..doc.channel_count() {
        let mut v = vec![0.0f32; frames as usize];
        let got = doc.read(c, range.start, &mut v)?;
        v.truncate(got);
        channels.push(v);
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(EditError::Cancelled);
    }
    let rate = doc.sample_rate() as f32;
    let mut out = if (ratio - 1.0).abs() < 1e-9 { channels } else { offline::time_stretch(&channels, ratio, rate) };
    if semitones.abs() > 1e-4 {
        out = offline::pitch_shift(&out, semitones as f32, rate);
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(EditError::Cancelled);
    }
    let clip = Clip::from_samples(doc.store(), doc.sample_rate(), &out)?;
    let mut edit = paste(doc, PasteAt::Replace(range), &clip, ChannelMask::ALL, xf)?;
    edit.label = LABEL.to_string();
    Ok(edit)
}

/// Pitch readout over `range` (the whole file when empty): the median fundamental, in Hz, of
/// up to 200 YIN frames spread over it, all channels mixed to their mean. `None` when no frame
/// is voiced. Reads only the frames it analyses.
pub fn detect_pitch(doc: &DocState, range: Range<u64>) -> io::Result<Option<f32>> {
    const FRAME: usize = 4096;
    let len = doc.len_frames();
    let range = if range.start < range.end { range.start.min(len)..range.end.min(len) } else { 0..len };
    let span = range.end.saturating_sub(range.start);
    if span < FRAME as u64 {
        return Ok(None);
    }
    let n = ((span / FRAME as u64) as usize).clamp(1, 200);
    let last = span - FRAME as u64;
    let chans = doc.channel_count();
    let mut found = Vec::new();
    let mut buf = vec![0.0f32; FRAME];
    let mut mix = vec![0.0f32; FRAME];
    for i in 0..n {
        let at = range.start + if n > 1 { last * i as u64 / (n as u64 - 1) } else { 0 };
        mix.iter_mut().for_each(|x| *x = 0.0);
        for c in 0..chans {
            let got = doc.read(c, at, &mut buf)?;
            buf[got..].iter_mut().for_each(|x| *x = 0.0);
            for (m, b) in mix.iter_mut().zip(&buf) {
                *m += b / chans as f32;
            }
        }
        if let Some(f) = pitch_detect::yin(&mix, doc.sample_rate() as f32, 50.0, 2000.0, 0.15) {
            found.push(f);
        }
    }
    if found.is_empty() {
        return Ok(None);
    }
    found.sort_by(f32::total_cmp);
    Ok(Some(found[found.len() / 2]))
}
