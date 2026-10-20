//! Convert Sample Type and Interpret Sample Rate (roadmap A136, A160-A162, A164).
//!
//! [`convert`] changes a document's sample rate, channel count and the bit depth a save will
//! use, as **one** [`Edit`] (one History step, so one Undo). It reads the source through
//! [`DocState::read`] a block at a time and writes the result through the store a block at a time:
//! the audio is never held whole in memory, and the source blocks stay where they are for Undo.
//!
//! - **Rate:** a windowed-sinc resampler of our own (Kaiser window, coefficients tabulated at
//!   `phases` fractional offsets and interpolated linearly between two neighbouring phases, each
//!   row normalised to a gain of 1 at DC), streamed: it keeps only the last few thousand input
//!   frames. [`Quality`] picks the number of sinc lobes, the window and how close to the lower
//!   Nyquist frequency the low-pass sits. Going down it low-passes below the new Nyquist
//!   frequency first (no aliasing); going up it low-passes at the old one (no images). The kernel
//!   is centred, so the output is not delayed; the ends see silence beyond the file. The output
//!   has `round(frames × new / old)` frames.
//! - **Channels:** 1 to many duplicates (the copies share the same blocks on disk), many to 1 mixes
//!   with a percentage per source channel (default: equal shares, 50 % each for stereo), any other
//!   change of count is refused. A mix is plain addition in `f32`, so 100 % + 100 % can exceed
//!   full scale: nothing clips until a save does (float headroom is its own row).
//! - **Bit depth:** a label, not a rewrite. It sets the sample format of the new state's
//!   [`SourceFormat`], which [`SampleType::for_source`] reads at save time. The dither settings ride
//!   along the same way (`SourceFormat::dither`, see [`crate::dither`]); off unless asked for. 32-bit integer is saved as 32-bit float (samples are held as `f32`), so the choices are
//!   16-bit, 24-bit and 32-bit float.
//! - **Interpret Sample Rate** ([`interpret_rate`]) only relabels the rate: no sample changes, so
//!   the audio plays faster or slower and is longer or shorter in time.
//! - **Per-document state** (markers, ...) is carried through both ([`DocState::with_parts`], not a
//!   rebuild from shared parts). Convert rescales frame positions by `new / old`
//!   ([`DocState::rescale_positions`]) so they keep their place in time; Interpret keeps them on
//!   the same frames, because the audio itself is what changes in time.
//!
//! Presets ([`Preset`], [`builtin_presets`], [`PresetFile`]) are named settings: a few built in
//! and any number the user saves, kept in `convert-presets.txt` beside `settings.txt`.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rayon::prelude::*;
use soundcraft_audio_io::SampleFormat;

use crate::dither::{DEFAULT_SEED, DitherSettings, DitherType, Shaping};
use crate::doc::edit::{Change, Edit, EditError};
use crate::doc::{Channel, DocState, Piece, SourceFormat};
use crate::io::MAX_SAMPLE_RATE_HZ;
use crate::save::SampleType;
use crate::store::{BLOCK_FRAMES, Store};

/// History label of [`convert`].
pub const CONVERT: &str = "Convert Sample Type";
/// History label of [`interpret_rate`].
pub const INTERPRET: &str = "Interpret Sample Rate";

// ---------------------------------------------------------------------------------------------
// Settings

/// How good the resampling is, and how slow. All four are free of audible aliasing in normal use;
/// the higher ones keep more of the top octave and reject more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Quality {
    Draft,
    Normal,
    #[default]
    High,
    Best,
}

impl Quality {
    pub const ALL: [Quality; 4] = [Quality::Draft, Quality::Normal, Quality::High, Quality::Best];

    pub fn name(self) -> &'static str {
        match self {
            Quality::Draft => "draft",
            Quality::Normal => "normal",
            Quality::High => "high",
            Quality::Best => "best",
        }
    }

    pub fn parse(s: &str) -> Option<Quality> {
        Quality::ALL.into_iter().find(|q| q.name().eq_ignore_ascii_case(s.trim()))
    }

    /// Sinc lobes (zero crossings) on each side of the centre, in units of the lower rate.
    fn lobes(self) -> f64 {
        match self {
            Quality::Draft => 8.0,
            Quality::Normal => 16.0,
            Quality::High => 32.0,
            Quality::Best => 64.0,
        }
    }

    /// Kaiser window shape: higher rejects more and widens the transition.
    fn beta(self) -> f64 {
        match self {
            Quality::Draft => 6.0,
            Quality::Normal => 8.0,
            Quality::High => 9.5,
            Quality::Best => 11.0,
        }
    }

    /// Where the low-pass sits, as a share of the lower Nyquist frequency.
    fn rolloff(self) -> f64 {
        match self {
            Quality::Draft => 0.90,
            Quality::Normal => 0.94,
            Quality::High => 0.96,
            Quality::Best => 0.98,
        }
    }

    /// Fractional offsets tabulated per input frame.
    fn phases(self) -> usize {
        match self {
            Quality::Draft => 128,
            Quality::Normal => 256,
            Quality::High => 512,
            Quality::Best => 1024,
        }
    }
}

/// What to change. `None` keeps the document's own value.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ConvertSettings {
    /// New sample rate in Hz.
    pub rate: Option<u32>,
    /// New channel count.
    pub channels: Option<u16>,
    /// Percent of each source channel in a mix to one channel; empty is equal shares.
    pub mix_pct: Vec<f32>,
    /// The sample type a save will use.
    pub bits: Option<SampleType>,
    /// Dither and noise shaping a save uses when it reduces the depth; `None` keeps the
    /// document's own (off for a file just opened).
    pub dither: Option<DitherSettings>,
    pub quality: Quality,
}

impl ConvertSettings {
    /// Settings from `key=value` words: `rate=44100 channels=1 bits=16 quality=high mix=50,50`.
    /// `bits` is 16, 24 or 32 (32-bit float); `mix` is percent per source channel.
    pub fn parse(text: &str) -> Result<ConvertSettings, String> {
        let mut s = ConvertSettings::default();
        let mut dither: Option<DitherSettings> = None;
        for word in text.split_whitespace() {
            let (k, v) = word.split_once('=').ok_or_else(|| format!("{word:?} is not key=value"))?;
            match k {
                "rate" => s.rate = Some(v.parse().map_err(|_| format!("rate={v}: not a number of Hz"))?),
                "channels" => s.channels = Some(v.parse().map_err(|_| format!("channels={v}: not a number"))?),
                "bits" => {
                    s.bits = Some(match v {
                        "16" => SampleType::Int16,
                        "24" => SampleType::Int24,
                        "32" | "32f" | "float" => SampleType::Float32,
                        _ => return Err(format!("bits={v}: use 16, 24 or 32 (32-bit float)")),
                    })
                }
                "dither" => {
                    dither.get_or_insert_with(DitherSettings::default).dither =
                        DitherType::parse(v).ok_or_else(|| format!("dither={v}: use none, triangular (tpdf) or rectangular"))?
                }
                "shaping" => {
                    dither.get_or_insert_with(DitherSettings::default).shaping =
                        Shaping::parse(v).ok_or_else(|| format!("shaping={v}: use none, light or strong"))?
                }
                "seed" => dither.get_or_insert_with(DitherSettings::default).seed = v.parse().map_err(|_| format!("seed={v}: not a number"))?,
                "quality" => s.quality = Quality::parse(v).ok_or_else(|| format!("quality={v}: use draft, normal, high or best"))?,
                "mix" => {
                    s.mix_pct = v
                        .split(',')
                        .map(|p| p.parse::<f32>().map_err(|_| format!("mix={v}: percentages separated by commas")))
                        .collect::<Result<_, _>>()?
                }
                _ => return Err(format!("unknown setting {k:?}")),
            }
        }
        s.dither = dither;
        Ok(s)
    }

    /// The settings as [`parse`](Self::parse) reads them.
    pub fn to_text(&self) -> String {
        let mut parts = Vec::new();
        if let Some(r) = self.rate {
            parts.push(format!("rate={r}"));
        }
        if let Some(c) = self.channels {
            parts.push(format!("channels={c}"));
        }
        if let Some(b) = self.bits {
            parts.push(format!("bits={}", bits_word(b)));
        }
        if let Some(d) = self.dither {
            parts.push(format!("dither={}", d.dither.name()));
            parts.push(format!("shaping={}", d.shaping.name()));
            if d.seed != DEFAULT_SEED {
                parts.push(format!("seed={}", d.seed));
            }
        }
        parts.push(format!("quality={}", self.quality.name()));
        if !self.mix_pct.is_empty() {
            parts.push(format!("mix={}", self.mix_pct.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(",")));
        }
        parts.join(" ")
    }
}

fn bits_word(t: SampleType) -> &'static str {
    match t {
        SampleType::Int16 => "16",
        SampleType::Int24 => "24",
        SampleType::Int32 | SampleType::Float32 => "32",
    }
}

/// The sample format a save of `t` reads back from a [`SourceFormat`] (32-bit integer saves as
/// float, so it is stored as float).
fn format_for(t: SampleType) -> SampleFormat {
    match t {
        SampleType::Int16 => SampleFormat::Int16,
        SampleType::Int24 => SampleFormat::Int24,
        SampleType::Int32 | SampleType::Float32 => SampleFormat::Float32,
    }
}

fn unsupported(why: impl Into<String>) -> EditError {
    EditError::Unsupported(why.into())
}

fn check_rate(rate: u32) -> Result<(), EditError> {
    if rate == 0 || rate > MAX_SAMPLE_RATE_HZ {
        return Err(unsupported(format!("{rate} Hz is not a usable sample rate (1 to {MAX_SAMPLE_RATE_HZ})")));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Convert

/// The channel mapping `settings` asks of a `src`-channel document: the weights of each source
/// channel in each working channel, plus how many copies of the working channels the result has.
struct Plan {
    /// One row per working channel: the weight of each source channel.
    rows: Vec<Vec<f32>>,
    /// The result has this many channels; working channel `i` feeds result channel `i % rows.len()`.
    out_channels: usize,
}

fn plan(src: usize, s: &ConvertSettings) -> Result<Plan, EditError> {
    let dst = usize::from(s.channels.unwrap_or(src as u16));
    if dst == 0 || dst > 64 {
        return Err(unsupported(format!("{dst} channels is not possible (1 to 64)")));
    }
    let identity = |n: usize| (0..n).map(|i| (0..n).map(|j| f32::from(u8::from(i == j))).collect()).collect::<Vec<Vec<f32>>>();
    if dst == src {
        return Ok(Plan { rows: identity(src), out_channels: dst });
    }
    if dst == 1 {
        let w: Vec<f32> = if s.mix_pct.is_empty() {
            vec![1.0 / src as f32; src]
        } else if s.mix_pct.len() == src {
            s.mix_pct.iter().map(|p| p / 100.0).collect()
        } else {
            return Err(unsupported(format!("the mix has {} percentages for {src} channels", s.mix_pct.len())));
        };
        if w.iter().any(|x| !x.is_finite()) {
            return Err(unsupported("a mix percentage is not a number"));
        }
        return Ok(Plan { rows: vec![w], out_channels: 1 });
    }
    if src == 1 {
        return Ok(Plan { rows: identity(1), out_channels: dst });
    }
    Err(unsupported(format!("converting {src} channels to {dst} is not supported (to 1 channel, or from 1 channel, is)")))
}

/// The settings change nothing about `doc`.
fn is_identity(doc: &DocState, s: &ConvertSettings) -> bool {
    let fmt_same = s.bits.is_none_or(|b| format_for(b) == doc.source().sample_format);
    let dither_same = s.dither.is_none_or(|d| d == doc.source().dither);
    s.rate.is_none_or(|r| r == doc.sample_rate()) && s.channels.is_none_or(|c| usize::from(c) == doc.channel_count()) && fmt_same && dither_same
}

/// Disk the result would take, for an up-front space check (4 bytes a sample, before the
/// store's own margin).
pub fn estimate_bytes(doc: &DocState, s: &ConvertSettings) -> u64 {
    let in_rate = u128::from(doc.sample_rate());
    let out_rate = u128::from(s.rate.unwrap_or(doc.sample_rate()));
    let frames = (u128::from(doc.len_frames()) * out_rate).div_ceil(in_rate) as u64;
    let work = match plan(doc.channel_count(), s) {
        Ok(p) => p.rows.len() as u64,
        Err(_) => doc.channel_count() as u64,
    };
    if s.rate.is_none_or(|r| r == doc.sample_rate()) && s.channels.is_none_or(|c| usize::from(c) == doc.channel_count()) {
        return 0;
    }
    frames * work * 4
}

/// Convert `doc` as `s` says, as one edit labelled [`CONVERT`]. `progress` gets 0 to 1;
/// `cancel` is checked before each block (then [`EditError::Cancelled`], nothing changed).
pub fn convert(doc: &DocState, s: &ConvertSettings, cancel: &AtomicBool, progress: &mut dyn FnMut(f64)) -> Result<Edit, EditError> {
    let in_rate = doc.sample_rate();
    let out_rate = s.rate.unwrap_or(in_rate);
    check_rate(out_rate)?;
    let plan = plan(doc.channel_count(), s)?;
    if is_identity(doc, s) {
        return Ok(Edit { state: doc.clone(), label: CONVERT.to_string(), change: Change::None });
    }
    let src = doc.channel_count();
    let resample = out_rate != in_rate;
    let mixing =
        plan.rows.len() != src || plan.rows.iter().enumerate().any(|(i, r)| r.iter().enumerate().any(|(j, &w)| w != f32::from(u8::from(i == j))));
    let in_len = doc.len_frames();
    let out_len =
        if resample { ((u128::from(in_len) * u128::from(out_rate) + u128::from(in_rate) / 2) / u128::from(in_rate)) as u64 } else { in_len };

    // Working channels: shared as they are when only the label changes.
    let work: Vec<Arc<Channel>> = if !resample && !mixing {
        doc.channels().to_vec()
    } else {
        stream(doc, &plan, resample.then_some((in_rate, out_rate, s.quality)), out_len, cancel, progress)?
    };
    let channels: Vec<Arc<Channel>> = (0..plan.out_channels).map(|i| work[i % work.len()].clone()).collect();

    let mut source = doc.source().as_ref().clone();
    source.sample_rate = out_rate;
    source.channels = plan.out_channels as u16;
    if let Some(b) = s.bits {
        source.sample_format = format_for(b);
    }
    if let Some(d) = s.dither {
        source.dither = d;
    }
    // Everything else per-document (markers, ...) rides along; frame positions follow the rate.
    let mut state = doc.with_parts(channels, out_rate, Arc::new(source))?;
    state.rescale_positions(in_rate, out_rate);
    progress(1.0);
    Ok(Edit { state, label: CONVERT.to_string(), change: Change::LengthFrom { frame: 0 } })
}

/// Relabel the sample rate without touching a sample (Interpret Sample Rate). No change when
/// the rate is the document's own.
pub fn interpret_rate(doc: &DocState, rate: u32) -> Result<Edit, EditError> {
    check_rate(rate)?;
    if rate == doc.sample_rate() {
        return Ok(Edit { state: doc.clone(), label: INTERPRET.to_string(), change: Change::None });
    }
    let mut source: SourceFormat = doc.source().as_ref().clone();
    source.sample_rate = rate;
    // Interpret keeps frame positions as they are (markers stay on the same sample, so they move
    // in time with the audio): no rescale. Everything per-document is carried over.
    let state = doc.with_parts(doc.channels().to_vec(), rate, Arc::new(source))?;
    Ok(Edit { state, label: INTERPRET.to_string(), change: Change::LengthFrom { frame: 0 } })
}

/// Appends samples to a channel as whole blocks of the store.
struct Writer {
    pending: Vec<f32>,
    channel: Channel,
}

impl Writer {
    fn new() -> Writer {
        Writer { pending: Vec::with_capacity(BLOCK_FRAMES * 2), channel: Channel::new() }
    }

    fn flush(&mut self, store: &Store, all: bool) -> io::Result<()> {
        while self.pending.len() >= BLOCK_FRAMES || (all && !self.pending.is_empty()) {
            let n = self.pending.len().min(BLOCK_FRAMES);
            let block = store.append(&self.pending[..n])?;
            if let Some(p) = Piece::whole(block) {
                self.channel.push(p);
            }
            self.pending.drain(..n);
        }
        Ok(())
    }
}

/// Read `doc` block by block, mix to the plan's working channels, resample, write.
fn stream(
    doc: &DocState,
    plan: &Plan,
    resample: Option<(u32, u32, Quality)>,
    out_len: u64,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f64),
) -> Result<Vec<Arc<Channel>>, EditError> {
    let (src, work) = (doc.channel_count(), plan.rows.len());
    let in_len = doc.len_frames();
    let table = resample.map(|(i, o, q)| Arc::new(Table::new(u64::from(i), u64::from(o), q)));
    let mut resamplers: Vec<Option<Resampler>> = (0..work)
        .map(|_| resample.zip(table.as_ref()).map(|((i, o, _), t)| Resampler::new(t.clone(), u64::from(i), u64::from(o), in_len, out_len)))
        .collect();
    let mut writers: Vec<Writer> = (0..work).map(|_| Writer::new()).collect();
    let mut input: Vec<Vec<f32>> = vec![vec![0.0; BLOCK_FRAMES]; src];
    let mut mixed: Vec<Vec<f32>> = vec![Vec::new(); work];
    let mut at = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(EditError::Cancelled);
        }
        let n = (in_len - at).min(BLOCK_FRAMES as u64) as usize;
        for (ch, buf) in input.iter_mut().enumerate() {
            let got = doc.read(ch, at, &mut buf[..n])?;
            if got != n {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "document shorter than its length").into());
            }
        }
        let last = at + n as u64 >= in_len;
        // Mix. A plain copy when a row is one source channel at full weight.
        for (row, out) in plan.rows.iter().zip(mixed.iter_mut()) {
            out.clear();
            out.resize(n, 0.0);
            for (j, &w) in row.iter().enumerate() {
                if w == 0.0 {
                    continue;
                }
                for (o, x) in out.iter_mut().zip(&input[j][..n]) {
                    *o += w * x;
                }
            }
        }
        // Resample (in parallel per channel) into each writer's pending samples.
        mixed.par_iter().zip(resamplers.par_iter_mut()).zip(writers.par_iter_mut()).for_each(|((m, rs), w)| match rs {
            Some(r) => r.process(m, last, &mut w.pending),
            None => w.pending.extend_from_slice(m),
        });
        for w in &mut writers {
            w.flush(doc.store(), false)?;
        }
        at += n as u64;
        progress(if in_len == 0 { 1.0 } else { (at as f64 / in_len as f64).min(1.0) * 0.99 });
        if last {
            break;
        }
    }
    let mut out = Vec::with_capacity(work);
    for mut w in writers {
        w.flush(doc.store(), true)?;
        out.push(Arc::new(w.channel));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// The resampler

/// Zeroth-order modified Bessel function, for the Kaiser window.
fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term, q) = (1.0, 1.0, x * x / 4.0);
    for k in 1..60 {
        term *= q / (f64::from(k) * f64::from(k));
        sum += term;
        if term < sum * 1e-17 {
            break;
        }
    }
    sum
}

/// The windowed-sinc coefficients for every tabulated fractional offset.
struct Table {
    /// Taps on each side of the centre: a row covers input offsets `-half+1 ..= half`.
    half: usize,
    phases: usize,
    /// `(phases + 1)` rows of `2 * half` coefficients; row `q` is for a fractional offset of `q / phases`.
    rows: Vec<f32>,
}

impl Table {
    fn new(in_rate: u64, out_rate: u64, q: Quality) -> Table {
        let lower = (out_rate as f64 / in_rate as f64).min(1.0);
        let fc = lower * q.rolloff();
        let reach = q.lobes() / fc;
        let half = reach.ceil() as usize;
        let taps = 2 * half;
        // Very low cutoffs mean very long rows: fewer phases keep the table bounded (~16 MB).
        let phases = q.phases().min((4_000_000 / taps).max(32));
        let beta = q.beta();
        let norm = bessel_i0(beta);
        let mut rows = vec![0f32; (phases + 1) * taps];
        for p in 0..=phases {
            let frac = p as f64 / phases as f64;
            let row = &mut rows[p * taps..(p + 1) * taps];
            let mut sum = 0.0;
            let mut vals = vec![0.0f64; taps];
            for (j, v) in vals.iter_mut().enumerate() {
                let x = (j as f64 - (half as f64 - 1.0)) - frac;
                let win = if x.abs() < reach { bessel_i0(beta * (1.0 - (x / reach).powi(2)).sqrt()) / norm } else { 0.0 };
                let arg = std::f64::consts::PI * fc * x;
                let sinc = if arg.abs() < 1e-12 { 1.0 } else { arg.sin() / arg };
                *v = fc * sinc * win;
                sum += *v;
            }
            for (r, v) in row.iter_mut().zip(&vals) {
                *r = (v / sum) as f32;
            }
        }
        Table { half, phases, rows }
    }
}

/// One channel's streaming resampler.
struct Resampler {
    table: Arc<Table>,
    in_rate: u64,
    out_rate: u64,
    in_len: u64,
    out_len: u64,
    /// Input frames kept: frame `base + k` is `hist[k]`.
    hist: Vec<f32>,
    base: i64,
    next: u64,
}

impl Resampler {
    fn new(table: Arc<Table>, in_rate: u64, out_rate: u64, in_len: u64, out_len: u64) -> Resampler {
        Resampler { table, in_rate, out_rate, in_len, out_len, hist: Vec::new(), base: 0, next: 0 }
    }

    /// Feed `input` and append every output frame that is now fully determined. With `last`, the
    /// input is over: the rest of the output is made with silence beyond the end.
    fn process(&mut self, input: &[f32], last: bool, out: &mut Vec<f32>) {
        self.hist.extend_from_slice(input);
        let (half, taps, phases) = (self.table.half, 2 * self.table.half, self.table.phases);
        let end = self.base + self.hist.len() as i64;
        let mut coef = vec![0f32; taps];
        while self.next < self.out_len {
            let num = self.next * self.in_rate;
            let i0 = (num / self.out_rate) as i64;
            if !last && i0 + half as i64 >= end {
                break;
            }
            let rem = num % self.out_rate;
            let scaled = rem * phases as u64;
            let (q, w) = ((scaled / self.out_rate) as usize, (scaled % self.out_rate) as f32 / self.out_rate as f32);
            let r0 = &self.table.rows[q * taps..(q + 1) * taps];
            let r1 = &self.table.rows[(q + 1) * taps..(q + 2) * taps];
            for (c, (a, b)) in coef.iter_mut().zip(r0.iter().zip(r1)) {
                *c = a + (b - a) * w;
            }
            let lo = i0 - half as i64 + 1;
            let acc = if lo >= self.base && lo + taps as i64 <= end {
                let s = (lo - self.base) as usize;
                self.hist[s..s + taps].iter().zip(&coef).map(|(x, c)| x * c).sum::<f32>()
            } else {
                // Near the ends: frames outside the file are silence.
                let mut acc = 0.0;
                for (j, c) in coef.iter().enumerate() {
                    let idx = lo + j as i64;
                    if idx >= 0 && (idx as u64) < self.in_len && idx >= self.base {
                        acc += self.hist[(idx - self.base) as usize] * c;
                    }
                }
                acc
            };
            out.push(acc);
            self.next += 1;
        }
        // Drop the input no later output reaches.
        let keep_from = ((self.next * self.in_rate / self.out_rate) as i64 - half as i64 + 1).max(self.base);
        let drop = ((keep_from - self.base) as usize).min(self.hist.len());
        if drop > 0 {
            self.hist.drain(..drop);
            self.base += drop as i64;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Presets

/// A named set of conversion settings.
#[derive(Debug, Clone, PartialEq)]
pub struct Preset {
    pub name: String,
    pub settings: ConvertSettings,
}

/// The presets that come with WavCraft (named for the job, not for any other program's lists).
pub fn builtin_presets() -> Vec<Preset> {
    let p = |name: &str, rate, channels, bits| Preset {
        name: name.to_string(),
        settings: ConvertSettings { rate: Some(rate), channels: Some(channels), bits: Some(bits), ..ConvertSettings::default() },
    };
    vec![
        p("Disc Stereo 44.1 kHz 16-bit", 44_100, 2, SampleType::Int16),
        p("Video Stereo 48 kHz 24-bit", 48_000, 2, SampleType::Int24),
        p("Voice Mono 22.05 kHz 16-bit", 22_050, 1, SampleType::Int16),
        p("Archive Stereo 96 kHz 32-bit float", 96_000, 2, SampleType::Float32),
    ]
}

/// The user's saved presets, in a text file: one `name | settings` line each.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PresetFile {
    path: Option<PathBuf>,
    user: Vec<Preset>,
}

impl PresetFile {
    /// `<config dir>/wavcraft/convert-presets.txt`.
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("wavcraft").join("convert-presets.txt"))
    }

    /// Read `path` (missing or unreadable is an empty list; lines that don't parse are skipped).
    pub fn load(path: Option<PathBuf>) -> PresetFile {
        let mut user = Vec::new();
        if let Some(text) = path.as_deref().and_then(|p| std::fs::read_to_string(p).ok()) {
            for line in text.lines() {
                let Some((name, rest)) = line.split_once('|') else { continue };
                if let (name, Ok(settings)) = (name.trim(), ConvertSettings::parse(rest))
                    && !name.is_empty()
                {
                    user.push(Preset { name: name.to_string(), settings });
                }
            }
        }
        PresetFile { path, user }
    }

    pub fn user(&self) -> &[Preset] {
        &self.user
    }

    /// Built-in presets, then the user's.
    pub fn all(&self) -> Vec<Preset> {
        let mut v = builtin_presets();
        v.extend(self.user.iter().cloned());
        v
    }

    pub fn find(&self, name: &str) -> Option<Preset> {
        self.all().into_iter().find(|p| p.name.eq_ignore_ascii_case(name.trim()))
    }

    /// Save (or replace) the user preset `name`. A built-in name, an empty name or one with `|` or
    /// a line break is refused.
    pub fn save(&mut self, name: &str, settings: ConvertSettings) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() || name.contains(['|', '\n', '\r']) {
            return Err("a preset name can't be empty or hold | or a line break".to_string());
        }
        if builtin_presets().iter().any(|p| p.name.eq_ignore_ascii_case(name)) {
            return Err(format!("{name:?} is a built-in preset; choose another name"));
        }
        self.user.retain(|p| !p.name.eq_ignore_ascii_case(name));
        self.user.push(Preset { name: name.to_string(), settings });
        self.write()
    }

    /// Delete the user preset `name` (built-in ones stay).
    pub fn delete(&mut self, name: &str) -> Result<(), String> {
        let before = self.user.len();
        self.user.retain(|p| !p.name.eq_ignore_ascii_case(name.trim()));
        if self.user.len() == before {
            return Err(format!("no saved preset named {:?} (the built-in ones can't be deleted)", name.trim()));
        }
        self.write()
    }

    fn write(&self) -> Result<(), String> {
        let Some(path) = &self.path else { return Ok(()) };
        let text: String = self.user.iter().map(|p| format!("{} | {}\n", p.name, p.settings.to_text())).collect();
        write_text(path, &text).map_err(|e| format!("could not save the presets to {}: {e}", path.display()))
    }
}

fn write_text(path: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, text)
}

#[cfg(test)]
mod carry_tests {
    use super::*;
    use crate::doc::SourceFormat;
    use crate::store::{Store, StoreOptions};

    fn doc(name: &str, rate: u32, frames: usize) -> DocState {
        let dir = std::env::temp_dir().join(format!("wavcraft-carry-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let s = Store::create_with(&dir, StoreOptions::default()).unwrap();
        let data = vec![0.25f32; frames];
        let ch = Channel::from_pieces([Piece::whole(s.append(&data).unwrap()).unwrap()]);
        DocState::new(s, vec![ch], rate, SourceFormat::synthetic(rate, 1)).unwrap()
    }

    #[test]
    fn convert_carries_per_document_state_and_rescales_it_with_the_rate() {
        let mut d = doc("rescale", 48_000, 4800);
        d.set_standin(vec![0, 2400, 4800]);
        let settings = ConvertSettings { rate: Some(24_000), ..ConvertSettings::default() };
        let e = convert(&d, &settings, &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!(e.state.sample_rate(), 24_000);
        assert_eq!(e.state.len_frames(), 2400);
        assert_eq!(e.state.standin(), &[0, 1200, 2400], "kept, and moved with the rate");
    }

    #[test]
    fn convert_without_a_rate_change_keeps_state_untouched() {
        let mut d = doc("bits", 48_000, 4800);
        d.set_standin(vec![7, 99]);
        let settings = ConvertSettings { bits: Some(SampleType::Int16), ..ConvertSettings::default() };
        let e = convert(&d, &settings, &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!(e.state.standin(), &[7, 99]);
    }

    #[test]
    fn interpret_carries_state_and_keeps_frame_positions() {
        let mut d = doc("interpret", 48_000, 4800);
        d.set_standin(vec![10, 4000]);
        let e = interpret_rate(&d, 24_000).unwrap();
        assert_eq!(e.state.sample_rate(), 24_000);
        assert_eq!(e.state.standin(), &[10, 4000], "same frames: they move in time with the audio");
    }
}
