//! f87: estimated file size, export presets, AIFF kinds.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::io::{OpenOptions, open};
use wavcraft_core::export_presets::{ExportChoice, ExportPresets};
use wavcraft_core::save::{AiffKind, SampleType, SaveFormat, estimate_size, save, save_aiff_kind};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 44_100;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-f87-test-{}-{name}", std::process::id()));
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

fn reopen(path: &Path, sessions: &Path) -> DocState {
    let opts = OpenOptions { sessions_root: Some(sessions.to_path_buf()), ..OpenOptions::default() };
    open(path, &opts, &mut |_| {}, &AtomicBool::new(false)).unwrap()
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

/// Stereo values exactly representable at `bits`, long enough to cross block edges.
fn exact(bits: u32, frames: usize) -> Vec<Vec<f32>> {
    let scale = (1u64 << (bits - 1)) as f64;
    (0..2)
        .map(|c| (0..frames).map(|k| (((k * 7_919 + c * 104_729) as u64 % (2 * scale as u64)) as f64 - scale) as f32 / scale as f32).collect())
        .collect()
}

fn music_like(frames: usize) -> Vec<Vec<f32>> {
    (0..2).map(|c| (0..frames).map(|k| (k as f32 * (0.03 + 0.01 * c as f32)).sin() * 0.5 + (k as f32 * 0.31).sin() * 0.1).collect()).collect()
}

#[test]
fn f87_pcm_estimates_are_exact_for_every_format_and_type() {
    let t = TempDir::new("est");
    let frames = 2 * BLOCK_FRAMES + 77;
    let doc = doc_from(&t.0.join("store"), &exact(16, frames));
    for format in [SaveFormat::Wav, SaveFormat::Aiff] {
        for ty in [SampleType::Int16, SampleType::Int24, SampleType::Int32, SampleType::Float32] {
            for kind in AiffKind::ALL {
                let est = estimate_size(&doc, format, ty, kind).unwrap();
                assert!(est.exact);
                let path = t.0.join(format!("{}-{}-{}.{}", format.key(), ty.key(), kind.key(), format.extension()));
                save_aiff_kind(&doc, &path, format, ty, kind, &mut |_| {}).unwrap();
                assert_eq!(est.bytes, std::fs::metadata(&path).unwrap().len(), "{format:?} {ty:?} {kind:?}");
            }
        }
    }
}

#[test]
fn f87_an_odd_length_pcm_estimate_counts_the_pad_byte() {
    let t = TempDir::new("odd");
    let doc = doc_from(&t.0.join("store"), &[vec![0.25; 1001]]); // mono 24-bit: 3003 data bytes
    for format in [SaveFormat::Wav, SaveFormat::Aiff] {
        let est = estimate_size(&doc, format, SampleType::Int24, AiffKind::Aiff).unwrap();
        let path = t.0.join(format!("o.{}", format.extension()));
        save(&doc, &path, format, SampleType::Int24, &mut |_| {}).unwrap();
        assert_eq!(est.bytes, std::fs::metadata(&path).unwrap().len());
    }
}

#[test]
fn f87_flac_estimate_is_labelled_and_close() {
    let t = TempDir::new("flac");
    let frames = 40 * BLOCK_FRAMES + 5;
    let doc = doc_from(&t.0.join("store"), &music_like(frames));
    for ty in [SampleType::Int16, SampleType::Int24] {
        let est = estimate_size(&doc, SaveFormat::Flac, ty, AiffKind::Aiff).unwrap();
        assert!(!est.exact);
        let path = t.0.join(format!("a{}.flac", ty.key()));
        save(&doc, &path, SaveFormat::Flac, ty, &mut |_| {}).unwrap();
        let actual = std::fs::metadata(&path).unwrap().len() as f64;
        let ratio = est.bytes as f64 / actual;
        assert!((0.85..1.15).contains(&ratio), "{ty:?}: estimate {} vs actual {actual}", est.bytes);
    }
    assert!(estimate_size(&doc, SaveFormat::Flac, SampleType::Float32, AiffKind::Aiff).is_err());
}

#[test]
fn f87_estimate_tracks_the_format_and_type() {
    let t = TempDir::new("track");
    let doc = doc_from(&t.0.join("store"), &exact(16, 10_000));
    let size = |f, ty| estimate_size(&doc, f, ty, AiffKind::Aiff).unwrap().bytes;
    assert!(size(SaveFormat::Wav, SampleType::Int16) < size(SaveFormat::Wav, SampleType::Int24));
    assert!(size(SaveFormat::Wav, SampleType::Int24) < size(SaveFormat::Wav, SampleType::Int32));
    assert_eq!(size(SaveFormat::Wav, SampleType::Int32), size(SaveFormat::Aiff, SampleType::Int32) - 54 + 44); // WAV 44-byte header vs AIFF 54
}

#[test]
fn f87_aiff_kinds_round_trip_and_declare_their_byte_order() {
    let t = TempDir::new("kinds");
    let frames = 2 * BLOCK_FRAMES + 123;
    for ty in [SampleType::Int16, SampleType::Int24, SampleType::Int32] {
        let bits = match ty {
            SampleType::Int16 => 16,
            SampleType::Int24 => 24,
            _ => 24, // 24-bit values survive 32-bit integer in f32
        };
        let data = exact(bits, frames);
        let doc = doc_from(&t.0.join(format!("{}-store", ty.key())), &data);
        for kind in AiffKind::ALL {
            let path = t.0.join(format!("{}-{}.aiff", ty.key(), kind.key()));
            save_aiff_kind(&doc, &path, SaveFormat::Aiff, ty, kind, &mut |_| {}).unwrap();
            let raw = std::fs::read(&path).unwrap();
            let form: &[u8] = if kind == AiffKind::Aiff { b"AIFF" } else { b"AIFC" };
            assert_eq!(&raw[8..12], form, "{ty:?} {kind:?}");
            let comp: Option<&[u8]> = match kind {
                AiffKind::Aiff => None,
                AiffKind::Aifc => Some(b"NONE"),
                AiffKind::AifcSowt => Some(b"sowt"),
            };
            if let Some(c) = comp {
                assert!(raw.windows(4).any(|w| w == c), "{ty:?} {kind:?}: compression type {c:?}");
            }
            let size = u32::from_be_bytes(raw[4..8].try_into().unwrap()) as usize;
            assert_eq!(size + 8, raw.len(), "{ty:?} {kind:?}: FORM size");
            let back = reopen(&path, &t.0.join("sessions"));
            assert_eq!((back.sample_rate(), back.channel_count(), back.len_frames()), (RATE, 2, frames as u64));
            assert_eq!(content(&back), data, "{ty:?} {kind:?} round trip");
        }
    }
}

#[test]
fn f87_sowt_samples_are_little_endian_and_classic_are_big() {
    let t = TempDir::new("order");
    let doc = doc_from(&t.0.join("store"), &[vec![0.5; 4]]); // 0.5 -> 0x4000 in 16 bit
    let tail = |kind| {
        let path = t.0.join(format!("{}.aiff", kind_key(kind)));
        save_aiff_kind(&doc, &path, SaveFormat::Aiff, SampleType::Int16, kind, &mut |_| {}).unwrap();
        let raw = std::fs::read(path).unwrap();
        raw[raw.len() - 8..raw.len() - 6].to_vec()
    };
    assert_eq!(tail(AiffKind::Aiff), [0x40, 0x00]);
    assert_eq!(tail(AiffKind::Aifc), [0x40, 0x00]);
    assert_eq!(tail(AiffKind::AifcSowt), [0x00, 0x40]);
}

fn kind_key(k: AiffKind) -> &'static str {
    k.key()
}

#[test]
fn f87_float_is_always_fl32_whatever_the_kind() {
    let t = TempDir::new("float");
    let data = vec![vec![0.1f32, -0.7, 1.5, 0.0]];
    let doc = doc_from(&t.0.join("store"), &data);
    for kind in AiffKind::ALL {
        let path = t.0.join(format!("{}.aiff", kind.key()));
        save_aiff_kind(&doc, &path, SaveFormat::Aiff, SampleType::Float32, kind, &mut |_| {}).unwrap();
        let raw = std::fs::read(&path).unwrap();
        assert!(raw.windows(4).any(|w| w == b"fl32"));
        assert_eq!(content(&reopen(&path, &t.0.join("s"))), data);
    }
}

#[test]
fn f87_presets_survive_a_restart() {
    let t = TempDir::new("presets");
    let path = t.0.join("cfg").join("export-presets.txt");
    let mut p = ExportPresets::default();
    p.set("Archive master", ExportChoice { format: SaveFormat::Flac, ty: SampleType::Int24, aiff: AiffKind::Aiff }).unwrap();
    p.set("Old sampler", ExportChoice { format: SaveFormat::Aiff, ty: SampleType::Int16, aiff: AiffKind::AifcSowt }).unwrap();
    p.set("Archive master", ExportChoice { format: SaveFormat::Flac, ty: SampleType::Int16, aiff: AiffKind::Aiff }).unwrap(); // replaces
    p.save(&path).unwrap();
    let back = ExportPresets::load(&path);
    assert_eq!(back, p);
    assert_eq!(back.presets.len(), 2);
    assert_eq!(back.get("Old sampler").unwrap().aiff, AiffKind::AifcSowt);
    assert_eq!(back.get("Archive master").unwrap().ty, SampleType::Int16);
    let mut back = back;
    assert!(back.remove("Old sampler"));
    assert!(!back.remove("Old sampler"));
    back.save(&path).unwrap();
    assert_eq!(ExportPresets::load(&path).presets.len(), 1);
    assert!(!path.with_extension("txt.new").exists());
}

#[test]
fn f87_presets_refuse_bad_names_and_fit_the_type_to_the_format() {
    let mut p = ExportPresets::default();
    let c = ExportChoice { format: SaveFormat::Flac, ty: SampleType::Float32, aiff: AiffKind::Aiff };
    assert!(p.set("  ", c).is_err());
    assert!(p.set("a\tb", c).is_err());
    assert!(p.set(&"x".repeat(65), c).is_err());
    p.set(" ok ", c).unwrap();
    assert_eq!(p.get("ok").unwrap().ty, SampleType::Int24, "FLAC cannot hold float");
    // A missing file is no presets; junk lines are skipped.
    assert!(ExportPresets::load(Path::new("/nonexistent/f87/presets.txt")).presets.is_empty());
    assert_eq!(ExportPresets::parse("junk\nname\twav\t24\taiff\nbad\tmp3\t24\taiff\n").presets.len(), 1);
}
