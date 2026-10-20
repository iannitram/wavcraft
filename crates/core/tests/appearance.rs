//! f94: the Appearance page's colour settings: colour parsing, the colour keys, named presets
//! (save, load, delete, round trip through the settings file).

use wavcraft_core::appearance::{format_hex, parse_hex};
use wavcraft_core::settings::{KEYS, Settings};

#[test]
fn colour_parsing() {
    assert_eq!(parse_hex("#ff8000"), Some([255, 128, 0]));
    assert_eq!(parse_hex("#FF8000"), Some([255, 128, 0]));
    assert_eq!(parse_hex("  #0a0b0c "), Some([10, 11, 12]));
    assert_eq!(parse_hex("ff8000"), None, "the # is required");
    assert_eq!(parse_hex("#fff"), None);
    assert_eq!(parse_hex("#ff80000"), None);
    assert_eq!(parse_hex("#gg0000"), None);
    assert_eq!(parse_hex("#+f0000"), None);
    assert_eq!(parse_hex(""), None);
    assert_eq!(format_hex([255, 128, 0]), "#ff8000");
    assert_eq!(parse_hex(&format_hex([1, 2, 3])), Some([1, 2, 3]));
}

#[test]
fn colour_keys_default_set_and_refuse() {
    let mut s = Settings::default();
    for item in ["waveform", "selection", "cursor", "playhead", "background"] {
        let key = format!("color.{item}");
        assert!(KEYS.contains(&key.as_str()), "{key} is listed in KEYS");
        assert_eq!(s.get(&key).unwrap(), "default");
        s.set(&key, "#102030").unwrap();
        assert_eq!(s.get(&key).unwrap(), "#102030");
        assert!(s.set(&key, "red").is_err());
        assert!(s.set(&key, "#12").is_err());
        assert_eq!(s.get(&key).unwrap(), "#102030", "a bad value changes nothing");
        s.set(&key, "default").unwrap();
        assert_eq!(s.get(&key).unwrap(), "default");
    }
    assert_eq!(s.get("spec_palette").unwrap(), "ember");
    s.set("spec_palette", "glacier").unwrap();
    assert!(s.set("spec_palette", "rainbow").is_err());
    assert_eq!(s.get("ui_contrast").unwrap(), "1");
    s.set("ui_contrast", "1.25").unwrap();
    assert!(s.set("ui_contrast", "3").is_err());
    assert!(s.set("color.nothing", "#000000").is_err());
}

#[test]
fn presets_save_load_delete() {
    let mut s = Settings::default();
    s.set("color.waveform", "#112233").unwrap();
    s.set("color.cursor", "#445566").unwrap();
    s.set("spec_palette", "forest").unwrap();
    s.set("ui_brightness", "0.8").unwrap();
    s.set("ui_contrast", "1.2").unwrap();
    s.save_preset("Night Owl").unwrap();
    assert_eq!(s.preset_names(), vec!["Night Owl".to_string()]);

    s.set("color.waveform", "default").unwrap();
    s.set("spec_palette", "ember").unwrap();
    s.set("ui_brightness", "1").unwrap();
    s.set("ui_contrast", "1").unwrap();
    s.load_preset("Night Owl").unwrap();
    assert_eq!(s.get("color.waveform").unwrap(), "#112233");
    assert_eq!(s.get("color.cursor").unwrap(), "#445566");
    assert_eq!(s.get("color.selection").unwrap(), "default");
    assert_eq!(s.get("spec_palette").unwrap(), "forest");
    assert_eq!(s.get("ui_brightness").unwrap(), "0.8");
    assert_eq!(s.get("ui_contrast").unwrap(), "1.2");

    // Saving under the same name replaces it; names are matched exactly.
    s.set("color.waveform", "#abcdef").unwrap();
    s.save_preset("Night Owl").unwrap();
    assert_eq!(s.preset_names().len(), 1);
    assert!(s.load_preset("night owl").is_err());
    assert!(s.delete_preset("nothing").is_err());
    s.delete_preset("Night Owl").unwrap();
    assert!(s.preset_names().is_empty());
    assert!(s.load_preset("Night Owl").is_err());
}

#[test]
fn preset_names_are_checked() {
    let mut s = Settings::default();
    for bad in ["", "   ", "a;b", "a=b", "a\nb", &"x".repeat(33)] {
        assert!(s.save_preset(bad).is_err(), "{bad:?} refused");
    }
    assert!(s.save_preset("Soft - dusk_2").is_ok());
}

#[test]
fn presets_and_colours_round_trip_through_the_file() {
    let dir = std::env::temp_dir().join(format!("wavcraft-f94-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("settings.txt");
    let mut s = Settings::default();
    s.set("color.background", "#010203").unwrap();
    s.set("color.selection", "#0a0b0c").unwrap();
    s.set("spec_palette", "mono").unwrap();
    s.set("ui_contrast", "0.75").unwrap();
    s.save_preset("First").unwrap();
    s.set("color.playhead", "#ffffff").unwrap();
    s.save_preset("Second one").unwrap();
    s.save(&file).unwrap();

    let back = Settings::load(&file);
    for k in ["color.background", "color.selection", "color.playhead", "color.waveform", "spec_palette", "ui_contrast"] {
        assert_eq!(back.get(k).unwrap(), s.get(k).unwrap(), "{k}");
    }
    assert_eq!(back.preset_names(), vec!["First".to_string(), "Second one".to_string()]);
    let mut back = back;
    back.load_preset("First").unwrap();
    assert_eq!(back.get("color.playhead").unwrap(), "default");
    assert_eq!(back.get("color.background").unwrap(), "#010203");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_damaged_preset_line_is_ignored() {
    let s = Settings::parse(
        "appearance_preset = broken;nonsense\nappearance_preset = Ok;color.cursor=#000001;spec_palette=dusk\nappearance_preset = Bad;color.cursor=zzz\n",
    );
    assert_eq!(s.preset_names(), vec!["Ok".to_string()]);
}
