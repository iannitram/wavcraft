//! f63: reading a file's header without opening it.

use std::path::{Path, PathBuf};

use wavcraft_core::probe::probe;
use wavcraft_core::{FileFormat, SampleFormat};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-probe-test-{}-{name}", std::process::id()));
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

fn chunk(id: &[u8; 4], body: &[u8], big_endian: bool) -> Vec<u8> {
    let mut out = id.to_vec();
    let n = body.len() as u32;
    out.extend(if big_endian { n.to_be_bytes() } else { n.to_le_bytes() });
    out.extend(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
    out
}

fn fmt_chunk(channels: u16, rate: u32, bits: u16, tag: u16) -> Vec<u8> {
    let align = channels * bits.div_ceil(8);
    let mut b = Vec::new();
    b.extend(tag.to_le_bytes());
    b.extend(channels.to_le_bytes());
    b.extend(rate.to_le_bytes());
    b.extend((rate * u32::from(align)).to_le_bytes());
    b.extend(align.to_le_bytes());
    b.extend(bits.to_le_bytes());
    chunk(b"fmt ", &b, false)
}

fn wav(chunks: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = chunks.iter().flatten().copied().collect();
    let mut out = b"RIFF".to_vec();
    out.extend((body.len() as u32 + 4).to_le_bytes());
    out.extend(b"WAVE");
    out.extend(body);
    out
}

fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let p = dir.0.join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}

fn info_list(title: &str) -> Vec<u8> {
    let mut b = b"INFO".to_vec();
    b.extend(chunk(b"ISFT", b"tool\0", false));
    b.extend(chunk(b"INAM", format!("{title}\0").as_bytes(), false));
    chunk(b"LIST", &b, false)
}

fn bext(description: &str) -> Vec<u8> {
    let mut b = vec![0u8; 602];
    b[..description.len()].copy_from_slice(description.as_bytes());
    chunk(b"bext", &b, false)
}

#[test]
fn a_wav_header_gives_format_length_and_the_info_title() {
    let dir = TempDir::new("wav");
    let bytes = wav(&[fmt_chunk(2, 48_000, 24, 1), info_list("Rain on a roof"), chunk(b"data", &vec![0u8; 6 * 4800], false)]);
    let p = write(&dir, "a.wav", &bytes);
    let i = probe(&p).unwrap();
    assert_eq!((i.format, i.sample_format, i.sample_rate, i.channels), (FileFormat::Wav, SampleFormat::Int24, 48_000, 2));
    assert_eq!(i.frames, 4800);
    assert_eq!(i.title, "Rain on a roof");
    assert_eq!(i.file_bytes, bytes.len() as u64);
    assert!(!i.estimated);
}

#[test]
fn the_bwf_description_is_the_title_when_there_is_no_info_name() {
    let dir = TempDir::new("bwf");
    let p = write(&dir, "b.wav", &wav(&[fmt_chunk(1, 44_100, 16, 1), bext("Take 7, boom mic"), chunk(b"data", &[0u8; 200], false)]));
    let i = probe(&p).unwrap();
    assert_eq!((i.sample_format, i.frames, i.title.as_str()), (SampleFormat::Int16, 100, "Take 7, boom mic"));
}

#[test]
fn a_float_wav_without_a_title_has_an_empty_one() {
    let dir = TempDir::new("float");
    let p = write(&dir, "f.wav", &wav(&[fmt_chunk(1, 96_000, 32, 3), chunk(b"data", &[0u8; 40], false)]));
    let i = probe(&p).unwrap();
    assert_eq!((i.sample_format, i.frames, i.title.as_str()), (SampleFormat::Float32, 10, ""));
}

#[test]
fn the_length_comes_from_the_header_not_from_reading_the_audio() {
    // A data chunk that claims 400 MB of a file that is a few bytes long, then the same with a
    // real (sparse) size: the answer needs no read of the audio either way.
    let dir = TempDir::new("big");
    let mut bytes = wav(&[fmt_chunk(2, 48_000, 16, 1)]);
    bytes.extend(b"data");
    bytes.extend(400_000_000u32.to_le_bytes());
    let p = write(&dir, "big.wav", &bytes);
    let f = std::fs::OpenOptions::new().write(true).open(&p).unwrap();
    f.set_len(bytes.len() as u64 + 400_000_000).unwrap();
    drop(f);
    assert_eq!(probe(&p).unwrap().frames, 100_000_000);
}

#[test]
fn an_aiff_name_chunk_is_the_title() {
    let dir = TempDir::new("aiff");
    let mut comm = Vec::new();
    comm.extend(2u16.to_be_bytes());
    comm.extend(1000u32.to_be_bytes());
    comm.extend(16u16.to_be_bytes());
    comm.extend([0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0]); // 44100 as an 80-bit float
    let mut ssnd = vec![0u8; 8];
    ssnd.extend(vec![0u8; 4000]);
    let mut body = b"AIFF".to_vec();
    body.extend(chunk(b"COMM", &comm, true));
    body.extend(chunk(b"NAME", b"Door slam", true));
    body.extend(chunk(b"SSND", &ssnd, true));
    let mut bytes = b"FORM".to_vec();
    bytes.extend((body.len() as u32).to_be_bytes());
    bytes.extend(body);
    let p = write(&dir, "d.aif", &bytes);
    let i = probe(&p).unwrap();
    assert_eq!((i.format, i.sample_format, i.sample_rate, i.channels, i.frames), (FileFormat::Aiff, SampleFormat::Int16, 44_100, 2, 1000));
    assert_eq!(i.title, "Door slam");
}

#[test]
fn a_file_that_is_not_audio_is_refused() {
    let dir = TempDir::new("junk");
    let p = write(&dir, "x.wav", b"this is not a sound file at all");
    assert!(probe(&p).is_err());
    assert!(probe(Path::new("/no/such/file.wav")).is_err());
    let p = write(&dir, "zero.wav", &wav(&[fmt_chunk(2, 0, 16, 1), chunk(b"data", &[0u8; 8], false)]));
    assert!(probe(&p).is_err(), "a zero sample rate is refused as the open refuses it");
}
