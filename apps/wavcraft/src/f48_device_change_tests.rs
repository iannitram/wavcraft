//! Locked #997 expectations: under `WAVCRAFT_NO_AUDIO=1` each simulated device change gives the silent
//! sink a new name, the play goes on from the same place, and replacing the audio with nothing stops it.
//! The builder must not edit this file; `player.rs` declares it as a test-only module.
use std::ffi::OsStr;
use std::process::Command;
use std::time::Duration;

use super::*;
use wavcraft_core::{AudioBuffer, Document};

/// Runs the test at `path` (its full path) again in a child process that has `WAVCRAFT_NO_AUDIO=1`
/// before its threads start, and returns true when it did, so the caller returns. Nothing sets the
/// variable in this process: `set_var` is unsafe while other test threads may read the environment.
fn silent_child(path: &str) -> bool {
    if std::env::var_os("WAVCRAFT_NO_AUDIO").as_deref() == Some(OsStr::new("1")) {
        return false;
    }
    let out = Command::new(std::env::current_exe().expect("the test executable"))
        .args(["--exact", path])
        .env("WAVCRAFT_NO_AUDIO", "1")
        .output()
        .expect("run the test with silent output");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && stdout.contains("1 passed"),
        "{path} failed with silent output:\n{stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    true
}

/// A player on the silent sink (no test needs a device). A test that calls this starts with `silent_child`.
fn silent_player() -> Player {
    let p = Player::new(OutputChoice::default(), ChannelMap::default(), Quality::default());
    assert!(p.has_output() && p.info().test_sink, "the silent sink is the output");
    p
}

fn ramp(n: usize) -> DocState {
    Document::from_buffer(AudioBuffer { sample_rate: 48_000, channels: vec![(0..n).map(|i| i as f32).collect()] }).unwrap().state
}

#[test]
fn the_sink_name_counts_the_simulated_device_changes() {
    assert_eq!(test_sink_name(0), "test sink (no audio)");
    assert_eq!(test_sink_name(1), "test sink (no audio) 1");
    assert_eq!(test_sink_name(2), "test sink (no audio) 2");
    assert_eq!(test_sink_name(12), "test sink (no audio) 12");
}

#[test]
fn the_first_open_keeps_the_plain_sink_name() {
    if silent_child("player::f48_device_change_tests::the_first_open_keeps_the_plain_sink_name") {
        return;
    }
    assert_eq!(silent_player().device, "test sink (no audio)");
}

#[test]
fn each_simulated_device_change_gives_the_sink_a_new_name_and_one_more_reopen() {
    if silent_child("player::f48_device_change_tests::each_simulated_device_change_gives_the_sink_a_new_name_and_one_more_reopen") {
        return;
    }
    let mut p = silent_player();
    let first = p.device.clone();
    assert_eq!(p.reopens(), 0);
    p.simulate_device_change();
    assert!(p.maintain(), "the change is noticed");
    assert_eq!((p.device.as_str(), p.reopens()), ("test sink (no audio) 1", 1));
    assert_ne!(p.device, first, "STATE's device= differs from before");
    p.simulate_device_change();
    assert!(p.maintain());
    assert_eq!((p.device.as_str(), p.reopens()), ("test sink (no audio) 2", 2));
    assert!(p.info().test_sink && p.has_output(), "still the silent sink, still an output");
}

#[test]
fn without_a_device_change_maintain_reopens_nothing_and_the_name_stays() {
    if silent_child("player::f48_device_change_tests::without_a_device_change_maintain_reopens_nothing_and_the_name_stays") {
        return;
    }
    let mut p = silent_player();
    assert!(!p.maintain());
    assert_eq!((p.device.as_str(), p.reopens()), ("test sink (no audio)", 0));
}

#[test]
fn a_simulated_device_change_keeps_the_play_going_from_the_same_position() {
    if silent_child("player::f48_device_change_tests::a_simulated_device_change_keeps_the_play_going_from_the_same_position") {
        return;
    }
    let mut p = silent_player();
    p.set_audio(Some(ramp(480_000)));
    assert!(p.start(&Plan::new(0.0, 480_000.0), 100_000.0));
    std::thread::sleep(Duration::from_millis(80));
    let before = p.position();
    assert!(before >= 100_000, "the sink moved it: {before}");
    p.simulate_device_change();
    assert!(p.maintain());
    assert!(p.is_playing(), "the play goes on after the reopen");
    assert!(p.position() >= before, "an old position is not written back (#376): {} < {before}", p.position());
    std::thread::sleep(Duration::from_millis(80));
    assert!(p.position() > before, "it keeps moving: {} vs {before}", p.position());
    p.stop();
}

#[test]
fn replacing_with_nothing_stops_playback() {
    if silent_child("player::f48_device_change_tests::replacing_with_nothing_stops_playback") {
        return;
    }
    let mut p = silent_player();
    p.set_audio(Some(ramp(480_000)));
    assert!(p.start(&Plan::new(0.0, 480_000.0), 0.0));
    std::thread::sleep(Duration::from_millis(40));
    assert!(p.is_playing());
    p.replace_audio(None, &EditSpan { at: 0, removed: 0, inserted: 0 });
    assert!(!p.is_playing());
}
