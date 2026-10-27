//! Roadmap A152 and A153: five internal clipboards, and the system clipboard's WAV files.

use std::path::{Path, PathBuf};

use wavcraft_core::actions::{ActionId, Editor, TABLE, by_name};
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
        let dir = std::env::temp_dir().join(format!("wavcraft-clipboards-test-{}-{name}", std::process::id()));
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

fn store(dir: &Path) -> Store {
    Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap()
}

/// Left k, right -k-1, scaled so every value is exact in 24 bits.
fn stereo(frames: usize) -> Vec<Vec<f32>> {
    let v = |k: usize| (k % 1000) as f32 / 1024.0;
    vec![(0..frames).map(v).collect(), (0..frames).map(|k| -v(k) - 1.0 / 1024.0).collect()]
}

fn doc_from(s: &Store, data: &[Vec<f32>]) -> DocState {
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    DocState::new(s.clone(), chans, RATE, SourceFormat::synthetic(RATE, data.len() as u16)).unwrap()
}

fn editor(dir: &Path, data: &[Vec<f32>]) -> Editor {
    let mut e = Editor::new(History::new(doc_from(&store(dir), data), u64::MAX));
    e.crossfades = Crossfades::NONE;
    e
}

#[test]
fn a152_five_clipboards_hold_their_own_clips_and_survive_switching() {
    let t = TempDir::new("five");
    let data = stereo(5000);
    let mut e = editor(&t.0, &data);
    assert_eq!(e.current_clipboard, 0);
    // Copy a different length into each clipboard.
    for n in 0..5 {
        e.run([ActionId::Clipboard1, ActionId::Clipboard2, ActionId::Clipboard3, ActionId::Clipboard4, ActionId::Clipboard5][n], 0..0).unwrap();
        assert_eq!(e.current_clipboard, n);
        assert!(e.clipboard().is_none(), "clipboard {} starts empty", n + 1);
        let len = 100 * (n as u64 + 1);
        e.run(ActionId::Copy, 0..len).unwrap();
    }
    // Back to each: still there. Paste uses the current one.
    for n in 0..5 {
        let id = by_name(&format!("edit.clipboard.{}", n + 1)).unwrap().id;
        let out = e.run(id, 0..0).unwrap();
        assert!(!out.doc_changed);
        assert_eq!(e.clipboard().map(|c| c.len_frames()), Some(100 * (n as u64 + 1)));
    }
    e.run(ActionId::Clipboard3, 0..0).unwrap();
    let before = e.history.current().len_frames();
    let out = e.run(ActionId::Paste, 10..10).unwrap();
    assert!(out.doc_changed);
    assert_eq!(e.history.current().len_frames(), before + 300);
    // Cut goes to the current clipboard only.
    e.run(ActionId::Clipboard5, 0..0).unwrap();
    e.run(ActionId::Cut, 0..50).unwrap();
    assert_eq!(e.clipboard().map(|c| c.len_frames()), Some(50));
    e.run(ActionId::Clipboard3, 0..0).unwrap();
    assert_eq!(e.clipboard().map(|c| c.len_frames()), Some(300));
}

#[test]
fn a152_the_actions_are_in_the_table_with_ctrl_1_to_5() {
    for n in 1..=5 {
        let a = by_name(&format!("edit.clipboard.{n}")).unwrap();
        let keymap = wavcraft_core::shortcuts::Keymap::new();
        let chords = keymap.chords(wavcraft_core::shortcuts::Command::Action(a.id));
        assert_eq!(chords.map(|k| k.text(false)).collect::<Vec<_>>(), [format!("Ctrl+{n}")]);
    }
    assert_eq!(TABLE.iter().filter(|a| a.name.starts_with("edit.clipboard.")).count(), 5);
}

#[test]
fn a153_a_clip_written_as_wav_reads_back_the_same() {
    let t = TempDir::new("roundtrip");
    let data = stereo(70_000);
    let doc = doc_from(&store(&t.0.join("s")), &data);
    let clip = edit::copy(&doc, 1000..60_000, ChannelMask::ALL).unwrap();
    let wav = t.0.join("clip.wav");
    clipboard::write_clip_wav(&clip, &wav).unwrap();
    assert_eq!(std::fs::metadata(&wav).unwrap().len(), clipboard::wav_bytes(&clip));
    let opts = OpenOptions { sessions_root: Some(t.0.join("sessions")), ..OpenOptions::default() };
    let back = clipboard::clip_from_file(&wav, &opts).unwrap();
    assert_eq!((back.len_frames(), back.channel_count(), back.sample_rate()), (59_000, 2, RATE));
    for (ch, want) in data.iter().enumerate() {
        let mut got = vec![0.0f32; 59_000];
        assert_eq!(back.read(ch, 0, &mut got).unwrap(), 59_000);
        assert_eq!(got, want[1000..60_000], "channel {ch}");
    }
    assert!(clipboard::is_audio_path(&wav) && clipboard::is_audio_path(Path::new("x.FLAC")));
    assert!(!clipboard::is_audio_path(Path::new("notes.txt")));
}

#[test]
fn a153_a_clip_over_the_limit_is_refused_and_nothing_is_written() {
    let t = TempDir::new("big");
    let s = store(&t.0.join("s"));
    // Zero pieces cost no disk: 50 M frames of stereo is 300 MB as 24-bit, over the 256 MiB limit.
    let mut chans = Vec::new();
    for _ in 0..2 {
        let mut c = Channel::new();
        c.push_zeros(50_000_000);
        chans.push(c);
    }
    let doc = DocState::new(s, chans, RATE, SourceFormat::synthetic(RATE, 2)).unwrap();
    let clip = edit::copy(&doc, 0..50_000_000, ChannelMask::ALL).unwrap();
    let wav = t.0.join("big.wav");
    assert!(matches!(clipboard::write_clip_wav(&clip, &wav), Err(ClipboardError::TooLarge { .. })));
    assert!(!wav.exists());
    // A file on the clipboard over the limit is refused unread.
    let f = std::fs::File::create(&wav).unwrap();
    f.set_len(clipboard::MAX_FILE_BYTES + 1).unwrap();
    drop(f);
    let opts = OpenOptions { sessions_root: Some(t.0.join("sessions")), ..OpenOptions::default() };
    assert!(matches!(clipboard::clip_from_file(&wav, &opts), Err(ClipboardError::TooLarge { .. })));
}

/// Guard for a double add of `range.start`: a clip taken from a range that does not start at 0
/// holds exactly those frames, and pasting it elsewhere puts them there once.
#[test]
fn a152_cut_and_paste_from_a_nonzero_start_move_exactly_those_frames() {
    let t = TempDir::new("offset");
    let data = stereo(5000);
    let mut e = editor(&t.0, &data);
    e.run(ActionId::Copy, 1200..1300).unwrap();
    let clip = e.clipboard().unwrap();
    assert_eq!(clip.len_frames(), 100);
    let mut got = vec![0.0f32; 100];
    assert_eq!(clip.read(0, 0, &mut got).unwrap(), 100);
    // Frame k holds (k % 1000) / 1024: frames 1200.. hold 200/1024, 201/1024, ...
    assert_eq!((got[0], got[1], got[99]), (200.0 / 1024.0, 201.0 / 1024.0, 299.0 / 1024.0));
    // Paste at 3000 (value 0 there): the pasted audio starts at 3000 with the clip's first frame.
    let out = e.run(ActionId::Paste, 3000..3000).unwrap();
    assert_eq!(out.selection, 3000..3100);
    let mut after = vec![0.0f32; 3];
    e.history.current().read(0, 3000, &mut after).unwrap();
    assert_eq!(after, [200.0 / 1024.0, 201.0 / 1024.0, 202.0 / 1024.0]);
    e.history.current().read(0, 3100, &mut after).unwrap();
    assert_eq!(after, [0.0, 1.0 / 1024.0, 2.0 / 1024.0]);
}
