//! Locked #1307 expectations (J2.1): while the player is muted for a take, a device change does not
//! reopen the output. The change is kept, and the output reopens after the take. Under `WAVCRAFT_NO_AUDIO=1`
//! the silent sink stands in for the device. `player.rs` declares this file as a test-only module.
use std::ffi::OsStr;
use std::process::Command;

use super::*;

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

#[test]
fn a_device_change_during_a_take_waits_for_the_take_to_end() {
    if silent_child("player::j2_no_output_tests::a_device_change_during_a_take_waits_for_the_take_to_end") {
        return;
    }
    let mut p = silent_player();
    p.set_muted(true);
    p.simulate_device_change();
    assert!(!p.maintain(), "muted: nothing reopens during the take");
    assert_eq!(p.reopens(), 0);
    assert_eq!(p.device, "test sink (no audio)", "the name stays during the take");
    assert!(!p.maintain(), "still muted: the kept change does nothing yet");
    p.set_muted(false);
    assert!(p.maintain(), "after the take the kept change reopens the output");
    assert_eq!(p.reopens(), 1);
    assert_eq!(p.device, "test sink (no audio) 1");
    assert!(!p.maintain(), "the reopen cleared the flag");
}
