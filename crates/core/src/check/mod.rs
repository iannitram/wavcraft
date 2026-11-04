//! Episode Check detectors, part 1 (K2): clipped runs, true peak over a ceiling, DC offset and
//! dead air.
//!
//! Each scan reads its channels through [`Samples`], a chunk at a time, so an hour of audio never
//! sits in RAM, and returns [`Finding`]s in frame order. The scans are pure: they change no
//! document. The fixes that answer a finding are other tools (K4).
//!
//! - Clipping: runs of at least [`Config::min_clip_run`] samples at or above
//!   [`Config::clip_level`] in magnitude (the same level as [`crate::stats`]).
//! - True peak: 4x oversampled (windowed-sinc interpolation) positions above
//!   [`Config::true_peak_ceiling_dbtp`]. Nearby stretches within [`Config::true_peak_merge_ms`]
//!   merge into one finding.
//! - DC offset: windows of [`Config::dc_window_ms`] whose mean is at least
//!   [`Config::dc_threshold`] in magnitude, with one sign, for at least [`Config::dc_min_ms`].
//! - Dead air: stretches where every channel stays under [`Config::silence_threshold_db`] for at
//!   least [`Config::silence_min_ms`].

use std::f64::consts::PI;

use crate::spectrogram::Samples;
use crate::stats::{CLIP_LEVEL, MIN_CLIP_RUN};

/// Frames read per step of a scan.
pub const CHUNK_FRAMES: usize = 65_536;
/// Most findings one scan keeps. Past this the report is [`Report::truncated`].
pub const MAX_FINDINGS: usize = 100_000;
/// Taps of the true peak interpolator on each side of a sample.
const HALF_TAPS: usize = 8;
/// Positions per sample: 4x oversampling. Phase 0 is the sample itself, so 3 are interpolated.
const PHASES: usize = 4;

impl Samples for &[f32] {
    fn len(&self) -> usize {
        <[f32]>::len(self)
    }

    fn read(&self, start: usize, out: &mut [f32]) {
        match self.get(start..start.saturating_add(out.len())) {
            Some(src) => out.copy_from_slice(src),
            None => out.fill(0.0),
        }
    }
}

/// What a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Clipping,
    TruePeak,
    DcOffset,
    DeadAir,
}

/// How bad a finding is. Clipping and true peak are errors; DC offset and dead air are warnings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    Error,
}

/// One thing a scan found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Finding {
    pub kind: Kind,
    pub severity: Severity,
    /// The channel, or `None` when the finding covers every channel (dead air).
    pub channel: Option<usize>,
    /// First frame of the finding.
    pub start: u64,
    /// One past the last frame.
    pub end: u64,
    /// Clipping: the largest magnitude in the run (linear). True peak: the highest interpolated
    /// peak (dBTP). DC offset: the mean over the region (linear). Dead air: the loudest sample in
    /// it (dBFS).
    pub value: f64,
}

/// What a scan found.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    /// The findings, in frame order (then channel).
    pub findings: Vec<Finding>,
    /// The scan found more than [`MAX_FINDINGS`] and kept the first ones.
    pub truncated: bool,
}

/// The detector settings. [`Config::new`] gives the defaults Episode Check uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Config {
    /// Sample rate of the source, in Hz. Millisecond settings are converted with it.
    pub sample_rate: u32,
    /// Samples at or above this magnitude count as clipped.
    pub clip_level: f32,
    /// Fewest consecutive clipped samples that make a finding.
    pub min_clip_run: u64,
    /// True peak ceiling, dBTP.
    pub true_peak_ceiling_dbtp: f64,
    /// Over-ceiling stretches closer together than this (ms) merge into one finding.
    pub true_peak_merge_ms: u32,
    /// Length of each DC window, ms.
    pub dc_window_ms: u32,
    /// DC window means at least this large (linear, of full scale) are DC.
    pub dc_threshold: f64,
    /// Shortest DC stretch reported, ms.
    pub dc_min_ms: u32,
    /// Dead air is quieter than this on every channel (dBFS).
    pub silence_threshold_db: f64,
    /// Shortest dead air reported, ms.
    pub silence_min_ms: u32,
}

impl Config {
    /// The defaults: clipping at [`CLIP_LEVEL`], true peak over -1 dBTP, DC windows of 20 ms at
    /// 0.02 (-34 dBFS) held for 200 ms, and dead air under -60 dBFS for 3 s.
    pub fn new(sample_rate: u32) -> Config {
        Config {
            sample_rate,
            clip_level: CLIP_LEVEL,
            min_clip_run: MIN_CLIP_RUN,
            true_peak_ceiling_dbtp: -1.0,
            true_peak_merge_ms: 10,
            dc_window_ms: 20,
            dc_threshold: 0.02,
            dc_min_ms: 200,
            silence_threshold_db: -60.0,
            silence_min_ms: 3_000,
        }
    }

    /// `ms` milliseconds in frames.
    fn ms(&self, ms: u32) -> u64 {
        u64::from(self.sample_rate) * u64::from(ms) / 1_000
    }
}

/// Scan `channels` (all the same length; the shortest one counts) for every detector.
pub fn scan(channels: &[&dyn Samples], cfg: &Config) -> Report {
    let len = channels.iter().map(|c| c.len()).min().unwrap_or(0) as u64;
    let mut sink = Sink::default();
    let mut carry: Vec<Carry> = channels.iter().map(|_| Carry::default()).collect();
    let mut silence: Option<(u64, f32)> = None;
    let quiet = 10f32.powf((cfg.silence_threshold_db / 20.0) as f32);
    let silence_min = cfg.ms(cfg.silence_min_ms);
    let merge_gap = cfg.ms(cfg.true_peak_merge_ms);
    let ceiling = 10f32.powf((cfg.true_peak_ceiling_dbtp / 20.0) as f32);
    let kernel = kernel();
    let mut buf = vec![0f32; CHUNK_FRAMES + 2 * HALF_TAPS];
    let mut frame_peak = vec![0f32; CHUNK_FRAMES];

    let mut start = 0u64;
    while start < len {
        let n = (len - start).min(CHUNK_FRAMES as u64) as usize;
        frame_peak[..n].fill(0.0);
        for (c, src) in channels.iter().enumerate() {
            // buf[j] is the source frame start - HALF_TAPS + j; frame i of the chunk is buf[i + HALF_TAPS].
            read_padded(*src, start as i64 - HALF_TAPS as i64, &mut buf[..n + 2 * HALF_TAPS]);
            let cy = &mut carry[c];
            for i in 0..n {
                let f = start + i as u64;
                let x = buf[i + HALF_TAPS];
                frame_peak[i] = frame_peak[i].max(x.abs());

                if x.abs() >= cfg.clip_level {
                    cy.clip = Some(match cy.clip {
                        Some((s, count, top)) => (s, count + 1, top.max(x.abs())),
                        None => (f, 1, x.abs()),
                    });
                } else if let Some((s, _, top)) = cy.clip.take().filter(|&(_, count, _)| count >= cfg.min_clip_run) {
                    sink.push(clip_finding(c, s, f, top));
                }

                let peak = true_peak_at(&buf, i + HALF_TAPS, &kernel);
                if peak > ceiling {
                    match cy.tp {
                        Some((first, last, top)) if f <= last + merge_gap => {
                            cy.tp = Some((first, f, top.max(peak)));
                        }
                        previous => {
                            if let Some((first, last, top)) = previous {
                                sink.push(true_peak_finding(c, first, last + 1, top));
                            }
                            cy.tp = Some((f, f, peak));
                        }
                    }
                }
            }
        }

        for (i, &p) in frame_peak[..n].iter().enumerate() {
            let f = start + i as u64;
            if p < quiet {
                silence = Some(match silence {
                    Some((s, top)) => (s, top.max(p)),
                    None => (f, p),
                });
            } else if let Some((s, top)) = silence.take().filter(|&(s, _)| f - s >= silence_min) {
                sink.push(dead_air_finding(s, f, top));
            }
        }
        start += n as u64;
    }

    for (c, cy) in carry.iter().enumerate() {
        if let Some((s, _, top)) = cy.clip.filter(|&(_, count, _)| count >= cfg.min_clip_run) {
            sink.push(clip_finding(c, s, len, top));
        }
        if let Some((first, last, top)) = cy.tp {
            sink.push(true_peak_finding(c, first, last + 1, top));
        }
    }
    if let Some((s, top)) = silence.filter(|&(s, _)| len - s >= silence_min) {
        sink.push(dead_air_finding(s, len, top));
    }
    for (c, src) in channels.iter().enumerate() {
        dc_pass(*src, c, cfg, &mut sink);
    }

    sink.findings.sort_by_key(|f| (f.start, f.channel));
    Report { findings: sink.findings, truncated: sink.truncated }
}

/// Findings kept so far, and whether any were dropped past [`MAX_FINDINGS`].
#[derive(Default)]
struct Sink {
    findings: Vec<Finding>,
    truncated: bool,
}

impl Sink {
    fn push(&mut self, finding: Finding) {
        if self.findings.len() >= MAX_FINDINGS {
            self.truncated = true;
        } else {
            self.findings.push(finding);
        }
    }
}

/// One channel's open runs, carried from chunk to chunk: clipped (first frame, length, largest
/// magnitude) and over-ceiling (first and last over frame, highest peak).
#[derive(Default)]
struct Carry {
    clip: Option<(u64, u64, f32)>,
    tp: Option<(u64, u64, f32)>,
}

fn clip_finding(channel: usize, start: u64, end: u64, top: f32) -> Finding {
    Finding { kind: Kind::Clipping, severity: Severity::Error, channel: Some(channel), start, end, value: f64::from(top) }
}

fn true_peak_finding(channel: usize, start: u64, end: u64, top: f32) -> Finding {
    Finding { kind: Kind::TruePeak, severity: Severity::Error, channel: Some(channel), start, end, value: db(f64::from(top)) }
}

fn dead_air_finding(start: u64, end: u64, top: f32) -> Finding {
    Finding { kind: Kind::DeadAir, severity: Severity::Warning, channel: None, start, end, value: db(f64::from(top)) }
}

fn db(x: f64) -> f64 {
    if x <= 0.0 { f64::NEG_INFINITY } else { 20.0 * x.log10() }
}

/// Fill `out` with `src` from `start`, which may be negative or run past the end. Outside the
/// source the samples are silence.
fn read_padded(src: &dyn Samples, start: i64, out: &mut [f32]) {
    out.fill(0.0);
    let lo = start.max(0);
    let hi = (start + out.len() as i64).min(src.len() as i64);
    if hi > lo {
        let at = (lo - start) as usize;
        src.read(lo as usize, &mut out[at..at + (hi - lo) as usize]);
    }
}

/// Windowed-sinc filters (Hann window, unit DC gain) for the interpolated positions 1/4, 2/4 and
/// 3/4 between two samples. Row `p` holds the taps for position `(p + 1) / PHASES`, applied to
/// the samples `-(HALF_TAPS - 1)..=HALF_TAPS` around the frame.
fn kernel() -> [[f32; 2 * HALF_TAPS]; PHASES - 1] {
    let mut out = [[0f32; 2 * HALF_TAPS]; PHASES - 1];
    let h = HALF_TAPS as f64;
    for (p, row) in out.iter_mut().enumerate() {
        let frac = (p + 1) as f64 / PHASES as f64;
        let mut taps = [0f64; 2 * HALF_TAPS];
        for (j, t) in taps.iter_mut().enumerate() {
            let u = (j as f64 - (h - 1.0)) - frac;
            *t = if u.abs() < h { sinc(u) * 0.5 * (1.0 + (PI * u / h).cos()) } else { 0.0 };
        }
        let sum: f64 = taps.iter().sum();
        for (r, t) in row.iter_mut().zip(taps) {
            *r = (t / sum) as f32;
        }
    }
    out
}

fn sinc(u: f64) -> f64 {
    if u == 0.0 {
        1.0
    } else {
        let x = PI * u;
        x.sin() / x
    }
}

/// The largest magnitude among the sample at `at` (an index into `buf`) and its interpolated
/// positions. `at` must be at least `HALF_TAPS` and `HALF_TAPS` short of the end of `buf`.
fn true_peak_at(buf: &[f32], at: usize, kernel: &[[f32; 2 * HALF_TAPS]; PHASES - 1]) -> f32 {
    let mut best = buf[at].abs();
    for row in kernel {
        let y: f32 = row.iter().enumerate().map(|(j, &t)| buf[at + j + 1 - HALF_TAPS] * t).sum();
        best = best.max(y.abs());
    }
    best
}

/// An open DC stretch: frames `start..end` whose window means share one sign.
struct DcRun {
    start: u64,
    end: u64,
    sum: f64,
    frames: u64,
    positive: bool,
}

fn dc_close(run: DcRun, channel: usize, cfg: &Config, sink: &mut Sink) {
    if run.frames >= cfg.ms(cfg.dc_min_ms) {
        sink.push(Finding {
            kind: Kind::DcOffset,
            severity: Severity::Warning,
            channel: Some(channel),
            start: run.start,
            end: run.end,
            value: run.sum / run.frames as f64,
        });
    }
}

fn dc_pass(src: &dyn Samples, channel: usize, cfg: &Config, sink: &mut Sink) {
    let len = src.len() as u64;
    let window = cfg.ms(cfg.dc_window_ms).max(1);
    let mut buf = vec![0f32; window as usize];
    let mut run: Option<DcRun> = None;
    let mut pos = 0u64;
    while pos < len {
        let n = window.min(len - pos) as usize;
        src.read(pos as usize, &mut buf[..n]);
        let sum: f64 = buf[..n].iter().map(|&x| f64::from(x)).sum();
        let mean = sum / n as f64;
        let end = pos + n as u64;
        if mean.abs() >= cfg.dc_threshold {
            let positive = mean > 0.0;
            if let Some(r) = run.as_mut().filter(|r| r.positive == positive) {
                r.end = end;
                r.sum += sum;
                r.frames += n as u64;
            } else {
                if let Some(r) = run.take() {
                    dc_close(r, channel, cfg, sink);
                }
                run = Some(DcRun { start: pos, end, sum, frames: n as u64, positive });
            }
        } else if let Some(r) = run.take() {
            dc_close(r, channel, cfg, sink);
        }
        pos = end;
    }
    if let Some(r) = run.take() {
        dc_close(r, channel, cfg, sink);
    }
}
