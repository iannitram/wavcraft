//! K3.4 (locked): `check.next` (F6) and `check.prev` (Shift+F6) move to the next or previous
//! Episode Check finding, select its range and play it. Each clip gives two rows with the same
//! range (Clipping, then True peak), so the step follows the current row while the selection is
//! still that row's range, and goes by the cursor (the selection start) otherwise.
//!
//! These run on the headless window with the silent output: each test re-runs itself in a child
//! process with `WAVCRAFT_NO_AUDIO=1`, as b2_1_spectral_toggle_tests.rs does, so nothing plays
//! through a real device and `playing=` is reliable.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use super::*;

const RATE: u32 = 48_000;
/// 12 s at 48 kHz.
const FRAMES: u32 = 12 * RATE;
/// The four rows `CHECK list` gives for the test file: a Clipping and a True peak row per clip.
const ROWS: &str = "OK rows=4 0,Clipping,144000,144480;1,True peak,144000,144480;2,Clipping,432000,432480;3,True peak,432000,432480";
const CLIP_1: &str = "144000-144480";
const CLIP_2: &str = "432000-432480";

/// True when this is the parent run: it ran `name` in a child with the silent output, and the
/// caller returns.
fn ran_in_silent_child(name: &str) -> bool {
    if std::env::var_os("WAVCRAFT_NO_AUDIO").as_deref() == Some(std::ffi::OsStr::new("1")) {
        return false;
    }
    let status = Command::new(std::env::current_exe().expect("the test executable"))
        .args(["--exact", &format!("app::k3_4_check_nav_tests::{name}"), "--nocapture"])
        .env("WAVCRAFT_NO_AUDIO", "1")
        .status()
        .expect("run the test with silent output");
    assert!(status.success(), "{name} failed with silent output");
    true
}

/// A 16-bit 48 kHz stereo WAV, 12 s of a quiet sawtooth, the left channel at full scale for
/// 480 frames at 3.0 s (144000) and at 9.0 s (432000).
fn write_wav(path: &Path) {
    let data_len = FRAMES * 4;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    for part in [&b"RIFF"[..], &(36 + data_len).to_le_bytes(), b"WAVEfmt ", &16u32.to_le_bytes(), &1u16.to_le_bytes(), &2u16.to_le_bytes()] {
        out.extend_from_slice(part);
    }
    for part in [&RATE.to_le_bytes()[..], &(RATE * 4).to_le_bytes(), &4u16.to_le_bytes(), &16u16.to_le_bytes(), b"data", &data_len.to_le_bytes()] {
        out.extend_from_slice(part);
    }
    for i in 0..FRAMES {
        let base = (5000.0 * (f64::from(i % 97) / 48.0 - 1.0)) as i16;
        let clipped = (144_000..144_480).contains(&i) || (432_000..432_480).contains(&i);
        out.extend_from_slice(&(if clipped { 32767 } else { base }).to_le_bytes());
        out.extend_from_slice(&base.to_le_bytes());
    }
    std::fs::write(path, out).expect("write the test WAV");
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wavcraft-k3-4-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch folder");
    dir
}

/// Run frames (opening, the check's poll) until `done` or 60 s.
fn frames_until(app: &mut WaveApp, done: impl Fn(&mut WaveApp) -> bool) {
    let ctx = app.ctx.clone();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.finish_opening();
            app.pump_open_queue(ui.ctx());
            app.check_run.poll();
        });
        output.textures_delta.clear();
        if done(app) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn ask(app: &mut WaveApp, line: &str) -> String {
    let ctx = app.ctx.clone();
    app.control_request(line, &ctx)
}

/// The test file open in the silent window, not yet checked.
fn opened(dir: &Path) -> WaveApp {
    let wav = dir.join("clips.wav");
    write_wav(&wav);
    let cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    let mut app = WaveApp::new(&cc, None, Some(dir.join("sessions")), false);
    (app.settings_path, app.recent_path) = (None, None);
    assert!(app.player.info().test_sink, "the silent output is in use");
    assert_eq!(ask(&mut app, &format!("OPEN {}", wav.display())), "OK");
    frames_until(&mut app, |a| a.opening.is_none());
    app
}

/// The test file open and checked, with the four rows of `ROWS`.
fn checked(dir: &Path) -> WaveApp {
    let mut app = opened(dir);
    assert_eq!(ask(&mut app, "CHECK run"), "OK");
    frames_until(&mut app, |a| ask(a, "CHECK state").starts_with("OK running=0 progress=100 "));
    assert_eq!(ask(&mut app, "CHECK list"), ROWS);
    app
}

/// After `KEY key`: (the current row from `CHECK state`, `selection=` and `playing=` from STATE).
fn press(app: &mut WaveApp, key: &str) -> (String, String, String) {
    let answer = ask(app, &format!("KEY {key}"));
    assert!(answer.starts_with("OK command=check."), "KEY {key}: {answer}");
    let field = |line: &str, name: &str| line.split(' ').find_map(|f| f.strip_prefix(name)).unwrap_or("?").to_string();
    let check = ask(app, "CHECK state");
    let state = ask(app, "STATE");
    (field(&check, "row="), field(&state, "selection="), field(&state, "playing="))
}

fn at(row: &str, selection: &str) -> (String, String, String) {
    (row.to_string(), selection.to_string(), "1".to_string())
}

#[test]
fn next_walks_every_row_in_order_and_wraps() {
    // Catches: a step by frame only (from row 0 it would skip row 1, whose range is the same), no
    // selection or no play, and no wrap from the last row to row 0.
    if ran_in_silent_child("next_walks_every_row_in_order_and_wraps") {
        return;
    }
    let mut app = checked(&scratch("next"));
    assert_eq!(press(&mut app, "F6"), at("0", CLIP_1));
    assert_eq!(press(&mut app, "F6"), at("1", CLIP_1));
    assert_eq!(press(&mut app, "F6"), at("2", CLIP_2));
    assert_eq!(press(&mut app, "F6"), at("3", CLIP_2));
    assert_eq!(press(&mut app, "F6"), at("0", CLIP_1));
}

#[test]
fn the_roadmap_case_next_next_prev_prev() {
    // Catches: prev not stepping back one row from the current row, and prev from row 0 not
    // wrapping to the last row (the K3.4 Verify: rows 0, 1, 0, then 3).
    if ran_in_silent_child("the_roadmap_case_next_next_prev_prev") {
        return;
    }
    let mut app = checked(&scratch("roadmap"));
    assert_eq!(press(&mut app, "F6"), at("0", CLIP_1));
    assert_eq!(press(&mut app, "F6"), at("1", CLIP_1));
    assert_eq!(press(&mut app, "Shift+F6"), at("0", CLIP_1));
    assert_eq!(press(&mut app, "Shift+F6"), at("3", CLIP_2));
}

#[test]
fn prev_from_the_start_goes_to_the_last_row() {
    // Catches: prev from the cursor at frame 0 not using list::prev_before (which wraps to row 3).
    if ran_in_silent_child("prev_from_the_start_goes_to_the_last_row") {
        return;
    }
    let mut app = checked(&scratch("prev"));
    assert_eq!(press(&mut app, "Shift+F6"), at("3", CLIP_2));
    assert_eq!(press(&mut app, "Shift+F6"), at("2", CLIP_2));
    assert_eq!(press(&mut app, "Shift+F6"), at("1", CLIP_1));
}

#[test]
fn a_moved_cursor_wins_over_the_last_row() {
    // Catches: the stored row used after the selection moved off its range (from row 0 it would go
    // to row 1); from the cursor at 200000, next is row 2 and prev is row 1.
    if ran_in_silent_child("a_moved_cursor_wins_over_the_last_row") {
        return;
    }
    let mut app = checked(&scratch("cursor"));
    assert_eq!(press(&mut app, "F6"), at("0", CLIP_1));
    assert_eq!(ask(&mut app, "SELECT 200000 200000"), "OK selection=200000-200000");
    assert_eq!(press(&mut app, "F6"), at("2", CLIP_2));
    assert_eq!(ask(&mut app, "SELECT 200000 200000"), "OK selection=200000-200000");
    assert_eq!(press(&mut app, "Shift+F6"), at("1", CLIP_1));
}

#[test]
fn check_click_sets_the_current_row() {
    // Catches: CHECK click not storing its row, so next from a clicked row 2 would go by frame to
    // row 0 (wrap) or stay in place; it must go to row 3.
    if ran_in_silent_child("check_click_sets_the_current_row") {
        return;
    }
    let mut app = checked(&scratch("click"));
    assert!(ask(&mut app, "CHECK click 2").starts_with("OK selection=432000-432480 "));
    assert!(ask(&mut app, "CHECK state").ends_with(" row=2"));
    assert_eq!(press(&mut app, "F6"), at("3", CLIP_2));
}

#[test]
fn before_a_check_there_is_no_row_and_the_keys_refuse() {
    // Catches: a panic or a silent OK with no finished check, and `row=` missing from CHECK state.
    if ran_in_silent_child("before_a_check_there_is_no_row_and_the_keys_refuse") {
        return;
    }
    let mut app = opened(&scratch("none"));
    assert_eq!(ask(&mut app, "CHECK state"), "OK running=0 progress=0 findings=0 stale=0 row=-");
    assert_eq!(ask(&mut app, "KEY F6"), "ERR no findings to go to");
    assert_eq!(ask(&mut app, "KEY Shift+F6"), "ERR no findings to go to");
    assert!(ask(&mut app, "STATE").contains(" selection=0-0 "));
}
