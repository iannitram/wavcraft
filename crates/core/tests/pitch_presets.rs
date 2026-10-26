//! B111: the pitch preset table for the Pitch Shifter.

use std::f64::consts::TAU;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use wavcraft_core::doc::edit::{ChannelMask, Crossfades};
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::effects::pitch_presets::{PitchPreset, PresetError, check_shift, lookup, names, params_for, presets};
use wavcraft_core::effects::{self, validate};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 44_100;

const ORDER: [&str; 15] = [
    "Octave Up",
    "Octave Down",
    "Two Octaves Up",
    "Two Octaves Down",
    "Fifth Up",
    "Fifth Down",
    "Fourth Up",
    "Fourth Down",
    "Major Third Up",
    "Minor Third Down",
    "Semitone Up",
    "Semitone Down",
    "Quarter Tone Up",
    "Quarter Tone Down",
    "A440 to A432",
];

fn out_of_range(param: &'static str, value: f64, min: f64, max: f64) -> Result<(), PresetError> {
    Err(PresetError::OutOfRange { param, value, min, max })
}

#[test]
fn the_table_lists_fifteen_presets_in_menu_order() {
    assert_eq!(presets().len(), 15);
    let listed: Vec<&str> = presets().iter().map(|p| p.name).collect();
    assert_eq!(listed, ORDER);
    assert_eq!(names(), ORDER);
}

#[test]
fn the_shifts_are_the_named_intervals() {
    let want = [
        ("Octave Up", 12.0, 0.0),
        ("Octave Down", -12.0, 0.0),
        ("Two Octaves Up", 24.0, 0.0),
        ("Two Octaves Down", -24.0, 0.0),
        ("Fifth Up", 7.0, 0.0),
        ("Fifth Down", -7.0, 0.0),
        ("Fourth Up", 5.0, 0.0),
        ("Fourth Down", -5.0, 0.0),
        ("Major Third Up", 4.0, 0.0),
        ("Minor Third Down", -3.0, 0.0),
        ("Semitone Up", 1.0, 0.0),
        ("Semitone Down", -1.0, 0.0),
        ("Quarter Tone Up", 0.0, 50.0),
        ("Quarter Tone Down", 0.0, -50.0),
        ("A440 to A432", 0.0, -31.77),
    ];
    for (name, semitones, cents) in want {
        let p = lookup(name).unwrap();
        assert_eq!((p.name, p.semitones, p.cents), (name, semitones, cents));
    }
}

#[test]
fn names_are_unique_ignoring_case() {
    let mut lower: Vec<String> = names().iter().map(|n| n.to_ascii_lowercase()).collect();
    lower.sort();
    lower.dedup();
    assert_eq!(lower.len(), presets().len());
}

#[test]
fn lookup_ignores_case_and_surrounding_spaces() {
    for text in ["Octave Up", "octave up", "OCTAVE UP", "  Octave Up  ", "oCtAvE uP"] {
        let p = lookup(text).unwrap();
        assert_eq!((p.name, p.semitones), ("Octave Up", 12.0), "{text:?}");
    }
    assert_eq!(lookup("fifth down").unwrap().semitones, -7.0);
}

#[test]
fn an_unknown_name_is_reported_as_typed() {
    assert_eq!(lookup("Octave Sideways"), Err(PresetError::UnknownName("Octave Sideways".to_string())));
    assert_eq!(lookup(""), Err(PresetError::UnknownName(String::new())));
    assert_eq!(lookup("Octave  Up"), Err(PresetError::UnknownName("Octave  Up".to_string())));
    assert_eq!(lookup("  Nope "), Err(PresetError::UnknownName("  Nope ".to_string())));
    assert_eq!(params_for("nope"), Err(PresetError::UnknownName("nope".to_string())));
}

#[test]
fn a_preset_converts_to_semitones_then_cents() {
    assert_eq!(lookup("Octave Up").unwrap().to_params(), Ok(vec![12.0, 0.0]));
    assert_eq!(lookup("Quarter Tone Down").unwrap().to_params(), Ok(vec![0.0, -50.0]));
    assert_eq!(params_for("fifth down"), Ok(vec![-7.0, 0.0]));
    assert_eq!(params_for("A440 to A432"), Ok(vec![0.0, -31.77]));
}

#[test]
fn every_preset_passes_the_pitch_shifters_own_validation() {
    let effect = effects::find("pitch_shift").unwrap();
    for p in presets() {
        let params = p.to_params().unwrap();
        assert_eq!(params.len(), 2, "{}", p.name);
        assert_eq!(validate(effect, &params), Ok(()), "{}", p.name);
        assert_eq!(check_shift(p.semitones, p.cents), Ok(()), "{}", p.name);
    }
}

#[test]
fn the_limits_are_accepted() {
    assert_eq!(check_shift(24.0, 100.0), Ok(()));
    assert_eq!(check_shift(-24.0, -100.0), Ok(()));
    assert_eq!(check_shift(0.0, 0.0), Ok(()));
}

#[test]
fn out_of_range_semitones_are_refused() {
    assert_eq!(check_shift(30.0, 0.0), out_of_range("semitones", 30.0, -24.0, 24.0));
    assert_eq!(check_shift(24.5, 0.0), out_of_range("semitones", 24.5, -24.0, 24.0));
    assert_eq!(check_shift(-24.5, 0.0), out_of_range("semitones", -24.5, -24.0, 24.0));
}

#[test]
fn out_of_range_cents_are_refused() {
    assert_eq!(check_shift(0.0, 200.0), out_of_range("cents", 200.0, -100.0, 100.0));
    assert_eq!(check_shift(0.0, 100.5), out_of_range("cents", 100.5, -100.0, 100.0));
    assert_eq!(check_shift(0.0, -100.5), out_of_range("cents", -100.5, -100.0, 100.0));
}

#[test]
fn semitones_are_checked_before_cents() {
    assert_eq!(check_shift(30.0, 200.0), out_of_range("semitones", 30.0, -24.0, 24.0));
}

#[test]
fn not_finite_values_are_refused() {
    assert!(matches!(check_shift(f64::NAN, 0.0), Err(PresetError::OutOfRange { param: "semitones", .. })));
    assert!(matches!(check_shift(f64::INFINITY, 0.0), Err(PresetError::OutOfRange { param: "semitones", .. })));
    assert!(matches!(check_shift(0.0, f64::NAN), Err(PresetError::OutOfRange { param: "cents", .. })));
    assert!(matches!(check_shift(0.0, f64::NEG_INFINITY), Err(PresetError::OutOfRange { param: "cents", .. })));
}

#[test]
fn converting_a_preset_checks_its_values() {
    let high = PitchPreset { name: "high", semitones: 25.0, cents: 0.0 };
    assert_eq!(high.to_params(), Err(PresetError::OutOfRange { param: "semitones", value: 25.0, min: -24.0, max: 24.0 }));
    let sharp = PitchPreset { name: "sharp", semitones: 0.0, cents: 101.0 };
    assert_eq!(sharp.to_params(), Err(PresetError::OutOfRange { param: "cents", value: 101.0, min: -100.0, max: 100.0 }));
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn the_ratio_is_two_to_the_total_semitones_over_twelve() {
    assert!(close(lookup("Octave Up").unwrap().ratio(), 2.0));
    assert!(close(lookup("Octave Down").unwrap().ratio(), 0.5));
    assert!(close(lookup("Two Octaves Up").unwrap().ratio(), 4.0));
    assert!(close(lookup("Fifth Up").unwrap().ratio(), 1.4983070768766815));
    assert!(close(lookup("Quarter Tone Up").unwrap().ratio(), 1.029302236643492));
    assert!(close(lookup("A440 to A432").unwrap().ratio(), 0.9818162840271993));
    assert!(close(PitchPreset { name: "x", semitones: 11.0, cents: 100.0 }.ratio(), 2.0));
    assert!(close(PitchPreset { name: "y", semitones: -3.0, cents: 0.0 }.ratio(), 0.8408964152537145));
}

#[test]
fn the_messages_name_what_went_wrong() {
    let unknown = PresetError::UnknownName("nope".to_string()).to_string();
    assert!(unknown.contains("nope"), "{unknown}");
    let range = check_shift(30.0, 0.0).unwrap_err().to_string();
    assert!(range.contains("semitones") && range.contains("30") && range.contains("-24 to 24"), "{range}");
    let cents = check_shift(0.0, -150.0).unwrap_err().to_string();
    assert!(cents.contains("cents") && cents.contains("-150") && cents.contains("-100 to 100"), "{cents}");
}

// End to end: a preset's parameters drive the real Pitch Shifter.

struct TempDir(PathBuf);
impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-b111-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run_preset(tag: &str, preset: &str, input: &[f32]) -> Vec<f32> {
    let tmp = TempDir::new(tag);
    let s = Store::create_with(&tmp.0, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let chan = Channel::from_pieces(input.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()));
    let d = DocState::new(s, vec![chan], RATE, SourceFormat::synthetic(RATE, 1)).unwrap();
    let e = effects::find("pitch_shift").unwrap();
    let params = params_for(preset).unwrap();
    let cancel = AtomicBool::new(false);
    let edit = effects::apply(e, &params, &d, 0..0, ChannelMask::ALL, Crossfades::NONE, &cancel).unwrap();
    let mut out = vec![0.0f32; input.len()];
    assert_eq!(edit.state.read(0, 0, &mut out).unwrap(), input.len());
    out
}

/// Strongest frequency in `lo..=hi` Hz over `x[from..to]`, 1 Hz steps (Goertzel).
fn peak_hz(x: &[f32], from: usize, to: usize, lo: u32, hi: u32) -> u32 {
    let seg = &x[from..to];
    let mut best = (0.0f64, 0u32);
    for f in lo..=hi {
        let w = TAU * f as f64 / RATE as f64;
        let (mut re, mut im) = (0.0, 0.0);
        for (i, s) in seg.iter().enumerate() {
            re += *s as f64 * (w * i as f64).cos();
            im += *s as f64 * (w * i as f64).sin();
        }
        let m = re * re + im * im;
        if m > best.0 {
            best = (m, f);
        }
    }
    best.1
}

#[test]
fn octave_up_and_down_move_a_440_hz_tone_by_an_octave() {
    let input: Vec<f32> = (0..3 * RATE as usize).map(|i| (0.5 * (TAU * 440.0 * i as f64 / RATE as f64).sin()) as f32).collect();
    let up = run_preset("up", "octave up", &input);
    let f = peak_hz(&up, RATE as usize / 2, 5 * RATE as usize / 2, 800, 960);
    assert!((878..=882).contains(&f), "measured {f} Hz");
    let down = run_preset("down", "Octave Down", &input);
    let f = peak_hz(&down, RATE as usize / 2, 5 * RATE as usize / 2, 150, 300);
    assert!((218..=222).contains(&f), "measured {f} Hz");
}
