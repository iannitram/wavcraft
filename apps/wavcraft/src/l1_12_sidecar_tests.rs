//! L1.12 (locked): Save and Save As write the transcript sidecar beside the file, and Open loads
//! it when there is one. Driven through the real window's `control_request` on eframe's
//! headless context, the way n3_mcp_window_tests.rs does it.

use super::*;
use std::path::{Path, PathBuf};
use std::time::Instant;

use wavcraft_core::transcript::{self, Transcript, Word};

const RATE: u32 = 48_000;
/// 2.5 s at 48 kHz.
const FRAMES: u32 = 120_000;

/// A 16-bit, 48000 Hz, stereo WAV, `FRAMES` long, of a quiet ramp.
fn write_wav(path: &Path) {
    let data_len = FRAMES * 2 * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..FRAMES {
        let v = (100 * ((i % 100) + 1)) as i16;
        out.extend_from_slice(&v.to_le_bytes());
        out.extend_from_slice(&(-v).to_le_bytes());
    }
    std::fs::write(path, out).expect("write the test WAV");
}

/// The three words every test uses: the first runs 48000-72000.
fn three_words() -> Transcript {
    let word = |text: &str, start: u64, end: u64| Word { text: text.to_string(), start, end };
    Transcript::new(RATE, vec![word("hello", 48_000, 72_000), word("big", 72_000, 84_000), word("world", 84_000, 96_000)])
}

/// A fresh scratch folder for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wavcraft-l1-12-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch folder");
    dir
}

/// A window with no settings or recent-files file, its session folder under `dir`.
fn window(dir: &Path) -> WaveApp {
    let cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    let mut app = WaveApp::new(&cc, None, Some(dir.join("sessions")), false);
    app.settings_path = None;
    app.recent_path = None;
    app
}

/// Run frames until nothing is opening, saving or queued.
fn settle(app: &mut WaveApp) {
    let ctx = app.ctx.clone();
    let deadline = Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.finish_opening();
            app.finish_saving(ui.ctx());
            app.pump_open_queue(ui.ctx());
        });
        output.textures_delta.clear();
        if app.opening.is_none() && app.saving.is_none() && app.open_queue.is_empty() && !app.open_after_save {
            return;
        }
        assert!(Instant::now() < deadline, "the window did not settle");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// One control line, then frames until the window settles.
fn ask(app: &mut WaveApp, line: &str) -> String {
    let ctx = app.ctx.clone();
    let answer = app.control_request(line, &ctx);
    settle(app);
    answer
}

fn open(app: &mut WaveApp, path: &Path) {
    let answer = ask(app, &format!("OPEN {}", path.display()));
    assert_eq!(answer, "OK", "OPEN {}", path.display());
    assert_eq!(ask(app, "LASTERROR"), "OK -", "opening {}", path.display());
}

fn save(app: &mut WaveApp, path: &Path) {
    let answer = ask(app, &format!("SAVE {}", path.display()));
    assert_eq!(answer, "OK", "SAVE {}", path.display());
    assert_eq!(ask(app, "LASTERROR"), "OK -", "saving {}", path.display());
}

#[test]
fn l1_12_open_loads_the_sidecar() {
    // Catches: finish_opening not calling transcript::load_beside (words=0).
    let dir = scratch("open");
    let wav = dir.join("a.wav");
    write_wav(&wav);
    transcript::save_beside(&wav, &three_words()).expect("write the sidecar");
    let mut app = window(&dir);
    open(&mut app, &wav);
    assert_eq!(ask(&mut app, "TRANSCRIPT state"), "OK running=0 words=3 first=48000-72000");
}

#[test]
fn l1_12_loading_the_sidecar_is_not_an_edit() {
    // Catches: the loaded transcript pushed as a History step (steps=2, undo=<name>) or the
    // saved mark taken before the transcript is set (unsaved=1). A plain open has steps=1.
    let dir = scratch("noedit");
    let wav = dir.join("a.wav");
    write_wav(&wav);
    transcript::save_beside(&wav, &three_words()).expect("write the sidecar");
    let mut app = window(&dir);
    open(&mut app, &wav);
    let state = ask(&mut app, "STATE");
    assert_eq!(ask(&mut app, "TRANSCRIPT state"), "OK running=0 words=3 first=48000-72000");
    assert!(state.contains(" steps=1 "), "STATE: {state}");
    assert!(state.contains(" unsaved=0 "), "STATE: {state}");
    assert!(state.contains(" undo=- "), "STATE: {state}");
}

#[test]
fn l1_12_a_file_with_no_sidecar_opens_with_no_words() {
    // Catches: a missing sidecar treated as an error (LASTERROR set, or the file not opened).
    let dir = scratch("none");
    let wav = dir.join("a.wav");
    write_wav(&wav);
    let mut app = window(&dir);
    open(&mut app, &wav);
    assert_eq!(ask(&mut app, "TRANSCRIPT state"), "OK running=0 words=0");
    assert!(ask(&mut app, "STATE").contains("frames=120000 channels=2 rate=48000"));
}

#[test]
fn l1_12_a_bad_sidecar_still_opens_the_audio() {
    // Catches: an unreadable sidecar stopping the open (the audio must open, with no words).
    let dir = scratch("bad");
    let wav = dir.join("a.wav");
    write_wav(&wav);
    std::fs::write(transcript::sidecar_path(&wav), "not a transcript").expect("write a bad sidecar");
    let mut app = window(&dir);
    open(&mut app, &wav);
    assert_eq!(ask(&mut app, "TRANSCRIPT state"), "OK running=0 words=0");
    assert!(ask(&mut app, "STATE").contains("frames=120000 channels=2 rate=48000"));
}

#[test]
fn l1_12_save_as_writes_the_sidecar_and_reopening_reads_it() {
    // Catches: finish_saving not calling transcript::save_beside (no x.wav.transcript.json), the
    // sidecar written beside the old path instead of the new one, and words lost on the way.
    let dir = scratch("saveas");
    let wav = dir.join("a.wav");
    write_wav(&wav);
    transcript::save_beside(&wav, &three_words()).expect("write the sidecar");
    let mut app = window(&dir);
    open(&mut app, &wav);
    let out = dir.join("x.wav");
    save(&mut app, &out);
    let sidecar = dir.join("x.wav.transcript.json");
    assert!(sidecar.is_file(), "no sidecar at {}", sidecar.display());
    assert_eq!(transcript::load_beside(&out).expect("read the sidecar"), Some(three_words()));

    let mut again = window(&dir);
    open(&mut again, &out);
    assert_eq!(ask(&mut again, "TRANSCRIPT state"), "OK running=0 words=3 first=48000-72000");
}

#[test]
fn l1_12_save_in_place_rewrites_the_sidecar() {
    // Catches: only Save As writing the sidecar (a plain Save to the same path must too): the
    // sidecar is deleted before the save, so it exists afterwards only if Save wrote it.
    let dir = scratch("inplace");
    let wav = dir.join("a.wav");
    write_wav(&wav);
    transcript::save_beside(&wav, &three_words()).expect("write the sidecar");
    let mut app = window(&dir);
    open(&mut app, &wav);
    std::fs::remove_file(transcript::sidecar_path(&wav)).expect("remove the sidecar");
    let answer = ask(&mut app, "SAVE");
    assert_eq!(answer, "OK", "SAVE");
    assert_eq!(ask(&mut app, "LASTERROR"), "OK -");
    assert_eq!(transcript::load_beside(&wav).expect("read the sidecar"), Some(three_words()));
}

#[test]
fn l1_12_a_document_with_no_transcript_writes_no_sidecar() {
    // Catches: an empty sidecar written for every save.
    let dir = scratch("nosidecar");
    let wav = dir.join("a.wav");
    write_wav(&wav);
    let mut app = window(&dir);
    open(&mut app, &wav);
    let out = dir.join("y.wav");
    save(&mut app, &out);
    assert!(out.is_file(), "the audio was not saved");
    assert!(!dir.join("y.wav.transcript.json").exists(), "a sidecar was written for a document with no transcript");
}
