//! Readers for the containers WavCraft reads itself besides WAV and AIFF (item f91): Sony Wave64,
//! Apple CAF (linear PCM and float; other CAF codecs go to symphonia), Sun/NeXT AU (PCM and
//! float), and headerless raw data described by the person ([`RawSpec`]). All of them stream
//! through `stream_pcm`, a chunk at a time, with the same conversion as WAV, so samples are exact.

use super::*;
use crate::save::{W64_DATA, W64_FMT, W64_RIFF, W64_WAVE};

/// The most channels a raw import accepts.
pub const RAW_MAX_CHANNELS: u16 = 32;
/// The highest rate a raw import accepts (Hz); the person says what the data is.
pub const RAW_MAX_RATE_HZ: u32 = 10_000_000;

/// How the samples of a headerless file are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawEncoding {
    U8,
    S8,
    S16,
    S24,
    S32,
    F32,
    F64,
}

/// What a headerless file holds, as the person gives it (File > Import > Raw Data).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawSpec {
    pub sample_rate: u32,
    pub channels: u16,
    pub encoding: RawEncoding,
    pub big_endian: bool,
    /// Bytes to skip at the start of the file.
    pub offset: u64,
}

impl RawEncoding {
    /// Parse "u8", "s8", "s16", "s24", "s32", "f32", "f64", each optionally followed by `le` or
    /// `be` ("s16le"). Returns the encoding and the byte order when the text names one.
    pub fn parse(text: &str) -> Option<(RawEncoding, Option<bool>)> {
        let t = text.trim().to_ascii_lowercase();
        let (base, order) = if let Some(b) = t.strip_suffix("le") {
            (b, Some(false))
        } else if let Some(b) = t.strip_suffix("be") {
            (b, Some(true))
        } else {
            (t.as_str(), None)
        };
        let enc = match base {
            "u8" => RawEncoding::U8,
            "s8" => RawEncoding::S8,
            "s16" => RawEncoding::S16,
            "s24" => RawEncoding::S24,
            "s32" => RawEncoding::S32,
            "f32" => RawEncoding::F32,
            "f64" => RawEncoding::F64,
            _ => return None,
        };
        Some((enc, order))
    }

    fn kind(self, big_endian: bool) -> (PcmKind, SampleFormat) {
        match self {
            RawEncoding::U8 => (PcmKind::Int { bytes: 1, big_endian, unsigned: true }, SampleFormat::Int8),
            RawEncoding::S8 => (PcmKind::Int { bytes: 1, big_endian, unsigned: false }, SampleFormat::Int8),
            RawEncoding::S16 => (PcmKind::Int { bytes: 2, big_endian, unsigned: false }, SampleFormat::Int16),
            RawEncoding::S24 => (PcmKind::Int { bytes: 3, big_endian, unsigned: false }, SampleFormat::Int24),
            RawEncoding::S32 => (PcmKind::Int { bytes: 4, big_endian, unsigned: false }, SampleFormat::Int32),
            RawEncoding::F32 => (PcmKind::Float { bytes: 4, big_endian }, SampleFormat::Float32),
            RawEncoding::F64 => (PcmKind::Float { bytes: 8, big_endian }, SampleFormat::Float64),
        }
    }
}

pub(super) fn is_w64(b: &[u8]) -> bool {
    b.get(..16) == Some(&W64_RIFF[..]) && b.get(24..40) == Some(&W64_WAVE[..])
}

pub(super) fn is_caf(b: &[u8]) -> bool {
    tag(b, 0).is_some_and(|t| &t == b"caff") && be_u16(b, 4) == Some(1)
}

pub(super) fn is_au(b: &[u8]) -> bool {
    tag(b, 0).is_some_and(|t| &t == b".snd")
}

/// The PCM layout of a WAV-style `fmt` for the containers that carry one (Wave64).
fn wav_kind(fmt: &Fmt, what: &str) -> Result<(PcmKind, SampleFormat), OpenError> {
    match (fmt.tag, fmt.bits) {
        (WAVE_FORMAT_PCM, bits @ 1..=32) => {
            let bytes = usize::from(bits.div_ceil(8));
            let sf = match fmt.valid_bits.min(bits) {
                0..=8 => SampleFormat::Int8,
                9..=16 => SampleFormat::Int16,
                17..=24 => SampleFormat::Int24,
                _ => SampleFormat::Int32,
            };
            Ok((PcmKind::Int { bytes, big_endian: false, unsigned: bytes == 1 }, sf))
        }
        (WAVE_FORMAT_IEEE_FLOAT, 32) => Ok((PcmKind::Float { bytes: 4, big_endian: false }, SampleFormat::Float32)),
        (WAVE_FORMAT_IEEE_FLOAT, 64) => Ok((PcmKind::Float { bytes: 8, big_endian: false }, SampleFormat::Float64)),
        _ => Err(unsupported(format!("{what} format {} with {} bits per sample (only PCM and float are read)", fmt.tag, fmt.bits))),
    }
}

/// Sony Wave64: 16-byte GUIDs, 64-bit chunk sizes that include the chunk's 24-byte header,
/// chunks padded to 8 bytes.
pub(super) fn open_w64(job: &mut Job<'_>, store: &Store, src: &mut SourceFormat) -> Result<Builder, OpenError> {
    src.format = FileFormat::Other;
    let file_len = job.file_len;
    let mut pos: u64 = 40;
    let mut fmt: Option<Fmt> = None;
    let mut data: Option<(u64, u64, u64)> = None; // (start, length used, declared)
    for _ in 0..100_000 {
        let h = job.read_at(pos, 24)?;
        let (Some(guid), Some(size)) = (h.get(..16), le_u64(&h, 16)) else { break };
        if size < 24 {
            return Err(malformed("a Wave64 chunk declares a size under its own 24-byte header"));
        }
        let body = pos + 24;
        let declared = size - 24;
        let remaining = file_len.saturating_sub(body);
        if guid == W64_DATA {
            data = Some((body, declared.min(remaining), declared));
            break;
        }
        if declared > remaining {
            break;
        }
        if guid == W64_FMT {
            fmt = Some(parse_fmt(&job.read_at(body, declared.min(64))?)?);
        }
        pos = size.checked_add(7).map(|s| s & !7).and_then(|s| pos.checked_add(s)).ok_or_else(|| malformed("Wave64 chunk size overflows"))?;
    }
    let fmt = fmt.ok_or_else(|| malformed("Wave64 has no fmt chunk before its data"))?;
    let (start, len, declared) = data.ok_or_else(|| malformed("Wave64 has no data chunk"))?;
    if fmt.channels == 0 {
        return Err(malformed("Wave64 declares zero channels"));
    }
    check_rate(fmt.sample_rate, "Wave64")?;
    src.sample_rate = fmt.sample_rate;
    src.channels = fmt.channels;
    let channels = usize::from(fmt.channels);
    let (kind, sample_format) = wav_kind(&fmt, "Wave64")?;
    src.sample_format = sample_format;
    let block = (kind.bytes() * channels) as u64;
    if u64::from(fmt.block_align) != block {
        return Err(malformed(format!("the Wave64 block-align is {} bytes, but {channels} channel(s) take {block} bytes a frame", fmt.block_align)));
    }
    let frames = len / block;
    if declared > len {
        src.warnings.push(format!("the file is shorter than its header says: the data chunk claims {declared} bytes but {len} are there; opened the {frames} whole frames present"));
    }
    stream_pcm(job, store, start, frames, kind, channels)
}

/// Apple CAF with linear PCM or float audio (any other codec is handed to symphonia).
pub(super) fn open_caf(job: &mut Job<'_>, store: &Store, src: &mut SourceFormat) -> Result<Builder, OpenError> {
    src.format = FileFormat::Caf;
    let file_len = job.file_len;
    let mut pos: u64 = 8;
    let mut desc: Option<(f64, u32, u32, u32, u32)> = None; // rate, flags, bytes/packet, channels, bits
    let mut data: Option<(u64, u64)> = None;
    for _ in 0..100_000 {
        let h = job.read_at(pos, 12)?;
        let (Some(id), Some(size)) = (tag(&h, 0), h.get(4..12).and_then(|b| <[u8; 8]>::try_from(b).ok()).map(i64::from_be_bytes)) else { break };
        let body = pos + 12;
        let remaining = file_len.saturating_sub(body);
        // A size of -1 on `data` means "to the end of the file".
        let declared = if size < 0 { remaining } else { size as u64 };
        match &id {
            b"desc" => {
                let d = job.read_at(body, 32)?;
                let (Some(rate), Some(fid), Some(flags), Some(bpp), Some(fpp), Some(ch), Some(bits)) = (
                    d.get(..8).and_then(|b| <[u8; 8]>::try_from(b).ok()).map(f64::from_be_bytes),
                    tag(&d, 8),
                    be_u32(&d, 12),
                    be_u32(&d, 16),
                    be_u32(&d, 20),
                    be_u32(&d, 24),
                    be_u32(&d, 28),
                ) else {
                    return Err(malformed("CAF desc chunk too short"));
                };
                if &fid != b"lpcm" {
                    let path = src.path.clone();
                    return open_symphonia(&path, Some("caf"), FileFormat::Caf, None, job, store, src);
                }
                if fpp != 1 {
                    return Err(unsupported("CAF linear PCM with more than one frame in a packet"));
                }
                desc = Some((rate, flags, bpp, ch, bits));
            }
            b"data" => {
                if declared < 4 {
                    return Err(malformed("CAF data chunk too short"));
                }
                data = Some((body + 4, declared.min(remaining).saturating_sub(4)));
                break;
            }
            _ => {}
        }
        if declared > remaining {
            break;
        }
        pos = body + declared;
    }
    let (rate, flags, bpp, ch, bits) = desc.ok_or_else(|| malformed("CAF has no desc chunk before its data"))?;
    let (start, len) = data.ok_or_else(|| malformed("CAF has no data chunk"))?;
    if !(rate.is_finite() && rate >= 1.0 && rate <= f64::from(MAX_SAMPLE_RATE_HZ)) {
        return Err(malformed(format!("CAF declares a sample rate of {rate} Hz")));
    }
    let sample_rate = rate.round() as u32;
    check_rate(sample_rate, "CAF")?;
    let channels = u16::try_from(ch).ok().filter(|&c| c > 0).ok_or_else(|| malformed("CAF declares no channels or too many"))?;
    let little = flags & 2 != 0;
    let (kind, sample_format) = match (flags & 1 != 0, bits) {
        (false, 8) => (PcmKind::Int { bytes: 1, big_endian: !little, unsigned: false }, SampleFormat::Int8),
        (false, 16) => (PcmKind::Int { bytes: 2, big_endian: !little, unsigned: false }, SampleFormat::Int16),
        (false, 24) => (PcmKind::Int { bytes: 3, big_endian: !little, unsigned: false }, SampleFormat::Int24),
        (false, 32) => (PcmKind::Int { bytes: 4, big_endian: !little, unsigned: false }, SampleFormat::Int32),
        (true, 32) => (PcmKind::Float { bytes: 4, big_endian: !little }, SampleFormat::Float32),
        (true, 64) => (PcmKind::Float { bytes: 8, big_endian: !little }, SampleFormat::Float64),
        _ => return Err(unsupported(format!("CAF linear PCM with {bits} bits per sample"))),
    };
    let block = (kind.bytes() * usize::from(channels)) as u64;
    if u64::from(bpp) != block {
        return Err(unsupported(format!("CAF packed frames of {bpp} bytes where {block} are expected (padded samples)")));
    }
    src.sample_rate = sample_rate;
    src.channels = channels;
    src.sample_format = sample_format;
    stream_pcm(job, store, start, len / block, kind, usize::from(channels))
}

/// Sun/NeXT AU with linear PCM (8, 16, 24, 32 bit) or float (32, 64 bit), big-endian.
pub(super) fn open_au(job: &mut Job<'_>, store: &Store, src: &mut SourceFormat) -> Result<Builder, OpenError> {
    src.format = FileFormat::Other;
    let file_len = job.file_len;
    let h = job.read_at(0, 24)?;
    let (Some(offset), Some(size), Some(enc), Some(rate), Some(ch)) = (be_u32(&h, 4), be_u32(&h, 8), be_u32(&h, 12), be_u32(&h, 16), be_u32(&h, 20))
    else {
        return Err(malformed("AU header too short"));
    };
    check_rate(rate, "AU")?;
    let channels = u16::try_from(ch).ok().filter(|&c| c > 0).ok_or_else(|| malformed("AU declares no channels or too many"))?;
    let (kind, sample_format) = match enc {
        2 => (PcmKind::Int { bytes: 1, big_endian: true, unsigned: false }, SampleFormat::Int8),
        3 => (PcmKind::Int { bytes: 2, big_endian: true, unsigned: false }, SampleFormat::Int16),
        4 => (PcmKind::Int { bytes: 3, big_endian: true, unsigned: false }, SampleFormat::Int24),
        5 => (PcmKind::Int { bytes: 4, big_endian: true, unsigned: false }, SampleFormat::Int32),
        6 => (PcmKind::Float { bytes: 4, big_endian: true }, SampleFormat::Float32),
        7 => (PcmKind::Float { bytes: 8, big_endian: true }, SampleFormat::Float64),
        other => return Err(unsupported(format!("AU encoding {other} (only linear PCM and float are read)"))),
    };
    let start = u64::from(offset);
    if offset < 24 || start > file_len {
        return Err(malformed(format!("AU declares its data at byte {offset}, outside the file")));
    }
    let remaining = file_len - start;
    let len = if size == u32::MAX { remaining } else { u64::from(size).min(remaining) };
    let block = (kind.bytes() * usize::from(channels)) as u64;
    let frames = len / block;
    if size != u32::MAX && u64::from(size) > remaining {
        src.warnings.push(format!("the file is shorter than its header says: {size} bytes of data claimed but {remaining} are there; opened the {frames} whole frames present"));
    }
    src.sample_rate = rate;
    src.channels = channels;
    src.sample_format = sample_format;
    stream_pcm(job, store, start, frames, kind, usize::from(channels))
}

/// Headerless data as `spec` describes it, streamed in from `spec.offset`.
pub(super) fn open_raw(job: &mut Job<'_>, store: &Store, src: &mut SourceFormat, spec: &RawSpec) -> Result<Builder, OpenError> {
    if spec.sample_rate == 0 || spec.sample_rate > RAW_MAX_RATE_HZ {
        return Err(malformed(format!("a raw import takes a sample rate from 1 Hz to {RAW_MAX_RATE_HZ} Hz, not {}", spec.sample_rate)));
    }
    if spec.channels == 0 || spec.channels > RAW_MAX_CHANNELS {
        return Err(malformed(format!("a raw import takes 1 to {RAW_MAX_CHANNELS} channels, not {}", spec.channels)));
    }
    let (kind, sample_format) = spec.encoding.kind(spec.big_endian);
    if spec.offset > job.file_len {
        return Err(malformed(format!("the start offset {} is past the end of the file ({} bytes)", spec.offset, job.file_len)));
    }
    let block = (kind.bytes() * usize::from(spec.channels)) as u64;
    let avail = job.file_len - spec.offset;
    let frames = avail / block;
    if frames == 0 {
        return Err(OpenError::Empty);
    }
    if !avail.is_multiple_of(block) {
        src.warnings.push(format!("{} trailing byte(s) do not make a whole frame and were ignored", avail % block));
    }
    src.format = FileFormat::Other;
    src.sample_rate = spec.sample_rate;
    src.channels = spec.channels;
    src.sample_format = sample_format;
    stream_pcm(job, store, spec.offset, frames, kind, usize::from(spec.channels))
}
