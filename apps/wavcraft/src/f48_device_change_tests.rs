//! Locked #997 expectations: under `WAVCRAFT_NO_AUDIO=1` each simulated device change gives the silent
//! sink a new name, the play goes on from the same place, and replacing the audio with nothing stops it.
//! The builder must not edit this file; `player.rs` declares it as a test-only module.
use std::sync::Once;
use std::time::Duration;

use super::*;
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

#[test]
fn the_sink_name_counts_the_simulated_device_changes() {
    assert_eq!(test_sink_name(0), "test sink (no audio)");
    assert_eq!(test_sink_name(1), "test sink (no audio) 1");
    assert_eq!(test_sink_name(2), "test sink (no audio) 2");
    assert_eq!(test_sink_name(12), "test sink (no audio) 12");
}

#[test]
fn the_first_open_keeps_the_plain_sink_name() {
    assert_eq!(silent_player().device, "test sink (no audio)");
}

#[test]
fn each_simulated_device_change_gives_the_sink_a_new_name_and_one_more_reopen() {
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
    let mut p = silent_player();
    assert!(!p.maintain());
    assert_eq!((p.device.as_str(), p.reopens()), ("test sink (no audio)", 0));
}

#[test]
fn a_simulated_device_change_keeps_the_play_going_from_the_same_position() {
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
    let mut p = silent_player();
    p.set_audio(Some(ramp(480_000)));
    assert!(p.start(&Plan::new(0.0, 480_000.0), 0.0));
    std::thread::sleep(Duration::from_millis(40));
    assert!(p.is_playing());
    p.replace_audio(None, &EditSpan { at: 0, removed: 0, inserted: 0 });
    assert!(!p.is_playing());
}
