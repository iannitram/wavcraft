//! What a file is, from its header alone: format, sample type, rate, channels, length and a
//! title (BWF description or `LIST/INFO INAM` in a WAV, `NAME` in an AIFF, the title tag of
//! anything else). It reads a few kilobytes at the start and at each chunk header, never the
//! audio, and makes no session store; File › Import and the Files panel's metadata columns use
//! it so a file can be listed without being opened.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use soundcraft_audio_io::{FileFormat, SampleFormat};
use symphonia::core::codecs::CODEC_TYPE_NULL;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::{MetadataOptions, MetadataRevision, StandardTagKey};
use symphonia::core::probe::Hint;

use crate::io::{
    OpenError, WAVE_FORMAT_IEEE_FLOAT, WAVE_FORMAT_PCM, be_u16, be_u32, check_rate, extended_to_f64, fixed_str, format_from_codec, is_aiff, is_wav,
    le_u32, le_u64, malformed, parse_fmt, sample_format_of, sym_err, tag,
};
use crate::space::frames_from_file_size;

/// What the header of a file says.
#[derive(Debug, Clone, PartialEq)]
pub struct FileInfo {
    pub format: FileFormat,
    pub sample_format: SampleFormat,
    pub sample_rate: u32,
    pub channels: u16,
    /// Length in frames. For a compressed file whose container does not say, a guess from the
    /// file's size (`estimated`).
    pub frames: u64,
    pub estimated: bool,
    /// The size of the file on disk, in bytes.
    pub file_bytes: u64,
    /// The BWF description or INFO / NAME / tag title; empty when the file has none.
    pub title: String,
}

/// Longest title kept, in bytes.
const TITLE_BYTES: u64 = 256;
/// How far into a LIST chunk the INFO entries are looked for.
const LIST_BYTES: u64 = 4096;
const MAX_CHUNKS: usize = 100_000;

fn read_at(file: &mut File, offset: u64, len: u64) -> Result<Vec<u8>, OpenError> {
    file.seek(SeekFrom::Start(offset))?;
    let mut out = Vec::new();
    file.take(len).read_to_end(&mut out)?;
    Ok(out)
}

/// Read the header of `path`. Fails like [`crate::io::open`] does for a file that is not audio
/// or whose header is unusable (a zero rate, no `fmt ` chunk …), so a probe that works is a
/// good sign the open will.
pub fn probe(path: &Path) -> Result<FileInfo, OpenError> {
    let mut file = File::open(path)?;
    let file_bytes = file.metadata()?.len();
    let head = read_at(&mut file, 0, 12)?;
    let ext = path.extension().and_then(|e| e.to_str());
    let (info, title) = if is_wav(&head) {
        probe_wav(&mut file, file_bytes)?
    } else if is_aiff(&head) {
        probe_aiff(&mut file, file_bytes)?
    } else {
        (None, String::new())
    };
    let mut info = match info {
        Some(i) => i,
        None => {
            let detected = if is_wav(&head) {
                FileFormat::Wav
            } else if is_aiff(&head) {
                FileFormat::Aiff
            } else {
                let more = read_at(&mut file, 0, 1 << 12)?;
                soundcraft_audio_io::detect_format(&more, ext)
            };
            let ext = if is_wav(&head) {
                Some("wav")
            } else if is_aiff(&head) {
                Some("aiff")
            } else {
                ext
            };
            let mut i = probe_symphonia(path, file_bytes, ext, detected)?;
            if !title.is_empty() {
                i.title = title.clone();
            }
            i
        }
    };
    info.file_bytes = file_bytes;
    if info.title.is_empty() {
        info.title = title;
    }
    Ok(info)
}

fn clean_title(b: &[u8]) -> String {
    fixed_str(b).chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string()
}

/// `INAM` out of the start of a `LIST` chunk's body, when it is an `INFO` list.
fn info_title(body: &[u8]) -> String {
    if tag(body, 0).is_none_or(|t| &t != b"INFO") {
        return String::new();
    }
    let mut pos = 4usize;
    while let (Some(id), Some(size)) = (tag(body, pos), le_u32(body, pos + 4)) {
        let start = pos + 8;
        let size = size as usize;
        if &id == b"INAM" {
            let end = start.saturating_add(size).min(body.len());
            return clean_title(body.get(start..end).unwrap_or_default());
        }
        pos = start.saturating_add(size).saturating_add(size & 1);
    }
    String::new()
}

fn pcm_sample_format(bits: u16) -> SampleFormat {
    match bits {
        0..=8 => SampleFormat::Int8,
        9..=16 => SampleFormat::Int16,
        17..=24 => SampleFormat::Int24,
        _ => SampleFormat::Int32,
    }
}

/// `(info, title)`; `info` is `None` for a WAV whose codec symphonia has to read (ADPCM …).
fn probe_wav(file: &mut File, file_len: u64) -> Result<(Option<FileInfo>, String), OpenError> {
    let first = read_at(file, 0, 12)?;
    let is_rf64 = tag(&first, 0).is_some_and(|t| &t != b"RIFF");
    let (mut ds64_data, mut fmt, mut data) = (None::<u64>, None, None::<u64>);
    let (mut bext_title, mut info_name) = (String::new(), String::new());
    let mut pos: u64 = 12;
    for _ in 0..MAX_CHUNKS {
        let header = read_at(file, pos, 8)?;
        let (Some(id), Some(size32)) = (tag(&header, 0), le_u32(&header, 4)) else { break };
        let body = pos + 8;
        let remaining = file_len.saturating_sub(body);
        let mut size = u64::from(size32);
        if &id == b"data" && is_rf64 && size32 == u32::MAX {
            size = ds64_data.unwrap_or(remaining);
        }
        let declared = size;
        if &id == b"data" {
            if data.is_none() {
                data = Some(if size32 == 0 && !is_rf64 { remaining } else { declared.min(remaining) });
            }
        } else if declared <= remaining {
            match &id {
                b"fmt " => fmt = Some(parse_fmt(&read_at(file, body, declared.min(64))?)?),
                b"bext" => bext_title = clean_title(&read_at(file, body, declared.min(TITLE_BYTES))?),
                b"ds64" => ds64_data = le_u64(&read_at(file, body, declared.min(64))?, 8),
                b"LIST" if info_name.is_empty() => info_name = info_title(&read_at(file, body, declared.min(LIST_BYTES))?),
                _ => {}
            }
        }
        if declared > remaining {
            break;
        }
        pos = body + declared + (declared & 1);
    }
    let title = if info_name.is_empty() { bext_title } else { info_name };
    let fmt = fmt.ok_or_else(|| malformed("WAV has no fmt chunk"))?;
    if fmt.channels == 0 {
        return Err(malformed("WAV declares zero channels"));
    }
    check_rate(fmt.sample_rate, "WAV")?;
    let data_len = data.ok_or_else(|| malformed("WAV has no data chunk"))?;
    let (sample_format, bytes) = match (fmt.tag, fmt.bits) {
        (WAVE_FORMAT_PCM, bits @ 1..=32) => (pcm_sample_format(fmt.valid_bits.min(bits)), u64::from(bits.div_ceil(8))),
        (WAVE_FORMAT_IEEE_FLOAT, 32) => (SampleFormat::Float32, 4),
        (WAVE_FORMAT_IEEE_FLOAT, 64) => (SampleFormat::Float64, 8),
        _ => return Ok((None, title)),
    };
    let block = (bytes * u64::from(fmt.channels)).max(1);
    let info = FileInfo {
        format: FileFormat::Wav,
        sample_format,
        sample_rate: fmt.sample_rate,
        channels: fmt.channels,
        frames: data_len / block,
        estimated: false,
        file_bytes: file_len,
        title: title.clone(),
    };
    Ok((Some(info), title))
}

fn probe_aiff(file: &mut File, file_len: u64) -> Result<(Option<FileInfo>, String), OpenError> {
    let first = read_at(file, 0, 12)?;
    let is_aifc = tag(&first, 8).is_some_and(|t| &t == b"AIFC");
    let mut comm: Option<(i16, u32, i16, f64, [u8; 4])> = None;
    let mut title = String::new();
    let mut pos: u64 = 12;
    for _ in 0..MAX_CHUNKS {
        let header = read_at(file, pos, 8)?;
        let (Some(id), Some(size32)) = (tag(&header, 0), be_u32(&header, 4)) else { break };
        let body = pos + 8;
        let remaining = file_len.saturating_sub(body);
        let declared = u64::from(size32);
        let len = declared.min(remaining);
        match &id {
            b"COMM" => {
                let c = read_at(file, body, len.min(4096))?;
                let short = || malformed("COMM chunk too short");
                let channels = be_u16(&c, 0).ok_or_else(short)? as i16;
                let frames = be_u32(&c, 2).ok_or_else(short)?;
                let bits = be_u16(&c, 6).ok_or_else(short)? as i16;
                let rate = extended_to_f64(c.get(8..18).ok_or_else(short)?).ok_or_else(|| malformed("invalid sample rate"))?;
                let compression = if is_aifc { tag(&c, 18).ok_or_else(short)? } else { *b"NONE" };
                comm = Some((channels, frames, bits, rate, compression));
            }
            b"NAME" if title.is_empty() => title = clean_title(&read_at(file, body, len.min(TITLE_BYTES))?),
            _ => {}
        }
        if declared > remaining {
            break;
        }
        pos = body + declared + (declared & 1);
    }
    let (channels, frames, bits, rate, compression) = comm.ok_or_else(|| malformed("AIFF has no COMM chunk"))?;
    if channels <= 0 {
        return Err(malformed("AIFF declares zero or negative channels"));
    }
    if !(1.0..=f64::from(crate::io::MAX_SAMPLE_RATE_HZ)).contains(&rate) {
        return Err(malformed(format!("AIFF sample rate {rate} out of range")));
    }
    let sample_format = match &compression {
        b"NONE" | b"twos" | b"sowt" if (1..=32).contains(&bits) => pcm_sample_format(bits as u16),
        b"fl32" | b"FL32" => SampleFormat::Float32,
        b"fl64" | b"FL64" => SampleFormat::Float64,
        _ => return Ok((None, title)),
    };
    let info = FileInfo {
        format: FileFormat::Aiff,
        sample_format,
        sample_rate: rate.round() as u32,
        channels: channels as u16,
        frames: u64::from(frames),
        estimated: false,
        file_bytes: file_len,
        title: title.clone(),
    };
    Ok((Some(info), title))
}

fn revision_title(rev: &MetadataRevision) -> Option<String> {
    rev.tags().iter().find(|t| t.std_key == Some(StandardTagKey::TrackTitle)).map(|t| clean_title(t.value.to_string().as_bytes()))
}

fn probe_symphonia(path: &Path, file_len: u64, ext: Option<&str>, detected: FileFormat) -> Result<FileInfo, OpenError> {
    let file = File::open(path)?;
    let ext = ext.map(str::to_string);
    let result = catch_unwind(AssertUnwindSafe(move || {
        let mss = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());
        let mut hint = Hint::new();
        if let Some(ext) = &ext {
            hint.with_extension(ext.trim_start_matches('.'));
        }
        let mut probed =
            symphonia::default::get_probe().format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default()).map_err(sym_err)?;
        let params = probed
            .format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .map(|t| t.codec_params.clone())
            .ok_or_else(|| malformed("no audio track"))?;
        let rate = params.sample_rate.filter(|&r| r > 0).ok_or_else(|| malformed("unknown sample rate"))?;
        check_rate(rate, "the file")?;
        let channels = params.channels.map_or(0, |c| c.count());
        let (frames, estimated) = match params.n_frames {
            Some(n) if n > 0 => (n, false),
            _ => (frames_from_file_size(file_len, rate), true),
        };
        let mut title = probed.format.metadata().current().and_then(revision_title);
        if title.is_none() {
            title = probed.metadata.get().and_then(|m| m.current().and_then(revision_title));
        }
        Ok(FileInfo {
            format: format_from_codec(params.codec, detected),
            sample_format: sample_format_of(&params),
            sample_rate: rate,
            channels: u16::try_from(channels).unwrap_or(0),
            frames,
            estimated,
            file_bytes: file_len,
            title: title.unwrap_or_default(),
        })
    }));
    result.unwrap_or_else(|_| Err(malformed("the decoder failed on malformed input")))
}
