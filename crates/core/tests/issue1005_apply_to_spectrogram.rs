//! Locked tests for #1005: `Settings::apply_to_spectrogram` sets the window once, its doc
//! comment says so, and the window preference reaches the analysis parameters.

use wavcraft_core::SpectrogramParams;
use wavcraft_core::settings::{Settings, SpecWindow};

/// The source of `Settings::apply_to_spectrogram`: its doc comment lines, then its body lines.
fn apply_to_spectrogram_source() -> (String, String) {
    let src = include_str!("../src/settings.rs");
    let lines: Vec<&str> = src.lines().collect();
    let at = lines.iter().position(|l| l.contains("pub fn apply_to_spectrogram")).expect("apply_to_spectrogram is in settings.rs");
    let mut start = at;
    while start > 0 && lines[start - 1].trim_start().starts_with("///") {
        start -= 1;
    }
    let end = (at..lines.len()).find(|&i| lines[i] == "    }").expect("the function ends");
    (lines[start..at].join("\n"), lines[at..=end].join("\n"))
}

fn applied(window: &str) -> SpectrogramParams {
    let mut s = Settings::default();
    s.set("spec_window", window).unwrap();
    let mut p = SpectrogramParams::default();
    s.apply_to_spectrogram(&mut p);
    p
}

#[test]
fn the_window_assignment_appears_once_in_apply_to_spectrogram() {
    let (_, body) = apply_to_spectrogram_source();
    assert_eq!(body.matches("params.window = self.spec_window;").count(), 1, "{body}");
}

#[test]
fn the_doc_comment_says_the_fft_length_window_and_lowest_level_are_put_into_params() {
    let (doc, _) = apply_to_spectrogram_source();
    assert!(doc.contains("window"), "the doc comment does not mention the window: {doc}");
    assert!(!doc.contains("stored only"), "the doc comment still says the window is stored only: {doc}");
    assert!(!doc.contains("uses Hann"), "the doc comment still says the analysis uses Hann: {doc}");
}

#[test]
fn a_blackman_window_preference_reaches_the_params() {
    assert_eq!(applied("blackman").window, SpecWindow::Blackman);
}

#[test]
fn every_window_preference_reaches_the_params_unchanged() {
    for &w in SpecWindow::ALL {
        assert_eq!(applied(w.name()).window, w, "{}", w.name());
    }
}

#[test]
fn the_fft_length_and_floor_are_still_put_into_the_params() {
    let mut s = Settings::default();
    s.set("spec_resolution", "512").unwrap();
    s.set("spec_db_range", "80").unwrap();
    s.set("spec_window", "blackman").unwrap();
    let mut p = SpectrogramParams::default();
    s.apply_to_spectrogram(&mut p);
    assert_eq!((p.fft_size, p.floor_db, p.ceil_db, p.window), (512, -80.0, 0.0, SpecWindow::Blackman));
}
