//! Amplitude statistics (roadmap A313, A316): per channel peak and where it is, minimum and
//! maximum sample, possibly clipped runs, DC offset, RMS over a window (minimum, maximum,
//! average, total), dynamic range and the bit depth the samples actually use.
//!
//! The maths is streamed: [`ChannelScan::add`] takes chunks in order, so a scan never needs the
//! whole file in memory. [`scan`] feeds it from a [`DocState`] in 64 K-frame chunks with
//! progress and a cancel flag. Loudness (LUFS, true peak) is not here; it lives with the
//! loudness measurement.
//!
//! Definitions (what the numbers mean):
//! - Levels are linear, 1.0 is full scale; `*_db` helpers give dBFS (`-inf` for 0).
//! - A *possibly clipped run* is two or more consecutive samples of the same sign at or above
//!   [`CLIP_LEVEL`] in magnitude. The run is reported by its first frame.
//! - *DC offset* is the mean sample value (fraction of full scale; `dc_percent` is x 100).
//! - *RMS window*: the selection is cut into windows of `window_frames` from its start; each full
//!   window has an RMS. Minimum, maximum and average (arithmetic mean of the window RMS values)
//!   are over those windows; a trailing part-window is ignored unless it is all there is. *Total*
//!   RMS is over every sample.
//! - *Dynamic range* is the loudest window RMS over the quietest window RMS that is not digital
//!   silence, in dB (`None` when there is no such window).
//! - *Bit depth* is the smallest number of bits (1..=24) in which every sample is a whole
//!   number of steps; samples that are not multiples of 2^-23 count as float (32). All zeros: 0.

use std::io;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::doc::DocState;

/// Magnitude at or above which a sample counts as at full scale (a 16-bit 32767 is 0.99997).
pub const CLIP_LEVEL: f32 = 0.9999;
/// Fewest consecutive full-scale samples that make a possibly clipped run.
pub const MIN_CLIP_RUN: u64 = 2;
/// Default RMS window.
pub const DEFAULT_RMS_WINDOW_MS: u32 = 50;
/// Run starts kept per channel (the counts go on past it).
pub const MAX_CLIP_RUNS_KEPT: usize = 100_000;
/// Frames read from the document at a time.
pub const SCAN_CHUNK_FRAMES: usize = 65_536;

/// Linear amplitude to dBFS (`-inf` for 0).
pub fn to_db(x: f64) -> f64 {
    if x <= 0.0 { f64::NEG_INFINITY } else { 20.0 * x.log10() }
}

/// The numbers for one channel.
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelStats {
    pub frames: u64,
    /// Largest magnitude, and the first frame (in the document) that has it.
    pub peak: f32,
    pub peak_frame: u64,
    pub min: f32,
    pub min_frame: u64,
    pub max: f32,
    pub max_frame: u64,
    /// Mean sample value, a fraction of full scale.
    pub dc_offset: f64,
    pub rms_min: f64,
    pub rms_max: f64,
    pub rms_avg: f64,
    pub rms_total: f64,
    /// RMS windows measured.
    pub windows: u64,
    pub clipped_runs: u64,
    pub clipped_samples: u64,
    /// First frame of each run, in order (at most [`MAX_CLIP_RUNS_KEPT`]).
    pub clip_starts: Vec<u64>,
    /// 0 for silence, 1..=24 for whole steps, 32 for float.
    pub bit_depth: u32,
}

impl ChannelStats {
    pub fn peak_db(&self) -> f64 {
        to_db(f64::from(self.peak))
    }
    pub fn dc_percent(&self) -> f64 {
        self.dc_offset * 100.0
    }
    pub fn rms_min_db(&self) -> f64 {
        to_db(self.rms_min)
    }
    pub fn rms_max_db(&self) -> f64 {
        to_db(self.rms_max)
    }
    pub fn rms_avg_db(&self) -> f64 {
        to_db(self.rms_avg)
    }
    pub fn rms_total_db(&self) -> f64 {
        to_db(self.rms_total)
    }
}

/// The streaming accumulator for one channel.
#[derive(Debug, Clone)]
pub struct ChannelScan {
    window: u64,
    first: u64,
    next: u64,
    peak: f32,
    peak_frame: u64,
    min: f32,
    min_frame: u64,
    max: f32,
    max_frame: u64,
    sum: f64,
    sum_sq: f64,
    win_sq: f64,
    win_n: u64,
    windows: u64,
    rms_min_quiet: f64,
    rms_min: f64,
    rms_max: f64,
    rms_sum: f64,
    run_start: u64,
    run_len: u64,
    run_negative: bool,
    runs: u64,
    clipped: u64,
    starts: Vec<u64>,
    bits_or: u64,
    fractional: bool,
}

impl ChannelScan {
    /// A scan whose first frame is `first_frame` of the document, with RMS windows of
    /// `window_frames` (at least 1).
    pub fn new(first_frame: u64, window_frames: u64) -> ChannelScan {
        ChannelScan {
            window: window_frames.max(1),
            first: first_frame,
            next: first_frame,
            peak: 0.0,
            peak_frame: first_frame,
            min: f32::INFINITY,
            min_frame: first_frame,
            max: f32::NEG_INFINITY,
            max_frame: first_frame,
            sum: 0.0,
            sum_sq: 0.0,
            win_sq: 0.0,
            win_n: 0,
            windows: 0,
            rms_min_quiet: f64::INFINITY,
            rms_min: f64::INFINITY,
            rms_max: 0.0,
            rms_sum: 0.0,
            run_start: 0,
            run_len: 0,
            run_negative: false,
            runs: 0,
            clipped: 0,
            starts: Vec::new(),
            bits_or: 0,
            fractional: false,
        }
    }

    fn close_run(&mut self) {
        if self.run_len >= MIN_CLIP_RUN {
            self.runs += 1;
            self.clipped += self.run_len;
            if self.starts.len() < MAX_CLIP_RUNS_KEPT {
                self.starts.push(self.run_start);
            }
        }
        self.run_len = 0;
    }

    fn close_window(&mut self) {
        let rms = (self.win_sq / self.win_n as f64).sqrt();
        self.windows += 1;
        self.rms_sum += rms;
        self.rms_min = self.rms_min.min(rms);
        if rms > 0.0 {
            self.rms_min_quiet = self.rms_min_quiet.min(rms);
        }
        self.rms_max = self.rms_max.max(rms);
        self.win_sq = 0.0;
        self.win_n = 0;
    }

    /// Take the next samples, in order. Non-finite values count as 0.
    pub fn add(&mut self, chunk: &[f32]) {
        for &raw in chunk {
            let frame = self.next;
            self.next += 1;
            let x = if raw.is_finite() { raw } else { 0.0 };
            if x.abs() > self.peak {
                self.peak = x.abs();
                self.peak_frame = frame;
            }
            if x < self.min {
                self.min = x;
                self.min_frame = frame;
            }
            if x > self.max {
                self.max = x;
                self.max_frame = frame;
            }
            let v = f64::from(x);
            self.sum += v;
            self.sum_sq += v * v;
            self.win_sq += v * v;
            self.win_n += 1;
            if self.win_n == self.window {
                self.close_window();
            }
            if x.abs() >= CLIP_LEVEL {
                let negative = x < 0.0;
                if self.run_len > 0 && self.run_negative != negative {
                    self.close_run();
                }
                if self.run_len == 0 {
                    self.run_start = frame;
                    self.run_negative = negative;
                }
                self.run_len += 1;
            } else if self.run_len > 0 {
                self.close_run();
            }
            let scaled = f64::from(x) * 8_388_608.0;
            if scaled.fract() == 0.0 {
                self.bits_or |= (scaled as i64).unsigned_abs();
            } else {
                self.fractional = true;
            }
        }
    }

    /// Frames taken so far.
    pub fn frames(&self) -> u64 {
        self.next - self.first
    }

    /// The numbers for what was added.
    pub fn finish(mut self) -> ChannelStats {
        self.close_run();
        let n = self.frames();
        // A trailing part-window counts only when it is all there is.
        if self.windows == 0 && self.win_n > 0 {
            self.close_window();
        }
        let have = n > 0;
        let bit_depth = if self.fractional {
            32
        } else if self.bits_or == 0 {
            0
        } else {
            24 - self.bits_or.trailing_zeros()
        };
        ChannelStats {
            frames: n,
            peak: self.peak,
            peak_frame: self.peak_frame,
            min: if have { self.min } else { 0.0 },
            min_frame: self.min_frame,
            max: if have { self.max } else { 0.0 },
            max_frame: self.max_frame,
            dc_offset: if have { self.sum / n as f64 } else { 0.0 },
            rms_min: if self.windows > 0 { self.rms_min } else { 0.0 },
            rms_max: self.rms_max,
            rms_avg: if self.windows > 0 { self.rms_sum / self.windows as f64 } else { 0.0 },
            rms_total: if have { (self.sum_sq / n as f64).sqrt() } else { 0.0 },
            windows: self.windows,
            clipped_runs: self.runs,
            clipped_samples: self.clipped,
            clip_starts: self.starts,
            bit_depth,
        }
    }

    fn dynamic_range_db(&self) -> Option<f64> {
        (self.rms_min_quiet.is_finite() && self.rms_max > 0.0).then(|| to_db(self.rms_max) - to_db(self.rms_min_quiet))
    }
}

/// The statistics of a range of the document, every channel.
#[derive(Debug, Clone, PartialEq)]
pub struct Stats {
    /// The frames measured.
    pub range: Range<u64>,
    pub sample_rate: u32,
    pub window_frames: u64,
    pub channels: Vec<ChannelStats>,
    /// Per channel: loudest window RMS over the quietest non-silent one, in dB.
    pub dynamic_range_db: Vec<Option<f64>>,
}

impl Stats {
    /// The channel and frame of the loudest sample of all channels (the first on a tie).
    pub fn peak_location(&self) -> Option<(usize, u64)> {
        let mut best: Option<(usize, f32, u64)> = None;
        for (i, c) in self.channels.iter().enumerate() {
            if c.frames > 0 && best.is_none_or(|(_, p, _)| c.peak > p) {
                best = Some((i, c.peak, c.peak_frame));
            }
        }
        best.map(|(i, _, f)| (i, f))
    }

    /// Every possibly clipped run's first frame, all channels, sorted, each frame once.
    pub fn clip_starts(&self) -> Vec<u64> {
        let mut all: Vec<u64> = self.channels.iter().flat_map(|c| c.clip_starts.iter().copied()).collect();
        all.sort_unstable();
        all.dedup();
        all
    }

    /// The first clipped run that starts after `frame`.
    pub fn next_clip_after(&self, frame: u64) -> Option<u64> {
        self.clip_starts().into_iter().find(|&s| s > frame)
    }

    /// The last clipped run that starts before `frame`.
    pub fn prev_clip_before(&self, frame: u64) -> Option<u64> {
        self.clip_starts().into_iter().rev().find(|&s| s < frame)
    }

    /// One line per channel for the control channel and logs.
    pub fn summary(&self) -> String {
        let mut out = format!("range={}-{} rate={} window={}", self.range.start, self.range.end, self.sample_rate, self.window_frames);
        for (i, c) in self.channels.iter().enumerate() {
            let dr = self.dynamic_range_db.get(i).copied().flatten().map_or_else(|| "-".to_string(), |d| format!("{d:.2}"));
            out += &format!(
                " | ch{i} peak={:.6} peak_db={:.2} peak_frame={} min={:.6} max={:.6} dc_pct={:.4} rms_min_db={:.2} rms_max_db={:.2} rms_avg_db={:.2} rms_total_db={:.2} dyn_db={dr} clip_runs={} clip_samples={} bits={}",
                c.peak,
                c.peak_db(),
                c.peak_frame,
                c.min,
                c.max,
                c.dc_percent(),
                c.rms_min_db(),
                c.rms_max_db(),
                c.rms_avg_db(),
                c.rms_total_db(),
                c.clipped_runs,
                c.clipped_samples,
                c.bit_depth
            );
        }
        out
    }
}

/// Measure `range` of `state` (clamped to the document) in 64 K-frame chunks. `progress` gets
/// 0..=1; `Ok(None)` when `cancel` was set. An empty range measures nothing and gives zeros.
pub fn scan(state: &DocState, range: Range<u64>, window_ms: u32, progress: &mut dyn FnMut(f64), cancel: &AtomicBool) -> io::Result<Option<Stats>> {
    let len = state.len_frames();
    let start = range.start.min(len);
    let end = range.end.clamp(start, len);
    let rate = state.sample_rate();
    let window_frames = (u64::from(rate) * u64::from(window_ms) / 1000).max(1);
    let n = state.channel_count();
    let mut scans: Vec<ChannelScan> = (0..n).map(|_| ChannelScan::new(start, window_frames)).collect();
    let mut buf = vec![0.0f32; SCAN_CHUNK_FRAMES];
    let mut at = start;
    while at < end {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let want = ((end - at) as usize).min(SCAN_CHUNK_FRAMES);
        for (ch, s) in scans.iter_mut().enumerate() {
            let got = state.read(ch, at, &mut buf[..want])?;
            if got < want {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the document ended early"));
            }
            s.add(&buf[..want]);
        }
        at += want as u64;
        progress((at - start) as f64 / (end - start) as f64);
    }
    progress(1.0);
    let dynamic_range_db = scans.iter().map(ChannelScan::dynamic_range_db).collect();
    let channels = scans.into_iter().map(ChannelScan::finish).collect();
    Ok(Some(Stats { range: start..end, sample_rate: rate, window_frames, channels, dynamic_range_db }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(data: &[f32], window: u64, chunk: usize) -> (ChannelStats, Option<f64>) {
        let mut s = ChannelScan::new(0, window);
        for c in data.chunks(chunk.max(1)) {
            s.add(c);
        }
        let dr = s.dynamic_range_db();
        (s.finish(), dr)
    }

    #[test]
    fn peak_min_max_and_where() {
        let (c, _) = run(&[0.0, 0.25, -0.5, 0.125, 0.5, -0.5], 4, 3);
        assert_eq!(c.peak, 0.5);
        assert_eq!(c.peak_frame, 2, "the first frame with the peak");
        assert_eq!((c.min, c.min_frame), (-0.5, 2));
        assert_eq!((c.max, c.max_frame), (0.5, 4));
        assert!((c.peak_db() - (-6.0206)).abs() < 1e-3);
    }

    #[test]
    fn dc_offset_is_the_mean() {
        let (c, _) = run(&[0.25; 100], 10, 7);
        assert_eq!(c.dc_offset, 0.25);
        assert_eq!(c.dc_percent(), 25.0);
        let (c, _) = run(&[0.5, -0.5, 0.5, -0.5], 2, 4);
        assert_eq!(c.dc_offset, 0.0);
    }

    #[test]
    fn rms_windows_have_exact_values() {
        // Windows of 4: silence, a constant 0.5, a constant 0.25, then a part-window (ignored).
        let mut d = vec![0.0f32; 4];
        d.extend([0.5; 4]);
        d.extend([0.25; 4]);
        d.extend([1.0; 2]);
        let (c, dr) = run(&d, 4, 5);
        assert_eq!(c.windows, 3);
        assert_eq!(c.rms_min, 0.0);
        assert_eq!(c.rms_max, 0.5);
        assert_eq!(c.rms_avg, 0.25);
        let total = ((4.0 * 0.25 + 4.0 * 0.0625 + 2.0) / 14.0f64).sqrt();
        assert!((c.rms_total - total).abs() < 1e-12);
        // Dynamic range skips the silent window: 0.5 over 0.25 is 6.02 dB.
        assert!((dr.unwrap() - 6.0206).abs() < 1e-3);
    }

    #[test]
    fn a_part_window_counts_when_it_is_all_there_is() {
        let (c, _) = run(&[0.5, 0.5, 0.5], 100, 2);
        assert_eq!((c.windows, c.rms_min, c.rms_max, c.rms_avg), (1, 0.5, 0.5, 0.5));
    }

    #[test]
    fn results_do_not_depend_on_the_chunk_size() {
        let data: Vec<f32> = (0..1000).map(|i| ((i * 37 % 200) as f32 - 100.0) / 128.0).collect();
        let whole = run(&data, 64, 1000);
        for chunk in [1, 7, 64, 65, 999] {
            assert_eq!(run(&data, 64, chunk).0, whole.0, "chunk {chunk}");
        }
    }

    #[test]
    fn clipped_runs_need_two_in_a_row_of_one_sign() {
        let m = 1.0f32;
        let d = [0.0, m, 0.0, m, m, m, 0.5, -m, -m, m, m, 0.0, m, m];
        let (c, _) = run(&d, 4, 4);
        // Runs: frames 3-5 (3 samples), 7-8 (2, negative), 9-10 (2, positive: sign change ends the run), 12-13 (2).
        assert_eq!(c.clipped_runs, 4);
        assert_eq!(c.clipped_samples, 9);
        assert_eq!(c.clip_starts, vec![3, 7, 9, 12]);
    }

    #[test]
    fn a_run_across_chunk_edges_is_one_run() {
        let mut d = vec![0.0f32; 10];
        d[4..9].fill(1.0);
        for chunk in [1, 2, 5, 10] {
            let (c, _) = run(&d, 4, chunk);
            assert_eq!((c.clipped_runs, c.clipped_samples, c.clip_starts.clone()), (1, 5, vec![4]), "chunk {chunk}");
        }
    }

    #[test]
    fn a_run_at_the_end_is_counted() {
        let (c, _) = run(&[0.0, 1.0, 1.0], 4, 2);
        assert_eq!((c.clipped_runs, c.clip_starts), (1, vec![1]));
    }

    #[test]
    fn the_first_frame_offsets_positions() {
        let mut s = ChannelScan::new(1000, 4);
        s.add(&[0.0, 1.0, 1.0, 0.5]);
        let c = s.finish();
        assert_eq!((c.peak_frame, c.clip_starts), (1001, vec![1001]));
    }

    #[test]
    fn bit_depth_is_the_smallest_step_present() {
        let q = |bits: u32, k: i32| k as f32 / (1u32 << (bits - 1)) as f32;
        assert_eq!(run(&[q(16, 3), q(16, -20000), 0.0], 2, 3).0.bit_depth, 16);
        assert_eq!(run(&[q(8, 5), q(8, -77)], 2, 2).0.bit_depth, 8);
        assert_eq!(run(&[q(24, 1), q(24, 1234567)], 2, 2).0.bit_depth, 24);
        assert_eq!(run(&[0.5, -0.25], 2, 2).0.bit_depth, 3, "steps of 0.25 need 3 bits");
        assert_eq!(run(&[0.1, 0.2], 2, 2).0.bit_depth, 32, "float");
        assert_eq!(run(&[0.0; 5], 2, 5).0.bit_depth, 0, "silence");
    }

    #[test]
    fn an_empty_scan_is_zeros() {
        let c = ChannelScan::new(0, 8).finish();
        assert_eq!((c.frames, c.peak, c.min, c.max, c.dc_offset, c.rms_total, c.windows), (0, 0.0, 0.0, 0.0, 0.0, 0.0, 0));
    }

    #[test]
    fn navigation_helpers() {
        let mk = |starts: Vec<u64>| {
            let mut s = ChannelScan::new(0, 4);
            s.add(&[0.0]);
            let mut c = s.finish();
            c.clip_starts = starts;
            c
        };
        let st = Stats {
            range: 0..100,
            sample_rate: 48000,
            window_frames: 4,
            channels: vec![mk(vec![10, 50]), mk(vec![30, 50])],
            dynamic_range_db: vec![None, None],
        };
        assert_eq!(st.clip_starts(), vec![10, 30, 50]);
        assert_eq!(st.next_clip_after(10), Some(30));
        assert_eq!(st.next_clip_after(50), None);
        assert_eq!(st.prev_clip_before(30), Some(10));
        assert_eq!(st.prev_clip_before(10), None);
    }
}
