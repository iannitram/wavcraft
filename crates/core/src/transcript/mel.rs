//! Audio to Whisper's input (L1.4, L1.5): the document mixed to mono and resampled to 16 kHz in
//! 30-second windows ([`windows`]), and each window as Whisper's 80-bin log-mel frames
//! ([`log_mel`]).
//!
//! [`windows`] streams: each window reads only the source frames it needs (30 s plus the
//! resampler's kernel reach) through [`DocState::read`], never the whole file (I7).

use std::io;

use crate::doc::DocState;
use crate::resample::{Quality, Resampler};

/// Whisper's input rate.
pub const SAMPLE_RATE: u32 = 16_000;
/// Samples in one window: 30 s at 16 kHz.
pub const WINDOW_SAMPLES: usize = 480_000;
/// Source frames read from the store per call.
const READ_CHUNK: usize = 65_536;

/// One 30-second window of the document at 16 kHz mono.
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    /// Which window: 0 for the first.
    pub index: usize,
    /// The source frame (at the document's rate) the window's first sample falls on, rounded down.
    pub offset: u64,
    /// Exactly [`WINDOW_SAMPLES`] samples; past the end of the document they are 0.
    pub samples: Vec<f32>,
}

/// The source frame (at `rate`) that 16 kHz sample `n` of the whole document falls on, rounded
/// to the nearest frame.
pub fn source_frame(n: u64, rate: u32) -> u64 {
    let rate = u128::from(rate);
    let den = u128::from(SAMPLE_RATE);
    u64::try_from((u128::from(n) * rate + den / 2) / den).unwrap_or(u64::MAX)
}

/// The windows of a document, one at a time ([`windows`]).
pub struct Windows<'a> {
    doc: &'a DocState,
    resampler: Resampler,
    /// 16 kHz samples the whole document makes.
    out_total: u64,
    total: usize,
    next: usize,
    failed: bool,
}

/// The document as 16 kHz mono windows: the channels averaged, resampled with the playback
/// [`Resampler`] (high quality), cut into [`WINDOW_SAMPLES`]-sample windows, the last one padded
/// with zeros. An empty document has none.
pub fn windows(doc: &DocState) -> Windows<'_> {
    let resampler = Resampler::new(doc.sample_rate(), SAMPLE_RATE, Quality::High);
    let out_total = resampler.out_frames(0.0, doc.len_frames() as f64);
    let total = usize::try_from(out_total.div_ceil(WINDOW_SAMPLES as u64)).unwrap_or(usize::MAX);
    Windows { doc, resampler, out_total, total, next: 0, failed: false }
}

impl Windows<'_> {
    /// How many windows the document makes in all (the ones already taken included).
    pub fn total(&self) -> usize {
        self.total
    }

    fn make(&self, index: usize) -> io::Result<Window> {
        let doc = self.doc;
        let len = doc.len_frames();
        let ratio = self.resampler.ratio();
        let hw = self.resampler.half_width();
        let n0 = index as u64 * WINDOW_SAMPLES as u64;
        let n_end = (n0 + WINDOW_SAMPLES as u64).min(self.out_total);
        let mut samples = zeroed(WINDOW_SAMPLES)?;
        if n_end <= n0 {
            return Ok(Window { index, offset: source_offset(n0, doc.sample_rate()), samples });
        }
        // The source frames the kernel touches for outputs n0..n_end.
        let x_first = n0 as f64 * ratio;
        let x_last = (n_end - 1) as f64 * ratio;
        let lo = ((x_first - hw).floor().max(0.0) as u64).min(len);
        let hi = ((x_last + hw).ceil().max(0.0) as u64).saturating_add(1).min(len);
        let mono = read_mono(doc, lo, hi)?;
        for (slot, n) in samples.iter_mut().zip(n0..n_end) {
            let x = n as f64 * ratio - lo as f64;
            *slot = self.resampler.sample_with(x, |k| usize::try_from(k).ok().and_then(|i| mono.get(i)).copied().unwrap_or(0.0));
        }
        Ok(Window { index, offset: source_offset(n0, doc.sample_rate()), samples })
    }
}

/// The source frame 16 kHz sample `n` falls on, rounded down.
fn source_offset(n: u64, rate: u32) -> u64 {
    u64::try_from(u128::from(n) * u128::from(rate) / u128::from(SAMPLE_RATE)).unwrap_or(u64::MAX)
}

fn zeroed(n: usize) -> io::Result<Vec<f32>> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| io::Error::new(io::ErrorKind::OutOfMemory, "no memory for a transcription window"))?;
    v.resize(n, 0.0);
    Ok(v)
}

/// Source frames `lo..hi` of `doc`, every channel averaged.
fn read_mono(doc: &DocState, lo: u64, hi: u64) -> io::Result<Vec<f32>> {
    let n = usize::try_from(hi.saturating_sub(lo)).map_err(|_| io::Error::other("transcription window too long"))?;
    let mut mono = zeroed(n)?;
    let channels = doc.channel_count().max(1);
    let gain = 1.0 / channels as f32;
    let mut buf = vec![0.0f32; READ_CHUNK.min(n.max(1))];
    for ch in 0..doc.channel_count() {
        let mut done = 0usize;
        while done < n {
            let want = (n - done).min(buf.len());
            let got = doc.read(ch, lo + done as u64, &mut buf[..want])?;
            if got < want {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the document ended early"));
            }
            for (m, v) in mono[done..done + want].iter_mut().zip(&buf[..want]) {
                *m += v * gain;
            }
            done += want;
        }
    }
    Ok(mono)
}

impl Iterator for Windows<'_> {
    type Item = io::Result<Window>;

    fn next(&mut self) -> Option<io::Result<Window>> {
        if self.failed || self.next >= self.total {
            return None;
        }
        let index = self.next;
        self.next += 1;
        let w = self.make(index);
        self.failed = w.is_err();
        Some(w)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = if self.failed { 0 } else { self.total - self.next };
        (0, Some(left))
    }
}

/// Mel bins per frame.
pub const N_MELS: usize = 80;
/// The STFT's window and FFT length: 25 ms at 16 kHz.
pub const N_FFT: usize = 400;
/// The STFT's hop: 10 ms at 16 kHz.
pub const HOP: usize = 160;
/// Frames in one window's log-mel: 30 s at a 10 ms hop.
pub const MEL_FRAMES: usize = WINDOW_SAMPLES / HOP;

/// Slaney's mel scale (the one Whisper's filterbank uses): linear, 3 mels per 200 Hz, below
/// 1 kHz, logarithmic above, 27 mels per factor of 6.4.
fn hz_to_mel(hz: f64) -> f64 {
    const F_SP: f64 = 200.0 / 3.0;
    let min_log_mel = 1_000.0 / F_SP;
    if hz >= 1_000.0 { min_log_mel + (hz / 1_000.0).ln() / (6.4f64.ln() / 27.0) } else { hz / F_SP }
}

/// The inverse of [`hz_to_mel`].
fn mel_to_hz(mel: f64) -> f64 {
    const F_SP: f64 = 200.0 / 3.0;
    let min_log_mel = 1_000.0 / F_SP;
    if mel >= min_log_mel { 1_000.0 * ((6.4f64.ln() / 27.0) * (mel - min_log_mel)).exp() } else { F_SP * mel }
}

/// The 80 triangular filters over the 201 FFT bins: centres evenly spaced in mels from 0 Hz to
/// 8 kHz, each scaled by 2 / its width in Hz (Slaney's area normalisation).
fn filterbank() -> Vec<Vec<f64>> {
    let bins = N_FFT / 2 + 1;
    let top = hz_to_mel(f64::from(SAMPLE_RATE) / 2.0);
    let edges: Vec<f64> = (0..N_MELS + 2).map(|i| mel_to_hz(top * i as f64 / (N_MELS + 1) as f64)).collect();
    let mut bank = Vec::with_capacity(N_MELS);
    for m in 0..N_MELS {
        let (lo, mid, hi) = (edges[m], edges[m + 1], edges[m + 2]);
        let norm = 2.0 / (hi - lo);
        let row = (0..bins)
            .map(|k| {
                let f = k as f64 * f64::from(SAMPLE_RATE) / N_FFT as f64;
                let rising = (f - lo) / (mid - lo);
                let falling = (hi - f) / (hi - mid);
                rising.min(falling).max(0.0) * norm
            })
            .collect();
        bank.push(row);
    }
    bank
}

/// Whisper's log-mel input for one 30-second window of 16 kHz audio: `samples` is padded with
/// zeros or cut to [`WINDOW_SAMPLES`], then
///
/// 1. a short-time Fourier transform with a periodic Hann window of [`N_FFT`] samples every
///    [`HOP`] samples, the signal reflected by `N_FFT / 2` at each end so frame `t` is centred on
///    sample `t * HOP`; the last frame is dropped, leaving [`MEL_FRAMES`];
/// 2. the power of each bin through the [`N_MELS`] Slaney filters;
/// 3. `log10` of each value, floored at `1e-10`, then raised to at least the window's largest
///    value less 8, and mapped by `(v + 4) / 4`.
///
/// Returns [`MEL_FRAMES`] frames of [`N_MELS`] values, in time order.
pub fn log_mel(samples: &[f32]) -> Vec<[f32; N_MELS]> {
    let half = N_FFT / 2;
    let x = |i: usize| -> f64 { samples.get(i).map_or(0.0, |v| f64::from(*v)) };
    // Index `j` of the reflect-padded signal (length WINDOW_SAMPLES + N_FFT).
    let padded = |j: usize| -> f64 {
        if j < half {
            x(half - j)
        } else if j - half < WINDOW_SAMPLES {
            x(j - half)
        } else {
            let back = j - half - WINDOW_SAMPLES; // 0 for the first sample past the end
            x(WINDOW_SAMPLES - 2 - back)
        }
    };
    let hann: Vec<f64> = (0..N_FFT).map(|n| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * n as f64 / N_FFT as f64).cos()).collect();
    let bank = filterbank();
    let fft = rustfft::FftPlanner::<f64>::new().plan_fft_forward(N_FFT);
    let mut buf = vec![rustfft::num_complex::Complex::new(0.0, 0.0); N_FFT];
    let mut scratch = vec![rustfft::num_complex::Complex::new(0.0, 0.0); fft.get_inplace_scratch_len()];
    let mut power = vec![0.0f64; half + 1];
    let mut out = vec![[0.0f32; N_MELS]; MEL_FRAMES];
    let mut max = f64::NEG_INFINITY;
    for (t, frame) in out.iter_mut().enumerate() {
        for (n, (c, w)) in buf.iter_mut().zip(&hann).enumerate() {
            *c = rustfft::num_complex::Complex::new(padded(t * HOP + n) * w, 0.0);
        }
        fft.process_with_scratch(&mut buf, &mut scratch);
        for (p, c) in power.iter_mut().zip(&buf) {
            *p = c.norm_sqr();
        }
        for (slot, row) in frame.iter_mut().zip(&bank) {
            let e: f64 = row.iter().zip(&power).map(|(w, p)| w * p).sum();
            let v = e.max(1e-10).log10();
            max = max.max(v);
            *slot = v as f32;
        }
    }
    let floor = (max - 8.0) as f32;
    for v in out.iter_mut().flatten() {
        *v = (v.max(floor) + 4.0) / 4.0;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{Channel, DocState, Piece, SourceFormat};
    use crate::store::{BLOCK_FRAMES, Store, StoreOptions};

    fn silent_doc(name: &str, rate: u32, channels: usize, frames: u64) -> (std::path::PathBuf, DocState) {
        let dir = std::env::temp_dir().join(format!("wavcraft-mel-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let s = Store::create_with(&dir, StoreOptions::default()).unwrap();
        let chans = (0..channels)
            .map(|_| {
                let mut c = Channel::new();
                c.push_zeros(frames);
                c
            })
            .collect();
        (dir, DocState::new(s, chans, rate, SourceFormat::synthetic(rate, channels as u16)).unwrap())
    }

    fn doc_of(name: &str, rate: u32, data: &[Vec<f32>]) -> (std::path::PathBuf, DocState) {
        let dir = std::env::temp_dir().join(format!("wavcraft-mel-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let s = Store::create_with(&dir, StoreOptions::default()).unwrap();
        let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
        (dir, DocState::new(s, chans, rate, SourceFormat::synthetic(rate, data.len() as u16)).unwrap())
    }

    #[test]
    fn a_65_s_file_makes_3_windows() {
        // Two channels, 0.5 and 0.25: the mono mix is 0.375 for 65 s, then the padding is 0.
        let frames = 65 * 48_000;
        let (dir, d) = doc_of("65s", 48_000, &[vec![0.5; frames], vec![0.25; frames]]);
        let w = windows(&d);
        assert_eq!(w.total(), 3);
        let all: Vec<Window> = w.map(|w| w.unwrap()).collect();
        assert_eq!(all.len(), 3);
        assert!(all.iter().all(|w| w.samples.len() == WINDOW_SAMPLES));
        assert_eq!(all.iter().map(|w| w.index).collect::<Vec<_>>(), vec![0, 1, 2]);
        // DC passes the resampler at unity; the edges of the file are left out.
        assert!(all[0].samples[1_000..].iter().all(|v| (v - 0.375).abs() < 1e-3));
        assert!(all[1].samples.iter().all(|v| (v - 0.375).abs() < 1e-3), "a middle window has no edge");
        let last = &all[2].samples;
        let end = 5 * 16_000; // 65 s is 5 s into the third window
        assert!(last[..end - 1_000].iter().all(|v| (v - 0.375).abs() < 1e-3));
        assert!(last[end..].iter().all(|v| *v == 0.0), "the last window is zero padded");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_16k_file_is_copied_exactly() {
        let data: Vec<f32> = (0..20_000).map(|i| (i % 100) as f32 / 100.0).collect();
        let (dir, d) = doc_of("16k", 16_000, std::slice::from_ref(&data));
        let all: Vec<Window> = windows(&d).map(|w| w.unwrap()).collect();
        assert_eq!(all.len(), 1);
        assert_eq!(&all[0].samples[..20_000], &data[..]);
        assert!(all[0].samples[20_000..].iter().all(|v| *v == 0.0));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_seam_between_windows_matches_one_long_resample() {
        // A stereo sweep at 44.1 kHz: around each seam the streamed windows give the samples a
        // resample of the whole signal at once gives (each window reads its kernel reach).
        let frames = 61 * 44_100;
        let l: Vec<f32> = (0..frames).map(|i| (i as f64 * 0.013 + (i as f64 * 1e-6).powi(2)).sin() as f32 * 0.5).collect();
        let r: Vec<f32> = (0..frames).map(|i| (i as f64 * 0.071).cos() as f32 * 0.25).collect();
        let mono: Vec<f32> = l.iter().zip(&r).map(|(a, b)| a * 0.5 + b * 0.5).collect();
        let (dir, d) = doc_of("seam", 44_100, &[l, r]);
        let all: Vec<Window> = windows(&d).map(|w| w.unwrap()).collect();
        assert_eq!(all.len(), 3);
        let rs = Resampler::new(44_100, SAMPLE_RATE, Quality::High);
        for (k, w) in all.iter().enumerate() {
            for i in (0..200).chain(WINDOW_SAMPLES - 200..WINDOW_SAMPLES) {
                let n = (k * WINDOW_SAMPLES + i) as f64;
                let want = rs.sample_at(&mono, n * rs.ratio());
                assert!((w.samples[i] - want).abs() < 1e-5, "window {k} sample {i}: {} vs {want}", w.samples[i]);
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_empty_file_makes_no_windows() {
        let (dir, d) = silent_doc("empty", 48_000, 1, 0);
        assert_eq!(windows(&d).total(), 0);
        assert_eq!(windows(&d).count(), 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn offsets_at_44k1_are_whole_frames() {
        let (dir, d) = silent_doc("44k1", 44_100, 1, 91 * 44_100);
        let offsets: Vec<u64> = windows(&d).map(|w| w.unwrap().offset).collect();
        assert_eq!(offsets, vec![0, 1_323_000, 2_646_000, 3_969_000]);
        assert_eq!(source_frame(16_000, 44_100), 44_100);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn offsets_at_48k_are_0_1440000_2880000() {
        let (dir, d) = silent_doc("offsets", 48_000, 1, 65 * 48_000);
        let offsets: Vec<u64> = windows(&d).map(|w| w.unwrap().offset).collect();
        assert_eq!(offsets, vec![0, 1_440_000, 2_880_000]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_window_is_3000_frames_of_80_bins() {
        let x: Vec<f32> = (0..WINDOW_SAMPLES).map(|i| ((i * 7919) % 1000) as f32 / 1000.0 - 0.5).collect();
        let m = log_mel(&x);
        assert_eq!(m.len(), MEL_FRAMES);
        assert_eq!(MEL_FRAMES, 3_000);
        assert_eq!(m[0].len(), N_MELS);
        assert_eq!(N_MELS, 80);
        assert!(m.iter().flatten().all(|v| v.is_finite()));
        // A shorter input is padded to 30 s: still 3000 frames.
        assert_eq!(log_mel(&x[..1_000]).len(), MEL_FRAMES);
    }

    fn peak_bin(frame: &[f32; N_MELS]) -> usize {
        let mut best = 0;
        for (i, v) in frame.iter().enumerate() {
            if *v > frame[best] {
                best = i;
            }
        }
        best
    }

    #[test]
    fn a_1_khz_sine_peaks_in_mel_bin_26() {
        let x: Vec<f32> = (0..WINDOW_SAMPLES).map(|i| (0.5 * (2.0 * std::f64::consts::PI * 1_000.0 * i as f64 / 16_000.0).sin()) as f32).collect();
        let m = log_mel(&x);
        // Every frame but the first: frame 0 is centred on the reflection point of the padding,
        // where the mirrored sine has a kink that spreads its energy up into bin 27. Whisper's own
        // reflect padding does the same (checked against an independent reference of the
        // published formulas, which also peaks frame 0 in bin 27 and every other frame in 26).
        assert_eq!(peak_bin(&m[0]), 27, "frame 0 straddles the reflected start");
        for (t, frame) in m.iter().enumerate().skip(1) {
            assert_eq!(peak_bin(frame), 26, "frame {t}");
        }
        // The top of the scale is 4 below the floor plus 8: (max - 8 + 4) / 4 is 1 below the peak.
        let max = m.iter().flatten().fold(f32::MIN, |a, &b| a.max(b));
        let min = m.iter().flatten().fold(f32::MAX, |a, &b| a.min(b));
        assert!((max - min - 2.0).abs() < 1e-4, "the range is clamped to 8 decades: {min}..{max}");
    }

    #[test]
    fn silence_is_the_floor_value() {
        let m = log_mel(&vec![0.0; WINDOW_SAMPLES]);
        assert_eq!(m.len(), MEL_FRAMES);
        // log10(1e-10) = -10, then (-10 + 4) / 4.
        assert!(m.iter().flatten().all(|v| *v == -1.5), "every bin equal");
    }

    #[test]
    fn the_filterbank_follows_slaney() {
        assert!((hz_to_mel(1_000.0) - 15.0).abs() < 1e-12);
        assert!((hz_to_mel(500.0) - 7.5).abs() < 1e-12);
        assert!((mel_to_hz(hz_to_mel(6_000.0)) - 6_000.0).abs() < 1e-9);
        let bank = filterbank();
        assert_eq!(bank.len(), N_MELS);
        assert!(bank.iter().all(|row| row.len() == N_FFT / 2 + 1 && row.iter().all(|w| *w >= 0.0)));
        // Every filter but the lowest catches at least one FFT bin.
        assert!(bank.iter().skip(1).all(|row| row.iter().any(|w| *w > 0.0)));
        // Spot values, from the formulas worked independently: filter 26 at 1 kHz (FFT bin 25),
        // filter 0 at 40 Hz, and the top filter's peak.
        assert!((bank[26][25] - 0.022_114_217_267_094_387).abs() < 1e-12);
        assert!((bank[0][1] - 0.024_862_593_984_176_087).abs() < 1e-12);
        assert!((bank[79].iter().fold(0.0f64, |a, &b| a.max(b)) - 0.003_164_711_739_176_461).abs() < 1e-12);
    }
}
