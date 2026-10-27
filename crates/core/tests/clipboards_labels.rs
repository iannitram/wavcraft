//! Roadmap A152/A153, the parts the first suite leaves out: menu labels, Repeat Last Command,
//! single-channel clips, and refusing an empty clip.

use std::path::PathBuf;

use wavcraft_core::actions::{ActionId, Editor};
use wavcraft_core::clipboard::{self, ClipboardError};
use wavcraft_core::doc::edit::{self, ChannelMask, Crossfades};
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::io::OpenOptions;
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);
impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-clipboards-labels-{}-{name}", std::process::id()));
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

fn doc(dir: &std::path::Path, frames: usize) -> DocState {
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let data: Vec<Vec<f32>> =
        vec![(0..frames).map(|k| (k % 1000) as f32 / 1024.0).collect(), (0..frames).map(|k| -((k % 1000) as f32) / 1024.0).collect()];
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    DocState::new(s, chans, RATE, SourceFormat::synthetic(RATE, 2)).unwrap()
}

fn editor(dir: &std::path::Path) -> Editor {
    let mut e = Editor::new(History::new(doc(dir, 3000), u64::MAX));
    e.crossfades = Crossfades::NONE;
    e
}

#[test]
fn the_menu_marks_the_current_clipboard_and_the_ones_holding_audio() {
    let t = TempDir::new("labels");
    let mut e = editor(&t.0);
    assert!(e.label(ActionId::Clipboard1).starts_with('\u{2713}'));
    assert!(!e.label(ActionId::Clipboard2).contains('\u{2713}'));
    assert!(!e.label(ActionId::Clipboard1).contains("has audio"));
    e.run(ActionId::Copy, 0..10).unwrap();
    assert!(e.label(ActionId::Clipboard1).contains("has audio"));
    assert!(!e.label(ActionId::Clipboard2).contains("has audio"));
    e.run(ActionId::Clipboard2, 0..0).unwrap();
    assert!(e.label(ActionId::Clipboard2).starts_with('\u{2713}'));
    assert!(!e.label(ActionId::Clipboard1).starts_with('\u{2713}'));
    assert!(e.label(ActionId::Clipboard1).contains("has audio"), "clipboard 1 keeps its clip");
}

#[test]
fn paste_is_disabled_on_an_empty_current_clipboard_and_switching_is_always_enabled() {
    let t = TempDir::new("enabled");
    let mut e = editor(&t.0);
    e.run(ActionId::Copy, 0..10).unwrap();
    assert!(e.enabled(ActionId::Paste, &(0..0)));
    e.run(ActionId::Clipboard4, 0..0).unwrap();
    assert!(!e.enabled(ActionId::Paste, &(0..0)), "clipboard 4 is empty");
    for id in [ActionId::Clipboard1, ActionId::Clipboard2, ActionId::Clipboard3, ActionId::Clipboard4, ActionId::Clipboard5] {
        assert!(e.enabled(id, &(0..0)));
    }
}

#[test]
fn switching_clipboards_does_not_become_the_command_repeat_last_command_runs() {
    let t = TempDir::new("repeat");
    let mut e = editor(&t.0);
    e.run(ActionId::Silence, 100..200).unwrap();
    let before = e.last.clone();
    assert!(before.is_some());
    e.run(ActionId::Clipboard2, 0..0).unwrap();
    assert_eq!(e.last, before);
}

#[test]
fn a_one_channel_clip_goes_out_as_a_mono_wav_and_comes_back() {
    let t = TempDir::new("mono");
    let d = doc(&t.0.join("s"), 2000);
    let clip = edit::copy(&d, 100..1100, ChannelMask::only(1)).unwrap();
    assert_eq!(clip.channel_count(), 1);
    let wav = t.0.join("mono.wav");
    clipboard::write_clip_wav(&clip, &wav).unwrap();
    let opts = OpenOptions { sessions_root: Some(t.0.join("sessions")), ..OpenOptions::default() };
    let back = clipboard::clip_from_file(&wav, &opts).unwrap();
    assert_eq!((back.len_frames(), back.channel_count()), (1000, 1));
    let mut got = vec![0.0f32; 3];
    back.read(0, 0, &mut got).unwrap();
    assert_eq!(got, [-100.0 / 1024.0, -101.0 / 1024.0, -102.0 / 1024.0]);
}

#[test]
fn the_size_is_predicted_exactly_and_bad_files_are_errors_not_panics() {
    let t = TempDir::new("edge");
    let d = doc(&t.0.join("s"), 1000);
    let clip = edit::copy(&d, 0..1000, ChannelMask::ALL).unwrap();
    assert_eq!(clipboard::wav_bytes(&clip), 44 + 1000 * 2 * 3);
    // An unreadable path is an Io error, not a panic or a TooLarge.
    let opts = OpenOptions { sessions_root: Some(t.0.join("sessions")), ..OpenOptions::default() };
    assert!(matches!(clipboard::clip_from_file(&t.0.join("missing.wav"), &opts), Err(ClipboardError::Io(_))));
    // A file that is not audio is an Open error.
    let junk = t.0.join("junk.wav");
    std::fs::write(&junk, b"not audio at all").unwrap();
    assert!(matches!(clipboard::clip_from_file(&junk, &opts), Err(ClipboardError::Open(_))));
}
