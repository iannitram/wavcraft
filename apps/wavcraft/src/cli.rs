//! The command line: `wavcraft [--session-dir DIR] [--reset-preferences] [FILE]` opens the window, and the
//! subcommands `run`, `convert` and `inspect` work without one. A pure parser: it opens no file and prints nothing.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::Args;

/// What the command line asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    /// No subcommand: the window, with today's arguments.
    Window(Args),
    /// `run <script> [FILE] [-o OUT]`.
    Run { script: PathBuf, file: Option<PathBuf>, out: Option<PathBuf> },
    /// `convert <in> <out>`.
    Convert { input: PathBuf, output: PathBuf },
    /// `inspect <file>`.
    Inspect { file: PathBuf },
}

const WINDOW_USAGE: &str = "usage: wavcraft [--session-dir DIR] [--reset-preferences] [FILE]";
const RUN_USAGE: &str = "usage: wavcraft run <script> [FILE] [-o OUT]";
const CONVERT_USAGE: &str = "usage: wavcraft convert <in> <out>";
const INSPECT_USAGE: &str = "usage: wavcraft inspect <file>";

/// Parses the arguments after the program name. An unknown flag or a wrong number of operands is an error whose
/// message names the problem and gives the usage line.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Command, String> {
    let args: Vec<OsString> = args.into_iter().collect();
    let Some((first, rest)) = args.split_first() else {
        return window(&args);
    };
    match first.to_str() {
        Some("run") => run(rest),
        Some("convert") => convert(rest),
        Some("inspect") => inspect(rest),
        _ => window(&args),
    }
}

/// No subcommand: the window's arguments. Only the flags the window knows are accepted; a file name is any other
/// argument, as before.
fn window(args: &[OsString]) -> Result<Command, String> {
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let text = arg.to_string_lossy();
        if text == "--session-dir" {
            rest.next(); // its value, which may look like a flag
        } else if is_flag(&text) && text != "--reset-preferences" && !text.starts_with("--session-dir=") {
            return Err(format!("unknown option '{text}'\n{WINDOW_USAGE}"));
        }
    }
    Ok(Command::Window(crate::parse_args(args.iter().cloned())))
}

fn run(args: &[OsString]) -> Result<Command, String> {
    let parsed = scan(args, RUN_USAGE, true)?;
    match parsed.operands.as_slice() {
        [script] => Ok(Command::Run { script: script.clone(), file: None, out: parsed.out }),
        [script, file] => Ok(Command::Run { script: script.clone(), file: Some(file.clone()), out: parsed.out }),
        _ => Err(operand_error(&parsed.operands, 2, RUN_USAGE)),
    }
}

fn convert(args: &[OsString]) -> Result<Command, String> {
    let parsed = scan(args, CONVERT_USAGE, false)?;
    match parsed.operands.as_slice() {
        [input, output] => Ok(Command::Convert { input: input.clone(), output: output.clone() }),
        _ => Err(operand_error(&parsed.operands, 2, CONVERT_USAGE)),
    }
}

fn inspect(args: &[OsString]) -> Result<Command, String> {
    let parsed = scan(args, INSPECT_USAGE, false)?;
    match parsed.operands.as_slice() {
        [file] => Ok(Command::Inspect { file: file.clone() }),
        _ => Err(operand_error(&parsed.operands, 1, INSPECT_USAGE)),
    }
}

/// A subcommand's arguments split into operands and the `-o` value (when the subcommand takes one).
struct Scanned {
    operands: Vec<PathBuf>,
    out: Option<PathBuf>,
}

/// Splits a subcommand's arguments. Any flag other than `-o` (when `takes_out`) is an error naming it.
fn scan(args: &[OsString], usage: &str, takes_out: bool) -> Result<Scanned, String> {
    let mut out = Scanned { operands: Vec::new(), out: None };
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let text = arg.to_string_lossy();
        if takes_out && text == "-o" {
            let value = rest.next().ok_or_else(|| format!("-o needs a file name\n{usage}"))?;
            out.out = Some(PathBuf::from(value));
        } else if is_flag(&text) {
            return Err(format!("unknown option '{text}'\n{usage}"));
        } else {
            out.operands.push(PathBuf::from(arg));
        }
    }
    Ok(out)
}

/// `-x` or `--name`; a lone `-` is an operand (standard input by convention).
fn is_flag(text: &str) -> bool {
    text.starts_with('-') && text.len() > 1
}

/// The error for the wrong number of operands: a missing one, or the first extra one.
fn operand_error(operands: &[PathBuf], wanted: usize, usage: &str) -> String {
    match operands.get(wanted) {
        Some(extra) => format!("unexpected argument '{}'\n{usage}", extra.display()),
        None => format!("missing operand\n{usage}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(a: &[&str]) -> Vec<OsString> {
        a.iter().map(OsString::from).collect()
    }

    /// The error message, or an empty string when the parse succeeded (so the assertion on it fails).
    fn err(a: &[&str]) -> String {
        parse(argv(a)).err().unwrap_or_default()
    }

    #[test]
    fn convert_takes_in_and_out() {
        assert_eq!(parse(argv(&["convert", "a.wav", "b.flac"])), Ok(Command::Convert { input: "a.wav".into(), output: "b.flac".into() }));
    }

    #[test]
    fn a_bad_flag_is_an_error_with_a_message() {
        assert!(err(&["convert", "a.wav", "b.flac", "--nope"]).contains("--nope"));
    }

    #[test]
    fn a_bad_flag_with_no_subcommand_is_an_error() {
        assert!(err(&["--nope"]).contains("--nope"));
    }

    #[test]
    fn a_missing_operand_is_an_error() {
        assert!(err(&["convert", "a.wav"]).contains("convert <in> <out>"));
    }

    #[test]
    fn no_subcommand_is_the_window() {
        assert_eq!(
            parse(argv(&["a.wav", "--session-dir=D"])),
            Ok(Command::Window(Args { file: Some("a.wav".into()), session_dir: Some("D".into()), reset_preferences: false }))
        );
    }

    #[test]
    fn run_takes_o() {
        assert_eq!(
            parse(argv(&["run", "s.txt", "a.wav", "-o", "b.wav"])),
            Ok(Command::Run { script: "s.txt".into(), file: Some("a.wav".into()), out: Some("b.wav".into()) })
        );
    }
}
