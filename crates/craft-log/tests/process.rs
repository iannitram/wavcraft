//! Whole-process checks of the command-line logger: things a unit test cannot reach because they
//! live in the process itself (its arguments, its standard error, a hang at open). This file has
//! its own `main` (`harness = false`): run as a test it starts copies of itself as the "program"
//! (`CRAFT_LOG_TEST_CHILD=<role>`), each with what the case needs, and checks how they end.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const APP: craft_log::App = craft_log::App { name: "app", version: "1.0.0", ours: "app", default_filter: "warn,app=info" };

fn main() {
    if let Ok(role) = std::env::var("CRAFT_LOG_TEST_CHILD") {
        child(&role);
        return;
    }
    let cases: &[(&str, fn())] = &[
        ("a_non_unicode_argument_is_logged_not_a_panic", a_non_unicode_argument_is_logged_not_a_panic),
        ("a_failing_command_with_standard_error_full_exits_1", a_failing_command_with_standard_error_full_exits_1),
        ("a_failing_test_capture_with_standard_error_full_does_not_abort", a_failing_test_capture_with_standard_error_full_does_not_abort),
        ("a_fifo_in_place_of_the_log_does_not_hang", a_fifo_in_place_of_the_log_does_not_hang),
        ("many_runs_at_once_on_a_full_log_lose_nothing", many_runs_at_once_on_a_full_log_lose_nothing),
        ("a_secret_argument_is_never_logged", a_secret_argument_is_never_logged),
        ("a_url_password_or_a_secret_switchs_value_is_never_logged", a_url_password_or_a_secret_switchs_value_is_never_logged),
    ];
    let filter = std::env::args().skip(1).find(|a| !a.starts_with('-'));
    let mut failed = Vec::new();
    for (name, case) in cases {
        if filter.as_deref().is_some_and(|f| !name.contains(f)) {
            continue;
        }
        let ok = std::panic::catch_unwind(case).is_ok();
        println!("test {name} ... {}", if ok { "ok" } else { "FAILED" });
        if !ok {
            failed.push(*name);
        }
    }
    if !failed.is_empty() {
        println!("failures: {failed:?}");
        std::process::exit(1);
    }
}

/// The "program": what a command-line run does with craft-log, by role.
fn child(role: &str) {
    let dir = std::env::var_os("CRAFT_LOG_TEST_DIR").map(PathBuf::from);
    match role {
        "run" => {
            wait_for_start();
            let _ = craft_log::cli::install(&APP, dir.as_deref(), &[]);
            log::warn!(target: "app", "child {} ran", std::env::var("CRAFT_LOG_TEST_TAG").unwrap_or_default());
            log::logger().flush();
        }
        "fail" => {
            let _ = craft_log::cli::install(&APP, dir.as_deref(), &[]);
            craft_log::cli::failed(&APP, "the reason it failed");
            std::process::exit(1);
        }
        "secret" => {
            let _ = craft_log::cli::install(&APP, dir.as_deref(), &["--token"]);
            log::warn!(target: "app", "connecting with s3cr3t-value");
            craft_log::cli::failed(&APP, "the server refused other-secret");
            std::process::exit(1);
        }
        "echo-fail" => {
            // the program echoes its arguments into its error, as a failing import does
            let _ = craft_log::cli::install(&APP, dir.as_deref(), &[]);
            let args: Vec<String> = std::env::args().skip(1).collect();
            craft_log::cli::failed(&APP, &format!("not found: {}", args.join(", ")));
            std::process::exit(1);
        }
        "capture-panic" => {
            let r = std::thread::spawn(|| {
                let _log = craft_log::testing::capture();
                log::error!(target: "app", "the reason the test failed");
                panic!("deliberate");
            })
            .join();
            assert!(r.is_err());
        }
        other => panic!("unknown role {other}"),
    }
}

/// Many runs started one after another start milliseconds apart; `CRAFT_LOG_TEST_START_AT`
/// (milliseconds since 1970) has them all begin at the same moment.
fn wait_for_start() {
    let Some(at) = std::env::var("CRAFT_LOG_TEST_START_AT").ok().and_then(|v| v.parse::<u64>().ok()) else { return };
    let at = std::time::UNIX_EPOCH + Duration::from_millis(at);
    if let Ok(d) = at.duration_since(std::time::SystemTime::now()) {
        std::thread::sleep(d);
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!("craft-log-process-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn me(role: &str, dir: &Path) -> Command {
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.env("CRAFT_LOG_TEST_CHILD", role).env("CRAFT_LOG_TEST_DIR", dir).env_remove("CRAFT_LOG").env_remove("RUST_LOG").stdin(Stdio::null());
    c
}

/// Wait for `child` at most `limit`; kill it and return `None` when it does not end.
fn wait_at_most(child: &mut std::process::Child, limit: Duration) -> Option<ExitStatus> {
    let t0 = Instant::now();
    loop {
        if let Some(s) = child.try_wait().unwrap() {
            return Some(s);
        }
        if t0.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Fault 1: `std::env::args()` panics on an argument that is not Unicode (a Latin-1 file name on
/// Linux), which turned a usage error (exit 2) into a crash (exit 101).
fn a_non_unicode_argument_is_logged_not_a_panic() {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let dir = temp_dir("nonunicode");
        let arg = std::ffi::OsString::from_vec(vec![b'f', 0xff, b'.', b'm', b'o', b'v']);
        let out = me("run", &dir).arg(&arg).output().unwrap();
        assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
        let log = std::fs::read_to_string(dir.join("app-cli.log")).unwrap();
        assert!(log.contains("f\u{fffd}.mov"), "{log}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Fault 2: `eprintln!` panics when standard error cannot be written, so a failing command with
/// standard error full (`2>/dev/full`) or closed exited 101 instead of 1.
fn a_failing_command_with_standard_error_full_exits_1() {
    let Ok(full) = std::fs::OpenOptions::new().write(true).open("/dev/full") else { return };
    let dir = temp_dir("devfull");
    let status = me("fail", &dir).stdout(Stdio::null()).stderr(full).status().unwrap();
    assert_eq!(status.code(), Some(1), "{status}");
    let log = std::fs::read_to_string(dir.join("app-cli.log")).unwrap();
    assert!(log.contains("the reason it failed"), "{log}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Fault 2, the test helper: a failing test's capture printed its lines with `eprint!` from a
/// destructor running during the panic; a second panic there aborts the whole process.
fn a_failing_test_capture_with_standard_error_full_does_not_abort() {
    let Ok(full) = std::fs::OpenOptions::new().write(true).open("/dev/full") else { return };
    let dir = temp_dir("capture");
    let status = me("capture-panic", &dir).stdout(Stdio::null()).stderr(full).status().unwrap();
    assert_eq!(status.code(), Some(0), "{status}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Fault 3: opening a FIFO for writing waits for a reader, so a FIFO in place of the log file
/// hung every command. The log is never waited for.
fn a_fifo_in_place_of_the_log_does_not_hang() {
    let dir = temp_dir("fifo");
    let fifo = dir.join("app-cli.log");
    match Command::new("mkfifo").arg(&fifo).status() {
        Ok(s) if s.success() => {}
        _ => return, // no mkfifo (Windows): nothing to check
    }
    let mut child = me("run", &dir).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap();
    let status = wait_at_most(&mut child, Duration::from_secs(10));
    assert_eq!(status.and_then(|s| s.code()), Some(0), "the run hung on the FIFO (killed after 10 s) or failed: {status:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Fault 5: 48 runs at once on a full log. The roll-over raced: a 16 MiB earlier file was replaced
/// by a 309-byte one, and lines written into a file that was then moved and replaced were lost.
/// Every run's line must land in the log or the earlier file, and the earlier file must still be
/// the full one.
fn many_runs_at_once_on_a_full_log_lose_nothing() {
    const RUNS: usize = 48;
    for round in 0..5 {
        let dir = temp_dir("concurrent");
        let full = dir.join("app-cli.log");
        let line = format!("{}\n", "x".repeat(99));
        let mut body = String::with_capacity(craft_log::cli::MAX_FILE_BYTES as usize);
        while (body.len() as u64) < craft_log::cli::MAX_FILE_BYTES {
            body.push_str(&line);
        }
        body.push_str("the last line of the full log\n");
        std::fs::write(&full, &body).unwrap();
        let full_len = body.len() as u64;
        let start = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap() + Duration::from_millis(1500);
        let start = start.as_millis().to_string();
        let children: Vec<_> = (0..RUNS)
            .map(|n| {
                me("run", &dir)
                    .env("CRAFT_LOG_TEST_TAG", format!("r{round}-n{n}"))
                    .env("CRAFT_LOG_TEST_START_AT", &start)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap()
            })
            .collect();
        for mut c in children {
            assert_eq!(wait_at_most(&mut c, Duration::from_secs(30)).and_then(|s| s.code()), Some(0));
        }
        let earlier = std::fs::read(dir.join("app-cli.1.log")).unwrap_or_default();
        assert!(earlier.len() as u64 >= full_len, "round {round}: the earlier file was replaced: {} bytes, was {full_len}", earlier.len());
        assert!(String::from_utf8_lossy(&earlier).contains("the last line of the full log"), "round {round}: the full log is gone");
        let both = format!("{}{}", String::from_utf8_lossy(&earlier), std::fs::read_to_string(&full).unwrap_or_default());
        let missing: Vec<usize> = (0..RUNS).filter(|n| !both.contains(&format!("child r{round}-n{n} ran\n"))).collect();
        assert!(missing.is_empty(), "round {round}: no line from runs {missing:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A secret argument (`--token <value>`, `--token=<value>`): its value is never written, in the
/// command line, in any later line (the failing run's error included), in text or JSON, in the
/// file or in the copy on standard error.
fn a_secret_argument_is_never_logged() {
    for json in ["0", "1"] {
        let dir = temp_dir("secret");
        let out = me("secret", &dir)
            .args(["export", "--token", "s3cr3t-value", "--token=other-secret", "x"])
            .env("CRAFT_LOG_JSON", json)
            .env("CRAFT_LOG_STDERR", "1")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1));
        let log = std::fs::read_to_string(dir.join("app-cli.log")).unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        for (what, text) in [("log", log.as_str()), ("stderr", &*stderr)] {
            assert!(!text.contains("s3cr3t-value") && !text.contains("other-secret"), "JSON {json}, {what}: {text}");
            assert!(text.contains("export --token *** --token=*** x"), "JSON {json}, {what}: {text}");
            assert!(text.contains("connecting with ***") && text.contains("the server refused ***"), "JSON {json}, {what}: {text}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Arguments nobody named as secret: a URL's password, a token used as a URL's user name, and a
/// stray argument equal to a secret environment switch's value never reach the log, in the
/// command line or in the failing run's error.
fn a_url_password_or_a_secret_switchs_value_is_never_logged() {
    let dir = temp_dir("url-env");
    let out = me("echo-fail", &dir)
        .args(["import", "https://sam:hunter22pw@example.com/a.mp4", "https://ghp_tokenuser1@example.com/b", "tok-envsecret-123"])
        .env("APP_SERVICE_TOKEN", "tok-envsecret-123")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let log = std::fs::read_to_string(dir.join("app-cli.log")).unwrap();
    for s in ["hunter22pw", "ghp_tokenuser1", "tok-envsecret-123"] {
        assert!(!log.contains(s), "{s}: {log}");
    }
    assert!(log.contains("import https://sam:***@example.com/a.mp4 https://***@example.com/b ***"), "{log}");
    let _ = std::fs::remove_dir_all(&dir);
}
