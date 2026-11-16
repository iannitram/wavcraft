//! A3.3: `SaveFormat::Mp3` is known by its path and its name, and a save refuses it until the
//! writer lands (A3.2 and the encoder are separate work). A3.10: its size is estimated at
//! constant bit rate (`estimate_mp3_size`; `estimate_size` uses 192 kbps).

use std::path::Path;

use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::mp3_options::Mp3Options;
use wavcraft_core::save::{AiffKind, SampleType, SaveError, SaveFormat, estimate_mp3_size, estimate_size, save};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

#[test]
fn mp3_is_a_save_format() {
    assert_eq!(SaveFormat::from_path(Path::new("a.MP3")), Some(SaveFormat::Mp3));
    assert_eq!(SaveFormat::parse(" mp3 "), Some(SaveFormat::Mp3));
    assert_eq!(SaveFormat::Mp3.name(), "MP3");
    assert_eq!(SaveFormat::Mp3.extension(), "mp3");
    assert!(SaveFormat::Mp3.accepts(SampleType::Float32));
}

#[test]
fn mp3_save_is_refused_as_unsupported_until_its_writer_lands() {
    let dir = std::env::temp_dir().join(format!("wavcraft-save-mp3-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::create_with(&dir.join("store"), StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let samples = vec![0.25_f32; BLOCK_FRAMES];
    let channel = Channel::from_pieces(std::iter::once(Piece::whole(store.append(&samples).unwrap()).unwrap()));
    let doc = DocState::new(store.clone(), vec![channel], 44_100, SourceFormat::synthetic(44_100, 1)).unwrap();
    let target = dir.join("out.mp3");
    let err = save(&doc, &target, SaveFormat::Mp3, SampleType::Float32, &mut |_| {}).unwrap_err();
    assert!(matches!(err, SaveError::Unsupported(_)), "{err:?}");
    assert!(!target.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A document of `frames` frames of 0.1 at `rate` Hz with `channels` channels, its store in a
/// folder under `dir` (made by the caller and removed by it).
fn silent_doc(dir: &Path, frames: u64, rate: u32, channels: usize) -> DocState {
    let folder = dir.join(format!("store-{rate}-{frames}-{channels}"));
    let store = Store::create_with(&folder, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let mut chans = Vec::with_capacity(channels);
    for _ in 0..channels {
        let mut pieces = Vec::new();
        let mut left = frames;
        while left > 0 {
            let n = left.min(BLOCK_FRAMES as u64);
            let samples = vec![0.1_f32; n as usize];
            pieces.push(Piece::whole(store.append(&samples).unwrap()).unwrap());
            left -= n;
        }
        chans.push(Channel::from_pieces(pieces));
    }
    DocState::new(store, chans, rate, SourceFormat::synthetic(rate, channels as u16)).unwrap()
}

#[test]
fn the_mp3_estimate_is_the_cbr_size() {
    let dir = std::env::temp_dir().join(format!("wavcraft-mp3-cbr-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // 10 s, 44.1 kHz stereo: 192 kbps is 240000 bytes (estimate_size uses the default rate).
    let stereo = silent_doc(&dir, 441_000, 44_100, 2);
    let est = estimate_size(&stereo, SaveFormat::Mp3, SampleType::Int16, AiffKind::Aiff).unwrap();
    assert!((237_600..=242_400).contains(&est.bytes), "{}", est.bytes);
    assert!(!est.exact);
    // 5 s, 48 kHz, mono mix at 128 kbps: 80000 bytes.
    let mono = silent_doc(&dir, 240_000, 48_000, 2);
    let est = estimate_mp3_size(&mono, Mp3Options { bitrate_kbps: 128, mono: true }).unwrap();
    assert!((79_200..=80_800).contains(&est.bytes), "{}", est.bytes);
    assert!(!est.exact);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_mp3_estimate_refuses_what_the_save_would() {
    let dir = std::env::temp_dir().join(format!("wavcraft-mp3-refuse-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let rate = silent_doc(&dir, 96_000, 96_000, 2);
    let err = estimate_mp3_size(&rate, Mp3Options::default()).unwrap_err();
    assert!(err.to_string().contains("96000"), "{err}");
    let bitrate = silent_doc(&dir, 44_100, 44_100, 2);
    let err = estimate_mp3_size(&bitrate, Mp3Options { bitrate_kbps: 100, mono: false }).unwrap_err();
    assert!(err.to_string().contains("100 kbps"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}
