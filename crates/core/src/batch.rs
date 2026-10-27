//! Batch Process (f73, roadmap B158/B159): run a chain of operations over a list of files, one
//! file at a time, and save each result as a WAV in an output folder.
//!
//! **Streaming.** Each file is opened into its own session store ([`crate::io::open`]), measured
//! and written 64 K frames at a time ([`save_wav_transformed`]), and the store is dropped (its
//! session folder removed) before the next file starts. No file is ever held whole in memory.
//!
//! **The user's files are not touched.** Inputs are only read. An output is never written over
//! an existing file unless [`Job::overwrite`] is on (default off), and never over one of the
//! job's own inputs even then unless that same option is on; a save goes through a temporary
//! file and a rename, so a failure leaves nothing half-written.
//!
//! **Plugging in later.** The chain is a list of [`Op`] values, a small enum. Another feature
//! (Match Loudness, an effect from the effect trait, another save format) adds a variant, a
//! line in [`Plan::push`] and, where it needs a measuring pass, a case in [`Plan::measure`];
//! the runner, progress, cancel and results list do not change.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::doc::DocState;
use crate::io::{OpenError, OpenOptions};
use crate::save::{SampleType, Transform, save_wav_transformed};
use crate::store::BLOCK_FRAMES;

/// One step of the chain, applied in order.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Scale so the file's loudest sample (at this point in the chain) is `target_db` dBFS.
    /// A silent file is left as it is.
    NormalizePeak { target_db: f64 },
    /// Plain gain in dB.
    Gain { db: f64 },
    /// Mix all channels down to one (the mean).
    ToMono,
}

impl Op {
    /// "normalize peak=-1", "gain db=3", "mono".
    pub fn describe(&self) -> String {
        match self {
            Op::NormalizePeak { target_db } => format!("normalize peak={target_db}"),
            Op::Gain { db } => format!("gain db={db}"),
            Op::ToMono => "mono".to_string(),
        }
    }

    /// Parse the words after `op`: `normalize peak=-1`, `gain db=3`, `mono`.
    pub fn parse(words: &str) -> Result<Op, String> {
        let mut it = words.split_whitespace();
        let name = it.next().ok_or("an operation name: normalize, gain or mono")?;
        let arg = |key: &str, it: &mut std::str::SplitWhitespace<'_>| -> Result<f64, String> {
            let w = it.next().ok_or(format!("{name} needs {key}=<dB>"))?;
            let v = w.strip_prefix(key).and_then(|v| v.strip_prefix('=')).ok_or(format!("{name} needs {key}=<dB>"))?;
            v.parse::<f64>().ok().filter(|v| v.is_finite()).ok_or(format!("{key} is not a number: {v}"))
        };
        match name {
            "normalize" => {
                let target_db = arg("peak", &mut it)?;
                if target_db > 0.0 {
                    return Err("peak must be 0 dBFS or below".to_string());
                }
                Ok(Op::NormalizePeak { target_db })
            }
            "gain" => Ok(Op::Gain { db: arg("db", &mut it)? }),
            "mono" => Ok(Op::ToMono),
            other => Err(format!("no operation named {other:?} (normalize, gain, mono)")),
        }
    }
}

/// What to run.
#[derive(Debug, Clone)]
pub struct Job {
    pub inputs: Vec<PathBuf>,
    pub ops: Vec<Op>,
    pub out_dir: PathBuf,
    /// The output name without extension (`.wav` is added): `{name}` is the input's name without
    /// its extension, `{n}` its place in the list from 1. No folder parts.
    pub pattern: String,
    /// Sample type of the outputs; `None` keeps what each source had.
    pub sample_type: Option<SampleType>,
    /// Allow an output to replace an existing file, an input included. Off by default.
    pub overwrite: bool,
}

impl Job {
    pub fn new(out_dir: PathBuf) -> Job {
        Job { inputs: Vec::new(), ops: Vec::new(), out_dir, pattern: "{name}".to_string(), sample_type: None, overwrite: false }
    }
}

/// How one file ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Ok {
        output: PathBuf,
    },
    Failed {
        reason: String,
    },
    /// Stopped by cancel (the file in flight) or not started because of it.
    Cancelled,
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Outcome::Ok { output } => write!(f, "ok {}", output.display()),
            Outcome::Failed { reason } => write!(f, "failed: {reason}"),
            Outcome::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// What the runner reports as it goes (all from the thread that called [`run`]).
#[derive(Debug, Clone)]
pub enum Event {
    Started {
        index: usize,
        total: usize,
    },
    /// `file` and `overall` are 0 to 1.
    Progress {
        index: usize,
        file: f64,
        overall: f64,
    },
    Finished {
        index: usize,
        outcome: Outcome,
    },
}

/// The output file name for input number `index` (0-based), or why the pattern is refused.
pub fn output_name(pattern: &str, input: &Path, index: usize) -> Result<String, String> {
    let stem = input.file_stem().map_or_else(|| "untitled".to_string(), |s| s.to_string_lossy().into_owned());
    let name = pattern.replace("{name}", &stem).replace("{n}", &(index + 1).to_string());
    if name.is_empty() || name.contains(['/', '\\', ':']) || name.contains("..") || name.starts_with('.') {
        return Err(format!("the name pattern gives {name:?}, which is not a plain file name"));
    }
    Ok(format!("{name}.wav"))
}

/// The ops folded into one per-chunk change. Measuring passes (normalize) run over the document
/// with the plan so far, so each op sees the result of the ones before it.
struct Plan {
    gain: f32,
    mono: bool,
}

impl Plan {
    fn out_channels(&self, doc: &DocState) -> usize {
        if self.mono { 1 } else { doc.channel_count() }
    }

    /// Apply to `n` frames of `input`, filling `out`.
    fn apply(&self, input: &[Vec<f32>], n: usize, out: &mut [Vec<f32>]) {
        if self.mono {
            let k = input.len().max(1) as f32;
            for i in 0..n {
                out[0][i] = input.iter().map(|c| c[i]).sum::<f32>() / k * self.gain;
            }
        } else {
            for (o, c) in out.iter_mut().zip(input) {
                for i in 0..n {
                    o[i] = c[i] * self.gain;
                }
            }
        }
    }

    /// Largest absolute sample of the document through the plan so far, read in chunks.
    fn measure(&self, doc: &DocState, cancel: &AtomicBool) -> io::Result<f32> {
        let frames = doc.len_frames();
        let mut input = vec![vec![0f32; BLOCK_FRAMES]; doc.channel_count()];
        let mut out = vec![vec![0f32; BLOCK_FRAMES]; self.out_channels(doc)];
        let (mut at, mut peak) = (0u64, 0f32);
        while at < frames {
            if cancel.load(Ordering::Relaxed) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            let n = (frames - at).min(BLOCK_FRAMES as u64) as usize;
            for (ch, buf) in input.iter_mut().enumerate() {
                if doc.read(ch, at, &mut buf[..n])? != n {
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "short read while measuring"));
                }
            }
            self.apply(&input, n, &mut out);
            peak = out.iter().flat_map(|c| c[..n].iter()).fold(peak, |p, x| if x.is_nan() { p } else { p.max(x.abs()) });
            at += n as u64;
        }
        Ok(peak)
    }

    fn push(&mut self, op: &Op, doc: &DocState, cancel: &AtomicBool) -> io::Result<()> {
        match op {
            Op::Gain { db } => self.gain *= db_to_lin(*db),
            Op::ToMono => self.mono = true,
            Op::NormalizePeak { target_db } => {
                let peak = self.measure(doc, cancel)?;
                if peak > 0.0 && peak.is_finite() {
                    self.gain *= db_to_lin(*target_db) / peak;
                }
            }
        }
        Ok(())
    }
}

fn db_to_lin(db: f64) -> f32 {
    10f64.powf(db / 20.0) as f32
}

/// Run the job. One [`Outcome`] per input, in order; the job never stops early for a failure,
/// only for `cancel` (the rest are then [`Outcome::Cancelled`]).
pub fn run(job: &Job, opts: &OpenOptions, cancel: &AtomicBool, on_event: &mut dyn FnMut(Event)) -> Vec<Outcome> {
    let total = job.inputs.len();
    let mut outcomes = Vec::with_capacity(total);
    let mut made: Vec<PathBuf> = Vec::new();
    for (index, input) in job.inputs.iter().enumerate() {
        let outcome = if cancel.load(Ordering::Relaxed) {
            Outcome::Cancelled
        } else {
            on_event(Event::Started { index, total });
            let mut report = |file: f64| on_event(Event::Progress { index, file, overall: (index as f64 + file) / total as f64 });
            let outcome = match one(job, input, index, opts, cancel, &made, &mut report) {
                Ok(output) => {
                    made.push(output.clone());
                    Outcome::Ok { output }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted && cancel.load(Ordering::Relaxed) => Outcome::Cancelled,
                Err(e) => Outcome::Failed { reason: e.to_string() },
            };
            on_event(Event::Finished { index, outcome: outcome.clone() });
            outcome
        };
        match &outcome {
            Outcome::Failed { reason } => log::warn!(target: "wavcraft_core::batch", "{}: failed: {reason}", input.display()),
            Outcome::Ok { output } => log::info!(target: "wavcraft_core::batch", "{} -> {}", input.display(), output.display()),
            Outcome::Cancelled => {}
        }
        outcomes.push(outcome);
    }
    outcomes
}

fn other(msg: impl Into<String>) -> io::Error {
    io::Error::other(msg.into())
}

/// One file: open, plan, stream out. The session store goes when `doc` drops at the end.
fn one(
    job: &Job,
    input: &Path,
    index: usize,
    opts: &OpenOptions,
    cancel: &AtomicBool,
    made: &[PathBuf],
    report: &mut dyn FnMut(f64),
) -> io::Result<PathBuf> {
    // Decide the output first, so a refused name costs no work.
    let name = output_name(&job.pattern, input, index).map_err(other)?;
    let target = job.out_dir.join(name);
    if made.contains(&target) {
        return Err(other(format!("another file in this run already made {}; use {{n}} in the pattern", target.display())));
    }
    if !job.overwrite && same_file(&target, input) {
        return Err(other("the output would replace its input (overwrite is off)"));
    }
    if !job.overwrite && target.exists() {
        return Err(other(format!("{} already exists (overwrite is off)", target.display())));
    }
    if job.overwrite && same_file(&target, input) {
        log::warn!(target: "wavcraft_core::batch", "overwrite is on: the output replaces its own input {}", input.display());
    }
    std::fs::create_dir_all(&job.out_dir).map_err(|e| other(format!("cannot make the output folder: {e}")))?;

    report(0.0);
    let doc = crate::io::open(input, opts, &mut |p| report(p * 0.25), cancel).map_err(|e| match e {
        OpenError::Cancelled => io::Error::new(io::ErrorKind::Interrupted, "cancelled"),
        e => other(format!("cannot read it: {e}")),
    })?;
    let mut plan = Plan { gain: 1.0, mono: false };
    for op in &job.ops {
        plan.push(op, &doc, cancel)?;
    }
    report(0.3);
    let ty = job.sample_type.unwrap_or_else(|| SampleType::for_source(doc.source()));
    let out_channels = plan.out_channels(&doc);
    let mut apply = |input: &[Vec<f32>], n: usize, out: &mut [Vec<f32>]| -> io::Result<()> {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        plan.apply(input, n, out);
        Ok(())
    };
    save_wav_transformed(&doc, &target, ty, Transform { channels: out_channels, apply: &mut apply }, &mut |p| report(0.3 + p * 0.7)).map_err(
        |e| match e {
            crate::save::SaveError::Io(e) => e,
            e => other(e.to_string()),
        },
    )?;
    report(1.0);
    Ok(target)
}

/// Do the two paths name the same file? (The target may not exist yet: its folder and name are compared.)
fn same_file(target: &Path, input: &Path) -> bool {
    let canon = |p: &Path| -> Option<PathBuf> {
        match p.canonicalize() {
            Ok(c) => Some(c),
            Err(_) => Some(p.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new(".")).canonicalize().ok()?.join(p.file_name()?)),
        }
    };
    match (canon(target), canon(input)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}
