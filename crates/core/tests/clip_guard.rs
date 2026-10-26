//! B026: Amplify's clip guard and the headroom calculation ("Calculate Now"), on a document longer
//! than one 64 K-frame chunk.

use std::path::{Path, PathBuf};

use wavcraft_core::doc::edit::{ChannelMask, Crossfades};
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::effects::{amplify_guarded, headroom_db};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;
const LEN: usize = 150_000;

/// A folder in the system's temp directory, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-clipguard-test-{}-{name}", std::process::id()));
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

/// Left: 0.25 everywhere, one 0.5 peak at frame 100 000. Right: -0.125 everywhere, one -0.4 at frame 5.
fn data() -> Vec<Vec<f32>> {
    let mut l = vec![0.25f32; LEN];
    l[100_000] = 0.5;
    let mut r = vec![-0.125f32; LEN];
    r[5] = -0.4;
    vec![l, r]
}

fn near(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

fn peak_of(c: &[Vec<f32>]) -> f32 {
    c.iter().flatten().fold(0.0f32, |p, v| p.max(v.abs()))
}

#[test]
fn headroom_is_the_gain_that_reaches_the_ceiling() {
    let t = TempDir::new("head");
    let doc = doc_from(&t.0, &data());
    let whole = 0..LEN as u64;
    // Peak 0.5: 6.0206 dB to full scale; 20 * log10(2).
    let h = headroom_db(&doc, whole.clone(), ChannelMask::ALL, 0.0).unwrap().unwrap();
    assert!(near(h, 6.0206, 1e-3), "{h}");
    // A ceiling of -6 dBFS leaves 0.0206 dB.
    let h = headroom_db(&doc, whole.clone(), ChannelMask::ALL, -6.0).unwrap().unwrap();
    assert!(near(h, 0.0206, 1e-3), "{h}");
    // A range already past the ceiling: -12 dBFS is 5.9794 dB below the 0.5 peak.
    let h = headroom_db(&doc, whole, ChannelMask::ALL, -12.0).unwrap().unwrap();
    assert!(near(h, -5.9794, 1e-3), "{h}");
}

#[test]
fn headroom_looks_only_at_the_range_and_the_active_channels() {
    let t = TempDir::new("scope");
    let doc = doc_from(&t.0, &data());
    // Frames 0..50 000 miss the left peak: the right channel's 0.4 is the loudest, 7.9588 dB.
    let h = headroom_db(&doc, 0..50_000, ChannelMask::ALL, 0.0).unwrap().unwrap();
    assert!(near(h, 7.9588, 1e-3), "{h}");
    // Right channel only over the whole file: also 0.4.
    let h = headroom_db(&doc, 0..LEN as u64, ChannelMask::only(1), 0.0).unwrap().unwrap();
    assert!(near(h, 7.9588, 1e-3), "{h}");
    // Frames 10..50 000 on the right channel miss the 0.4 sample: peak 0.125 gives 18.0618 dB.
    let h = headroom_db(&doc, 10..50_000, ChannelMask::only(1), 0.0).unwrap().unwrap();
    assert!(near(h, 18.0618, 1e-3), "{h}");
}

#[test]
fn headroom_of_silence_is_none() {
    let t = TempDir::new("silence");
    let doc = doc_from(&t.0, &[vec![0.0f32; 1000]]);
    assert_eq!(headroom_db(&doc, 0..1000, ChannelMask::ALL, 0.0).unwrap(), None);
}

#[test]
fn guarded_amplify_passes_a_safe_gain_through() {
    let t = TempDir::new("safe");
    let d = data();
    let doc = doc_from(&t.0, &d);
    let (edit, applied) = amplify_guarded(&doc, 0..LEN as u64, ChannelMask::ALL, 3.0, 0.0, Crossfades::NONE).unwrap();
    assert_eq!(applied, 3.0);
    assert_eq!(edit.label, "Amplify");
    let c = content(&edit.state);
    // 10^(3/20) = 1.4125375.
    assert!(near(c[0][100_000], 0.70627, 1e-4));
    assert!(near(c[0][0], 0.35313, 1e-4));
    assert!(near(c[1][5], -0.56502, 1e-4));
    // A cut is never limited.
    let (edit, applied) = amplify_guarded(&doc, 0..LEN as u64, ChannelMask::ALL, -6.0, -20.0, Crossfades::NONE).unwrap();
    assert_eq!(applied, -6.0);
    let c = content(&edit.state);
    assert!(near(c[0][100_000], 0.25059, 1e-4));
}

#[test]
fn guarded_amplify_stops_at_the_ceiling() {
    let t = TempDir::new("clip");
    let doc = doc_from(&t.0, &data());
    let (edit, applied) = amplify_guarded(&doc, 0..LEN as u64, ChannelMask::ALL, 12.0, 0.0, Crossfades::NONE).unwrap();
    assert!(near(applied, 6.0206, 1e-3), "{applied}");
    let c = content(&edit.state);
    assert!(near(c[0][100_000], 1.0, 1e-4));
    assert!(peak_of(&c) <= 1.0 + 1e-4, "{}", peak_of(&c));
    // The right channel is scaled by the same gain (x2.0): -0.4 becomes -0.8.
    assert!(near(c[1][5], -0.8, 1e-4));
    // A ceiling of -6 dBFS lets only 0.0206 dB through: the 0.5 peak ends at 0.50119.
    let (edit, applied) = amplify_guarded(&doc, 0..LEN as u64, ChannelMask::ALL, 12.0, -6.0, Crossfades::NONE).unwrap();
    assert!(near(applied, 0.0206, 1e-3), "{applied}");
    assert!(near(content(&edit.state)[0][100_000], 0.50119, 1e-4));
}

#[test]
fn guarded_amplify_never_boosts_or_cuts_a_range_past_the_ceiling() {
    let t = TempDir::new("past");
    let d = data();
    let doc = doc_from(&t.0, &d);
    // The 0.5 peak is already above -12 dBFS: a +3 dB request becomes 0 dB, not a cut.
    let (edit, applied) = amplify_guarded(&doc, 0..LEN as u64, ChannelMask::ALL, 3.0, -12.0, Crossfades::NONE).unwrap();
    assert_eq!(applied, 0.0);
    assert_eq!(content(&edit.state), d);
}

#[test]
fn guarded_amplify_uses_the_active_channels_and_ignores_silence() {
    let t = TempDir::new("mask");
    let d = data();
    let doc = doc_from(&t.0, &d);
    // Right channel only: its 0.4 peak allows 7.9588 dB, so +12 dB is cut to that; left is untouched.
    let (edit, applied) = amplify_guarded(&doc, 0..LEN as u64, ChannelMask::only(1), 12.0, 0.0, Crossfades::NONE).unwrap();
    assert!(near(applied, 7.9588, 1e-3), "{applied}");
    let c = content(&edit.state);
    assert!(near(c[1][5], -1.0, 1e-4));
    assert_eq!(c[0], d[0]);
    // Silence has no peak to protect: the full request is applied.
    let t2 = TempDir::new("mask-silence");
    let quiet = doc_from(&t2.0, &[vec![0.0f32; 1000]]);
    let (_edit, applied) = amplify_guarded(&quiet, 0..1000, ChannelMask::ALL, 12.0, 0.0, Crossfades::NONE).unwrap();
    assert_eq!(applied, 12.0);
}

#[test]
fn guarded_amplify_measures_only_its_own_range() {
    let t = TempDir::new("range");
    let doc = doc_from(&t.0, &data());
    // Frames 0..50 000 miss the left peak: the right channel's 0.4 allows 7.9588 dB, not the 6.0206 dB of the whole file.
    let (edit, applied) = amplify_guarded(&doc, 0..50_000, ChannelMask::ALL, 12.0, 0.0, Crossfades::NONE).unwrap();
    assert!(near(applied, 7.9588, 1e-3), "{applied}");
    let c = content(&edit.state);
    assert!(near(c[1][5], -1.0, 1e-4));
    // Outside the range nothing moves.
    assert_eq!(c[0][100_000], 0.5);
}
