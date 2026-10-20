//! Recording a take in the window (roadmap C020-C024): the dialog, the live input (a real device
//! or the fake one chosen by `WAVCRAFT_FAKE_INPUT`), and the status readout. The audio path
//! itself is `wavcraft_core::record`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use egui::vec2;
use wavcraft_core::record::{self, FakeInput, FakeSpec, RecFormat, Recorder, Ring, Take};
use wavcraft_core::space::{format_bytes, free_space};

/// Where the audio comes from while a take runs.
enum Source {
    Fake(FakeInput),
    Device(cpal::Stream),
}

struct Active {
    recorder: Recorder,
    source: Source,
    ring: Arc<Ring>,
    dir: PathBuf,
    device: String,
    free: Option<u64>,
    free_at: Instant,
    /// The newest peak, decaying; and whether any sample reached full scale.
    peak: f32,
    clipped: bool,
}

#[derive(Default)]
pub struct Recording {
    active: Option<Active>,
    /// File › New Recording…: the format being chosen while the dialog is up.
    pub dialog: Option<RecFormat>,
}

/// The folder takes go in: `WAVCRAFT_RECORD_DIR`, else `WavCraft Recordings` in the system's
/// audio (or documents, or home) folder.
pub fn record_dir() -> Result<PathBuf, String> {
    if let Some(d) = std::env::var_os("WAVCRAFT_RECORD_DIR").filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(d));
    }
    dirs::audio_dir()
        .or_else(dirs::document_dir)
        .or_else(dirs::home_dir)
        .map(|d| d.join("WavCraft Recordings"))
        .ok_or_else(|| "no folder for recordings: set WAVCRAFT_RECORD_DIR".to_string())
}

/// `1:02:03` or `4:05` for a number of seconds.
fn clock(secs: u64) -> String {
    if secs >= 3600 { format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60) } else { format!("{}:{:02}", secs / 60, secs % 60) }
}

impl Recording {
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// Start a take in the recordings folder. Returns the new file's path.
    pub fn start(&mut self, want: RecFormat) -> Result<PathBuf, String> {
        if self.active.is_some() {
            return Err("already recording".to_string());
        }
        let want = want.validate()?;
        let dir = record_dir()?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let make = |fmt: RecFormat| record::create_take(&dir, fmt, now).map_err(|e| format!("cannot make a take in {}: {e}", dir.display()));
        let (recorder, source, ring, device) = match FakeSpec::from_env()? {
            Some(spec) => {
                let ring = Ring::new(want.rate as usize * 2, want.channels);
                let recorder = Recorder::start(make(want)?, ring.clone()).map_err(|e| e.to_string())?;
                let fake = FakeInput::spawn(spec, want, ring.clone()).map_err(|e| e.to_string())?;
                (recorder, Source::Fake(fake), ring, format!("fake sine {} Hz", spec.hz))
            }
            None => {
                let opened = crate::input::open(want)?;
                let recorder = Recorder::start(make(opened.fmt)?, opened.ring.clone()).map_err(|e| e.to_string())?;
                cpal::traits::StreamTrait::play(&opened.stream).map_err(|e| format!("cannot start the input: {e}"))?;
                (recorder, Source::Device(opened.stream), opened.ring, opened.device)
            }
        };
        let path = recorder.path().to_path_buf();
        log::info!(target: "wavcraft", "recording {} from {device}: {:?}", path.display(), recorder.format());
        self.active = Some(Active {
            recorder,
            source,
            ring,
            dir,
            device,
            free: None,
            free_at: Instant::now() - Duration::from_secs(10),
            peak: 0.0,
            clipped: false,
        });
        self.refresh();
        Ok(path)
    }

    /// Stop the input, then finish the take (queue drained, header final, file synced).
    pub fn stop(&mut self) -> Option<Take> {
        let a = self.active.take()?;
        match a.source {
            Source::Fake(f) => f.stop(),
            Source::Device(s) => drop(s),
        }
        let take = a.recorder.stop();
        log::info!(target: "wavcraft", "take {} finished: {} frames, {} dropped, error {:?}", take.path.display(), take.frames, take.dropped_frames, take.error);
        Some(take)
    }

    /// Once per frame: the meter, the free space (once a second). Returns the writer's error
    /// when it stopped early (the caller then stops the take).
    pub fn refresh(&mut self) -> Option<String> {
        let a = self.active.as_mut()?;
        let p = a.ring.take_peak();
        a.peak = p.max(a.peak * 0.9);
        a.clipped |= p >= 0.999;
        if a.free_at.elapsed() >= Duration::from_secs(1) {
            a.free = free_space(&a.dir);
            a.free_at = Instant::now();
        }
        a.recorder.status().error
    }

    /// Seconds of recording that fit in the free space, if known.
    fn left_secs(a: &Active) -> Option<u64> {
        a.free.map(|f| f / a.recorder.format().bytes_per_second().max(1))
    }

    /// ` recording=<0|1> …` for the control channel's STATE (before ` undo=`).
    pub fn state_fields(&self) -> String {
        match &self.active {
            None => " recording=0 rec_frames=0 rec_bytes=0 rec_dropped=0 rec_left_s=-1".to_string(),
            Some(a) => {
                let s = a.recorder.status();
                let left = Self::left_secs(a).map_or(-1, |l| l.min(i64::MAX as u64) as i64);
                format!(
                    " recording=1 rec_frames={} rec_bytes={} rec_dropped={} rec_left_s={left} rec_rate={} rec_ch={} rec_bits={}",
                    s.frames,
                    s.file_bytes,
                    s.dropped_frames,
                    a.recorder.format().rate,
                    a.recorder.format().channels,
                    a.recorder.format().bits
                )
            }
        }
    }

    /// The status bar while recording: (text, whether something is wrong or clipping).
    pub fn status_text(&self) -> Option<(String, bool)> {
        let a = self.active.as_ref()?;
        let (s, fmt) = (a.recorder.status(), a.recorder.format());
        let elapsed = s.frames / u64::from(fmt.rate.max(1));
        let left = Self::left_secs(a).map_or_else(|| "?".to_string(), clock);
        let db = if a.peak > 0.0 { format!("{:.1} dB", 20.0 * a.peak.log10()) } else { "-inf dB".to_string() };
        let mut text = format!(
            "● REC {} · {} · {left} left on disk · {} dropped · peak {db}{} · {} Hz {} ch {}-bit · {}",
            clock(elapsed),
            format_bytes(s.file_bytes),
            s.dropped_frames,
            if a.clipped { " CLIP" } else { "" },
            fmt.rate,
            fmt.channels,
            fmt.bits,
            a.device
        );
        if let Some(e) = s.error {
            text = format!("{text} · {e}");
        }
        Some((text, a.clipped || s.dropped_frames > 0))
    }

    /// The new-recording dialog. Returns the format when Start is clicked.
    pub fn dialog(&mut self, ctx: &egui::Context) -> Option<RecFormat> {
        let fmt = self.dialog.as_mut()?;
        let mut choice = None;
        egui::Window::new("New Recording").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            egui::Grid::new("rec-format").num_columns(2).show(ui, |ui| {
                ui.label("Sample rate:");
                egui::ComboBox::from_id_salt("rec-rate").selected_text(format!("{} Hz", fmt.rate)).show_ui(ui, |ui| {
                    for r in RecFormat::RATES {
                        ui.selectable_value(&mut fmt.rate, r, format!("{r} Hz"));
                    }
                });
                ui.end_row();
                ui.label("Channels:");
                egui::ComboBox::from_id_salt("rec-ch").selected_text(if fmt.channels == 1 { "Mono" } else { "Stereo" }).show_ui(ui, |ui| {
                    ui.selectable_value(&mut fmt.channels, 1, "Mono");
                    ui.selectable_value(&mut fmt.channels, 2, "Stereo");
                });
                ui.end_row();
                ui.label("Bit depth:");
                egui::ComboBox::from_id_salt("rec-bits")
                    .selected_text(if fmt.bits == 32 { "32-bit float".to_string() } else { format!("{}-bit", fmt.bits) })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut fmt.bits, 16, "16-bit");
                        ui.selectable_value(&mut fmt.bits, 24, "24-bit");
                        ui.selectable_value(&mut fmt.bits, 32, "32-bit float");
                    });
                ui.end_row();
            });
            ui.label(match record_dir() {
                Ok(d) => format!("The take is saved in {}", d.display()),
                Err(e) => e,
            });
            ui.horizontal(|ui| {
                if ui.button("Record").clicked() {
                    choice = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(false);
                }
            });
        });
        match choice {
            Some(true) => self.dialog.take(),
            Some(false) => {
                self.dialog = None;
                None
            }
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::clock;

    #[test]
    fn the_clock_reads_minutes_then_hours() {
        assert_eq!(clock(0), "0:00");
        assert_eq!(clock(65), "1:05");
        assert_eq!(clock(3723), "1:02:03");
    }
}
