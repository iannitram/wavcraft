//! Command line for a headless batch run (roadmap C239, sub-job a): a pure parser from the
//! argument list to a checked [`BatchCli`], and the [`Job`] it describes. No file is opened and
//! nothing is printed here; running the job and the exit codes are sub-job b.
//!
//! ```text
//! --batch --out DIR [--name PATTERN] [--bits 16|24|32f|same] [--step "<op words>"]... FILE...
//! ```
//!
//! Rules:
//! - `args` is the argument list WITHOUT the program name (`std::env::args().skip(1)`).
//! - Flags and input files may come in any order. A flag value is given as `--flag value` or
//!   `--flag=value` (the value of `--flag=value` may start with `--`).
//! - `--flag value` takes the next argument as the value unless there is none or it starts with
//!   `--`: then the flag "needs a value".
//! - `--batch` is required and takes no value. `--out` is required and not empty. At least one
//!   input file is required. `--name` must give a plain file name (checked with [`output_name`]).
//! - `--step` may repeat (the steps keep the command-line order, each parsed by [`Op::parse`]).
//!   `--batch`, `--out`, `--name` and `--bits` given twice are refused.
//! - An argument that starts with `-` and is not one of the five flags is refused, naming it.
//! - `--` ends the flags: every later argument is an input file, even one starting with `-`.
//! - Arguments are `&str`: a file name that is not UTF-8 is out of scope.

use std::path::{Path, PathBuf};

use crate::batch::{Job, Op, output_name};
use crate::save::SampleType;

/// A checked batch command line.
#[derive(Debug, Clone, PartialEq)]
pub struct BatchCli {
    /// The input files, in command-line order. Never empty.
    pub inputs: Vec<PathBuf>,
    /// The output folder (`--out`).
    pub out_dir: PathBuf,
    /// The output name pattern (`--name`), `{name}` when not given.
    pub name_pattern: String,
    /// The steps (`--step`), in command-line order; may be empty.
    pub steps: Vec<Op>,
    /// The sample type of the outputs (`--bits`); `None` keeps each source's own.
    pub sample_type: Option<SampleType>,
}

/// Split `--flag=value` at the first `=` into `("--flag", Some("value"))`; an argument without
/// `=` gives `(arg, None)`.
fn split_flag(arg: &str) -> (&str, Option<&str>) {
    match arg.split_once('=') {
        Some((flag, value)) => (flag, Some(value)),
        None => (arg, None),
    }
}

/// `16`, `24`, `32f` or `same` (the last is `None`), else an error that starts with `--bits`.
pub fn parse_bits(text: &str) -> Result<Option<SampleType>, String> {
    match text {
        "16" => Ok(Some(SampleType::Int16)),
        "24" => Ok(Some(SampleType::Int24)),
        "32f" => Ok(Some(SampleType::Float32)),
        "same" => Ok(None),
        other => Err(format!("--bits must be 16, 24, 32f or same, not {other:?}")),
    }
}

/// Parse the batch command line (see the module docs for the rules).
pub fn parse_batch_args(args: &[String]) -> Result<BatchCli, String> {
    let mut batch = false;
    let mut out_dir: Option<PathBuf> = None;
    let mut name_pattern: Option<String> = None;
    let mut sample_type: Option<SampleType> = None;
    let mut bits_seen = false;
    let mut steps: Vec<Op> = Vec::new();
    let mut inputs: Vec<PathBuf> = Vec::new();
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        i += 1;
        if arg == "--" {
            inputs.extend(args.iter().skip(i).map(PathBuf::from));
            break;
        }
        if !arg.starts_with('-') {
            inputs.push(PathBuf::from(arg));
            continue;
        }
        let (flag, inline) = split_flag(arg);
        if flag == "--batch" {
            if inline.is_some() {
                return Err("--batch takes no value".to_string());
            }
            if batch {
                return Err("--batch given more than once".to_string());
            }
            batch = true;
            continue;
        }
        if !["--out", "--name", "--bits", "--step"].contains(&flag) {
            return Err(format!("unknown option {flag}"));
        }
        let value = match inline {
            Some(value) => value,
            None => match args.get(i) {
                Some(next) if !next.starts_with("--") => {
                    i += 1;
                    next.as_str()
                }
                _ => return Err(format!("{flag} needs a value")),
            },
        };
        match flag {
            "--out" => {
                if out_dir.is_some() {
                    return Err("--out given more than once".to_string());
                }
                if value.is_empty() {
                    return Err("--out needs a value".to_string());
                }
                out_dir = Some(PathBuf::from(value));
            }
            "--name" => {
                if name_pattern.is_some() {
                    return Err("--name given more than once".to_string());
                }
                output_name(value, Path::new("x.wav"), 0).map_err(|e| format!("--name: {e}"))?;
                name_pattern = Some(value.to_string());
            }
            "--bits" => {
                if bits_seen {
                    return Err("--bits given more than once".to_string());
                }
                bits_seen = true;
                sample_type = parse_bits(value)?;
            }
            _ => {
                let op = Op::parse(value).map_err(|e| format!("--step {value:?}: {e}"))?;
                steps.push(op);
            }
        }
    }
    if !batch {
        return Err("missing --batch".to_string());
    }
    let Some(out_dir) = out_dir else {
        return Err("missing --out".to_string());
    };
    if inputs.is_empty() {
        return Err("no input files".to_string());
    }
    Ok(BatchCli { inputs, out_dir, name_pattern: name_pattern.unwrap_or_else(|| "{name}".to_string()), steps, sample_type })
}

/// The help text for the batch command line.
pub fn usage() -> String {
    [
        "usage: wavcraft --batch --out DIR [--name PATTERN] [--bits 16|24|32f|same] [--step \"<op words>\"]... [--] FILE...",
        "  --batch         run without a window (required)",
        "  --out DIR       output folder (required)",
        "  --name PATTERN  output name without .wav; {name} is the input's name, {n} its place from 1 (default {name})",
        "  --bits TYPE     sample type: 16, 24, 32f (float) or same as the source (default same)",
        "  --step \"WORDS\"  one step: normalize peak=<dBFS>, gain db=<dB> or mono; repeat for a chain, run in order",
        "  --              end of options: every later argument is an input file",
    ]
    .join(
        "
",
    )
}

/// The job this command line describes (`overwrite` stays off).
pub fn to_job(cli: &BatchCli) -> Job {
    Job {
        inputs: cli.inputs.clone(),
        ops: cli.steps.clone(),
        out_dir: cli.out_dir.clone(),
        pattern: cli.name_pattern.clone(),
        sample_type: cli.sample_type,
        overwrite: false,
    }
}
