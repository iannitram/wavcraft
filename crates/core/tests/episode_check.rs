//! K2: Episode Check detectors (clipping, true peak, DC offset, dead air, loudness drift, mains
//! hum, plosives), on synthetic audio. The speech-like signal is generated here (it is not a recording). A copy with four
//! planted faults must report each one within 50 ms of where it was planted, and the clean copy
//! must report nothing. Runs that cross the 64 K-frame chunk joins must come out whole.
//!
//! K1: `scan_with` stops when cancelled and reports progress from 0 to 1.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

use wavcraft_core::check::{self, Config, Finding, Kind, Report, Severity};
use wavcraft_core::spectrogram::Samples;

const RATE: u32 = 48_000;
const SECONDS: f64 = 20.0;
const TOLERANCE: u64 = 2_400; // 50 ms

fn frame(seconds: f64) -> u64 {
    (seconds * f64::from(RATE)).round() as u64
}

/// A speech-like channel: a harmonic voice in 1.5 s phrases with 0.5 s gaps. The gaps hold a
/// noise floor of about -50 dBFS (peaks at 0.003), so no stretch of the clean signal is quiet
/// enough, for long enough, to be dead air. Voice peaks stay near 0.35.
fn speechlike(seed: u32, f0: f64) -> Vec<f32> {
    speechlike_for(seed, f0, SECONDS, 0.5)
}

/// [`speechlike`] for `seconds` seconds, with `gap` seconds of each 2 s period left to the noise
/// floor, and the envelope swings 20 % instead of 50 %. Gaps and deep dips pull the gated integrated
/// loudness above the short-term level of the voice, so the loudness drift tests use none.
fn speechlike_for(seed: u32, f0: f64, seconds: f64, gap: f64) -> Vec<f32> {
    let mut lcg = seed;
    let total = frame(seconds) as usize;
    (0..total)
        .map(|n| {
            lcg = lcg.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (f64::from(lcg >> 8) / f64::from(1u32 << 24) * 2.0 - 1.0) * 0.003;
            let t = n as f64 / f64::from(RATE);
            let phrase = t % 2.0;
            if phrase >= 2.0 - gap {
                return noise as f32;
            }
            let swing = if gap == 0.0 { 0.2 } else { 0.5 };
            let envelope = (1.0 - swing) - swing * (2.0 * PI_F * phrase / (2.0 - gap)).cos();
            // The pitch swings 15 Hz around f0 at 0.7 Hz; this is the phase that gives that pitch.
            let phase = 2.0 * PI_F * f0 * t - 15.0 / 0.7 * (2.0 * PI_F * 0.7 * t).cos();
            let voice: f64 = (1..=8).map(|k| (f64::from(k) * phase).sin() / f64::from(k)).sum();
            (envelope * 0.12 * voice + noise) as f32
        })
        .collect()
}

const PI_F: f64 = std::f64::consts::PI;

/// The clean two-channel test signal.
fn clean() -> (Vec<f32>, Vec<f32>) {
    (speechlike(1, 120.0), speechlike(2, 150.0))
}

fn scan(ch0: &[f32], ch1: &[f32]) -> Report {
    let cfg = Config::new(RATE);
    let (a, b): (&[f32], &[f32]) = (ch0, ch1);
    let sources: [&dyn Samples; 2] = [&a, &b];
    check::scan(&sources, &cfg)
}

fn assert_near(what: &str, got: u64, want: u64) {
    let diff = got.abs_diff(want);
    assert!(diff <= TOLERANCE, "{what}: reported {got}, planted {want} (off by {diff} frames, limit {TOLERANCE})");
}

#[test]
fn clean_speech_reports_nothing() {
    let (ch0, ch1) = clean();
    let report = scan(&ch0, &ch1);
    assert!(!report.truncated);
    assert!(report.findings.is_empty(), "clean speech reported {:?}", report.findings);
}

/// The clean signal with the four planted faults of [`each_planted_fault_is_reported_within_50_ms`]:
/// clipping at 3.0 s, true peak at 7.0 s, DC at 12.0 s, dead air at 16.0 s. Also returns their
/// ranges in that order.
fn planted() -> (Vec<f32>, Vec<f32>, [(u64, u64); 4]) {
    let (mut ch0, mut ch1) = clean();

    // Clipping on channel 0: 20 ms of full scale at 3.0 s.
    let clip = (frame(3.0), frame(3.02));
    ch0[clip.0 as usize..clip.1 as usize].fill(1.0);

    // True peak on channel 1: 1 s of a quarter-rate sine at 0.95 with a 45 degree phase.
    // Its samples peak at 0.67, below the sample-peak clip level, but its true peak is 0.95,
    // over -1 dBTP (0.891).
    let tp = (frame(7.0), frame(8.0));
    for n in tp.0..tp.1 {
        let phase = std::f64::consts::FRAC_PI_4 + (n as f64) * std::f64::consts::FRAC_PI_2;
        ch1[n as usize] = (0.95 * phase.sin()) as f32;
    }

    // DC offset on channel 0: +0.1 for 2 s from 12.0 s.
    let dc = (frame(12.0), frame(14.0));
    for x in &mut ch0[dc.0 as usize..dc.1 as usize] {
        *x += 0.1;
    }

    // Dead air on both channels: 3.5 s of digital silence from 16.0 s.
    let dead = (frame(16.0), frame(19.5));
    ch0[dead.0 as usize..dead.1 as usize].fill(0.0);
    ch1[dead.0 as usize..dead.1 as usize].fill(0.0);
    (ch0, ch1, [clip, tp, dc, dead])
}

#[test]
fn each_planted_fault_is_reported_within_50_ms() {
    let (ch0, ch1, [clip, tp, dc, dead]) = planted();
    let report = scan(&ch0, &ch1);
    assert!(!report.truncated);

    // A clipped run is also over the true peak ceiling, so channel 0 gets a true peak finding
    // over the clip as well. That overlap is expected; the detectors stay independent.
    let overlaps_clip = report.findings.iter().any(|x| x.kind == Kind::TruePeak && x.channel == Some(0) && x.start < clip.1 && x.end > clip.0);
    assert!(overlaps_clip, "the clipped run should also be over the true peak ceiling");

    let f: Vec<Finding> = report.findings.iter().filter(|x| !(x.kind == Kind::TruePeak && x.channel == Some(0))).copied().collect();
    assert_eq!(f.len(), 4, "expected one finding per planted fault, got {f:?}");

    let by_kind = |k: Kind| -> Finding {
        let hits: Vec<&Finding> = f.iter().filter(|x| x.kind == k).collect();
        assert_eq!(hits.len(), 1, "{k:?}: {hits:?}");
        *hits[0]
    };

    let c = by_kind(Kind::Clipping);
    assert_eq!((c.channel, c.severity), (Some(0), Severity::Error));
    assert_near("clipping start", c.start, clip.0);
    assert_near("clipping end", c.end, clip.1);

    let t = by_kind(Kind::TruePeak);
    assert_eq!((t.channel, t.severity), (Some(1), Severity::Error));
    assert_near("true peak start", t.start, tp.0);
    assert_near("true peak end", t.end, tp.1);
    assert!(t.value > -1.0, "true peak value {} dBTP should be over the ceiling", t.value);

    let d = by_kind(Kind::DcOffset);
    assert_eq!((d.channel, d.severity), (Some(0), Severity::Warning));
    assert_near("DC start", d.start, dc.0);
    assert_near("DC end", d.end, dc.1);
    assert!((d.value - 0.1).abs() < 0.02, "DC mean {} should be near 0.1", d.value);

    let a = by_kind(Kind::DeadAir);
    assert_eq!((a.channel, a.severity), (None, Severity::Warning));
    assert_near("dead air start", a.start, dead.0);
    assert_near("dead air end", a.end, dead.1);
}

#[test]
fn runs_crossing_chunk_joins_come_out_whole() {
    // 5 s of digital silence on channel 1, and on channel 0 a 16-sample full-scale run that
    // straddles the join at frame 65,536 (the scan's first chunk boundary).
    let total = frame(5.0) as usize;
    let mut ch0 = vec![0f32; total];
    let ch1 = vec![0f32; total];
    let clip = (65_530usize, 65_546usize);
    ch0[clip.0..clip.1].fill(1.0);

    let report = scan(&ch0, &ch1);
    assert!(!report.truncated);

    let clipping: Vec<&Finding> = report.findings.iter().filter(|f| f.kind == Kind::Clipping).collect();
    assert_eq!(clipping.len(), 1, "{clipping:?}");
    assert_eq!((clipping[0].start, clipping[0].end, clipping[0].channel), (clip.0 as u64, clip.1 as u64, Some(0)));

    // The silence runs from the end of the clip to the end of the file, across two more joins.
    let dead: Vec<&Finding> = report.findings.iter().filter(|f| f.kind == Kind::DeadAir).collect();
    assert_eq!(dead.len(), 1, "{dead:?}");
    assert_eq!((dead[0].start, dead[0].end), (clip.1 as u64, total as u64));

    // Nothing else: the 1.0 samples are over the true peak ceiling too, but in one stretch.
    for f in &report.findings {
        assert!(matches!(f.kind, Kind::Clipping | Kind::TruePeak | Kind::DeadAir), "unexpected {f:?}");
    }
    let tp: Vec<&Finding> = report.findings.iter().filter(|f| f.kind == Kind::TruePeak).collect();
    assert!(tp.len() <= 1, "true peak split into {tp:?}");
}

const LONG: f64 = 240.0;

fn long_clean() -> (Vec<f32>, Vec<f32>) {
    (speechlike_for(1, 120.0, LONG, 0.0), speechlike_for(2, 150.0, LONG, 0.0))
}

fn of_kind(report: &Report, kind: Kind) -> Vec<Finding> {
    report.findings.iter().filter(|f| f.kind == kind).copied().collect()
}

/// Add mains hum (the fundamental and harmonics 2x to 5x) to `ch` from `from` to `to` seconds.
fn add_hum(ch: &mut [f32], hz: f64, from: f64, to: f64) {
    let first = frame(from) as usize;
    for (i, x) in ch[first..frame(to) as usize].iter_mut().enumerate() {
        let t = (first + i) as f64 / f64::from(RATE);
        let hum: f64 = (1..=5).map(|k| (if k == 1 { 0.02 } else { 0.01 }) * (2.0 * PI_F * hz * f64::from(k) * t).sin()).sum();
        *x += hum as f32;
    }
}

#[test]
fn loudness_drift_is_reported_at_the_planted_start() {
    // 12 s at +9 LU (x2.82) in a 240 s file (a shorter one would raise the integrated loudness too
    // close to the loud section for it to be 6 LU over it), on both channels.
    let (mut ch0, mut ch1) = long_clean();
    let section = (frame(101.0), frame(113.0));
    for ch in [&mut ch0, &mut ch1] {
        for x in &mut ch[section.0 as usize..section.1 as usize] {
            *x *= 2.818_383;
        }
    }
    let report = scan(&ch0, &ch1);
    let drift = of_kind(&report, Kind::LoudnessDrift);
    assert_eq!(drift.len(), 1, "{drift:?}");
    assert_eq!((drift[0].channel, drift[0].severity), (None, Severity::Warning));
    assert_near("drift start", drift[0].start, section.0);
    assert_near("drift end", drift[0].end, section.1);
    assert!(drift[0].value > 6.0 && drift[0].value < 10.0, "deviation {} LU", drift[0].value);
}

#[test]
fn a_quiet_stretch_reports_negative_drift() {
    // 16 s at -9 LU (x0.3548) in a 240 s file, on both channels. Only the 3 s readings at least 6 LU
    // under the file count, which is about 2 s less than the section; 16 s leaves the 10 s minimum
    // clear. A 12 s section falls just short of it.
    let (mut ch0, mut ch1) = long_clean();
    let section = (frame(101.0), frame(117.0));
    for ch in [&mut ch0, &mut ch1] {
        for x in &mut ch[section.0 as usize..section.1 as usize] {
            *x *= 0.354_813;
        }
    }
    let report = scan(&ch0, &ch1);
    let drift = of_kind(&report, Kind::LoudnessDrift);
    assert_eq!(drift.len(), 1, "{drift:?}");
    assert!(drift[0].value > -10.0 && drift[0].value < -6.0, "deviation {} LU", drift[0].value);
}

#[test]
fn mains_hum_is_reported_with_its_frequency() {
    for hz in [60.0, 50.0] {
        let (mut ch0, ch1) = clean();
        let region = (frame(5.0), frame(15.0));
        add_hum(&mut ch0, hz, 5.0, 15.0);
        let report = scan(&ch0, &ch1);
        let hum = of_kind(&report, Kind::MainsHum);
        assert_eq!(hum.len(), 1, "{hz} Hz: {hum:?}");
        assert_eq!((hum[0].channel, hum[0].severity, hum[0].value), (Some(0), Severity::Warning, hz));
        assert_near(&format!("{hz} Hz hum start"), hum[0].start, region.0);
        assert_near(&format!("{hz} Hz hum end"), hum[0].end, region.1);
    }
}

#[test]
fn a_40_hz_burst_in_a_gap_is_a_plosive() {
    let (mut ch0, ch1) = clean();
    let at = frame(5.55);
    for n in 0..frame(0.08) as usize {
        let t = n as f64 / f64::from(RATE);
        ch0[at as usize + n] += (0.3 * (-t / 0.025).exp() * (2.0 * PI_F * 40.0 * t).sin()) as f32;
    }
    let report = scan(&ch0, &ch1);
    let plosive = of_kind(&report, Kind::Plosive);
    assert_eq!(plosive.len(), 1, "{plosive:?}");
    assert_eq!((plosive[0].channel, plosive[0].severity), (Some(0), Severity::Warning));
    assert_near("plosive start", plosive[0].start, at);
    assert_near("plosive end", plosive[0].end, at + 3_840);
}

#[test]
fn clean_long_speech_reports_none_of_the_new_kinds() {
    let (ch0, ch1) = long_clean();
    let report = scan(&ch0, &ch1);
    for kind in [Kind::LoudnessDrift, Kind::MainsHum, Kind::Plosive] {
        assert!(of_kind(&report, kind).is_empty(), "{kind:?}: {:?}", of_kind(&report, kind));
    }
}

/// `scan_with` over two slices, with every progress value it reports.
fn scan_with(ch0: &[f32], ch1: &[f32], cancel: &AtomicBool, mut on_progress: impl FnMut(f64)) -> Option<Report> {
    let cfg = Config::new(RATE);
    let (a, b): (&[f32], &[f32]) = (ch0, ch1);
    let sources: [&dyn Samples; 2] = [&a, &b];
    check::scan_with(&sources, &cfg, cancel, &mut on_progress)
}

#[test]
fn a_cancelled_scan_returns_none() {
    let (ch0, ch1, _) = planted();
    let cancel = AtomicBool::new(true);
    let calls = Cell::new(0u32);
    let report = scan_with(&ch0, &ch1, &cancel, |_| calls.set(calls.get() + 1));
    assert!(report.is_none(), "a scan cancelled before it starts gives no report");
    assert_eq!(calls.get(), 0, "a scan cancelled before it starts reports no progress");
}

#[test]
fn progress_rises_to_one() {
    let (ch0, ch1, _) = planted();
    let mut seen: Vec<f64> = Vec::new();
    let report = scan_with(&ch0, &ch1, &AtomicBool::new(false), |p| seen.push(p));
    assert!(report.is_some());
    assert!(!seen.is_empty(), "no progress reported");
    for w in seen.windows(2) {
        assert!(w[1] >= w[0], "progress went down: {} then {}", w[0], w[1]);
    }
    assert!(seen.iter().all(|p| (0.0..=1.0).contains(p)), "progress outside 0..=1");
    assert_eq!(seen.last().copied(), Some(1.0));
}

#[test]
fn scan_with_matches_scan() {
    let (ch0, ch1, _) = planted();
    let plain = scan(&ch0, &ch1);
    let with = scan_with(&ch0, &ch1, &AtomicBool::new(false), |_| {});
    assert_eq!(with, Some(plain));
}

#[test]
fn cancel_after_the_main_loop_stops_the_passes() {
    // The callback sets cancel at the first progress above 0.5 (the passes after the main loop);
    // the scan must stop with at most one more progress call.
    let (ch0, ch1) = long_clean();
    let cancel = AtomicBool::new(false);
    let mut after = None::<u32>;
    let report = scan_with(&ch0, &ch1, &cancel, |p| match after.as_mut() {
        Some(n) => *n += 1,
        None if p > 0.5 => {
            cancel.store(true, Ordering::Relaxed);
            after = Some(0);
        }
        None => {}
    });
    assert!(report.is_none(), "a scan cancelled in the passes gives no report");
    let after = after.unwrap_or_else(|| panic!("no progress above 0.5 was reported"));
    assert!(after <= 1, "{after} progress calls after cancel");
}
