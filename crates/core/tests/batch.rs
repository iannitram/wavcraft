//! f73: Batch Process. Three small synthetic files, one of them unreadable.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use wavcraft_core::batch::{Event, Job, Op, Outcome, output_name, run};
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::io::{OpenOptions, open};
use wavcraft_core::save::{SampleType, save_wav};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);
impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-batch-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write_wav(dir: &Path, name: &str, data: &[Vec<f32>]) -> PathBuf {
    let s = Store::create_with(&dir.join(format!("store-{name}")), StoreOptions { cache_bytes: 16 << 20, ..StoreOptions::default() }).unwrap();
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    let doc = DocState::new(s, chans, RATE, SourceFormat::synthetic(RATE, data.len() as u16)).unwrap();
    let path = dir.join(name);
    save_wav(&doc, &path, SampleType::Float32, &mut |_| {}).unwrap();
    path
}

fn sine(frames: usize, amp: f32, hz: f32) -> Vec<f32> {
    (0..frames).map(|i| amp * (i as f32 * hz * std::f32::consts::TAU / RATE as f32).sin()).collect()
}

fn read_back(path: &Path, sessions: &Path) -> Vec<Vec<f32>> {
    let opts = OpenOptions { sessions_root: Some(sessions.to_path_buf()), ..OpenOptions::default() };
    let doc = open(path, &opts, &mut |_| {}, &AtomicBool::new(false)).unwrap();
    (0..doc.channel_count())
        .map(|c| {
            let mut v = vec![0f32; doc.len_frames() as usize];
            assert_eq!(doc.read(c, 0, &mut v).unwrap(), v.len());
            v
        })
        .collect()
}

fn peak(c: &[Vec<f32>]) -> f32 {
    c.iter().flatten().fold(0f32, |p, x| p.max(x.abs()))
}

#[test]
fn a_batch_of_three_with_one_unreadable_file() {
    let t = TempDir::new("three");
    let (inp, out, sessions) = (t.0.join("in"), t.0.join("out"), t.0.join("sessions"));
    std::fs::create_dir_all(&inp).unwrap();
    // More than one block, so the stream crosses block edges.
    let a = write_wav(&inp, "a.wav", &[sine(70_000, 0.25, 440.0), sine(70_000, 0.1, 660.0)]);
    let bad = inp.join("b.wav");
    std::fs::write(&bad, b"this is not a wav file at all").unwrap();
    let c = write_wav(&inp, "c.wav", &[sine(20_000, 0.5, 220.0)]);
    let before: Vec<Vec<u8>> = [&a, &bad, &c].iter().map(|p| std::fs::read(p).unwrap()).collect();

    let mut job = Job::new(out.clone());
    job.inputs = vec![a.clone(), bad.clone(), c.clone()];
    job.ops = vec![Op::parse("normalize peak=-1").unwrap()];
    let opts = OpenOptions { sessions_root: Some(sessions.clone()), ..OpenOptions::default() };
    let mut events = Vec::new();
    let outcomes = run(&job, &opts, &AtomicBool::new(false), &mut |e| events.push(e));

    assert_eq!(outcomes.len(), 3);
    assert!(matches!(outcomes[0], Outcome::Ok { .. }));
    match &outcomes[1] {
        Outcome::Failed { reason } => assert!(reason.contains("cannot read"), "{reason}"),
        o => panic!("the unreadable file should fail, got {o:?}"),
    }
    assert!(matches!(outcomes[2], Outcome::Ok { .. }));

    // Outputs: the right length and channels, peak at -1 dBFS (float output).
    let target = 10f32.powf(-1.0 / 20.0);
    let oa = read_back(&out.join("a.wav"), &sessions);
    assert_eq!((oa.len(), oa[0].len()), (2, 70_000));
    assert!((peak(&oa) - target).abs() < 1e-5, "peak {}", peak(&oa));
    // Channel balance kept: the quieter channel stays 0.1/0.25 of the louder.
    let ratio = oa[1].iter().fold(0f32, |p, x| p.max(x.abs())) / oa[0].iter().fold(0f32, |p, x| p.max(x.abs()));
    assert!((ratio - 0.4).abs() < 0.01, "{ratio}");
    let oc = read_back(&out.join("c.wav"), &sessions);
    assert_eq!((oc.len(), oc[0].len()), (1, 20_000));
    assert!((peak(&oc) - target).abs() < 1e-5);
    assert!(!out.join("b.wav").exists(), "nothing is made for a failed file");

    // Inputs byte-identical; no temporary files; every session store removed.
    for (p, b) in [&a, &bad, &c].iter().zip(&before) {
        assert_eq!(&std::fs::read(p).unwrap(), b, "{}", p.display());
    }
    let names: Vec<String> = std::fs::read_dir(&out).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names.len(), 2, "{names:?}");
    assert_eq!(std::fs::read_dir(&sessions).map(|d| d.count()).unwrap_or(0), 0, "session stores left behind");

    // Progress: starts and finishes for every file, overall ends at 1, never goes backwards.
    let finished = events.iter().filter(|e| matches!(e, Event::Finished { .. })).count();
    assert_eq!(finished, 3);
    let overall: Vec<f64> = events.iter().filter_map(|e| if let Event::Progress { overall, .. } = e { Some(*overall) } else { None }).collect();
    assert!(overall.windows(2).all(|w| w[0] <= w[1] + 1e-12));
    assert!((overall.last().unwrap() - 1.0).abs() < 1e-9, "{overall:?}");
}

#[test]
fn an_output_never_replaces_an_input_or_an_existing_file_unless_overwrite_is_on() {
    let t = TempDir::new("overwrite");
    let (inp, sessions) = (t.0.join("in"), t.0.join("sessions"));
    std::fs::create_dir_all(&inp).unwrap();
    let a = write_wav(&inp, "a.wav", &[sine(5_000, 0.2, 440.0)]);
    let original = std::fs::read(&a).unwrap();
    let opts = OpenOptions { sessions_root: Some(sessions), ..OpenOptions::default() };

    // Output folder = input folder, pattern {name}: the output would be the input.
    let mut job = Job::new(inp.clone());
    job.inputs = vec![a.clone()];
    job.ops = vec![Op::Gain { db: -6.0 }];
    let o = run(&job, &opts, &AtomicBool::new(false), &mut |_| {});
    assert!(matches!(&o[0], Outcome::Failed { .. }), "{o:?}");
    assert_eq!(std::fs::read(&a).unwrap(), original);

    // Explicit option on: now it is replaced.
    job.overwrite = true;
    let o = run(&job, &opts, &AtomicBool::new(false), &mut |_| {});
    assert!(matches!(&o[0], Outcome::Ok { .. }), "{o:?}");
    assert_ne!(std::fs::read(&a).unwrap(), original);
}

#[test]
fn cancel_stops_the_run_and_leaves_no_partial_file() {
    let t = TempDir::new("cancel");
    let (inp, out, sessions) = (t.0.join("in"), t.0.join("out"), t.0.join("sessions"));
    std::fs::create_dir_all(&inp).unwrap();
    let a = write_wav(&inp, "a.wav", &[sine(300_000, 0.2, 440.0)]);
    let b = write_wav(&inp, "b.wav", &[sine(1_000, 0.2, 440.0)]);
    let mut job = Job::new(out.clone());
    job.inputs = vec![a, b];
    let cancel = AtomicBool::new(false);
    let opts = OpenOptions { sessions_root: Some(sessions), ..OpenOptions::default() };
    let o = run(&job, &opts, &cancel, &mut |e| {
        if let Event::Progress { index: 0, file, .. } = e
            && file > 0.5
        {
            cancel.store(true, Ordering::Relaxed);
        }
    });
    assert_eq!(o, vec![Outcome::Cancelled, Outcome::Cancelled]);
    let left: Vec<_> = std::fs::read_dir(&out).map(|d| d.flatten().collect()).unwrap_or_default();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn name_patterns() {
    let p = Path::new("/x/take 3.flac");
    assert_eq!(output_name("{name}", p, 0).unwrap(), "take 3.wav");
    assert_eq!(output_name("{name}_norm_{n}", p, 4).unwrap(), "take 3_norm_5.wav");
    for bad in ["", "../x", "a/b", "a\\b", ".hidden"] {
        assert!(output_name(bad, p, 0).is_err(), "{bad:?}");
    }
}

#[test]
fn ops_parse() {
    assert_eq!(Op::parse("normalize peak=-1").unwrap(), Op::NormalizePeak { target_db: -1.0 });
    assert_eq!(Op::parse("gain db=3").unwrap(), Op::Gain { db: 3.0 });
    assert_eq!(Op::parse("mono").unwrap(), Op::ToMono);
    assert!(Op::parse("normalize peak=3").is_err());
    assert!(Op::parse("normalize").is_err());
    assert!(Op::parse("echo").is_err());
}

#[test]
fn mono_then_gain_mixes_down() {
    let t = TempDir::new("mono");
    let (inp, out, sessions) = (t.0.join("in"), t.0.join("out"), t.0.join("sessions"));
    std::fs::create_dir_all(&inp).unwrap();
    let a = write_wav(&inp, "a.wav", &[vec![0.5; 3000], vec![-0.1; 3000]]);
    let mut job = Job::new(out.clone());
    job.inputs = vec![a];
    job.ops = vec![Op::ToMono, Op::Gain { db: 0.0 }];
    let opts = OpenOptions { sessions_root: Some(sessions.clone()), ..OpenOptions::default() };
    let o = run(&job, &opts, &AtomicBool::new(false), &mut |_| {});
    assert!(matches!(&o[0], Outcome::Ok { .. }), "{o:?}");
    let r = read_back(&out.join("a.wav"), &sessions);
    assert_eq!(r.len(), 1);
    assert!(r[0].iter().all(|x| (x - 0.2).abs() < 1e-6));
}
