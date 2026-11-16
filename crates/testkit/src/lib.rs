//! WavCraft's test kit: synthetic audio files that are the same, byte for byte, every time they
//! are written with the same seed.
//!
//! [`write_kit`] writes every file of the kit into a folder (keep it outside the repo: audio is
//! never committed). The 3-hour file is written only when asked for ([`KitOptions::long`]), and the
//! speech-plus-noise mixes only when a clean speech file and a noise file are given.
//!
//! Every WAV here goes through this crate's own streaming writer ([`WavWriter`]), so even the
//! 2 GB file is written a second at a time. The AIFF and FLAC copies of the tone go through
//! `soundcraft-audio-io`'s encoder, which quantises exactly as its decoder reads, so they hold the
//! same samples as the WAV.
//!
//! Sines come from the platform's `f64::sin`: the bytes are the same run after run on one
//! machine, and should be across machines, but that is not promised.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::f64::consts::TAU;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use soundcraft_audio_io::{AudioBuffer, BitDepth, EncodeOptions, FileFormat};

/// The seed used when none is given ("WAVE" in ASCII).
pub const DEFAULT_SEED: u64 = 0x5741_5645;

pub const TONE: &str = "tone-1k-m20-48k-stereo-16.wav";
pub const TONE_AIFF: &str = "tone-1k-m20-48k-stereo-16.aiff";
pub const TONE_FLAC: &str = "tone-1k-m20-48k-stereo-16.flac";
pub const PINK: &str = "pink-48k-mono-24.wav";
pub const LR: &str = "lr-different-48k-stereo-float.wav";
pub const CLICKS: &str = "clicks-44k1-mono-16.wav";
pub const CLICKS_TXT: &str = "clicks-44k1-mono-16.txt";
pub const RAMP: &str = "ramp-48k-mono-16.wav";
pub const TONES: &str = "tones-10min-48k-mono-16.wav";
pub const TONES_TXT: &str = "tones-10min-48k-mono-16.txt";
pub const LONG: &str = "long-3h-48k-stereo-16.wav";
/// Over 4 GiB, so written as RF64: 4 h 10 min of 48 kHz stereo 24-bit (4.32 GB).
pub const HUGE: &str = "huge-4h10m-48k-stereo-24-rf64.wav";
/// The huge file's length in frames.
pub const HUGE_FRAMES: u64 = (4 * 3600 + 10 * 60) * 48_000;
pub const SPEECH_CLEAN: &str = "speech-clean.wav";
pub const SPEECH_SNR10: &str = "speech-plus-noise-snr10.wav";
pub const SPEECH_SNR20: &str = "speech-plus-noise-snr20.wav";
pub const SPEECH_TXT: &str = "speech-plus-noise.txt";

/// What a kit audio file should open as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Expect {
    pub name: &'static str,
    pub channels: usize,
    pub sample_rate: u32,
    pub frames: usize,
}

/// The audio files [`write_kit`] writes without speech input, with what each should open as.
pub fn expected(long: bool) -> Vec<Expect> {
    let e = |name, channels, sample_rate, frames| Expect { name, channels, sample_rate, frames };
    let mut files = vec![
        e(TONE, 2, 48_000, 10 * 48_000),
        e(TONE_AIFF, 2, 48_000, 10 * 48_000),
        e(TONE_FLAC, 2, 48_000, 10 * 48_000),
        e(PINK, 1, 48_000, 60 * 48_000),
        e(LR, 2, 48_000, 30 * 48_000),
        e(CLICKS, 1, 44_100, 60 * 44_100),
        e(RAMP, 1, 48_000, 10 * 48_000),
        e(TONES, 1, 48_000, 600 * 48_000),
    ];
    if long {
        files.push(e(LONG, 2, 48_000, 3 * 3600 * 48_000));
    }
    files
}

/// What to write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KitOptions {
    /// Same seed, same bytes.
    pub seed: u64,
    /// Also write the 3-hour file (about 2 GB).
    pub long: bool,
    /// Also write the 4 h 10 min RF64 file (about 4.3 GB).
    pub huge: bool,
    /// A clean speech WAV and a noise WAV to mix at 10 and 20 dB SNR.
    pub speech: Option<(PathBuf, PathBuf)>,
}

impl Default for KitOptions {
    fn default() -> Self {
        KitOptions { seed: DEFAULT_SEED, long: false, huge: false, speech: None }
    }
}

/// Write the kit into `dir` (made if missing). Returns the paths written, in order.
pub fn write_kit(dir: &Path, opts: &KitOptions) -> io::Result<Vec<PathBuf>> {
    std::fs::create_dir_all(dir)?;
    let mut written = Vec::new();
    let seed = opts.seed;
    written.extend(tone(dir)?);
    written.push(pink(dir, seed)?);
    written.push(left_right(dir)?);
    written.extend(clicks(dir, seed)?);
    written.push(ramp(dir)?);
    written.extend(tones(dir, seed)?);
    if opts.long {
        written.push(long(dir, seed)?);
    }
    if opts.huge {
        written.push(huge(dir)?);
    }
    if let Some((speech, noise)) = &opts.speech {
        written.extend(speech_plus_noise(dir, speech, noise)?);
    }
    Ok(written)
}

// ---------------------------------------------------------------------------------------------
// Random numbers

/// SplitMix64: small, fast, and the same sequence on every platform.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    /// A generator for one stream: the kit's seed mixed with a name, so each file has its own.
    pub fn new(seed: u64, stream: &str) -> Rng {
        let mixed = stream.bytes().fold(seed ^ 0xCBF2_9CE4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01B3));
        Rng(mixed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in [-1, 1).
    pub fn signed(&mut self) -> f64 {
        self.unit() * 2.0 - 1.0
    }

    /// Uniform in [lo, hi).
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }

    /// Uniform in [0, n).
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next_u64() % n }
    }
}

// ---------------------------------------------------------------------------------------------
// WAV writing

/// How samples are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Int16,
    Int24,
    Float32,
}

impl Encoding {
    fn bytes(self) -> usize {
        match self {
            Encoding::Int16 => 2,
            Encoding::Int24 => 3,
            Encoding::Float32 => 4,
        }
    }
}

/// A WAV file written front to back: the header first (the length is known in advance), then
/// interleaved samples in as many calls as suit the caller. Integer samples are
/// `round(x * 2^(bits-1))`, clamped: the exact inverse of how `soundcraft-audio-io` reads them.
pub struct WavWriter {
    out: BufWriter<File>,
    encoding: Encoding,
    /// Samples (not frames) still owed.
    remaining: u64,
    data_len: u64,
    bytes: Vec<u8>,
}

impl WavWriter {
    /// Create `path` for `frames` frames of `channels` channels. Above 4 GiB the file is RF64
    /// (`RF64` with a `ds64` chunk holding the 64-bit sizes, EBU Tech 3306).
    pub fn create(path: &Path, channels: u16, sample_rate: u32, encoding: Encoding, frames: u64) -> io::Result<WavWriter> {
        let bytes = encoding.bytes() as u64;
        let data_len = frames.checked_mul(u64::from(channels)).and_then(|s| s.checked_mul(bytes)).ok_or_else(|| invalid("WAV too long"))?;
        let float = encoding == Encoding::Float32;
        // fmt (16, or 18 with cbSize for a non-PCM tag) and, for float, a fact chunk.
        let fmt_len: u32 = if float { 18 } else { 16 };
        let fact = if float { 12 } else { 0 };
        let riff_len = 4 + (8 + u64::from(fmt_len)) + fact + 8 + data_len + (data_len & 1);
        let rf64 = u32::try_from(riff_len).is_err();
        let block_align = channels.checked_mul(bytes as u16).ok_or_else(|| invalid("too many channels"))?;
        let mut out = BufWriter::with_capacity(1 << 20, File::create(path)?);
        if rf64 {
            // ds64: RIFF size, data size, sample count (frames), table length.
            out.write_all(b"RF64")?;
            out.write_all(&u32::MAX.to_le_bytes())?;
            out.write_all(b"WAVEds64")?;
            out.write_all(&28u32.to_le_bytes())?;
            out.write_all(&(riff_len + 36).to_le_bytes())?;
            out.write_all(&data_len.to_le_bytes())?;
            out.write_all(&frames.to_le_bytes())?;
            out.write_all(&0u32.to_le_bytes())?;
            out.write_all(b"fmt ")?;
        } else {
            out.write_all(b"RIFF")?;
            out.write_all(&(riff_len as u32).to_le_bytes())?;
            out.write_all(b"WAVEfmt ")?;
        }
        out.write_all(&fmt_len.to_le_bytes())?;
        out.write_all(&(if float { 3u16 } else { 1u16 }).to_le_bytes())?;
        out.write_all(&channels.to_le_bytes())?;
        out.write_all(&sample_rate.to_le_bytes())?;
        let byte_rate = sample_rate.checked_mul(u32::from(block_align)).ok_or_else(|| invalid("sample rate too high"))?;
        out.write_all(&byte_rate.to_le_bytes())?;
        out.write_all(&block_align.to_le_bytes())?;
        out.write_all(&(bytes as u16 * 8).to_le_bytes())?;
        if float {
            out.write_all(&0u16.to_le_bytes())?;
            out.write_all(b"fact")?;
            out.write_all(&4u32.to_le_bytes())?;
            out.write_all(&u32::try_from(frames).unwrap_or(u32::MAX).to_le_bytes())?;
        }
        out.write_all(b"data")?;
        out.write_all(&u32::try_from(data_len).unwrap_or(u32::MAX).to_le_bytes())?;
        Ok(WavWriter { out, encoding, remaining: frames * u64::from(channels), data_len, bytes: Vec::new() })
    }

    /// Append interleaved samples (full scale is ±1.0).
    pub fn write(&mut self, interleaved: &[f64]) -> io::Result<()> {
        let n = interleaved.len() as u64;
        if n > self.remaining {
            return Err(invalid("more samples than the header promised"));
        }
        self.remaining -= n;
        self.bytes.clear();
        for &x in interleaved {
            match self.encoding {
                Encoding::Int16 => self.bytes.extend_from_slice(&(quantize(x, 16) as i16).to_le_bytes()),
                Encoding::Int24 => self.bytes.extend_from_slice(&quantize(x, 24).to_le_bytes()[..3]),
                Encoding::Float32 => self.bytes.extend_from_slice(&(x as f32).to_le_bytes()),
            }
        }
        self.out.write_all(&self.bytes)
    }

    /// Check every promised sample was written, pad, and flush.
    pub fn finish(mut self) -> io::Result<()> {
        if self.remaining != 0 {
            return Err(invalid("fewer samples than the header promised"));
        }
        if self.data_len & 1 == 1 {
            self.out.write_all(&[0])?;
        }
        self.out.flush()
    }
}

/// `x` (full scale ±1) as a `bits`-bit integer, rounded and clamped.
pub fn quantize(x: f64, bits: u32) -> i32 {
    let scale = f64::from(1u32 << (bits - 1));
    let x = if x.is_nan() { 0.0 } else { x };
    (x * scale).round().clamp(-scale, scale - 1.0) as i32
}

fn invalid(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what.to_string())
}

/// Write a whole file from a generator of one interleaved second (or less) at a time.
fn write_wav(
    path: &Path,
    channels: u16,
    rate: u32,
    encoding: Encoding,
    frames: u64,
    mut chunk: impl FnMut(u64, &mut Vec<f64>),
) -> io::Result<PathBuf> {
    let mut wav = WavWriter::create(path, channels, rate, encoding, frames)?;
    let mut buf = Vec::new();
    let mut at = 0u64;
    while at < frames {
        let n = (frames - at).min(u64::from(rate));
        buf.clear();
        for frame in at..at + n {
            chunk(frame, &mut buf);
        }
        wav.write(&buf)?;
        at += n;
    }
    wav.finish()?;
    Ok(path.to_path_buf())
}

/// A sine of `hz` at frame `n` of a file at `rate`, with the phase taken modulo one second so
/// that long files lose no precision (every frequency here is a whole number of Hz).
fn sine(hz: u32, n: u64, rate: u32) -> f64 {
    let rate64 = u64::from(rate);
    let cycle = (u64::from(hz) * (n % rate64)) % rate64;
    (TAU * cycle as f64 / rate64 as f64).sin()
}

// ---------------------------------------------------------------------------------------------
// The files

/// 10 s of 1 kHz at -20 dBFS peak (amplitude 0.1), 48 kHz stereo 16-bit, plus AIFF and FLAC copies.
fn tone(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let wav = write_wav(&dir.join(TONE), 2, 48_000, Encoding::Int16, 10 * 48_000, |n, out| {
        let v = 0.1 * sine(1_000, n, 48_000);
        out.extend([v, v]);
    })?;
    // The copies hold the WAV's own samples, read back through the decoder WavCraft uses.
    let bytes = std::fs::read(&wav)?;
    let (_, audio) = soundcraft_audio_io::decode(&bytes, Some("wav")).map_err(|e| invalid(&e.to_string()))?;
    let mut paths = vec![wav];
    for (name, format) in [(TONE_AIFF, FileFormat::Aiff), (TONE_FLAC, FileFormat::Flac)] {
        let opts = EncodeOptions { format, bit_depth: BitDepth::Int16, dither: false, bwf: None };
        let encoded = soundcraft_audio_io::encode(&audio, &opts).map_err(|e| invalid(&e.to_string()))?;
        let path = dir.join(name);
        std::fs::write(&path, encoded)?;
        paths.push(path);
    }
    Ok(paths)
}

/// The level of `x` as RMS in dBFS (a full-scale square wave reads 0).
pub fn rms_dbfs(x: &[f64]) -> f64 {
    let power = x.iter().map(|v| v * v).sum::<f64>() / x.len().max(1) as f64;
    10.0 * power.max(1e-30).log10()
}

/// 60 s of pink noise at -30 dBFS RMS, 48 kHz mono 24-bit. Voss-McCartney: 16 rows of white
/// noise, row `k` redrawn every `2^k` samples, summed with one fresh white value per sample.
fn pink(dir: &Path, seed: u64) -> io::Result<PathBuf> {
    let frames = 60 * 48_000usize;
    let mut rng = Rng::new(seed, PINK);
    let mut rows = [0.0f64; 16];
    for r in rows.iter_mut() {
        *r = rng.signed();
    }
    let mut x: Vec<f64> = (0..frames)
        .map(|n| {
            if n > 0 {
                let k = (n.trailing_zeros() as usize).min(rows.len() - 1);
                if let Some(r) = rows.get_mut(k) {
                    *r = rng.signed();
                }
            }
            rows.iter().sum::<f64>() + rng.signed()
        })
        .collect();
    let mean = x.iter().sum::<f64>() / frames as f64;
    x.iter_mut().for_each(|v| *v -= mean);
    let gain = 10f64.powf((-30.0 - rms_dbfs(&x)) / 20.0);
    write_wav(&dir.join(PINK), 1, 48_000, Encoding::Int24, frames as u64, |n, out| out.push(x.get(n as usize).copied().unwrap_or(0.0) * gain))
}

/// 30 s, 440 Hz on the left and 660 Hz on the right, each at -6 dBFS peak, 48 kHz 32-bit float.
fn left_right(dir: &Path) -> io::Result<PathBuf> {
    write_wav(&dir.join(LR), 2, 48_000, Encoding::Float32, 30 * 48_000, |n, out| {
        out.extend([0.5 * sine(440, n, 48_000), 0.5 * sine(660, n, 48_000)]);
    })
}

/// Where the clicks go in a file of `frames` frames: the edges, both sides of the waveform
/// peaks' block and level boundaries and of a second, then 40 places chosen by the seed.
/// Sorted, no repeats. The values alternate +0.5 and -0.5 of full scale (16384 and -16384).
pub fn click_positions(seed: u64, frames: u64) -> Vec<(u64, i32)> {
    let mut rng = Rng::new(seed, CLICKS);
    let mut at: Vec<u64> =
        vec![0, 1, 31, 32, 33, 127, 128, 1_023, 1_024, 44_099, 44_100, 44_101, frames / 2, frames.saturating_sub(2), frames.saturating_sub(1)];
    at.extend((0..40).map(|_| rng.below(frames)));
    at.retain(|f| *f < frames);
    at.sort_unstable();
    at.dedup();
    at.iter().enumerate().map(|(i, &f)| (f, if i % 2 == 0 { 16_384 } else { -16_384 })).collect()
}

/// 60 s of digital silence with one-sample clicks, 44.1 kHz mono 16-bit, and a sidecar listing them.
fn clicks(dir: &Path, seed: u64) -> io::Result<Vec<PathBuf>> {
    let frames = 60 * 44_100u64;
    let positions = click_positions(seed, frames);
    let mut next = positions.iter().peekable();
    let wav = write_wav(&dir.join(CLICKS), 1, 44_100, Encoding::Int16, frames, |n, out| {
        let value = match next.peek() {
            Some((at, value)) if *at == n => {
                next.next();
                f64::from(*value) / 32_768.0
            }
            _ => 0.0,
        };
        out.push(value);
    })?;
    let mut text = format!("# {CLICKS}: one-sample clicks in silence, 44100 Hz mono 16-bit, {frames} frames\n# frame value\n");
    for (at, value) in &positions {
        text.push_str(&format!("{at} {value}\n"));
    }
    let txt = dir.join(CLICKS_TXT);
    std::fs::write(&txt, text)?;
    Ok(vec![wav, txt])
}

/// The value sample `n` of the ramp file holds: `(n mod 65536) - 32768`.
pub fn ramp_value(n: u64) -> i32 {
    (n % 65_536) as i32 - 32_768
}

/// 10 s where sample `n` holds the 16-bit value `(n mod 65536) - 32768`, 48 kHz mono.
fn ramp(dir: &Path) -> io::Result<PathBuf> {
    write_wav(&dir.join(RAMP), 1, 48_000, Encoding::Int16, 10 * 48_000, |n, out| out.push(f64::from(ramp_value(n)) / 32_768.0))
}

/// The tones in the 10-minute file: (start in seconds, length in seconds, Hz).
pub const TONE_LIST: [(f64, f64, u32); 4] = [(60.0, 0.05, 3_000), (180.0, 1.0, 1_000), (300.0, 5.0, 500), (420.0, 20.0, 6_000)];

/// 10 minutes of quiet hiss (about -65 dBFS RMS) holding four tones of 50 ms to 20 s at -6 dBFS
/// peak, each with 1 ms fades, 48 kHz mono 16-bit, and a sidecar listing them. The spectrogram's
/// "nothing lost when zoomed out" check, by eye.
fn tones(dir: &Path, seed: u64) -> io::Result<Vec<PathBuf>> {
    let rate = 48_000u32;
    let mut rng = Rng::new(seed, TONES);
    let spans: Vec<(u64, u64, u32)> =
        TONE_LIST.iter().map(|(start, len, hz)| ((start * f64::from(rate)) as u64, (len * f64::from(rate)) as u64, *hz)).collect();
    let fade = u64::from(rate) / 1_000;
    let wav = write_wav(&dir.join(TONES), 1, rate, Encoding::Int16, 600 * u64::from(rate), |n, out| {
        let mut v = rng.signed() * 0.001;
        for (start, len, hz) in &spans {
            if n >= *start && n < start + len {
                let k = (n - start).min(start + len - 1 - n);
                let ramp = if k < fade { 0.5 - 0.5 * (std::f64::consts::PI * k as f64 / fade as f64).cos() } else { 1.0 };
                v += 0.5 * ramp * sine(*hz, n - start, rate);
            }
        }
        out.push(v);
    })?;
    let mut text = format!("# {TONES}: tones in -65 dBFS hiss, 48000 Hz mono 16-bit\n# start_s length_s hz\n");
    for (start, len, hz) in TONE_LIST {
        text.push_str(&format!("{start} {len} {hz}\n"));
    }
    let txt = dir.join(TONES_TXT);
    std::fs::write(&txt, text)?;
    Ok(vec![wav, txt])
}

/// One-pole low-pass state.
#[derive(Debug, Clone, Copy, Default)]
struct OnePole {
    y: f64,
}

impl OnePole {
    fn run(&mut self, x: f64, a: f64) -> f64 {
        self.y += a * (x - self.y);
        self.y
    }
}

/// The coefficient of a one-pole low-pass at `hz` for `rate`.
fn pole(hz: f64, rate: f64) -> f64 {
    1.0 - (-TAU * hz / rate).exp()
}

/// 3 hours, 48 kHz stereo 16-bit (about 2 GB): two "speakers" taking turns. Each turn is band-
/// limited noise (about 150 Hz to 3 kHz) shaped into syllables at 3 to 6 per second, at about
/// -20 dBFS RMS while talking, 0.4 to 4 s long, with 0.15 to 1.5 s gaps. The other channel hears
/// the talker at -20 dB, and both always carry room noise at about -65 dBFS RMS. Not speech: a
/// load with the shape of a two-person podcast, for speed and memory.
fn long(dir: &Path, seed: u64) -> io::Result<PathBuf> {
    let rate = 48_000.0;
    let frames = 3 * 3600 * 48_000u64;
    let mut turns = Rng::new(seed, "long-turns");
    let mut noise = [Rng::new(seed, "long-noise-l"), Rng::new(seed, "long-noise-r")];
    let mut voice = Rng::new(seed, "long-voice");
    let (low, high) = (pole(3_000.0, rate), pole(150.0, rate));
    let (mut lp, mut hp) = (OnePole::default(), OnePole::default());
    // The current turn: who talks (0, 1, or 2 for a gap), until which frame, at what syllable rate.
    let (mut talker, mut until, mut syllables) = (2usize, 0u64, 4.0f64);
    let mut phase = 0.0f64;
    write_wav(&dir.join(LONG), 2, 48_000, Encoding::Int16, frames, |n, out| {
        if n >= until {
            if talker == 2 {
                talker = turns.below(2) as usize;
                until = n + (turns.range(0.4, 4.0) * rate) as u64;
                syllables = turns.range(3.0, 6.0);
            } else {
                talker = 2;
                until = n + (turns.range(0.15, 1.5) * rate) as u64;
            }
        }
        let band = {
            let x = voice.signed();
            let l = lp.run(x, low);
            l - hp.run(l, high)
        };
        let speech = if talker < 2 {
            phase = (phase + syllables / rate).fract();
            // Syllables: a rectified sine envelope, so each one rises and falls.
            band * 0.6 * (std::f64::consts::PI * phase).sin()
        } else {
            0.0
        };
        let [nl, nr] = &mut noise;
        let (l, r) = (nl.signed() * 0.001, nr.signed() * 0.001);
        let (gl, gr) = match talker {
            0 => (1.0, 0.1),
            1 => (0.1, 1.0),
            _ => (0.0, 0.0),
        };
        out.extend([l + speech * gl, r + speech * gr]);
    })
}

/// The value the huge file's right channel holds at frame `n`, as a 24-bit integer: a counter
/// that wraps every 2^24 frames (5.8 minutes), so any frame's position can be checked.
pub fn huge_counter(n: u64) -> i32 {
    (n % (1 << 24)) as i32 - (1 << 23)
}

/// 4 h 10 min, 48 kHz stereo 24-bit, as RF64: a 440 Hz sine at -12 dBFS on the left, and the
/// frame counter [`huge_counter`] on the right. Proves files over 4 GiB (and past the old 2^30
/// sample decode limit) open.
fn huge(dir: &Path) -> io::Result<PathBuf> {
    let table: Vec<f64> = (0..48_000u64).map(|n| 0.25 * sine(440, n, 48_000)).collect();
    write_wav(&dir.join(HUGE), 2, 48_000, Encoding::Int24, HUGE_FRAMES, |n, out| {
        out.extend([table.get((n % 48_000) as usize).copied().unwrap_or(0.0), f64::from(huge_counter(n)) / f64::from(1u32 << 23)]);
    })
}

/// Mix a clean speech file with a noise file at 10 dB and 20 dB SNR, and write the clean
/// reference beside them (all 32-bit float, so nothing clips). SNR is over the whole files: the
/// speech file's mean power (gaps included) over the noise's. The noise loops if it is shorter;
/// a stereo noise file feeds its channels in turn to the speech's. Both must have the same rate.
fn speech_plus_noise(dir: &Path, speech: &Path, noise: &Path) -> io::Result<Vec<PathBuf>> {
    let read = |path: &Path| -> io::Result<AudioBuffer> {
        let bytes = std::fs::read(path)?;
        let ext = path.extension().and_then(|e| e.to_str());
        let (_, audio) = soundcraft_audio_io::decode(&bytes, ext).map_err(|e| invalid(&format!("{}: {e}", path.display())))?;
        Ok(audio)
    };
    let (speech_audio, noise_audio) = (read(speech)?, read(noise)?);
    if speech_audio.sample_rate != noise_audio.sample_rate {
        return Err(invalid(&format!("speech is {} Hz but noise is {} Hz", speech_audio.sample_rate, noise_audio.sample_rate)));
    }
    let (frames, channels) = (speech_audio.frames(), speech_audio.num_channels());
    let noise_frames = noise_audio.frames();
    if frames == 0 || channels == 0 || noise_frames == 0 || noise_audio.num_channels() == 0 {
        return Err(invalid("speech or noise holds no audio"));
    }
    let noise_at = |ch: usize, n: usize| -> f64 {
        let src = noise_audio.channels.get(ch % noise_audio.num_channels()).map_or(&[][..], |v| v.as_slice());
        f64::from(src.get(n % noise_frames).copied().unwrap_or(0.0))
    };
    let speech_at = |ch: usize, n: usize| -> f64 { f64::from(speech_audio.channels.get(ch).and_then(|c| c.get(n)).copied().unwrap_or(0.0)) };
    let power =
        |f: &dyn Fn(usize, usize) -> f64| (0..channels).flat_map(|ch| (0..frames).map(move |n| (ch, n))).map(|(ch, n)| f(ch, n).powi(2)).sum::<f64>();
    let (ps, pn) = (power(&speech_at), power(&noise_at));
    if pn <= 0.0 || ps <= 0.0 {
        return Err(invalid("speech or noise is silent"));
    }
    let rate = speech_audio.sample_rate;
    let ch16 = u16::try_from(channels).map_err(|_| invalid("too many channels"))?;
    let mut written = Vec::new();
    let mut notes = format!("# speech: {}\n# noise: {}\n# snr_db noise_gain file\n", speech.display(), noise.display());
    for (name, gain) in [(SPEECH_CLEAN, 0.0), (SPEECH_SNR10, (ps / (pn * 10.0)).sqrt()), (SPEECH_SNR20, (ps / (pn * 100.0)).sqrt())] {
        let path = write_wav(&dir.join(name), ch16, rate, Encoding::Float32, frames as u64, |n, out| {
            let n = n as usize;
            out.extend((0..channels).map(|ch| speech_at(ch, n) + gain * noise_at(ch, n)));
        })?;
        if gain > 0.0 {
            let snr = 10.0 * (ps / (pn * gain * gain)).log10();
            notes.push_str(&format!("{snr:.1} {gain:.6} {name}\n"));
        }
        written.push(path);
    }
    let txt = dir.join(SPEECH_TXT);
    std::fs::write(&txt, notes)?;
    written.push(txt);
    Ok(written)
}

// ---------------------------------------------------------------------------------------------
// Random edits (task 04's long test and the edit bench)

/// One random edit made and pushed by [`random_edit`].
#[derive(Debug, Clone)]
pub struct RandomEdit {
    pub label: String,
    /// Time to make the new state.
    pub edit: std::time::Duration,
    /// Time for `History::push_edit`, the disk-cap sum included.
    pub push: std::time::Duration,
    /// Steps behind the cursor when it was pushed.
    pub behind: usize,
}

/// Make one random edit on the history's current state and push it: Delete (all channels or one),
/// Cut, Copy then Paste (insert or over a range), Silence, Insert Silence, a gentle Crop (keeps
/// 99 %), or a `replace_range` gain of up to 10 s. Ranges are up to `max_range` frames.
pub fn random_edit(
    h: &mut wavcraft_core::History,
    rng: &mut Rng,
    xf: wavcraft_core::Crossfades,
    max_range: u64,
    clipboard: &mut Option<wavcraft_core::Clip>,
) -> Result<RandomEdit, wavcraft_core::EditError> {
    use wavcraft_core::doc::edit::{self, ChannelMask, PasteAt};
    let doc = h.current().clone();
    let len = doc.len_frames();
    let n = doc.channel_count();
    let s = rng.below(len + 1);
    let e = (s + rng.below(max_range.min(len - s) + 1)).min(len);
    let one = ChannelMask::only(rng.below(n as u64) as usize);
    let t = std::time::Instant::now();
    let ed = match rng.below(9) {
        0 => edit::delete(&doc, s..e, ChannelMask::ALL, xf)?,
        1 => edit::delete(&doc, s..e, one, xf)?,
        2 => {
            let (ed, clip) = edit::cut(&doc, s..e, ChannelMask::ALL, xf)?;
            *clipboard = Some(clip);
            ed
        }
        3 | 4 => {
            let clip = match clipboard.take() {
                Some(c) => c,
                None => edit::copy(&doc, s..e, ChannelMask::ALL)?,
            };
            let at = rng.below(len + 1);
            let target = if rng.below(2) == 0 { PasteAt::Insert(at) } else { PasteAt::Replace(at..(at + rng.below(max_range) + 1).min(len)) };
            let ed = edit::paste(&doc, target, &clip, ChannelMask::ALL, xf)?;
            *clipboard = Some(clip);
            ed
        }
        5 => edit::silence(&doc, s..e, if rng.below(2) == 0 { ChannelMask::ALL } else { one }, xf)?,
        6 => edit::insert_silence(&doc, s, rng.below(10 * u64::from(doc.sample_rate())) + 1, xf)?,
        7 => {
            let keep = len - len / 100;
            let from = rng.below(len - keep + 1);
            edit::crop(&doc, from..from + keep, ChannelMask::ALL, xf)?
        }
        _ => {
            let e = (s + rng.below(10 * u64::from(doc.sample_rate())) + 1).min(len);
            let gain = 10f32.powf(rng.range(-12.0, 6.0) as f32 / 20.0);
            edit::replace_range(&doc, s..e, ChannelMask::ALL, "Amplify", xf, &std::sync::atomic::AtomicBool::new(false), |_, i, o| {
                for (a, b) in i.iter().zip(o.iter_mut()) {
                    for (x, y) in a.iter().zip(b.iter_mut()) {
                        *y = x * gain;
                    }
                }
                Ok(())
            })?
        }
    };
    let made = t.elapsed();
    let label = ed.label.clone();
    let behind = h.cursor();
    let t = std::time::Instant::now();
    h.push_edit(ed);
    Ok(RandomEdit { label, edit: made, push: t.elapsed(), behind })
}

/// A hash of every sample of every channel, read 64 K frames at a time.
pub fn content_hash(doc: &wavcraft_core::DocState) -> io::Result<u64> {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let mut buf = vec![0.0f32; wavcraft_core::store::BLOCK_FRAMES];
    doc.len_frames().hash(&mut h);
    for c in 0..doc.channel_count() {
        let mut at = 0;
        while at < doc.len_frames() {
            let n = doc.read(c, at, &mut buf)?;
            if n == 0 {
                return Err(io::Error::other("document shorter than its length"));
            }
            for x in buf.iter().take(n) {
                x.to_bits().hash(&mut h);
            }
            at += n as u64;
        }
    }
    Ok(h.finish())
}

// ---------------------------------------------------------------------------------------------
// Noise Reduction measure (G2.7; the wavcraft-nr-bench binary prints it)

/// The Noise Reduction settings [`nr_measure`] runs with. The default is Audition's: Reduce By
/// 16 dB, Noise Reduction 50 %, FFT size 4096.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NrSettings {
    pub reduce_db: f64,
    pub reduction: f64,
    pub fft_size: u32,
}

impl Default for NrSettings {
    fn default() -> NrSettings {
        NrSettings { reduce_db: 16.0, reduction: 50.0, fft_size: 4096 }
    }
}

/// What [`nr_measure`] found, all in dB except `print_frames`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NrMeasure {
    /// The level of the gaps before minus after (positive when the floor dropped).
    pub floor_drop_db: f64,
    /// The level of the speech windows after minus before (0 when speech is untouched).
    pub speech_change_db: f64,
    /// The length in frames of the stretch the noise print was captured from.
    pub print_frames: u64,
}

/// Run Noise Reduction on `noisy` and measure it against `clean` (the same audio without the
/// noise; same rate, length and channel count, or the answer is an error). Channel 0 is measured;
/// every channel is processed.
///
/// Windows are 100 ms. A window is a gap when the clean file's power in it is at most 1e-4 (-40 dB)
/// of the loudest window's, and speech when it is at least 1e-2 (-20 dB) of it. The print is
/// captured from the longest run of gap windows in `noisy` (an error if the run is under 4096
/// frames, or there is no gap or no speech window). The print is the session-wide one in
/// `wavcraft_core::noise`, and this replaces it.
pub fn nr_measure(clean: &wavcraft_core::DocState, noisy: &wavcraft_core::DocState, s: &NrSettings) -> Result<NrMeasure, String> {
    use wavcraft_core::actions::Editor;
    use wavcraft_core::doc::history::History;
    use wavcraft_core::noise;

    if clean.sample_rate() != noisy.sample_rate() || clean.len_frames() != noisy.len_frames() || clean.channel_count() != noisy.channel_count() {
        return Err("the clean and noisy files differ in rate, length or channel count".to_string());
    }
    let window = (u64::from(noisy.sample_rate()) / 10).max(1);
    let windows = noisy.len_frames() / window;
    let read = |doc: &wavcraft_core::DocState, w: u64| -> Result<Vec<f32>, String> {
        let mut buf = vec![0.0f32; window as usize];
        let n = doc.read(0, w * window, &mut buf).map_err(|e| e.to_string())?;
        buf.truncate(n);
        Ok(buf)
    };
    let power = |x: &[f32]| x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>();
    let clean_power: Vec<f64> = (0..windows).map(|w| read(clean, w).map(|x| power(&x))).collect::<Result<_, _>>()?;
    let loudest = clean_power.iter().copied().fold(0.0f64, f64::max);
    let is_gap = |w: u64| clean_power.get(w as usize).is_some_and(|p| *p <= loudest * 1e-4);
    let is_speech = |w: u64| clean_power.get(w as usize).is_some_and(|p| *p >= loudest * 1e-2);
    let (mut best, mut run_start, mut run) = ((0u64, 0u64), 0u64, 0u64);
    for w in 0..=windows {
        if w < windows && is_gap(w) {
            if run == 0 {
                run_start = w;
            }
            run += 1;
            if run > best.1 {
                best = (run_start, run);
            }
        } else {
            run = 0;
        }
    }
    let print_frames = best.1 * window;
    if print_frames < 4096 || !(0..windows).any(is_speech) {
        return Err("the clean file needs a gap of at least 4096 frames and some speech".to_string());
    }
    let print = noise::capture(noisy, best.0 * window..best.0 * window + print_frames)?;
    noise::set_current(print);

    let (index, params) = nr_effect(s)?;
    let mut editor = Editor::new(History::new(noisy.clone(), u64::MAX));
    editor.run_effect(index, params, 0..0).map_err(|e| e.to_string())?;
    let after = editor.history.current();

    let (mut gap_before, mut gap_after, mut speech_before, mut speech_after) = (0.0, 0.0, 0.0, 0.0);
    for w in 0..windows {
        let (gap, speech) = (is_gap(w), is_speech(w));
        if gap || speech {
            let (before, now) = (power(&read(noisy, w)?), power(&read(after, w)?));
            if gap {
                gap_before += before;
                gap_after += now;
            } else {
                speech_before += before;
                speech_after += now;
            }
        }
    }
    let db = |a: f64, b: f64| 10.0 * (a.max(1e-30) / b.max(1e-30)).log10();
    Ok(NrMeasure { floor_drop_db: db(gap_before, gap_after), speech_change_db: db(speech_after, speech_before), print_frames })
}

/// The Noise Reduction effect's index and parameters for `s`, as the editor runs them.
fn nr_effect(s: &NrSettings) -> Result<(u16, Vec<f64>), String> {
    use wavcraft_core::effects;

    let index = effects::all().iter().position(|e| e.id() == "noise_reduction").ok_or("no Noise Reduction effect")?;
    let text = format!("reduce_db={} reduction={} fft_size={}", s.reduce_db, s.reduction, s.fft_size);
    let params = effects::parse_params(effects::all()[index], &text)?;
    Ok((u16::try_from(index).map_err(|e| e.to_string())?, params))
}

/// Builds a synthetic stereo 48 kHz file of 3600 s times `scale`, runs Noise Reduction at `s` on
/// all of it, and returns the seconds the run took. Building the file is not timed. The file is
/// written block by block (never whole in RAM) into a `nr-time` folder under `dir`, which the
/// caller removes. `scale` must be finite, at most 100, and give at least 4 s of audio.
pub fn nr_time(scale: f64, s: &NrSettings, dir: &Path) -> Result<f64, String> {
    use wavcraft_core::actions::Editor;
    use wavcraft_core::doc::history::History;
    use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
    use wavcraft_core::noise;
    use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

    if !scale.is_finite() || scale > 100.0 {
        return Err("the scale must be a finite number, at most 100".to_string());
    }
    let frames = (3600.0 * 48_000.0 * scale).round() as u64;
    if frames < 192_000 {
        return Err("the scale is too small: the file must be at least 4 s".to_string());
    }
    let store = Store::create_with(&dir.join("nr-time"), StoreOptions::default()).map_err(|e| e.to_string())?;
    let (mut left_rng, mut right_rng) = (Rng::new(DEFAULT_SEED, "nr-time-l"), Rng::new(DEFAULT_SEED, "nr-time-r"));
    let (mut left, mut right) = (Vec::new(), Vec::new());
    let mut at = 0u64;
    while at < frames {
        let n = (frames - at).min(BLOCK_FRAMES as u64) as usize;
        let (mut l, mut r) = (Vec::with_capacity(n), Vec::with_capacity(n));
        for k in 0..n {
            let i = at + k as u64;
            let burst = if i >= 96_000 && ((i / 24_000) & 1) == 0 { 0.3 * (TAU * 1000.0 * i as f64 / 48_000.0).sin() } else { 0.0 };
            l.push((0.0367 * left_rng.signed() + burst) as f32);
            r.push((0.0367 * right_rng.signed() + burst) as f32);
        }
        left.push(Piece::whole(store.append(&l).map_err(|e| e.to_string())?).ok_or("empty block")?);
        right.push(Piece::whole(store.append(&r).map_err(|e| e.to_string())?).ok_or("empty block")?);
        at += n as u64;
    }
    let channels = vec![Channel::from_pieces(left), Channel::from_pieces(right)];
    let doc = DocState::new(store, channels, 48_000, SourceFormat::synthetic(48_000, 2)).map_err(|e| e.to_string())?;
    noise::set_current(noise::capture(&doc, 0..96_000)?);
    let (index, params) = nr_effect(s)?;
    let mut editor = Editor::new(History::new(doc, u64::MAX));
    let start = std::time::Instant::now();
    editor.run_effect(index, params, 0..0).map_err(|e| e.to_string())?;
    Ok(start.elapsed().as_secs_f64())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod nr_tests {
    use super::*;
    use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
    use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

    fn doc(dir: &Path, x: &[f32]) -> DocState {
        let s = Store::create_with(dir, StoreOptions::default()).unwrap();
        let ch = Channel::from_pieces(x.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()));
        DocState::new(s, vec![ch], 48_000, SourceFormat::synthetic(48_000, 1)).unwrap()
    }

    #[test]
    fn g2_measure_reports_floor_drop_and_speech_change() {
        // 12 s at 48 kHz: 2 s of silence, then 0.5 s bursts of a 1 kHz tone (amplitude 0.3) every
        // second; white noise (uniform, amplitude 0.0367) added everywhere: 20 dB under the bursts.
        let n = 12 * 48_000usize;
        let clean: Vec<f32> = (0..n)
            .map(|i| if i >= 96_000 && (i / 24_000) % 2 == 0 { (0.3 * (TAU * 1000.0 * i as f64 / 48_000.0).sin()) as f32 } else { 0.0 })
            .collect();
        let mut rng = Rng::new(DEFAULT_SEED, "g2-measure");
        let noisy: Vec<f32> = clean.iter().map(|c| c + (0.0367 * rng.signed()) as f32).collect();
        let dir = std::env::temp_dir().join(format!("wavcraft-nr-measure-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (a, b) = (doc(&dir.join("clean"), &clean), doc(&dir.join("noisy"), &noisy));
        let s = NrSettings { reduce_db: 16.0, reduction: 100.0, fft_size: 4096 };
        let m = nr_measure(&a, &b, &s);
        let _ = std::fs::remove_dir_all(&dir);
        let m = m.unwrap();
        println!("{m:?}");
        assert!((14.0..=18.0).contains(&m.floor_drop_db), "floor drop {} dB", m.floor_drop_db);
        assert!(m.speech_change_db.abs() < 1.0, "speech change {} dB", m.speech_change_db);
        assert!(m.print_frames >= 96_000);
    }

    #[test]
    fn g2_time_refuses_bad_scale() {
        // Refused before any file is built, so the folder is never created.
        let dir = std::env::temp_dir().join(format!("wavcraft-nr-time-refuse-{}", std::process::id()));
        let s = NrSettings::default();
        for scale in [0.0, f64::NAN, 0.0001, 101.0] {
            assert!(nr_time(scale, &s, &dir).is_err(), "scale {scale} should be refused");
        }
        assert!(!dir.exists());
    }
}

// The rack bench (F1.6): times a rack of three effects that keep state, over generated audio.

/// The sample rate of the rack bench's audio.
pub const RACK_BENCH_RATE: u32 = 48_000;

/// One hour in seconds: the rack bench scales its timing to this.
pub const RACK_BENCH_HOUR: f64 = 3600.0;

/// The rack the bench times: Parametric EQ, Dynamics and De-esser in slots 1 to 3, at their defaults.
pub fn rack_bench_config() -> Result<wavcraft_core::rack::RackConfig, String> {
    let mut config = wavcraft_core::rack::RackConfig::new();
    config.set_slot(0, "parametric_eq", None)?;
    config.set_slot(1, "dynamics", None)?;
    config.set_slot(2, "deesser", None)?;
    Ok(config)
}

/// The time one hour of stereo audio would take through the bench rack, measured over
/// `audio_seconds` seconds of generated audio (one second of it, processed again and again).
pub fn rack_bench_seconds(audio_seconds: u64) -> Result<f64, String> {
    if audio_seconds == 0 {
        return Err("the rack bench needs at least one second of audio".into());
    }
    let config = rack_bench_config()?;
    let mut rack = wavcraft_core::rack::RackProcessor::new(&config, 2, RACK_BENCH_RATE)?;
    let len = RACK_BENCH_RATE as usize;
    let mut rng = Rng::new(DEFAULT_SEED, "rack-bench");
    let input: Vec<Vec<f32>> = (0..2).map(|_| (0..len).map(|_| (rng.signed() * 0.25) as f32).collect()).collect();
    let (mut left, mut right) = (vec![0.0f32; len], vec![0.0f32; len]);
    let start = std::time::Instant::now();
    for _ in 0..audio_seconds {
        left.copy_from_slice(&input[0]);
        right.copy_from_slice(&input[1]);
        let mut io: [&mut [f32]; 2] = [left.as_mut_slice(), right.as_mut_slice()];
        rack.process_block(&mut io);
    }
    let elapsed = start.elapsed();
    std::hint::black_box((&left, &right));
    Ok(elapsed.as_secs_f64() * RACK_BENCH_HOUR / audio_seconds as f64)
}

/// The bench's one output line: `rack_3fx_1h_seconds=<n>`.
pub fn rack_bench_line(audio_seconds: u64) -> Result<String, String> {
    Ok(format!("rack_3fx_1h_seconds={:.3}", rack_bench_seconds(audio_seconds)?))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod rack_bench_tests {
    use super::*;

    #[test]
    fn f1_bench_rack_reports_seconds() {
        let line = rack_bench_line(1).unwrap();
        let value = line.strip_prefix("rack_3fx_1h_seconds=").expect("the line starts with the key");
        let n: f64 = value.parse().unwrap();
        assert!(n.is_finite() && n > 0.0, "{line}");
        assert_eq!(line.lines().count(), 1);
    }

    #[test]
    fn f1_bench_rack_config_has_three_streaming_slots() {
        let config = rack_bench_config().unwrap();
        let expect = ["parametric_eq", "dynamics", "deesser"];
        for (i, id) in expect.iter().enumerate() {
            assert_eq!(config.slots[i].effect.as_deref(), Some(*id));
            assert!(config.slots[i].enabled, "slot {} is off", i + 1);
        }
    }

    #[test]
    fn f1_bench_rack_refuses_zero_seconds() {
        assert!(rack_bench_seconds(0).is_err());
    }
}
