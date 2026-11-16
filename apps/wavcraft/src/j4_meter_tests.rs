//! Tests for J4.3: the Levels bars show the take while recording, else the input, else the player.
use super::*;

#[test]
fn the_bars_show_the_take_while_recording() {
    // A take's peak wins, even when the input is closed and a file plays.
    assert_eq!(bars_source(Some(0.5), true, false, [0.1, 0.1], [0.0, 0.0]), ([0.5, 0.5], true));
    assert_eq!(bars_source(Some(0.5), false, true, [0.1, 0.1], [0.3, 0.3]), ([0.5, 0.5], true));
    // No take, input open and nothing plays: the input's peaks, and the bars show the input.
    assert_eq!(bars_source(None, true, false, [0.1, 0.2], [0.3, 0.4]), ([0.1, 0.2], true));
    // No take, but a file plays: the player's peaks, and the bars show the output.
    assert_eq!(bars_source(None, true, true, [0.1, 0.2], [0.3, 0.4]), ([0.3, 0.4], false));
    // No take, input closed: the player's peaks, and the bars show the output.
    assert_eq!(bars_source(None, false, false, [0.1, 0.2], [0.3, 0.4]), ([0.3, 0.4], false));
}
