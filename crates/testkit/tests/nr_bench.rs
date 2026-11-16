//! G2.11: the noise reduction bench's `--time` mode runs on a 10 s file and prints one
//! `nr_1h_stereo_seconds=<number>` line.

use std::process::Command;

#[test]
fn g2_bench_prints_nr_1h_stereo_seconds() {
    // 10 s of a 1-hour file: 10 / 3600 of the 1-hour scale.
    let dir = std::env::temp_dir().join(format!("wavcraft-nr-bench-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out =
        Command::new(env!("CARGO_BIN_EXE_wavcraft-nr-bench")).args(["--time", "--scale", "0.0027777778", "--sessions"]).arg(&dir).output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(out.status.success(), "bench failed: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 1, "expected one line, got {stdout:?}");
    let number = lines[0].strip_prefix("nr_1h_stereo_seconds=").unwrap_or_else(|| panic!("bad line {:?}", lines[0]));
    let seconds: f64 = number.parse().unwrap();
    assert!(seconds.is_finite() && seconds >= 0.0, "seconds {seconds}");
}
