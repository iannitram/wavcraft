//! Roadmap B003 (background job runner) and B012 (float headroom kept above 0 dBFS until save).

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use wavcraft_core::doc::edit::{self, ChannelMask, Crossfades, EditError};
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::io::{OpenOptions, open};
use wavcraft_core::jobs::{Job, JobPoll};
use wavcraft_core::save::{SampleType, save_wav};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-jobs-test-{}-{name}", std::process::id()));
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

fn doc_from(dir: &Path, data: &[Vec<f32>]) -> DocState {
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    DocState::new(s.clone(), chans, RATE, SourceFormat::synthetic(RATE, data.len() as u16)).unwrap()
}

fn content(doc: &DocState) -> Vec<Vec<f32>> {
    (0..doc.channel_count())
        .map(|c| {
            let mut v = vec![f32::NAN; doc.len_frames() as usize];
            assert_eq!(doc.read(c, 0, &mut v).unwrap(), v.len());
            v
        })
        .collect()
}

fn wave(frames: usize, amp: f32) -> Vec<Vec<f32>> {
    (0..2).map(|c| (0..frames).map(|i| amp * ((i as f32) * 0.01 * (c as f32 + 1.0)).sin()).collect()).collect()
}

fn wait(job: &mut Job) -> Result<edit::Edit, EditError> {
    let t = Instant::now();
    loop {
        match job.poll() {
            JobPoll::Done(r) => return r,
            JobPoll::Running => {
                assert!(t.elapsed() < Duration::from_secs(60), "the job never finished");
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }
}

fn copy_slowly(ms: u64) -> impl FnMut(u64, &[&[f32]], &mut [&mut [f32]]) -> io::Result<()> + Send + 'static {
    move |_, input, output| {
        std::thread::sleep(Duration::from_millis(ms));
        for (a, b) in input.iter().zip(output.iter_mut()) {
            b.copy_from_slice(a);
        }
        Ok(())
    }
}

fn gain(db: f32) -> impl FnMut(u64, &[&[f32]], &mut [&mut [f32]]) -> io::Result<()> + Send + 'static {
    let g = 10f32.powf(db / 20.0);
    move |_, input, output| {
        for (i, o) in input.iter().zip(output.iter_mut()) {
            for (a, b) in i.iter().zip(o.iter_mut()) {
                *b = a * g;
            }
        }
        Ok(())
    }
}

#[test]
fn a_job_runs_off_the_calling_thread_reports_progress_and_hands_back_the_edit() {
    let t = TempDir::new("run");
    let frames = BLOCK_FRAMES * 6 + 100;
    let doc = Arc::new(doc_from(&t.0, &wave(frames, 0.5)));
    let caller = std::thread::current().id();
    let mut slow = copy_slowly(20);
    let mut job = Job::spawn_replace(doc.clone(), 0..frames as u64, ChannelMask::ALL, "Slow", Crossfades::NONE, None, move |at, i, o| {
        assert_ne!(std::thread::current().id(), caller, "the producer runs on the worker");
        slow(at, i, o)
    });
    assert!(Arc::ptr_eq(job.base(), &doc), "the job remembers the state it started from");
    assert_eq!(job.label(), "Slow");
    let mut last = 0.0;
    let mut mid = false;
    let edit = loop {
        let p = job.progress();
        assert!(p >= last && (0.0..=1.0).contains(&p), "progress {p} after {last}");
        last = p;
        mid |= p > 0.0 && p < 1.0;
        match job.poll() {
            JobPoll::Done(r) => break r.unwrap(),
            JobPoll::Running => std::thread::sleep(Duration::from_millis(2)),
        }
    };
    assert!(mid, "progress was seen between 0 and 100 %");
    assert_eq!(job.progress(), 1.0);
    assert_eq!(content(&edit.state), content(&doc), "an identity producer leaves the audio as it was");
    // The window's thread pushes the undo step.
    let mut h = History::new((*doc).clone(), 1 << 30);
    h.push_edit(edit);
    assert_eq!(h.steps().len(), 2);
    assert_eq!(h.undo_label(), Some("Slow"));
}

#[test]
fn cancel_stops_the_job_and_leaves_the_document_alone() {
    let t = TempDir::new("cancel");
    let frames = BLOCK_FRAMES * 50;
    let doc = Arc::new(doc_from(&t.0, &wave(frames, 0.5)));
    let before = content(&doc);
    let mut job = Job::spawn_replace(doc.clone(), 0..frames as u64, ChannelMask::ALL, "Slow", Crossfades::NONE, None, copy_slowly(30));
    while job.progress() == 0.0 {
        std::thread::sleep(Duration::from_millis(2));
    }
    job.cancel();
    assert!(job.is_cancelled());
    assert!(matches!(wait(&mut job), Err(EditError::Cancelled)));
    assert!(job.progress() < 1.0);
    assert_eq!(content(&doc), before);
}

#[test]
fn a_producer_error_comes_back_as_the_result() {
    let t = TempDir::new("error");
    let doc = Arc::new(doc_from(&t.0, &wave(BLOCK_FRAMES * 2, 0.5)));
    let mut job = Job::spawn_replace(doc, 0..(BLOCK_FRAMES * 2) as u64, ChannelMask::ALL, "Bad", Crossfades::NONE, None, |_, _, _| {
        Err(io::Error::other("test: the effect failed"))
    });
    assert!(matches!(wait(&mut job), Err(EditError::Io(_))));
}

#[test]
fn a_panicking_producer_is_an_error_not_a_hang() {
    let t = TempDir::new("panic");
    let doc = Arc::new(doc_from(&t.0, &wave(BLOCK_FRAMES, 0.5)));
    let mut job = Job::spawn_replace(doc, 0..BLOCK_FRAMES as u64, ChannelMask::ALL, "Boom", Crossfades::NONE, None, |_, _, _| panic!("test panic"));
    assert!(matches!(wait(&mut job), Err(EditError::Io(_))));
}

#[test]
fn the_wake_callback_runs_when_the_job_ends() {
    let t = TempDir::new("wake");
    let doc = Arc::new(doc_from(&t.0, &wave(BLOCK_FRAMES, 0.5)));
    let woke = Arc::new(AtomicU32::new(0));
    let w = woke.clone();
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        w.fetch_add(1, Ordering::Relaxed);
    });
    let mut job = Job::spawn_replace(doc, 0..BLOCK_FRAMES as u64, ChannelMask::ALL, "Gain", Crossfades::NONE, Some(wake), gain(0.0));
    wait(&mut job).unwrap();
    // The worker wakes after sending; give it a moment.
    let t0 = Instant::now();
    while woke.load(Ordering::Relaxed) == 0 && t0.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(woke.load(Ordering::Relaxed) >= 1);
}

/// B012: +6 dB then -6 dB gets the original back; the +6 dB step holds samples above full scale
/// as they are, not clipped.
#[test]
fn b012_samples_above_full_scale_are_kept_and_plus_6_then_minus_6_db_returns_the_original() {
    let t = TempDir::new("headroom");
    let frames = BLOCK_FRAMES * 2 + 5;
    let original = wave(frames, 0.9);
    let doc = Arc::new(doc_from(&t.0, &original));
    let cancel = AtomicBool::new(false);
    let up = edit::replace_range(&doc, 0..frames as u64, ChannelMask::ALL, "+6 dB", Crossfades::NONE, &cancel, gain(6.0)).unwrap();
    let peak = content(&up.state).iter().flatten().fold(0f32, |m, x| m.max(x.abs()));
    assert!(peak > 1.7, "0.9 x +6 dB is about 1.8 and is stored as it is, got {peak}");
    let up_state = Arc::new(up.state);
    let down = edit::replace_range(&up_state, 0..frames as u64, ChannelMask::ALL, "-6 dB", Crossfades::NONE, &cancel, gain(-6.0)).unwrap();
    for (a, b) in content(&down.state).iter().flatten().zip(original.iter().flatten()) {
        assert!((a - b).abs() <= 1e-6, "{a} vs {b}");
    }
}

/// Exact version: gains of 2 and 0.5 are exact in f32, so the round trip is bit-identical.
#[test]
fn b012_an_exact_gain_round_trip_is_bit_identical() {
    let t = TempDir::new("exact");
    let frames = BLOCK_FRAMES + 77;
    let original = wave(frames, 0.99);
    let doc = Arc::new(doc_from(&t.0, &original));
    let cancel = AtomicBool::new(false);
    let scale = |k: f32| {
        move |_: u64, i: &[&[f32]], o: &mut [&mut [f32]]| -> io::Result<()> {
            for (a, b) in i.iter().zip(o.iter_mut()) {
                for (x, y) in a.iter().zip(b.iter_mut()) {
                    *y = x * k;
                }
            }
            Ok(())
        }
    };
    let up = Arc::new(edit::replace_range(&doc, 0..frames as u64, ChannelMask::ALL, "x2", Crossfades::NONE, &cancel, scale(2.0)).unwrap().state);
    assert!(content(&up).iter().flatten().any(|x| x.abs() > 1.5));
    let down = edit::replace_range(&up, 0..frames as u64, ChannelMask::ALL, "x0.5", Crossfades::NONE, &cancel, scale(0.5)).unwrap();
    let a: Vec<u32> = content(&down.state).iter().flatten().map(|x| x.to_bits()).collect();
    let b: Vec<u32> = original.iter().flatten().map(|x| x.to_bits()).collect();
    assert_eq!(a, b);
}

/// B012: the audio is only limited at Save: integer formats clamp to full scale, and 32-bit
/// float keeps the value.
#[test]
fn b012_save_converts_hot_samples_to_the_target_format() {
    let t = TempDir::new("save");
    let data = vec![vec![1.5f32, -1.5, 0.5, 0.0, 2.0, -3.0]];
    let doc = doc_from(&t.0.join("s"), &data);
    for (ty, name) in [(SampleType::Int16, "a16.wav"), (SampleType::Int24, "a24.wav"), (SampleType::Float32, "f32.wav")] {
        let out = t.0.join(name);
        save_wav(&doc, &out, ty, &mut |_| {}).unwrap();
        let opts = OpenOptions { sessions_root: Some(t.0.join(format!("r-{name}"))), ..OpenOptions::default() };
        let reopened = open(&out, &opts, &mut |_| {}, &AtomicBool::new(false)).unwrap();
        let got = content(&reopened).remove(0);
        match ty {
            SampleType::Float32 => assert_eq!(got, data[0], "float keeps values above full scale"),
            _ => {
                assert!((got[0] - 1.0).abs() < 1e-3 && (got[1] + 1.0).abs() < 1e-3, "clamped to full scale at save: {got:?}");
                assert!(got.iter().all(|x| x.abs() <= 1.0));
                assert!((got[2] - 0.5).abs() < 1e-3);
            }
        }
    }
}
