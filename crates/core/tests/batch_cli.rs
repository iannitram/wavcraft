//! C239-a: the batch command-line parser. Pure functions over argument lists; no file is touched.

use std::path::{Path, PathBuf};

use wavcraft_core::batch::{Op, output_name};
use wavcraft_core::batch_cli::{BatchCli, parse_batch_args, parse_bits, to_job, usage};
use wavcraft_core::save::SampleType;

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| (*w).to_string()).collect()
}

fn parse(words: &[&str]) -> Result<BatchCli, String> {
    parse_batch_args(&argv(words))
}

fn ok(words: &[&str]) -> BatchCli {
    parse(words).unwrap()
}

fn err(words: &[&str]) -> String {
    parse(words).unwrap_err()
}

fn p(text: &str) -> PathBuf {
    PathBuf::from(text)
}

#[test]
fn a_full_valid_line() {
    let cli = ok(&[
        "--batch",
        "--out",
        "/home/user/out",
        "--name",
        "{name}_x",
        "--bits",
        "24",
        "--step",
        "normalize peak=-1",
        "--step",
        "gain db=3",
        "/home/user/in/a.wav",
        "/home/user/in/b.wav",
    ]);
    assert_eq!(
        cli,
        BatchCli {
            inputs: vec![p("/home/user/in/a.wav"), p("/home/user/in/b.wav")],
            out_dir: p("/home/user/out"),
            name_pattern: "{name}_x".to_string(),
            steps: vec![Op::NormalizePeak { target_db: -1.0 }, Op::Gain { db: 3.0 }],
            sample_type: Some(SampleType::Int24),
        }
    );
}

#[test]
fn the_smallest_line_uses_the_defaults() {
    let cli = ok(&["--batch", "--out", "/home/user/out", "/home/user/a.wav"]);
    assert_eq!(cli.inputs, vec![p("/home/user/a.wav")]);
    assert_eq!(cli.out_dir, p("/home/user/out"));
    assert_eq!(cli.name_pattern, "{name}");
    assert!(cli.steps.is_empty());
    assert_eq!(cli.sample_type, None);
}

#[test]
fn flags_and_files_in_any_order() {
    let cli = ok(&["/home/user/a.wav", "--out", "/home/user/o", "--batch", "/home/user/my song.wav", "--bits", "16", "/home/user/x=y.wav"]);
    assert_eq!(cli.inputs, vec![p("/home/user/a.wav"), p("/home/user/my song.wav"), p("/home/user/x=y.wav")]);
    assert_eq!(cli.out_dir, p("/home/user/o"));
    assert_eq!(cli.sample_type, Some(SampleType::Int16));
}

#[test]
fn the_equals_form_is_accepted() {
    let cli = ok(&["--batch", "--out=/home/user/o", "--name={n}", "--bits=32f", "--step=mono", "/home/user/a.wav"]);
    assert_eq!(cli.out_dir, p("/home/user/o"));
    assert_eq!(cli.name_pattern, "{n}");
    assert_eq!(cli.sample_type, Some(SampleType::Float32));
    assert_eq!(cli.steps, vec![Op::ToMono]);
}

#[test]
fn a_value_keeps_every_equals_sign_after_the_first() {
    let cli = ok(&["--batch", "--out=/home/user/a=b", "--step=gain db=3", "/home/user/a.wav"]);
    assert_eq!(cli.out_dir, p("/home/user/a=b"));
    assert_eq!(cli.steps, vec![Op::Gain { db: 3.0 }]);
}

#[test]
fn a_value_may_start_with_one_dash_or_with_two_dashes_in_the_equals_form() {
    let cli = ok(&["--batch", "--out", "-d", "--name=--x", "/home/user/a.wav"]);
    assert_eq!(cli.out_dir, p("-d"));
    assert_eq!(cli.name_pattern, "--x");
}

#[test]
fn bits_values() {
    assert_eq!(parse_bits("16"), Ok(Some(SampleType::Int16)));
    assert_eq!(parse_bits("24"), Ok(Some(SampleType::Int24)));
    assert_eq!(parse_bits("32f"), Ok(Some(SampleType::Float32)));
    assert_eq!(parse_bits("same"), Ok(None));
    assert_eq!(ok(&["--batch", "--out", "o", "--bits", "same", "a.wav"]).sample_type, None);
}

#[test]
fn a_bad_bits_value_is_refused_and_shown() {
    assert_eq!(parse_bits("x"), Err("--bits must be 16, 24, 32f or same, not \"x\"".to_string()));
    assert_eq!(parse_bits(""), Err("--bits must be 16, 24, 32f or same, not \"\"".to_string()));
    for bad in ["32", "8", "24 ", "SAME", "+16", "16f"] {
        assert!(parse_bits(bad).is_err(), "{bad:?} should be refused");
    }
    assert_eq!(err(&["--batch", "--out", "o", "--bits", "20", "a.wav"]), "--bits must be 16, 24, 32f or same, not \"20\"");
    assert_eq!(err(&["--batch", "--out", "o", "--bits=", "a.wav"]), "--bits must be 16, 24, 32f or same, not \"\"");
}

#[test]
fn a_missing_batch_flag_is_named() {
    assert_eq!(err(&["--out", "/home/user/o", "/home/user/a.wav"]), "missing --batch");
}

#[test]
fn a_missing_out_flag_is_named() {
    assert_eq!(err(&["--batch", "/home/user/a.wav"]), "missing --out");
}

#[test]
fn no_input_files_is_an_error() {
    assert_eq!(err(&["--batch", "--out", "/home/user/o"]), "no input files");
    assert_eq!(err(&["--batch", "--out", "/home/user/o", "--",]), "no input files");
}

#[test]
fn empty_input_names_the_first_missing_thing() {
    assert_eq!(err(&[]), "missing --batch");
    assert_eq!(err(&["--out", "/home/user/o"]), "missing --batch");
    assert_eq!(err(&["--batch"]), "missing --out");
}

#[test]
fn an_unknown_flag_is_named() {
    assert_eq!(err(&["--batch", "--out", "/home/user/o", "--frob", "/home/user/a.wav"]), "unknown option --frob");
    assert_eq!(err(&["--batch", "--out", "/home/user/o", "--frob=3", "/home/user/a.wav"]), "unknown option --frob");
    assert_eq!(err(&["--batch", "--out", "/home/user/o", "-x", "/home/user/a.wav"]), "unknown option -x");
    assert_eq!(err(&["--batch", "--out", "/home/user/o", "-", "/home/user/a.wav"]), "unknown option -");
    // It is reported before anything missing.
    assert_eq!(err(&["--frob"]), "unknown option --frob");
}

#[test]
fn a_flag_that_takes_no_value_refuses_one() {
    assert_eq!(err(&["--batch=yes", "--out", "o", "a.wav"]), "--batch takes no value");
}

#[test]
fn double_dash_ends_the_flags() {
    let cli = ok(&["--batch", "--out", "/home/user/o", "--", "--frob", "-x", "--", "/home/user/a.wav"]);
    assert_eq!(cli.inputs, vec![p("--frob"), p("-x"), p("--"), p("/home/user/a.wav")]);
}

#[test]
fn a_flag_after_double_dash_is_a_file_not_a_flag() {
    assert_eq!(err(&["--out", "/home/user/o", "--", "--batch", "/home/user/a.wav"]), "missing --batch");
    let cli = ok(&["--batch", "--out", "o", "--", "--out", "--step", "mono"]);
    assert_eq!(cli.out_dir, p("o"));
    assert_eq!(cli.inputs, vec![p("--out"), p("--step"), p("mono")]);
    assert!(cli.steps.is_empty());
}

#[test]
fn a_missing_value_is_named_for_each_flag() {
    for flag in ["--out", "--name", "--bits", "--step"] {
        let mut v = argv(&["--batch", "/home/user/a.wav", "--out", "/home/user/o"]);
        if flag == "--out" {
            v.truncate(2);
        }
        v.push(flag.to_string());
        let e = parse_batch_args(&v).unwrap_err();
        assert_eq!(e, format!("{flag} needs a value"));
    }
}

#[test]
fn a_flag_does_not_swallow_the_next_flag_as_its_value() {
    assert_eq!(err(&["--batch", "--out", "--name", "x", "/home/user/a.wav"]), "--out needs a value");
    assert_eq!(err(&["--batch", "--out", "/home/user/o", "--name", "--batch", "/home/user/a.wav"]), "--name needs a value");
    assert_eq!(err(&["--batch", "--out", "/home/user/o", "--step", "--", "/home/user/a.wav"]), "--step needs a value");
}

#[test]
fn an_empty_out_is_refused_in_both_forms() {
    assert_eq!(err(&["--batch", "--out=", "/home/user/a.wav"]), "--out needs a value");
    assert_eq!(err(&["--batch", "--out", "", "/home/user/a.wav"]), "--out needs a value");
}

#[test]
fn a_repeated_single_flag_is_refused() {
    assert_eq!(err(&["--batch", "--batch", "--out", "o", "a.wav"]), "--batch given more than once");
    assert_eq!(err(&["--batch", "--out", "a", "--out", "b", "f.wav"]), "--out given more than once");
    assert_eq!(err(&["--batch", "--out", "a", "--out=a", "f.wav"]), "--out given more than once");
    assert_eq!(err(&["--batch", "--out", "o", "--name", "x", "--name=y", "f.wav"]), "--name given more than once");
    assert_eq!(err(&["--batch", "--out", "o", "--bits", "16", "--bits", "16", "f.wav"]), "--bits given more than once");
    assert_eq!(err(&["--batch", "--out", "o", "--bits", "same", "--bits", "24", "f.wav"]), "--bits given more than once");
}

#[test]
fn repeated_steps_keep_their_order() {
    let cli = ok(&[
        "--batch",
        "--step",
        "gain db=1",
        "--out",
        "o",
        "--step",
        "mono",
        "--step=gain db=-2",
        "a.wav",
        "--step",
        "normalize peak=-3",
        "--step",
        "gain db=1",
    ]);
    assert_eq!(cli.steps, vec![Op::Gain { db: 1.0 }, Op::ToMono, Op::Gain { db: -2.0 }, Op::NormalizePeak { target_db: -3.0 }, Op::Gain { db: 1.0 }]);
}

#[test]
fn a_bad_step_shows_the_error_of_op_parse() {
    let want = Op::parse("frob x").unwrap_err();
    assert_eq!(want, "no operation named \"frob\" (normalize, gain, mono)");
    assert_eq!(err(&["--batch", "--out", "o", "--step", "frob x", "a.wav"]), format!("--step \"frob x\": {want}"));

    let want = Op::parse("gain db=abc").unwrap_err();
    assert_eq!(want, "db is not a number: abc");
    assert_eq!(err(&["--batch", "--out", "o", "--step=gain db=abc", "a.wav"]), format!("--step \"gain db=abc\": {want}"));

    let want = Op::parse("normalize peak=5").unwrap_err();
    assert_eq!(want, "peak must be 0 dBFS or below");
    assert_eq!(err(&["--batch", "--out", "o", "--step", "normalize peak=5", "a.wav"]), format!("--step \"normalize peak=5\": {want}"));

    let want = Op::parse("").unwrap_err();
    assert_eq!(err(&["--batch", "--out", "o", "--step=", "a.wav"]), format!("--step \"\": {want}"));
}

#[test]
fn the_first_bad_step_is_the_one_reported() {
    let e = err(&["--batch", "--out", "o", "--step", "mono", "--step", "frob", "--step", "gain", "a.wav"]);
    assert!(e.contains("\"frob\""), "{e}");
    assert!(!e.contains("gain needs"), "{e}");
}

#[test]
fn a_bad_name_pattern_is_refused_at_parse_time() {
    for bad in ["a/b", "a\\b", "c:x", "x..y", ".hidden", ""] {
        let flag = format!("--name={bad}");
        let e = err(&["--batch", "--out", "o", flag.as_str(), "a.wav"]);
        assert!(e.starts_with("--name: the name pattern gives "), "{bad:?}: {e}");
        assert!(e.ends_with("which is not a plain file name"), "{bad:?}: {e}");
    }
    let cli = ok(&["--batch", "--out", "o", "--name", "{n}-{name}", "a.wav"]);
    assert_eq!(cli.name_pattern, "{n}-{name}");
}

#[test]
fn usage_names_every_flag_and_value() {
    let u = usage();
    assert!(u.starts_with("usage: wavcraft --batch --out DIR"), "{u}");
    assert!(u.contains("[--name PATTERN]"), "{u}");
    assert!(u.contains("[--bits 16|24|32f|same]"), "{u}");
    assert!(u.contains("FILE..."), "{u}");
    assert!(u.contains("run without a window"), "{u}");
    assert!(u.contains("output folder"), "{u}");
    assert!(u.contains("{n} its place"), "{u}");
    assert!(u.contains("32f (float)"), "{u}");
    assert!(u.contains("normalize peak=<dBFS>"), "{u}");
    assert!(u.contains("end of options"), "{u}");
    assert!(u.lines().count() >= 7, "{u}");
}

#[test]
fn to_job_carries_every_field() {
    let cli = BatchCli {
        inputs: vec![p("/home/user/in/b.wav"), p("/home/user/in/a.wav")],
        out_dir: p("/home/user/out"),
        name_pattern: "{n}_{name}".to_string(),
        steps: vec![Op::Gain { db: 3.0 }, Op::ToMono],
        sample_type: Some(SampleType::Float32),
    };
    let job = to_job(&cli);
    assert_eq!(job.inputs, vec![p("/home/user/in/b.wav"), p("/home/user/in/a.wav")]);
    assert_eq!(job.out_dir, p("/home/user/out"));
    assert_eq!(job.pattern, "{n}_{name}");
    assert_eq!(job.ops, vec![Op::Gain { db: 3.0 }, Op::ToMono]);
    assert_eq!(job.sample_type, Some(SampleType::Float32));
    assert!(!job.overwrite);
}

#[test]
fn to_job_with_the_other_values() {
    let cli = BatchCli {
        inputs: vec![p("/home/user/c.wav")],
        out_dir: p("/home/user/other"),
        name_pattern: "{name}".to_string(),
        steps: Vec::new(),
        sample_type: None,
    };
    let job = to_job(&cli);
    assert_eq!(job.inputs, vec![p("/home/user/c.wav")]);
    assert_eq!(job.out_dir, p("/home/user/other"));
    assert_eq!(job.pattern, "{name}");
    assert!(job.ops.is_empty());
    assert_eq!(job.sample_type, None);
    assert!(!job.overwrite);
}

#[test]
fn a_parsed_line_becomes_the_job_it_says() {
    let cli = ok(&["--batch", "--out", "/home/user/out", "--bits", "16", "--step", "mono", "/home/user/in/a.wav", "/home/user/in/b.wav"]);
    let job = to_job(&cli);
    assert_eq!(job.inputs, cli.inputs);
    assert_eq!(job.ops, vec![Op::ToMono]);
    assert_eq!(job.sample_type, Some(SampleType::Int16));
    assert_eq!(job.pattern, "{name}");
    // The default pattern gives the batch runner's usual names.
    assert_eq!(output_name(&job.pattern, Path::new("/home/user/in/a.wav"), 0), Ok("a.wav".to_string()));
}
