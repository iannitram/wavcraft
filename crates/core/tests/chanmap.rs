//! The channel map and buffer size logic behind the Audio Hardware / Channel Mapping preferences.
use wavcraft_core::chanmap::{ChannelMap, buffer_choices, choose_buffer};
use wavcraft_core::settings::Settings;

#[test]
fn stereo_to_outputs_three_and_four() {
    let mut m = ChannelMap::default();
    assert!(m.is_default());
    m.set(0, 2).unwrap();
    m.set(1, 3).unwrap();
    assert_eq!(m.to_string(), "1:3,2:4");
    assert_eq!(m.sources(2).collect::<Vec<_>>(), vec![0]);
    assert_eq!(m.sources(3).collect::<Vec<_>>(), vec![1]);
    assert_eq!(m.sources(0).count(), 0, "outputs 1 and 2 are silent");
    assert_eq!(m.get(1), Some(3));
}

#[test]
fn setting_a_channel_again_replaces_it_and_two_can_share_a_target() {
    let mut m = ChannelMap::default();
    m.set(0, 2).unwrap();
    m.set(0, 5).unwrap();
    m.set(1, 5).unwrap();
    assert_eq!(m.to_string(), "1:6,2:6");
    assert_eq!(m.sources(5).collect::<Vec<_>>(), vec![0, 1], "summed on one output");
    m.remove(0);
    assert_eq!(m.to_string(), "2:6");
}

#[test]
fn text_round_trips_and_bad_text_is_refused() {
    let m = ChannelMap::parse("2:1, 1:2").unwrap();
    assert_eq!(m.to_string(), "1:2,2:1");
    assert_eq!(ChannelMap::parse(&m.to_string()).unwrap(), m);
    assert!(ChannelMap::parse("").unwrap().is_default());
    for bad in ["0:1", "1:0", "1-2", "a:b", "1:", "1:257", "1:2,1:3"] {
        assert!(ChannelMap::parse(bad).is_err(), "{bad}");
    }
    assert!(ChannelMap::default().set(300, 0).is_err());
}

#[test]
fn the_buffer_is_clamped_into_the_devices_range() {
    assert_eq!(choose_buffer(None, Some((32, 4096))), None, "nothing asked: device default");
    assert_eq!(choose_buffer(Some(0), Some((32, 4096))), None);
    assert_eq!(choose_buffer(Some(512), Some((32, 4096))), Some(512));
    assert_eq!(choose_buffer(Some(8), Some((32, 4096))), Some(32));
    assert_eq!(choose_buffer(Some(99999), Some((32, 4096))), Some(4096));
    assert_eq!(choose_buffer(Some(512), None), None, "unknown range: do not risk the stream");
}

#[test]
fn buffer_choices_are_powers_of_two_inside_the_range() {
    assert_eq!(buffer_choices((32, 1024)), vec![32, 64, 128, 256, 512, 1024]);
    assert_eq!(buffer_choices((15, 100)), vec![15, 16, 32, 64, 100]);
    assert_eq!(buffer_choices((256, 256)), vec![256]);
}

#[test]
fn the_new_preferences_are_settings_keys_and_persist() {
    let mut s = Settings::default();
    assert_eq!(s.get("buffer_frames").unwrap(), "Device default");
    assert_eq!(s.get("audio_host").unwrap(), "System default");
    assert_eq!(s.get("output_map").unwrap(), "");
    s.set("buffer_frames", "256").unwrap();
    s.set("audio_host", "ALSA").unwrap();
    s.set("output_map", "1:3,2:4").unwrap();
    s.set("input_map", "3:1").unwrap();
    assert!(s.set("buffer_frames", "banana").is_err());
    assert!(s.set("buffer_frames", "99999999").is_err());
    assert!(s.set("output_map", "1:x").is_err());
    let back = Settings::parse(&s.to_text());
    assert_eq!(back, s);
    assert_eq!(back.buffer_frames, Some(256));
    assert_eq!(back.output_map.to_string(), "1:3,2:4");
    s.set("buffer_frames", "Device default").unwrap();
    s.set("output_map", "").unwrap();
    assert_eq!((s.buffer_frames, s.output_map.is_default()), (None, true));
}
