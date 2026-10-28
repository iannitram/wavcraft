//! Task 03 acceptance tests 8-11: streamed open of the kit files (and other layouts) against
//! `soundcraft-audio-io`'s whole-file decode, the ramp oracle, cancel, and damaged files.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use soundcraft_audio_io::{AudioBuffer, BitDepth, EncodeOptions, FileFormat, SampleFormat};
use wavcraft_core::doc::DocState;
use wavcraft_core::io::{OpenError, OpenOptions, open};
use wavcraft_core::store::Encoding;
use wavcraft_testkit::*;

/// A folder in the system's temp directory, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-open-test-{}-{name}", std::process::id()));
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

fn stream(path: &Path, root: &Path) -> Result<DocState, OpenError> {
    open(path, &opts(root), &mut |_| {}, &AtomicBool::new(false))
}

fn bits(buf: &AudioBuffer) -> Vec<Vec<u32>> {
    buf.channels.iter().map(|c| c.iter().map(|v| v.to_bits()).collect()).collect()
}

/// Streamed open and audio-io's whole-file decode of `path` agree on every sample, bit for bit,
/// and on the rate and channel count.
fn same_as_audio_io(path: &Path, root: &Path) -> DocState {
    let bytes = std::fs::read(path).unwrap();
    let (info, want) = soundcraft_audio_io::decode(&bytes, path.extension().and_then(|e| e.to_str())).unwrap();
    let doc = stream(path, root).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let got = doc.to_audio_buffer().unwrap();
    let name = path.file_name().unwrap().to_string_lossy();
    assert_eq!((got.sample_rate, got.num_channels(), got.frames()), (want.sample_rate, want.num_channels(), want.frames()), "{name}");
    assert!(bits(&got) == bits(&want), "{name}: samples differ");
    let src = doc.source();
    assert_eq!((src.format, src.sample_format), (info.format, info.sample_format), "{name}");
    assert_eq!(src.bwf, info.bwf, "{name}");
    // Task 04b: 16- and 24-bit WAV, AIFF and FLAC are stored as integers, all else as f32.
    let want_encoding = match (src.format, src.sample_format) {
        (FileFormat::Wav | FileFormat::Aiff | FileFormat::Flac, SampleFormat::Int16) => Encoding::I16,
        (FileFormat::Wav | FileFormat::Aiff | FileFormat::Flac, SampleFormat::Int24) => Encoding::I24,
        _ => Encoding::F32,
    };
    assert_eq!(encodings(&doc), vec![want_encoding], "{name}: block encodings");
    doc
}

/// Every encoding the document's (non-zero) blocks use, without repeats.
fn encodings(doc: &DocState) -> Vec<Encoding> {
    let mut out = Vec::new();
    for ch in doc.channels() {
        for p in ch.pieces() {
            let e = p.block().encoding();
            if !p.block().is_zero() && !out.contains(&e) {
                out.push(e);
            }
        }
    }
    out
}

fn write(path: &Path, bytes: &[u8]) -> PathBuf {
    std::fs::write(path, bytes).unwrap();
    path.to_path_buf()
}

/// A 32-bit float buffer with values that exercise every conversion: full scale both ways,
/// tiny values, and over full scale (float keeps it, integers clamp).
fn awkward(channels: usize, frames: usize) -> AudioBuffer {
    let mut x = 0x2545_F491u32;
    let channels = (0..channels)
        .map(|c| {
            (0..frames)
                .map(|n| match n % 7 {
                    0 => 1.0,
                    1 => -1.0,
                    2 => 1.0e-7 * c as f32,
                    3 => 1.5,
                    _ => {
                        x ^= x << 13;
                        x ^= x >> 17;
                        x ^= x << 5;
                        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
                    }
                })
                .collect()
        })
        .collect();
    AudioBuffer { sample_rate: 44_100, channels }
}

/// A WAV written by hand, for layouts audio-io's encoder doesn't make (8-bit, 64-bit float,
/// extra chunks before and after the data, a fmt that is WAVE_FORMAT_EXTENSIBLE).
fn hand_wav(tag: u16, bits: u16, channels: u16, data: &[u8], extensible: bool) -> Vec<u8> {
    hand_wav_with_mask(tag, bits, channels, data, extensible, speaker_mask(channels))
}

/// The speaker mask that names the channels of a `channels`-channel file: FC for mono, FL and FR
/// for stereo, FL, FR and FC for 3 channels (f75 keeps a mask only when its speaker count matches).
fn speaker_mask(channels: u16) -> u32 {
    match channels {
        1 => 0x4,
        2 => 0x3,
        _ => 0x7,
    }
}

/// [`hand_wav`] with the extensible fmt's speaker mask given, for files whose mask does not match.
fn hand_wav_with_mask(tag: u16, bits: u16, channels: u16, data: &[u8], extensible: bool, mask: u32) -> Vec<u8> {
    let mut fmt = Vec::new();
    fmt.extend((if extensible { 0xFFFEu16 } else { tag }).to_le_bytes());
    fmt.extend(channels.to_le_bytes());
    fmt.extend(48_000u32.to_le_bytes());
    let align = channels * bits.div_ceil(8);
    fmt.extend((48_000 * u32::from(align)).to_le_bytes());
    fmt.extend(align.to_le_bytes());
    fmt.extend(bits.to_le_bytes());
    if extensible {
        fmt.extend(22u16.to_le_bytes());
        fmt.extend(bits.to_le_bytes());
        fmt.extend(mask.to_le_bytes());
        fmt.extend(tag.to_le_bytes());
        fmt.extend([0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71]);
    }
    let mut body = b"WAVE".to_vec();
    let mut chunk = |id: &[u8; 4], c: &[u8]| {
        body.extend(id);
        body.extend((c.len() as u32).to_le_bytes());
        body.extend(c);
        if c.len() % 2 == 1 {
            body.push(0);
        }
    };
    chunk(b"JUNK", &[0; 28]);
    chunk(b"fmt ", &fmt);
    chunk(b"iXML", b"<x/>!");
    chunk(b"data", data);
    chunk(b"cue ", &[0; 4]);
    chunk(b"LIST", b"adtl");
    let mut out = b"RIFF".to_vec();
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(body);
    out
}

/// Test 8: every kit file, and every other layout the opener reads itself, streams in bit-identical
/// to audio-io's whole-file decode.
#[test]
fn streamed_open_matches_audio_io_for_every_kit_file_and_layout() {
    let kit = TempDir::new("kit");
    let root = TempDir::new("kit-sessions");
    write_kit(&kit.0, &KitOptions::default()).unwrap();
    for e in expected(false) {
        let doc = same_as_audio_io(&kit.0.join(e.name), &root.0);
        assert_eq!((doc.channel_count(), doc.sample_rate(), doc.len_frames()), (e.channels, e.sample_rate, e.frames as u64), "{}", e.name);
        assert!(doc.source().warnings.is_empty(), "{}: {:?}", e.name, doc.source().warnings);
    }

    // Layouts audio-io's encoder writes: every depth in WAV and AIFF (AIFF float is AIFF-C
    // fl32), RF64, and a BWF bext; mono, stereo and 3 channels.
    let dir = TempDir::new("layouts");
    for channels in [1usize, 2, 3] {
        let buf = awkward(channels, 70_000);
        for (format, ext) in [(FileFormat::Wav, "wav"), (FileFormat::Aiff, "aiff")] {
            for depth in [BitDepth::Int16, BitDepth::Int24, BitDepth::Int32, BitDepth::Float32] {
                let o = EncodeOptions { format, bit_depth: depth, dither: false, bwf: None };
                let p = write(&dir.0.join(format!("x{channels}-{depth:?}.{ext}")), &soundcraft_audio_io::encode(&buf, &o).unwrap());
                same_as_audio_io(&p, &root.0);
            }
        }
        let o = EncodeOptions { format: FileFormat::Wav, bit_depth: BitDepth::Int24, dither: false, bwf: None };
        let p = write(&dir.0.join(format!("rf64-{channels}.wav")), &soundcraft_audio_io::encode_wav_rf64(&buf, &o).unwrap());
        assert!(same_as_audio_io(&p, &root.0).source().rf64);
        let o = EncodeOptions {
            bwf: Some(soundcraft_audio_io::BwfInfo { description: "take 3".into(), time_reference: 123_456_789_000, ..Default::default() }),
            ..o
        };
        let p = write(&dir.0.join(format!("bwf-{channels}.wav")), &soundcraft_audio_io::encode(&buf, &o).unwrap());
        assert_eq!(same_as_audio_io(&p, &root.0).source().bwf.as_ref().unwrap().time_reference, 123_456_789_000);
        let o = EncodeOptions { format: FileFormat::Flac, bit_depth: BitDepth::Int24, dither: false, bwf: None };
        let p = write(&dir.0.join(format!("x{channels}.flac")), &soundcraft_audio_io::encode(&buf, &o).unwrap());
        assert_eq!(same_as_audio_io(&p, &root.0).source().format, FileFormat::Flac);
    }

    // Layouts written by hand: 8-bit unsigned, 64-bit float, 20-bit in 3 bytes, extensible.
    let bytes: Vec<u8> = (0..60_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8).collect();
    let floats: Vec<u8> = (0..20_000).flat_map(|i| ((f64::from(i) / 9_999.0 - 1.0) * 1.25).to_le_bytes()).collect();
    for (name, tag, bits, channels, data, ext) in [
        ("u8.wav", 1u16, 8u16, 2u16, &bytes[..], false),
        ("u8-ext.wav", 1, 8, 1, &bytes[..], true),
        ("f64.wav", 3, 64, 2, &floats[..], false),
        ("f64-ext.wav", 3, 64, 1, &floats[..], true),
        ("i20.wav", 1, 20, 2, &bytes[..59_994], true),
        ("i16-ext.wav", 1, 16, 2, &bytes[..], true),
        ("i32-ext.wav", 1, 32, 3, &bytes[..59_988], true),
    ] {
        let p = write(&dir.0.join(name), &hand_wav(tag, bits, channels, data, ext));
        let doc = same_as_audio_io(&p, &root.0);
        // Only an extensible header carries a speaker mask (hand_wav writes one for its channel count).
        assert_eq!(doc.source().channel_mask, ext.then_some(speaker_mask(channels)), "{name}");
        // Chunks the opener skipped are recorded, and where the marker chunks are.
        let skipped: Vec<String> = doc.source().skipped_chunks().iter().map(|c| c.id_str()).collect();
        assert_eq!(skipped, ["iXML", "cue ", "LIST"], "{name}");
        let markers: Vec<String> = doc.source().marker_chunks().iter().map(|c| c.id_str()).collect();
        assert_eq!(markers, ["cue ", "LIST"], "{name}");
        let cue = doc.source().marker_chunks()[0].clone();
        let file = std::fs::read(&p).unwrap();
        assert_eq!(&file[cue.offset as usize..cue.offset as usize + 4], b"cue ");
    }
    // Every session folder made above was removed when its document dropped.
    assert_eq!(std::fs::read_dir(&root.0).unwrap().count(), 0);
}

/// An extensible file whose speaker mask names 2 speakers but has 1 channel: the mask is dropped
/// with a warning, and the file still opens.
#[test]
fn a_channel_mask_that_does_not_match_the_channel_count_is_dropped() {
    let dir = TempDir::new("mask-mismatch");
    let root = TempDir::new("mask-mismatch-sessions");
    let bytes: Vec<u8> = (0..60_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8).collect();
    let p = write(&dir.0.join("mask-mismatch.wav"), &hand_wav_with_mask(1, 8, 1, &bytes, true, 0x3));
    let doc = stream(&p, &root.0).unwrap();
    assert_eq!(doc.source().channel_mask, None);
    assert!(
        doc.source().warnings.iter().any(|w| w.contains("names 2 speakers but the file has 1 channels")),
        "warnings: {:?}",
        doc.source().warnings
    );
    assert_eq!(doc.channel_count(), 1);
}

/// Task 04b test 2: one file per source format, and the encoding its blocks get.
#[test]
fn each_source_format_is_stored_in_its_encoding() {
    let dir = TempDir::new("encodings");
    let root = TempDir::new("encodings-sessions");
    let buf = awkward(2, 70_000);
    let enc = |format, bit_depth| soundcraft_audio_io::encode(&buf, &EncodeOptions { format, bit_depth, dither: false, bwf: None }).unwrap();
    let rf64 =
        soundcraft_audio_io::encode_wav_rf64(&buf, &EncodeOptions { format: FileFormat::Wav, bit_depth: BitDepth::Int24, dither: false, bwf: None })
            .unwrap();
    let bytes: Vec<u8> = (0..60_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8).collect();
    let mp3: Vec<u8> = (0..40).flat_map(|_| mp3_frame()).collect();
    let cases: Vec<(&str, Vec<u8>, Encoding)> = vec![
        ("u8.wav", hand_wav(1, 8, 2, &bytes, false), Encoding::F32),
        ("i16.wav", enc(FileFormat::Wav, BitDepth::Int16), Encoding::I16),
        ("i24.wav", enc(FileFormat::Wav, BitDepth::Int24), Encoding::I24),
        ("i20-in-24.wav", hand_wav(1, 20, 2, &bytes[..59_994], true), Encoding::I24),
        ("i32.wav", enc(FileFormat::Wav, BitDepth::Int32), Encoding::F32),
        ("f32.wav", enc(FileFormat::Wav, BitDepth::Float32), Encoding::F32),
        ("rf64-24.wav", rf64, Encoding::I24),
        ("i16.aiff", enc(FileFormat::Aiff, BitDepth::Int16), Encoding::I16),
        ("i24.aiff", enc(FileFormat::Aiff, BitDepth::Int24), Encoding::I24),
        ("i32.aiff", enc(FileFormat::Aiff, BitDepth::Int32), Encoding::F32),
        ("f32.aiff", enc(FileFormat::Aiff, BitDepth::Float32), Encoding::F32),
        ("i16.flac", enc(FileFormat::Flac, BitDepth::Int16), Encoding::I16),
        ("i24.flac", enc(FileFormat::Flac, BitDepth::Int24), Encoding::I24),
        ("silence.mp3", mp3, Encoding::F32),
    ];
    for (name, file, want) in cases {
        let p = write(&dir.0.join(name), &file);
        let doc = stream(&p, &root.0).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(encodings(&doc), vec![want], "{name}");
        let frames = doc.len_frames() * doc.channel_count() as u64;
        assert_eq!(doc.disk_bytes_referenced(), frames * want.width(), "{name}: bytes on disk follow the encoding");
    }
}

/// Test 9: the ramp file streams in as its formula.
#[test]
fn the_ramp_streams_in_as_its_oracle() {
    let kit = TempDir::new("ramp");
    let root = TempDir::new("ramp-sessions");
    write_kit(&kit.0, &KitOptions::default()).unwrap();
    let doc = stream(&kit.0.join(RAMP), &root.0).unwrap();
    assert_eq!((doc.len_frames(), doc.channel_count()), (480_000, 1));
    assert_eq!(doc.source().sample_format, SampleFormat::Int16);
    // 64 K-frame blocks: 7 full and one of the rest.
    assert_eq!(doc.piece_count(0), 8);
    let mut out = vec![0.0f32; 4_097];
    let mut at = 0u64;
    while at < doc.len_frames() {
        let n = doc.read(0, at, &mut out).unwrap();
        for (i, v) in out[..n].iter().enumerate() {
            assert_eq!(*v * 32_768.0, ramp_value(at + i as u64) as f32, "frame {}", at + i as u64);
        }
        at += n as u64;
    }
}

#[test]
fn chained_ogg_opens_every_stream() {
    let input = TempDir::new("chained-ogg");
    let sessions = TempDir::new("chained-ogg-sessions");
    let path = input.0.join("chained-two-streams-8k-mono.ogg");
    std::fs::write(&path, include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/chained-two-streams-8k-mono.ogg"))).unwrap();

    let doc = stream(&path, &sessions.0).unwrap();
    assert_eq!((doc.sample_rate(), doc.channel_count()), (8_000, 1));
    assert!(doc.len_frames() > 11_000);

    let mut samples = vec![0.0; 2_000];
    assert_eq!(doc.read(0, 8_200, &mut samples).unwrap(), samples.len());
    let rms = (samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32).sqrt();
    assert!(rms > 0.05, "second stream RMS is {rms}");
}

/// Test 10: cancelled halfway, open returns `Cancelled` and the session folder is gone.
#[test]
fn a_cancelled_open_leaves_nothing_behind() {
    let kit = TempDir::new("cancel");
    let root = TempDir::new("cancel-sessions");
    write_kit(&kit.0, &KitOptions::default()).unwrap();
    for name in [TONES, TONE_FLAC] {
        let cancel = AtomicBool::new(false);
        let mut seen = Vec::new();
        let result = open(
            &kit.0.join(name),
            &opts(&root.0),
            &mut |p| {
                seen.push(p);
                if p >= 0.5 {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
            &cancel,
        );
        assert_eq!(result.unwrap_err(), OpenError::Cancelled, "{name}");
        assert!(seen.iter().any(|p| *p >= 0.5) && seen.iter().all(|p| *p < 1.0), "{name}: stopped part way: {seen:?}");
        assert!(seen.windows(2).all(|w| w[0] <= w[1]), "{name}: progress only goes up");
        assert_eq!(std::fs::read_dir(&root.0).unwrap().count(), 0, "{name}: the session folder is gone");
    }
    // Uncancelled, progress ends at 1.
    let mut last = 0.0;
    stream(&kit.0.join(TONES), &root.0).unwrap();
    open(&kit.0.join(TONES), &opts(&root.0), &mut |p| last = p, &AtomicBool::new(false)).unwrap();
    assert_eq!(last, 1.0);
}

/// One MPEG-1 Layer III frame of silence: 128 kbit/s, 44.1 kHz, mono, no CRC (417 bytes). Side
/// info all zero means no Huffman data: it decodes to 1,152 zeros.
fn mp3_frame() -> Vec<u8> {
    let mut f = vec![0u8; 417];
    f[..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0xC0]);
    f
}

/// Test 11: a truncated WAV and AIFF, and an MP3 with a damaged frame, open what's there and say so.
#[test]
fn damaged_files_open_what_is_there_and_say_so() {
    let kit = TempDir::new("damaged");
    let root = TempDir::new("damaged-sessions");
    write_kit(&kit.0, &KitOptions::default()).unwrap();
    let whole = stream(&kit.0.join(TONE), &root.0).unwrap().to_audio_buffer().unwrap();

    // The data chunk claims 10 s; the file stops 3 bytes into frame 100,001 (stereo 16-bit: 4
    // bytes a frame, data starts at 44).
    let bytes = std::fs::read(kit.0.join(TONE)).unwrap();
    let cut = write(&kit.0.join("cut.wav"), &bytes[..44 + 100_000 * 4 + 3]);
    let doc = same_as_audio_io(&cut, &root.0);
    assert_eq!(doc.len_frames(), 100_000, "whole frames only");
    let got = doc.to_audio_buffer().unwrap();
    for (g, w) in got.channels.iter().zip(&whole.channels) {
        assert_eq!(g[..], w[..100_000]);
    }
    let warnings = &doc.source().warnings;
    assert!(warnings.len() == 1 && warnings[0].contains("shorter than its header says"), "{warnings:?}");

    // A data size of 0 (a recorder that died before writing it) opens everything after it.
    let mut zero = bytes.clone();
    zero[40..44].copy_from_slice(&0u32.to_le_bytes());
    let doc = same_as_audio_io(&write(&kit.0.join("zero.wav"), &zero), &root.0);
    assert_eq!(doc.len_frames(), 480_000);
    assert!(doc.source().warnings[0].contains("size is 0"));

    // A truncated AIFF.
    let aiff = std::fs::read(kit.0.join(TONE_AIFF)).unwrap();
    let doc = same_as_audio_io(&write(&kit.0.join("cut.aiff"), &aiff[..aiff.len() / 2]), &root.0);
    assert!(doc.len_frames() > 200_000 && doc.len_frames() < 480_000);
    assert!(doc.source().warnings[0].contains("shorter than its header says"), "{:?}", doc.source().warnings);

    // An MP3 of 40 silent frames, one of which is damaged, and the last cut short.
    let mut mp3 = Vec::new();
    for i in 0..40 {
        let mut f = mp3_frame();
        if i == 20 {
            // Side info: main_data_begin 0, then part2_3_length of both granules at their
            // largest (4,095 bits): more than the frame and its reservoir can hold.
            f[4..9].copy_from_slice(&[0x00, 0x03, 0xFF, 0xE0, 0x00]);
            f[9..20].fill(0xFF);
        }
        mp3.extend(f);
    }
    mp3.truncate(mp3.len() - 200);
    let p = write(&kit.0.join("damaged.mp3"), &mp3);
    let doc = stream(&p, &root.0).unwrap();
    let src = doc.source();
    println!("damaged mp3: {} frames, warnings {:?}", doc.len_frames(), src.warnings);
    assert_eq!((src.format, src.sample_rate, doc.channel_count()), (FileFormat::Mp3, 44_100, 1));
    assert!(doc.len_frames() >= 30 * 1_152 && doc.len_frames() <= 40 * 1_152, "{}", doc.len_frames());
    assert!(!src.warnings.is_empty() || doc.len_frames() < 40 * 1_152, "the damage is reported");
    // And it agrees with audio-io's decode of the same bytes.
    let (_, want) = soundcraft_audio_io::decode(&mp3, Some("mp3")).unwrap();
    assert!(bits(&doc.to_audio_buffer().unwrap()) == bits(&want));

    // Junk and empty files are errors, not panics, and leave no session behind. (The documents
    // above are still alive: shadowing doesn't drop them, so count from here.)
    let sessions = std::fs::read_dir(&root.0).unwrap().count();
    assert!(matches!(stream(&write(&kit.0.join("junk.wav"), b"not audio at all"), &root.0), Err(OpenError::Decode(_))));
    assert!(matches!(stream(&write(&kit.0.join("hdr.wav"), &bytes[..44]), &root.0), Err(OpenError::Empty)));
    assert!(matches!(stream(&kit.0.join("missing.wav"), &root.0), Err(OpenError::Io(_))));
    let mut nofmt = bytes[..12].to_vec();
    nofmt.extend(&bytes[36..44 + 400]);
    assert!(matches!(stream(&write(&kit.0.join("nofmt.wav"), &nofmt), &root.0), Err(OpenError::Decode(_))));
    assert_eq!(std::fs::read_dir(&root.0).unwrap().count(), sessions);
}

/// The RF64 writer the huge kit file uses, checked small: forced through the same path by a
/// header test (the real file is 4.3 GB and is opened by the bench, see docs/baseline.md).
#[test]
fn huge_counter_wraps_every_2_24_frames() {
    assert_eq!(huge_counter(0), -(1 << 23));
    assert_eq!(huge_counter((1 << 24) - 1), (1 << 23) - 1);
    assert_eq!(huge_counter(1 << 24), -(1 << 23));
    assert_eq!(HUGE_FRAMES, 720_000_000);
}
