//! Tests for #377: Play pressed right after an edit must play the edited audio, not the buffer the
//! player held before the asynchronous rebuild finished.
//!
//! Hook-up, both in apps/wavcraft/src/app.rs: at the end of the file add
//! `#[cfg(test)] #[path = "issue377_tests.rs"] mod issue377_tests;`.
use std::sync::Once;
use std::time::{Duration, Instant};

use super::*;
use crate::player::{OutputChoice, Player};
use wavcraft_core::transport::Plan;
use wavcraft_core::{AudioBuffer, Document};

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

fn ramp(n: usize) -> DocState {
    Document::from_buffer(AudioBuffer { sample_rate: 48_000, channels: vec![(0..n).map(|i| i as f32).collect()] }).unwrap().state
}

/// The issue's case at the player: the old 100-frame buffer is held, the edit shortens it to 60
/// frames, and the play that starts on the edited audio plays those 60 frames and then stops.
#[test]
fn a_play_on_the_edited_audio_plays_the_sixty_edited_frames() {
    let mut p = silent_player();
    p.set_audio(Some(ramp(100)));
    let span = EditSpan::between(&ramp(100), &ramp(60));
    p.replace_audio(Some(ramp(60)), &span);
    assert!(p.start(&Plan::new(0.0, 60.0), 0.0), "the play starts");
    let deadline = Instant::now() + Duration::from_secs(5);
    while p.is_playing() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!p.is_playing(), "the 60 frames run out and the play ends");
    assert_eq!(p.position(), 60, "the play went over the 60 edited frames");
}

/// The app defers a Play pressed while the rebuild is pending (it must not start on the old audio)
/// and `finish_rebuild` starts it once the edited audio is in place.
#[test]
fn a_play_pressed_during_a_rebuild_waits_for_the_edited_audio() {
    let app = include_str!("app.rs");
    let start = app.find("    fn start_play(&mut self) {").expect("start_play");
    let body = &app[start..start + 400];
    assert!(body.contains("if self.rebuilding.is_some()"), "start_play does not wait for the rebuild:\n{body}");
    assert!(body.contains("self.play_after_edit = true;"), "start_play does not remember the play:\n{body}");

    let finish = app.find("    fn finish_rebuild(&mut self) {").expect("finish_rebuild");
    let fin = &app[finish..finish + 1600];
    assert!(fin.contains("std::mem::take(&mut self.play_after_edit)"), "finish_rebuild does not take the pending play");
    assert!(fin.contains("self.start_play();"), "finish_rebuild does not start the pending play");
}
