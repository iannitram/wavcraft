//! The MP3 export options and the check that a rate, channel count and options triple can be
//! encoded as MP3 (#1311). Pure: no file, no encoder.

use wavcraft_core::mp3_options::{MP3_BITRATES, MP3_DEFAULT_BITRATE, Mp3Options, mp3_check};

#[test]
fn the_v1_choices() {
    assert_eq!(MP3_BITRATES, [32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320]);
    assert_eq!(MP3_DEFAULT_BITRATE, 192);

    let stereo_192 = Mp3Options { bitrate_kbps: 192, mono: false };
    let mono_128 = Mp3Options { bitrate_kbps: 128, mono: true };

    // Stereo in, stereo out; mono forced on a stereo source gives one channel.
    assert_eq!(mp3_check(44100, 2, stereo_192), Ok(2));
    assert_eq!(mp3_check(48000, 2, mono_128), Ok(1));

    // Rate outside 32000, 44100 and 48000 Hz.
    let err = mp3_check(96000, 2, stereo_192).unwrap_err();
    assert!(err.contains("32000, 44100 or 48000"), "{err}");

    // More than two channels.
    let err = mp3_check(44100, 6, stereo_192).unwrap_err();
    assert!(err.contains("mono or stereo"), "{err}");

    // A bitrate that is not in the list.
    let off_list = Mp3Options { bitrate_kbps: 100, mono: false };
    assert!(mp3_check(44100, 2, off_list).is_err());
}
