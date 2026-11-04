//! K2: Episode Check detectors, part 1 (clipping, true peak, DC offset, dead air), on synthetic
//! audio. The speech-like signal is generated here (it is not a recording). A copy with four
//! planted faults must report each one within 50 ms of where it was planted, and the clean copy
//! must report nothing. Runs that cross the 64 K-frame chunk joins must come out whole.

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
    let mut lcg = seed;
    let total = frame(SECONDS) as usize;
    (0..total)
        .map(|n| {
            lcg = lcg.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (f64::from(lcg >> 8) / f64::from(1u32 << 24) * 2.0 - 1.0) * 0.003;
            let t = n as f64 / f64::from(RATE);
            let phrase = t % 2.0;
            if phrase >= 1.5 {
                return noise as f32;
            }
            let envelope = 0.5 - 0.5 * (2.0 * PI_F * phrase / 1.5).cos();
            let pitch = f0 + 15.0 * (2.0 * PI_F * 0.7 * t).sin();
            let voice: f64 = (1..=8).map(|k| (2.0 * PI_F * f64::from(k) * pitch * t).sin() / f64::from(k)).sum();
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

#[test]
fn each_planted_fault_is_reported_within_50_ms() {
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
