//! The command line as data: `parse` turns the arguments after the program name into a `Command`.
//! It opens no file, reads no environment and prints nothing; `main.rs` acts on the result.
//!
//! A first argument of `run`, `convert` or `inspect` picks a subcommand. Anything else makes the
//! window's command line: `[--session-dir DIR | --session-dir=DIR] [--reset-preferences] [FILE]`.

use std::ffi::OsString;
use std::path::PathBuf;

const WINDOW_USAGE: &str = "usage: wavcraft [--session-dir DIR] [--reset-preferences] [FILE]";
const RUN_USAGE: &str = "usage: wavcraft run <script> <file> [-o <out>]";
const CONVERT_USAGE: &str = "usage: wavcraft convert <in> <out>";
const INSPECT_USAGE: &str = "usage: wavcraft inspect <file>";

/// The window's command line.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Args {
    pub file: Option<PathBuf>,
    pub session_dir: Option<PathBuf>,
    /// `--reset-preferences`: move the settings file aside and start with the defaults.
    pub reset_preferences: bool,
}

/// What the command line asks for.
#[derive(Debug, PartialEq, Eq)]
// The subcommands' fields are read once N2.2 to N2.4 act on them.
#[allow(dead_code)]
pub enum Command {
    Window(Args),
    Run { script: PathBuf, file: PathBuf, out: Option<PathBuf> },
    Convert { input: PathBuf, output: PathBuf },
    Inspect { file: PathBuf },
}

/// Parses the arguments after the program name. An error is a message for standard error: the
/// problem, then the usage line of the command it was about.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Command, String> {
    let args: Vec<OsString> = args.into_iter().collect();
    match args.split_first() {
        Some((first, rest)) => match first.to_str() {
            Some("run") => run(rest),
            Some("convert") => convert(rest),
            Some("inspect") => inspect(rest),
            _ => window(&args),
        },
        None => window(&args),
    }
}

fn window(args: &[OsString]) -> Result<Command, String> {
    let mut out = Args::default();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let text = arg.to_string_lossy();
        if text == "--session-dir" {
            out.session_dir = rest.next().map(PathBuf::from);
        } else if text == "--reset-preferences" {
            out.reset_preferences = true;
        } else if let Some(dir) = text.strip_prefix("--session-dir=") {
            out.session_dir = Some(PathBuf::from(dir));
        } else if is_flag(&text) {
            return Err(unknown_option(&text, WINDOW_USAGE));
        } else if out.file.is_none() {
            out.file = Some(PathBuf::from(arg));
        }
    }
    Ok(Command::Window(out))
}

fn run(rest: &[OsString]) -> Result<Command, String> {
    let (found, out) = operands(rest, RUN_USAGE, true)?;
    let [script, file] = exactly::<2>(found, RUN_USAGE)?;
    Ok(Command::Run { script, file, out })
}

fn convert(rest: &[OsString]) -> Result<Command, String> {
    let (found, _) = operands(rest, CONVERT_USAGE, false)?;
    let [input, output] = exactly::<2>(found, CONVERT_USAGE)?;
    Ok(Command::Convert { input, output })
}

fn inspect(rest: &[OsString]) -> Result<Command, String> {
    let (found, _) = operands(rest, INSPECT_USAGE, false)?;
    let [file] = exactly::<1>(found, INSPECT_USAGE)?;
    Ok(Command::Inspect { file })
}

/// The positional arguments of a subcommand, and its `-o` value when the subcommand takes one.
/// Any other flag is an error.
fn operands(rest: &[OsString], usage: &str, takes_out: bool) -> Result<(Vec<PathBuf>, Option<PathBuf>), String> {
    let mut found = Vec::new();
    let mut out = None;
    let mut args = rest.iter();
    while let Some(arg) = args.next() {
        let text = arg.to_string_lossy();
        if takes_out && text == "-o" {
            let value = args.next().ok_or_else(|| format!("-o needs a file name\n{usage}"))?;
            out = Some(PathBuf::from(value));
        } else if is_flag(&text) {
            return Err(unknown_option(&text, usage));
        } else {
            found.push(PathBuf::from(arg));
        }
    }
    Ok((found, out))
}

fn exactly<const N: usize>(found: Vec<PathBuf>, usage: &str) -> Result<[PathBuf; N], String> {
    <[PathBuf; N]>::try_from(found).map_err(
        |found| {
            if found.len() < N { format!("missing operand\n{usage}") } else { format!("too many operands\n{usage}") }
        },
    )
}

/// A dash followed by something: a flag, not a file name. A lone dash is a file name.
fn is_flag(text: &str) -> bool {
    text.starts_with('-') && text.len() > 1
}

fn unknown_option(flag: &str, usage: &str) -> String {
    format!("unknown option '{flag}'\n{usage}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<OsString> {
        a.iter().map(OsString::from).collect()
    }

    /// The error message, or an empty string when the parse succeeded.
    fn err(a: &[&str]) -> String {
        parse(args(a)).err().unwrap_or_default()
    }

    #[test]
    fn convert_takes_in_and_out() {
        assert_eq!(parse(args(&["convert", "a.wav", "b.flac"])), Ok(Command::Convert { input: "a.wav".into(), output: "b.flac".into() }));
    }

    #[test]
    fn inspect_takes_one_file() {
        assert_eq!(parse(args(&["inspect", "a.wav"])), Ok(Command::Inspect { file: "a.wav".into() }));
    }

    #[test]
    fn a_bad_flag_is_an_error_with_a_message() {
        let message = err(&["convert", "a.wav", "b.flac", "--nope"]);
        assert!(message.contains("--nope"), "{message}");
    }

    #[test]
    fn a_bad_flag_with_no_subcommand_is_an_error() {
        let message = err(&["--nope"]);
        assert!(message.contains("--nope"), "{message}");
    }

    #[test]
    fn a_missing_operand_is_an_error() {
        let message = err(&["convert", "a.wav"]);
        assert!(message.contains("convert <in> <out>"), "{message}");
    }

    #[test]
    fn no_subcommand_is_the_window() {
        assert_eq!(
            parse(args(&["a.wav", "--session-dir=D"])),
            Ok(Command::Window(Args { file: Some("a.wav".into()), session_dir: Some("D".into()), reset_preferences: false }))
        );
    }

    #[test]
    fn run_takes_o() {
        assert_eq!(
            parse(args(&["run", "s.txt", "a.wav", "-o", "b.wav"])),
            Ok(Command::Run { script: "s.txt".into(), file: "a.wav".into(), out: Some("b.wav".into()) })
        );
    }
}
