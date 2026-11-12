//! Roadmap A3.2: the MP3 choices (bitrates, default, mono) and the check that a file can be saved
//! as MP3. Core only: no encoder runs and no file is written.

use wavcraft_core::mp3_options::{MP3_BITRATES, MP3_DEFAULT_BITRATE, Mp3Options, mp3_check};

fn stereo(bitrate_kbps: u32) -> Mp3Options {
    Mp3Options { bitrate_kbps, mono: false }
}

fn mono(bitrate_kbps: u32) -> Mp3Options {
    Mp3Options { bitrate_kbps, mono: true }
}

#[test]
fn the_v1_choices() {
    assert_eq!(MP3_BITRATES, [32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320]);
    assert_eq!(MP3_DEFAULT_BITRATE, 192);
    assert_eq!(Mp3Options::default(), stereo(192));

    assert_eq!(mp3_check(44_100, 2, stereo(192)), Ok(2));
    assert_eq!(mp3_check(48_000, 2, mono(128)), Ok(1));

    let rate = mp3_check(96_000, 2, stereo(192)).unwrap_err();
    assert!(rate.contains("32000, 44100 or 48000"), "{rate}");

    let channels = mp3_check(44_100, 6, stereo(192)).unwrap_err();
    assert!(channels.contains("mono or stereo"), "{channels}");

    assert!(mp3_check(44_100, 2, stereo(100)).is_err());
}
