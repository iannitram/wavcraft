//! A3.3: `SaveFormat::Mp3` is known by its path and its name, and a save refuses it until the
//! writer lands (A3.2 and the encoder are separate work).

use std::path::Path;

use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::save::{SampleType, SaveError, SaveFormat, save};
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
