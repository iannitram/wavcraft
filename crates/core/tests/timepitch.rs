//! Stretch and Pitch and the pitch readout (f127).

use std::path::{Path, PathBuf};

use wavcraft_core::actions::Editor;
use wavcraft_core::doc::edit::Crossfades;
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};
use wavcraft_core::timepitch;

const RATE: u32 = 48_000;

struct TempDir(PathBuf);
impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-timepitch-test-{}-{name}", std::process::id()));
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

fn editor(dir: &Path, data: &[Vec<f32>]) -> Editor {
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    let doc = DocState::new(s, chans, RATE, SourceFormat::synthetic(RATE, data.len() as u16)).unwrap();
    let mut e = Editor::new(History::new(doc, u64::MAX));
    e.crossfades = Crossfades::NONE;
    e
}

fn sine(hz: f64, secs: f64) -> Vec<f32> {
    (0..(secs * RATE as f64) as usize).map(|i| (0.5 * (2.0 * std::f64::consts::PI * hz * i as f64 / RATE as f64).sin()) as f32).collect()
}

fn read(e: &Editor, ch: usize) -> Vec<f32> {
    let d = e.history.current();
    let mut v = vec![0.0; d.len_frames() as usize];
    assert_eq!(d.read(ch, 0, &mut v).unwrap(), v.len());
    v
}

/// Frequency from positive-going zero crossings in the middle half.
fn freq(x: &[f32]) -> f64 {
    let (a, b) = (x.len() / 4, x.len() * 3 / 4);
    let n = (a..b).filter(|&i| x[i - 1] < 0.0 && x[i] >= 0.0).count();
    n as f64 * RATE as f64 / (b - a) as f64
}

#[test]
fn stretch_by_one_and_a_half_changes_length_not_pitch() {
    let dir = TempDir::new("stretch");
    let mut e = editor(&dir.0, &[sine(220.0, 2.0), sine(220.0, 2.0)]);
    let before = e.history.current().len_frames();
    let out = e.stretch_pitch(1.5, 0.0, 0..0).unwrap();
    assert!(out.doc_changed);
    let after = e.history.current().len_frames();
    assert!((after as f64 / before as f64 - 1.5).abs() < 0.005, "{before} -> {after}");
    assert_eq!(out.selection, 0..after);
    assert_eq!(e.history.current().channel_count(), 2);
    let l = read(&e, 0);
    assert!((freq(&l) - 220.0).abs() < 4.0, "{}", freq(&l));
    assert_eq!(e.history.undo_label(), Some("Stretch and Pitch"));
    e.run(wavcraft_core::actions::ActionId::Undo, 0..0).unwrap();
    assert_eq!(e.history.current().len_frames(), before);
}

#[test]
fn stretch_a_selection_moves_the_rest() {
    let dir = TempDir::new("sel");
    let mut e = editor(&dir.0, &[sine(220.0, 3.0)]);
    let before = e.history.current().len_frames();
    let r = RATE as u64;
    e.stretch_pitch(2.0, 0.0, r..2 * r).unwrap();
    assert_eq!(e.history.current().len_frames(), before + r);
}

#[test]
fn pitch_up_an_octave_keeps_length_and_doubles_frequency() {
    let dir = TempDir::new("pitch");
    let mut e = editor(&dir.0, &[sine(220.0, 2.0)]);
    let before = e.history.current().len_frames();
    e.stretch_pitch(1.0, 12.0, 0..0).unwrap();
    let after = e.history.current().len_frames();
    assert!((after as f64 - before as f64).abs() < before as f64 * 0.01, "{before} -> {after}");
    assert!((freq(&read(&e, 0)) - 440.0).abs() < 8.0, "{}", freq(&read(&e, 0)));
}

#[test]
fn bad_settings_are_refused() {
    let dir = TempDir::new("bad");
    let mut e = editor(&dir.0, &[sine(220.0, 1.0)]);
    assert!(e.stretch_pitch(0.0, 0.0, 0..0).is_err());
    assert!(e.stretch_pitch(1.0, 60.0, 0..0).is_err());
    assert_eq!(e.history.steps().len(), 1);
}

#[test]
fn yin_reads_a_220_hz_tone() {
    let dir = TempDir::new("yin");
    let e = editor(&dir.0, &[sine(220.0, 2.0), sine(220.0, 2.0)]);
    let hz = timepitch::detect_pitch(e.history.current(), 0..0).unwrap().unwrap();
    assert!((hz - 220.0).abs() < 1.0, "{hz}");
    let hz = timepitch::detect_pitch(e.history.current(), RATE as u64 / 2..RATE as u64).unwrap().unwrap();
    assert!((hz - 220.0).abs() < 1.0, "{hz}");
}

#[test]
fn yin_on_silence_reads_nothing() {
    let dir = TempDir::new("silent");
    let e = editor(&dir.0, &[vec![0.0; RATE as usize]]);
    assert_eq!(timepitch::detect_pitch(e.history.current(), 0..0).unwrap(), None);
}
