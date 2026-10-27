//! Mixdown (f111, roadmap C190, C188, C193): render a multitrack session, or only chosen clips of
//! it, through SoundCraft's offline mix engine (`soundcraft-mix`, `RangeRender`) into a new
//! waveform document. The engine is not rebuilt here: this module only streams its blocks into a
//! session store, a block of 65,536 frames per channel at a time, so the whole mix is never in
//! RAM. An optional normalise to a peak runs at the end as a second streamed pass.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use soundcraft_mix::{RangeRender, main_channels};
use soundcraft_model::time::Range;
use soundcraft_model::{ClipId, Session};

use crate::doc::{Channel, DocState, Piece, SourceFormat};
use crate::multitrack::MtError;
use crate::store::{BLOCK_FRAMES, Store, StoreOptions};

/// Frames the engine renders per call.
const RENDER_BLOCK: usize = 4096;

/// What to mix down.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MixdownOptions {
    /// Only these clips (C188); `None` mixes every clip. An empty list is refused.
    pub only_clips: Option<Vec<ClipId>>,
    /// Scale the result so its highest peak is this many dB (at most 0) below full scale (C193).
    pub normalize_db: Option<f32>,
}

fn fail<T>(s: impl Into<String>) -> Result<T, MtError> {
    Err(MtError(s.into()))
}

/// Render `session` (as the mix engine plays it: faders, pans, mute and solo) to a new document
/// in a new store under `sessions_root`. `progress` gets 0 to 1; `cancel` stops it early.
pub fn mixdown(
    session: &Session,
    opts: &MixdownOptions,
    sessions_root: Option<&Path>,
    progress: &mut dyn FnMut(f64),
    cancel: &AtomicBool,
) -> Result<DocState, MtError> {
    let mut s = session.clone();
    if let Some(keep) = &opts.only_clips {
        if keep.is_empty() {
            return fail("no clips are selected");
        }
        let mut found = 0;
        for t in &mut s.tracks {
            if let Some(pl) = t.playlist_mut() {
                for c in &mut pl.clips {
                    if keep.contains(&c.id) {
                        found += 1;
                    } else {
                        c.muted = true;
                    }
                }
            }
        }
        if found == 0 {
            return fail("none of the selected clips is in the session");
        }
    }
    if let Some(db) = opts.normalize_db
        && !(db.is_finite() && db <= 0.0)
    {
        return fail("normalize to a peak of 0 dB or lower");
    }
    // The end is that of the clips mixed (selected only, if so), not of the whole session.
    let end = s
        .tracks
        .iter()
        .filter_map(|t| t.playlist())
        .flat_map(|p| p.clips.iter())
        .filter(|c| !c.muted && c.is_audio())
        .map(|c| c.start + c.length)
        .max()
        .unwrap_or(0);
    if end <= 0 {
        return fail("the session has no audio to mix down");
    }
    let nch = main_channels(&s);
    if nch == 0 {
        return fail("the session has no output channels");
    }
    let rate = s.sample_rate.hz();
    let store = Store::create_session(sessions_root, StoreOptions::default())
        .map_err(|e| MtError(format!("cannot make the mixdown's session folder: {e}")))?;
    let io = |e: std::io::Error| MtError(format!("writing the mixdown failed: {e}"));

    let mut render = RangeRender::new(&s, None, Range::new(0, end), RENDER_BLOCK);
    let mut channels: Vec<Channel> = (0..nch).map(|_| Channel::new()).collect();
    let mut planes: Vec<Vec<f32>> = vec![Vec::with_capacity(BLOCK_FRAMES); nch];
    let mut peak = 0.0_f32;
    let mut done = 0_u64;
    while let Some((chunk, n)) = render.next_block(&s) {
        if cancel.load(Ordering::Relaxed) {
            return fail("the mixdown was cancelled");
        }
        for (plane, c) in planes.iter_mut().zip(chunk) {
            let part = c.get(..n).unwrap_or_default();
            peak = part.iter().fold(peak, |m, x| m.max(x.abs()));
            plane.extend_from_slice(part);
        }
        done += n as u64;
        if planes[0].len() >= BLOCK_FRAMES {
            flush(&store, &mut planes, &mut channels).map_err(io)?;
        }
        progress((done as f64 / end as f64).min(1.0) * if opts.normalize_db.is_some() { 0.8 } else { 1.0 });
    }
    flush(&store, &mut planes, &mut channels).map_err(io)?;
    if channels[0].is_empty() {
        return fail("the mix is empty");
    }

    if let Some(db) = opts.normalize_db
        && peak > 0.0
    {
        let gain = 10f32.powf(db / 20.0) / peak;
        let mut scaled = Vec::with_capacity(nch);
        let total: u64 = channels[0].len();
        let mut buf = vec![0.0_f32; BLOCK_FRAMES];
        for ch in &channels {
            let mut out = Channel::new();
            let mut at = 0_u64;
            while at < total {
                if cancel.load(Ordering::Relaxed) {
                    return fail("the mixdown was cancelled");
                }
                let n = ((total - at) as usize).min(BLOCK_FRAMES);
                let got = ch.read(&store, at, &mut buf[..n]).map_err(io)?;
                if got == 0 {
                    break;
                }
                for x in &mut buf[..got] {
                    *x *= gain;
                }
                if let Some(p) = Piece::whole(store.append(&buf[..got]).map_err(io)?) {
                    out.push(p);
                }
                at += got as u64;
            }
            scaled.push(out);
            progress(0.8 + 0.2 * scaled.len() as f64 / nch as f64);
        }
        channels = scaled;
    }
    store.flush().map_err(io)?;
    progress(1.0);
    let src = SourceFormat::synthetic(rate, u16::try_from(nch).unwrap_or(2));
    DocState::new(store, channels, rate, src).map_err(|e| MtError(e.to_string()))
}

fn flush(store: &Store, planes: &mut [Vec<f32>], channels: &mut [Channel]) -> std::io::Result<()> {
    for (plane, ch) in planes.iter_mut().zip(channels.iter_mut()) {
        if plane.is_empty() {
            continue;
        }
        if let Some(p) = Piece::whole(store.append(plane)?) {
            ch.push(p);
        }
        plane.clear();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multitrack::Multitrack;
    use soundcraft_model::BitDepthSetting;
    use soundcraft_audio_io::AudioBuffer as ModelBuffer;

    const RATE: u32 = 48_000;

    fn sine(hz: f32, amp: f32, secs: f32, ch: usize) -> ModelBuffer {
        let n = (RATE as f32 * secs) as usize;
        let one: Vec<f32> = (0..n).map(|i| amp * (2.0 * std::f32::consts::PI * hz * i as f32 / RATE as f32).sin()).collect();
        ModelBuffer { sample_rate: RATE, channels: vec![one; ch] }
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("wavcraft-mix-{tag}-{}-{}", std::process::id(), crate::random::hex(4).unwrap_or_default()));
        std::fs::create_dir_all(&d).unwrap_or_else(|e| panic!("{e}"));
        d
    }

    fn render(mt: &Multitrack, opts: &MixdownOptions, dir: &Path) -> Vec<Vec<f32>> {
        let st = mixdown(&mt.session, opts, Some(dir), &mut |_| {}, &AtomicBool::new(false)).unwrap_or_else(|e| panic!("{e}"));
        let b = st.to_audio_buffer().unwrap_or_else(|e| panic!("{e}"));
        b.channels
    }

    /// Two stereo tracks, a 440 Hz and a 660 Hz tone at 0.25 each, both from 0 to 1 s.
    fn two_tones() -> (Multitrack, Vec<ClipId>) {
        let mut m = Multitrack::new("T", RATE, BitDepthSetting::Float32).unwrap_or_else(|e| panic!("{e}"));
        let a = m.add_track(None, 2).unwrap_or_else(|e| panic!("{e}"));
        let b = m.add_track(None, 2).unwrap_or_else(|e| panic!("{e}"));
        let c1 = m.add_clip_buffer(a, "A", "a.wav", sine(440.0, 0.25, 1.0, 2), 0.0).unwrap_or_else(|e| panic!("{e}"));
        let c2 = m.add_clip_buffer(b, "B", "b.wav", sine(660.0, 0.25, 1.0, 2), 0.0).unwrap_or_else(|e| panic!("{e}"));
        (m, vec![c1, c2])
    }

    fn peak(ch: &[f32]) -> f32 {
        ch.iter().fold(0.0, |m, x| m.max(x.abs()))
    }

    #[test]
    fn two_tracks_with_known_tones_mix_to_the_sum() {
        let (m, ids) = two_tones();
        let d = scratch("sum");
        let both = render(&m, &MixdownOptions::default(), &d);
        let a = render(&m, &MixdownOptions { only_clips: Some(vec![ids[0]]), normalize_db: None }, &d);
        let b = render(&m, &MixdownOptions { only_clips: Some(vec![ids[1]]), normalize_db: None }, &d);
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(both.len(), 2);
        assert_eq!(both[0].len(), RATE as usize);
        assert!((peak(&a[0]) - 0.25).abs() < 0.03, "a 0.25 tone alone stays near 0.25, got {}", peak(&a[0]));
        let worst = (0..both[0].len()).map(|i| (both[0][i] - (a[0][i] + b[0][i])).abs()).fold(0.0, f32::max);
        assert!(worst < 1e-4, "the mix is the sum of the two tracks, worst error {worst}");
        assert!(peak(&both[0]) > peak(&a[0]) * 1.2, "the sum is louder than one track");
    }

    #[test]
    fn only_the_selected_clips_are_mixed() {
        let (m, ids) = two_tones();
        let d = scratch("sel");
        let only_a = render(&m, &MixdownOptions { only_clips: Some(vec![ids[0]]), normalize_db: None }, &d);
        let none =
            mixdown(&m.session, &MixdownOptions { only_clips: Some(vec![]), normalize_db: None }, Some(&d), &mut |_| {}, &AtomicBool::new(false));
        let _ = std::fs::remove_dir_all(&d);
        let t440 = sine(440.0, 0.25, 1.0, 1).channels[0].clone();
        let t660 = sine(660.0, 0.25, 1.0, 1).channels[0].clone();
        let dot = |x: &[f32], y: &[f32]| x.iter().zip(y).map(|(a, b)| a * b).sum::<f32>();
        assert!(dot(&only_a[0], &t440) > 100.0 * dot(&only_a[0], &t660).abs(), "the 440 Hz track is there and the 660 Hz track is not");
        assert!(none.is_err(), "an empty selection is refused");
    }

    #[test]
    fn normalize_hits_the_target_peak() {
        let (m, _) = two_tones();
        let d = scratch("norm");
        let out = render(&m, &MixdownOptions { only_clips: None, normalize_db: Some(-1.0) }, &d);
        let _ = std::fs::remove_dir_all(&d);
        let want = 10f32.powf(-1.0 / 20.0);
        let got = out.iter().map(|c| peak(c)).fold(0.0, f32::max);
        assert!((got - want).abs() < 1e-4, "peak {got}, wanted {want}");
        assert!(
            mixdown(&m.session, &MixdownOptions { only_clips: None, normalize_db: Some(3.0) }, None, &mut |_| {}, &AtomicBool::new(false)).is_err()
        );
    }

    #[test]
    fn an_empty_session_has_nothing_to_mix() {
        let m = Multitrack::new("E", RATE, BitDepthSetting::Float32).unwrap_or_else(|e| panic!("{e}"));
        assert!(mixdown(&m.session, &MixdownOptions::default(), None, &mut |_| {}, &AtomicBool::new(false)).is_err());
    }
}
