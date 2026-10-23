//! Edit › Mix Paste in the window (f22): the control line, the dialog's settings and the
//! source file. The editing itself is `wavcraft_core::doc::edit::mix_paste`.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use wavcraft_core::doc::edit::{self, ChannelMask, Clip, MixMode, MixOptions};
use wavcraft_core::io::OpenOptions;

/// What a Mix Paste asked for: the settings and where the audio comes from.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub opts: MixOptions,
    /// `None`: the clipboard.
    pub file: Option<PathBuf>,
}

/// Parse `mode=overlap copied=100 existing=50 loops=2 invert=1 fade=5 [file=<path>]`. `copied` and
/// `existing` take one percent, or one per channel (`100,50`); `fade` is the edge crossfade in
/// milliseconds. `file=` takes the rest of the line, so a path may hold spaces; put it last.
pub fn parse(rest: &str) -> Result<Request, String> {
    const USAGE: &str = "MIXPASTE mode=insert|overlap|overwrite|modulate [copied=<pct>[,<pct>]] [existing=<pct>[,<pct>]] [invert=0|1] [fade=<ms>] [loops=<n>] [file=<path>]";
    let (head, file) = match rest.find("file=") {
        Some(i) => (&rest[..i], Some(PathBuf::from(rest[i + 5..].trim()))),
        None => (rest, None),
    };
    let mut opts = MixOptions::new(MixMode::Insert);
    let pcts = |v: &str| -> Result<Vec<f64>, String> {
        v.split(',')
            .map(|p| p.trim().parse::<f64>().ok().filter(|x| x.is_finite() && *x >= 0.0).ok_or_else(|| format!("bad percent {p:?}")))
            .collect()
    };
    for kv in head.split_whitespace() {
        let (k, v) = kv.split_once('=').ok_or_else(|| USAGE.to_string())?;
        match k {
            "mode" => opts.mode = MixMode::parse(v).ok_or_else(|| format!("unknown mode {v:?}"))?,
            "copied" => opts.copied_pct = pcts(v)?,
            "existing" => opts.existing_pct = pcts(v)?,
            "invert" => opts.invert = matches!(v, "1" | "true" | "yes"),
            "fade" => {
                opts.crossfade = Duration::from_secs_f64(v.parse::<f64>().ok().filter(|x| x.is_finite() && *x >= 0.0).ok_or("bad fade")? / 1000.0)
            }
            "loops" => opts.loops = v.parse::<u32>().ok().filter(|n| *n >= 1).ok_or("loops must be 1 or more")?,
            _ => return Err(USAGE.to_string()),
        }
    }
    if matches!(file, Some(ref p) if p.as_os_str().is_empty()) {
        return Err("file= needs a path".to_string());
    }
    Ok(Request { opts, file })
}

/// Open `path` with the normal streamed open into its own session store and take all of it as a
/// clip (pieces only; Mix Paste streams it into the document's store).
pub fn clip_from_file(path: &Path, sessions_root: &Path) -> Result<Clip, String> {
    let opts = OpenOptions { sessions_root: Some(sessions_root.to_path_buf()), ..OpenOptions::default() };
    let state = wavcraft_core::io::open(path, &opts, &mut |_| {}, &AtomicBool::new(false)).map_err(|e| e.to_string())?;
    edit::copy(&state, 0..state.len_frames(), ChannelMask::ALL).map_err(|e| e.to_string())
}

/// The dialog's fields while it is up.
#[derive(Debug, Clone)]
pub struct Dialog {
    pub mode: MixMode,
    pub copied: [String; 2],
    pub existing: [String; 2],
    pub invert: bool,
    pub fade_ms: String,
    pub loops: String,
    pub from_file: bool,
    pub file: String,
}

impl Dialog {
    pub fn new() -> Dialog {
        Dialog {
            mode: MixMode::Overlap,
            copied: ["100".into(), "100".into()],
            existing: ["100".into(), "100".into()],
            invert: false,
            fade_ms: "0".into(),
            loops: "1".into(),
            from_file: false,
            file: String::new(),
        }
    }

    /// The request the fields describe, or what is wrong with them.
    pub fn request(&self) -> Result<Request, String> {
        let pct = |f: &[String; 2]| -> Result<Vec<f64>, String> {
            f.iter()
                .map(|s| s.trim().parse::<f64>().ok().filter(|x| x.is_finite() && *x >= 0.0).ok_or_else(|| format!("bad percent {s:?}")))
                .collect()
        };
        let mut opts = MixOptions::new(self.mode);
        opts.copied_pct = pct(&self.copied)?;
        opts.existing_pct = pct(&self.existing)?;
        opts.invert = self.invert;
        opts.crossfade =
            Duration::from_secs_f64(self.fade_ms.trim().parse::<f64>().ok().filter(|x| x.is_finite() && *x >= 0.0).ok_or("bad crossfade")? / 1000.0);
        opts.loops = self.loops.trim().parse::<u32>().ok().filter(|n| *n >= 1).ok_or("Loop Paste must be 1 or more")?;
        let file = if self.from_file {
            let f = self.file.trim();
            if f.is_empty() {
                return Err("type the path of the file to paste".to_string());
            }
            Some(PathBuf::from(f))
        } else {
            None
        };
        Ok(Request { opts, file })
    }

    /// Draw the fields; `Some(true)` is OK, `Some(false)` Cancel.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<bool> {
        let mut choice = None;
        egui::ComboBox::from_label("Paste type")
            .selected_text(match self.mode {
                MixMode::Insert => "Insert",
                MixMode::Overlap => "Overlap (mix)",
                MixMode::Overwrite => "Overwrite",
                MixMode::Modulate => "Modulate",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.mode, MixMode::Insert, "Insert");
                ui.selectable_value(&mut self.mode, MixMode::Overlap, "Overlap (mix)");
                ui.selectable_value(&mut self.mode, MixMode::Overwrite, "Overwrite");
                ui.selectable_value(&mut self.mode, MixMode::Modulate, "Modulate");
            });
        for (label, f) in
            [("Copied audio volume (%), left / right", &mut self.copied), ("Existing audio volume (%), left / right", &mut self.existing)]
        {
            ui.horizontal(|ui| {
                ui.label(label);
                ui.add(egui::TextEdit::singleline(&mut f[0]).desired_width(48.0));
                ui.add(egui::TextEdit::singleline(&mut f[1]).desired_width(48.0));
            });
        }
        ui.checkbox(&mut self.invert, "Invert the pasted audio");
        ui.horizontal(|ui| {
            ui.label("Crossfade at the edges (ms)");
            ui.add(egui::TextEdit::singleline(&mut self.fade_ms).desired_width(48.0));
            ui.label("Loop Paste count");
            ui.add(egui::TextEdit::singleline(&mut self.loops).desired_width(36.0));
        });
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.from_file, false, "Clipboard");
            ui.radio_value(&mut self.from_file, true, "File");
            ui.add_enabled(self.from_file, egui::TextEdit::singleline(&mut self.file).hint_text("path of an audio file"));
        });
        ui.horizontal(|ui| {
            if ui.button("OK").clicked() {
                choice = Some(true);
            }
            if ui.button("Cancel").clicked() {
                choice = Some(false);
            }
        });
        choice
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_control_line() {
        let r = parse("mode=overlap copied=100 existing=50,25 loops=2 invert=1 fade=5 file=/tmp/a b.wav").unwrap();
        assert_eq!(r.opts.mode, MixMode::Overlap);
        assert_eq!(r.opts.existing_pct, vec![50.0, 25.0]);
        assert_eq!(r.opts.loops, 2);
        assert!(r.opts.invert);
        assert_eq!(r.opts.crossfade, Duration::from_millis(5));
        assert_eq!(r.file, Some(PathBuf::from("/tmp/a b.wav")));
        assert_eq!(parse("mode=insert").unwrap().file, None);
    }

    #[test]
    fn refuses_nonsense() {
        assert!(parse("mode=blend").is_err());
        assert!(parse("loops=0").is_err());
        assert!(parse("copied=-3").is_err());
        assert!(parse("speed=2").is_err());
        assert!(parse("file=").is_err());
    }

    #[test]
    fn the_dialog_makes_a_request() {
        let mut d = Dialog::new();
        d.mode = MixMode::Modulate;
        d.loops = "3".into();
        let r = d.request().unwrap();
        assert_eq!((r.opts.mode, r.opts.loops, r.file), (MixMode::Modulate, 3, None));
        d.from_file = true;
        assert!(d.request().is_err(), "a file source needs a path");
        d.loops = "0".into();
        d.from_file = false;
        assert!(d.request().is_err());
    }
}
