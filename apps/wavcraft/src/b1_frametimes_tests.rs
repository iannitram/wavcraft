//! B1.3: the frame timer and the FRAMETIMES verb through the window. `FRAMETIMES reset` → `OK n=0`;
//! a timed frame is counted; the list keeps only the newest frames; an unknown argument is refused.
//!
//! Hook-up, in apps/wavcraft/src/app.rs: `#[cfg(test)] #[path = "b1_frametimes_tests.rs"] mod b1_frametimes_tests;`

use super::*;

/// A folder for one test, removed when it ends. Declare it before the window.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("wavcraft-b1-frametimes-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch folder");
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn a_window(scratch: &Scratch) -> WaveApp {
    let cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    let mut app = WaveApp::new(&cc, None, Some(scratch.0.join("sessions")), false);
    app.settings_path = None;
    app.recent_path = None;
    app
}

/// One control line, as the channel's reader would run it.
fn ask(app: &mut WaveApp, line: &str) -> String {
    let ctx = app.ctx.clone();
    app.control_request(line, &ctx)
}

#[test]
fn reset_answers_n_zero() {
    let scratch = Scratch::new("reset");
    let mut app = a_window(&scratch);
    assert_eq!(ask(&mut app, "FRAMETIMES reset"), "OK n=0");
    assert_eq!(ask(&mut app, "FRAMETIMES"), "OK n=0 median_ms=0.00 p95_ms=0.00 worst_ms=0.00 over=0");
}

#[test]
fn a_timed_frame_is_counted() {
    let scratch = Scratch::new("timed");
    let mut app = a_window(&scratch);
    app.frame_timer_begin();
    std::thread::sleep(std::time::Duration::from_millis(5));
    app.frame_timer_end();

    let answer = ask(&mut app, "FRAMETIMES");
    assert!(answer.starts_with("OK n=1 median_ms="), "{answer}");
    assert!(answer.contains(" p95_ms="), "{answer}");
    assert!(answer.contains(" worst_ms="), "{answer}");
    assert!(answer.contains(" over="), "{answer}");
    let worst: f64 =
        answer.split(" worst_ms=").nth(1).and_then(|rest| rest.split(' ').next()).and_then(|v| v.parse().ok()).expect("a worst_ms value");
    assert!(worst >= 4.0, "{answer}");

    assert_eq!(ask(&mut app, "FRAMETIMES reset"), "OK n=0");
    assert!(ask(&mut app, "FRAMETIMES").starts_with("OK n=0"));
}

#[test]
fn ending_without_a_start_records_nothing() {
    let scratch = Scratch::new("nostart");
    let mut app = a_window(&scratch);
    app.frame_timer_end();
    assert!(ask(&mut app, "FRAMETIMES").starts_with("OK n=0"));
}

#[test]
fn the_list_keeps_only_the_newest_frames() {
    let scratch = Scratch::new("cap");
    let mut app = a_window(&scratch);
    for _ in 0..FRAME_TIMES_CAP + 5 {
        app.frame_timer_begin();
        app.frame_timer_end();
    }
    assert_eq!(app.frame_times.len(), FRAME_TIMES_CAP);
}

#[test]
fn an_unknown_argument_is_refused() {
    let scratch = Scratch::new("badarg");
    let mut app = a_window(&scratch);
    assert!(ask(&mut app, "FRAMETIMES foo").starts_with("ERR"));
}
