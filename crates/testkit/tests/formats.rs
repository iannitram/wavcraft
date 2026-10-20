//! Item f91: Sony Wave64, Apple CAF and Sun/NeXT AU written by `save_as` and read back by our
//! own readers, sample for sample, and raw (headerless) import of known bytes.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::doc::DocState;
use wavcraft_core::io::{OpenError, OpenOptions, RawEncoding, RawSpec, open, open_raw};
use wavcraft_core::save::{Container, SampleType, quantize, save_as};
use wavcraft_core::{AudioBuffer, SampleFormat};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-formats-test-{}-{name}", std::process::id()));
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

fn opts(root: &Path) -> OpenOptions {
    OpenOptions { sessions_root: Some(root.to_path_buf()), ..OpenOptions::default() }
}

fn load(path: &Path, root: &Path) -> Result<DocState, OpenError> {
    open(path, &opts(root), &mut |_| {}, &AtomicBool::new(false))
}

fn buffer(channels: usize, frames: usize) -> AudioBuffer {
    let mut x = 0x1234_5678u32;
    let channels = (0..channels)
        .map(|c| {
            (0..frames)
                .map(|n| match n % 5 {
                    0 => 0.999,
                    1 => -1.0,
                    2 => 0.0,
                    _ => {
                        x ^= x << 13;
                        x ^= x >> 17;
                        x ^= x << 5;
                        ((x as f32 / u32::MAX as f32) * 2.0 - 1.0) * (0.5 + 0.1 * c as f32)
                    }
                })
                .collect()
        })
        .collect();
    AudioBuffer { sample_rate: 44_100, channels }
}

/// A document holding `buf`: written as a float WAV (exact), opened.
fn doc_of(buf: &AudioBuffer, dir: &Path) -> DocState {
    let src = dir.join("src.wav");
    let d = {
        let tmp = dir.join("seed.wav");
        let bytes = soundcraft_audio_io::encode(
            buf,
            &soundcraft_audio_io::EncodeOptions {
                format: soundcraft_audio_io::FileFormat::Wav,
                bit_depth: soundcraft_audio_io::BitDepth::Float32,
                dither: false,
                bwf: None,
            },
        )
        .unwrap();
        std::fs::write(&tmp, bytes).unwrap();
        tmp
    };
    std::fs::rename(&d, &src).unwrap();
    load(&src, &dir.join("sessions")).unwrap()
}

/// What `ty` stores of `x` and reads back.
fn expected(x: f32, ty: SampleType) -> f32 {
    match ty {
        SampleType::Int16 => (f64::from(quantize(x, 16)) / 32_768.0) as f32,
        SampleType::Int24 => (f64::from(quantize(x, 24)) / 8_388_608.0) as f32,
        SampleType::Int32 => (f64::from(quantize(x, 32)) / 2_147_483_648.0) as f32,
        SampleType::Float32 | SampleType::Float64 => x,
    }
}

#[test]
fn every_container_and_sample_type_round_trips_sample_exact() {
    let t = TempDir::new("roundtrip");
    let buf = buffer(2, 70_000); // more than one 64 K block
    let doc = doc_of(&buf, &t.0);
    let cases = [
        (Container::W64, "w64", vec![SampleType::Int16, SampleType::Int24, SampleType::Int32, SampleType::Float32, SampleType::Float64]),
        (Container::Caf, "caf", vec![SampleType::Int16, SampleType::Int24, SampleType::Int32, SampleType::Float32, SampleType::Float64]),
        (Container::Au, "au", vec![SampleType::Int16, SampleType::Int24, SampleType::Int32, SampleType::Float32, SampleType::Float64]),
    ];
    for (container, ext, types) in cases {
        for ty in types {
            let path = t.0.join(format!("out-{ty:?}.{ext}"));
            save_as(&doc, &path, container, ty, &mut |_| {}).unwrap();
            let back = load(&path, &t.0.join("sessions")).unwrap_or_else(|e| panic!("{container:?} {ty:?}: {e}"));
            assert_eq!((back.sample_rate(), back.channel_count(), back.len_frames()), (44_100, 2, 70_000), "{container:?} {ty:?}");
            let got = back.to_audio_buffer().unwrap();
            for (c, ch) in got.channels.iter().enumerate() {
                for (n, &v) in ch.iter().enumerate() {
                    let want = expected(buf.channels[c][n], ty);
                    assert!(v.to_bits() == want.to_bits(), "{container:?} {ty:?} ch{c} frame {n}: {v} != {want}");
                }
            }
            let sf = back.source().sample_format;
            let want_sf = match ty {
                SampleType::Int16 => SampleFormat::Int16,
                SampleType::Int24 => SampleFormat::Int24,
                SampleType::Int32 => SampleFormat::Int32,
                SampleType::Float32 => SampleFormat::Float32,
                SampleType::Float64 => SampleFormat::Float64,
            };
            assert_eq!(sf, want_sf, "{container:?} {ty:?}");
            // A second pass is bit-identical (idempotent).
            let again = t.0.join(format!("again-{ty:?}.{ext}"));
            save_as(&back, &again, container, ty, &mut |_| {}).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), std::fs::read(&again).unwrap(), "{container:?} {ty:?}: resave differs");
        }
    }
}

#[test]
fn an_odd_length_mono_wave64_is_padded_to_eight_bytes() {
    let t = TempDir::new("w64pad");
    let doc = doc_of(&buffer(1, 1001), &t.0);
    let path = t.0.join("odd.w64");
    save_as(&doc, &path, Container::W64, SampleType::Int16, &mut |_| {}).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(bytes.len() % 8, 0);
    assert_eq!(u64::from_le_bytes(bytes[16..24].try_into().unwrap()), bytes.len() as u64, "the riff size is the file size");
    assert_eq!(load(&path, &t.0.join("sessions")).unwrap().len_frames(), 1001);
}

#[test]
fn raw_import_reads_known_bytes() {
    let t = TempDir::new("raw");
    let root = t.0.join("sessions");
    let spec = |encoding, big_endian, channels, offset| RawSpec { sample_rate: 22_050, channels, encoding, big_endian, offset };
    let run = |bytes: &[u8], s: RawSpec| -> DocState {
        let path = t.0.join("raw.bin");
        std::fs::write(&path, bytes).unwrap();
        open_raw(&path, &s, &opts(&root), &mut |_| {}, &AtomicBool::new(false)).unwrap()
    };
    let vals = |d: &DocState| d.to_audio_buffer().unwrap().channels;

    // 16-bit little-endian stereo after a 4-byte header: frames (L,R) = (0x4000,-0x4000), (0x7FFF,-0x8000).
    let mut b = vec![9, 9, 9, 9];
    for v in [0x4000i16, -0x4000, 0x7FFF, -0x8000] {
        b.extend(v.to_le_bytes());
    }
    let d = run(&b, spec(RawEncoding::S16, false, 2, 4));
    assert_eq!((d.sample_rate(), d.channel_count(), d.len_frames()), (22_050, 2, 2));
    let v = vals(&d);
    assert_eq!(v[0], vec![0.5, 32767.0 / 32768.0]);
    assert_eq!(v[1], vec![-0.5, -1.0]);

    // Same samples big-endian.
    let mut b = Vec::new();
    for v in [0x4000i16, 0x7FFF] {
        b.extend(v.to_be_bytes());
    }
    assert_eq!(vals(&run(&b, spec(RawEncoding::S16, true, 1, 0)))[0], vec![0.5, 32767.0 / 32768.0]);

    // 8-bit unsigned and signed.
    assert_eq!(vals(&run(&[0x80, 0xC0, 0x00], spec(RawEncoding::U8, false, 1, 0)))[0], vec![0.0, 0.5, -1.0]);
    assert_eq!(vals(&run(&[0x00, 0x40, 0x80], spec(RawEncoding::S8, false, 1, 0)))[0], vec![0.0, 0.5, -1.0]);

    // 24-bit little-endian: 0x400000 is a quarter... of 2^23 = 0.5.
    assert_eq!(vals(&run(&[0x00, 0x00, 0x40, 0x00, 0x00, 0xC0], spec(RawEncoding::S24, false, 1, 0)))[0], vec![0.5, -0.5]);

    // 32-bit integer big-endian.
    assert_eq!(vals(&run(&0x4000_0000i32.to_be_bytes(), spec(RawEncoding::S32, true, 1, 0)))[0], vec![0.5]);

    // 32- and 64-bit float, both orders.
    assert_eq!(vals(&run(&0.25f32.to_le_bytes(), spec(RawEncoding::F32, false, 1, 0)))[0], vec![0.25]);
    assert_eq!(vals(&run(&(-0.75f32).to_be_bytes(), spec(RawEncoding::F32, true, 1, 0)))[0], vec![-0.75]);
    assert_eq!(vals(&run(&0.125f64.to_le_bytes(), spec(RawEncoding::F64, false, 1, 0)))[0], vec![0.125]);
    assert_eq!(vals(&run(&0.5f64.to_be_bytes(), spec(RawEncoding::F64, true, 1, 0)))[0], vec![0.5]);

    // A trailing partial frame is dropped with a warning.
    let d = run(&[0, 0x40, 0, 0x40, 7], spec(RawEncoding::S16, false, 2, 0));
    assert_eq!(d.len_frames(), 1);
    assert!(!d.source().warnings.is_empty());
}

#[test]
fn raw_import_refuses_bad_settings() {
    let t = TempDir::new("rawbad");
    let root = t.0.join("sessions");
    let path = t.0.join("raw.bin");
    std::fs::write(&path, [0u8; 64]).unwrap();
    let try_open = |rate, channels, offset| {
        let s = RawSpec { sample_rate: rate, channels, encoding: RawEncoding::S16, big_endian: false, offset };
        open_raw(&path, &s, &opts(&root), &mut |_| {}, &AtomicBool::new(false))
    };
    assert!(try_open(0, 1, 0).is_err());
    assert!(try_open(10_000_001, 1, 0).is_err());
    assert!(try_open(48_000, 0, 0).is_err());
    assert!(try_open(48_000, 33, 0).is_err());
    assert!(try_open(48_000, 1, 65).is_err(), "offset past the end");
    assert_eq!(try_open(48_000, 1, 64).err(), Some(OpenError::Empty), "nothing after the offset");
    assert!(try_open(10_000_000, 32, 0).is_ok());
}

/// A Wave64, CAF and AU file cut short opens what is there (never a panic).
#[test]
fn truncated_files_open_what_is_there_or_refuse() {
    let t = TempDir::new("trunc");
    let doc = doc_of(&buffer(2, 5000), &t.0);
    for (c, ext) in [(Container::W64, "w64"), (Container::Caf, "caf"), (Container::Au, "au")] {
        let path = t.0.join(format!("full.{ext}"));
        save_as(&doc, &path, c, SampleType::Int16, &mut |_| {}).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        for cut in [10usize, 30, 60, 90, bytes.len() / 2] {
            let p = t.0.join(format!("cut{cut}.{ext}"));
            std::fs::write(&p, &bytes[..cut.min(bytes.len())]).unwrap();
            let _ = load(&p, &t.0.join("sessions")); // either result is fine; it must not panic
        }
    }
}
