//! Tests for C5.5: Space, then Space, with Return on Stop on and then off. Both presses are control
//! lines (`KEY Space`) through `control_request`, so the key table is in the path, on the headless
//! window with `WAVCRAFT_NO_AUDIO=1` (the silent sink moves the playhead in real time).
use std::sync::Once;
use std::time::{Duration, Instant};

use super::*;
use crate::player::{OutputChoice, Player};
use wavcraft_core::{AudioBuffer, Document};

/// Where the cursor is when each play starts.
const START: usize = 1_000;
/// Ten seconds at 48 kHz: a play is still running when the second Space comes.
const FRAMES: usize = 480_000;

/// A player on the silent sink (the env var is set once for the test process; no test needs a device).
#[allow(unsafe_code)]
fn silent_player() -> Player {
    static SET: Once = Once::new();
    // SAFETY: set once, before any player of this module reads it; nothing here ever changes it again.
    SET.call_once(|| unsafe { std::env::set_var("WAVCRAFT_NO_AUDIO", "1") });
    let p = Player::new(OutputChoice::default(), ChannelMap::default(), Quality::default());
    assert!(p.has_output() && p.info().test_sink, "the silent sink is the output");
    p
}

/// A folder for one test, removed when it ends. Declare it before the window, so the window (and
/// its session store inside) is dropped first.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("wavcraft-c5-{}-{name}", std::process::id()));
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

/// The window as the program builds it, with its session folder in `scratch`. Its settings file and
/// recent-files list are cut loose at once, so nothing a test does is written to the user's own.
fn a_window(scratch: &Scratch) -> WaveApp {
    let _ = silent_player();
    let cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    let mut app = WaveApp::new(&cc, None, Some(scratch.0.join("sessions")), false);
    assert!(app.sessions.path.starts_with(&scratch.0), "the window's sessions are in the scratch folder, not {}", app.sessions.path.display());
    app.settings_path = None;
    app.recent_path = None;
    app
}

fn ramp(n: usize) -> DocState {
    Document::from_buffer(AudioBuffer { sample_rate: 48_000, channels: vec![(0..n).map(|i| i as f32).collect()] }).expect("a ramp document").state
}

/// One control line, as the channel's reader would run it.
fn ask(app: &mut WaveApp, line: &str) -> String {
    let ctx = app.ctx.clone();
    app.control_request(line, &ctx)
}

/// A `key=value` field of a STATE answer.
fn field(state: &str, key: &str) -> String {
    state.split_whitespace().find_map(|w| w.strip_prefix(&format!("{key}=")).map(str::to_string)).unwrap_or_default()
}

/// The playhead frame in a STATE answer (`pos=`).
fn pos(app: &mut WaveApp) -> usize {
    field(&ask(app, "STATE"), "pos").parse().unwrap_or(usize::MAX)
}

/// Load a ten-second file, let its rebuild finish, and put the cursor at `START` with nothing selected.
fn a_long_file_with_the_cursor_at_start(app: &mut WaveApp) {
    let ctx = app.ctx.clone();
    app.add_document(ramp(FRAMES), &ctx);
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.rebuilding.is_some() && Instant::now() < deadline {
        app.finish_rebuild();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(app.rebuilding.is_none(), "the file finished rebuilding");
    assert_eq!(app.doc.as_ref().map(Document::frames), Some(FRAMES));
    app.selection = Selection::at(START);
}

/// Wait (up to five seconds) until the silent sink has moved the playhead past `START`.
fn wait_until_the_playhead_moves(app: &mut WaveApp) -> usize {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let at = pos(app);
        if at > START || Instant::now() >= deadline {
            return at;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn space_then_space_with_return_on_stop_on_puts_the_playhead_back_at_the_start() {
    let scratch = Scratch::new("return-on");
    let mut app = a_window(&scratch);
    a_long_file_with_the_cursor_at_start(&mut app);
    assert_eq!(field(&ask(&mut app, "STATE"), "return"), "1", "Return on Stop is on by default");

    let first = ask(&mut app, "KEY Space");
    assert!(first.starts_with("OK command=transport.playStop"), "Space is Play/Stop in the key table: {first}");
    assert_eq!(field(&ask(&mut app, "STATE"), "play"), "playing", "the first Space starts playback");
    assert!(wait_until_the_playhead_moves(&mut app) > START, "the playhead moves while playing");

    let second = ask(&mut app, "KEY Space");
    assert!(second.starts_with("OK command=transport.playStop"), "{second}");
    let stopped = ask(&mut app, "STATE");
    assert_eq!(field(&stopped, "play"), "stopped", "the second Space stops playback");
    assert_eq!(field(&stopped, "playing"), "0", "nothing is still playing");
    assert_eq!(field(&stopped, "pos"), START.to_string(), "Return on Stop puts the playhead back at the start frame");

    // It stays there: nothing moves it after the stop.
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(pos(&mut app), START, "the playhead stays at the start frame");
}

#[test]
fn space_then_space_with_return_on_stop_off_leaves_the_playhead_where_it_stopped() {
    let scratch = Scratch::new("return-off");
    let mut app = a_window(&scratch);
    a_long_file_with_the_cursor_at_start(&mut app);

    let toggled = ask(&mut app, "KEY Shift+X");
    assert!(toggled.starts_with("OK"), "Shift+X toggles Return on Stop: {toggled}");
    assert_eq!(field(&ask(&mut app, "STATE"), "return"), "0", "Return on Stop is now off");

    let first = ask(&mut app, "KEY Space");
    assert!(first.starts_with("OK command=transport.playStop"), "{first}");
    assert_eq!(field(&ask(&mut app, "STATE"), "play"), "playing", "the first Space starts playback");
    assert!(wait_until_the_playhead_moves(&mut app) > START, "the playhead moves while playing");

    let second = ask(&mut app, "KEY Space");
    assert!(second.starts_with("OK command=transport.playStop"), "{second}");
    let stopped = ask(&mut app, "STATE");
    assert_eq!(field(&stopped, "play"), "stopped", "the second Space stops playback");
    assert_eq!(field(&stopped, "playing"), "0", "nothing is still playing");
    let stopped_at: usize = field(&stopped, "pos").parse().unwrap_or(0);
    assert!(stopped_at > START && stopped_at < FRAMES, "the playhead stopped part way through the file, at {stopped_at}");

    // It stays where it stopped.
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(pos(&mut app), stopped_at, "the playhead stays where it stopped");
}
