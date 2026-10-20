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
    /// Stop by itself after this many recorded frames (C028); paused time does not count.
    max_frames: Option<u64>,
    /// Where in the file (frames) each resume after a pause begins (C029).
    resumes: Vec<u64>,
    /// Dropped frames already reported in the log (C035).
    logged_dropped: u64,
    warn_at: Instant,
}

/// A take waiting for its start time (C028).
struct Armed {
    at: Instant,
    fmt: RecFormat,
    max_secs: Option<u64>,
}

/// What a start asks for beyond the format (C028).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StartOpts {
    /// Stop by itself after this many seconds of recorded audio.
    pub max_secs: Option<u64>,
    /// Wait this many seconds before starting.
    pub delay_secs: Option<u64>,
}

/// Split `max=<s>`, `in=<s>` and `at=<unix s>` off a `RECORD start` argument line; the rest is
/// the format's arguments. `now_unix` turns `at=` into a delay.
pub fn parse_start_args(args: &str, now_unix: u64) -> Result<(RecFormat, StartOpts), String> {
    let (mut opts, mut rest) = (StartOpts::default(), Vec::new());
    for w in args.split_whitespace() {
        let num = |v: &str| v.parse::<u64>().map_err(|_| format!("{w:?}: expected a whole number of seconds"));
        match w.split_once('=') {
            Some(("max", v)) => opts.max_secs = Some(num(v)?).filter(|m| *m > 0).ok_or("max=0: a take needs at least a second")?.into(),
            Some(("in", v)) => opts.delay_secs = Some(num(v)?),
            Some(("at", v)) => opts.delay_secs = Some(num(v)?.saturating_sub(now_unix)),
            _ => rest.push(w),
        }
    }
    Ok((RecFormat::parse_args(&rest.join(" "))?, opts))
}

#[derive(Default)]
pub struct Recording {
    active: Option<Active>,
    armed: Option<Armed>,
    /// The format of the last take started; Quick Record (C031) and the dialog begin from it.
    last_fmt: Option<RecFormat>,
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

    /// The last format used, else the default (48 kHz, mono, 24-bit).
    pub fn last_format(&self) -> RecFormat {
        self.last_fmt.unwrap_or_default()
    }

    pub fn is_armed(&self) -> bool {
        self.armed.is_some()
    }

    /// Wait `delay` and then start (C028). The caller polls [`Recording::due`].
    pub fn arm(&mut self, want: RecFormat, delay: Duration, max_secs: Option<u64>) -> Result<(), String> {
        if self.active.is_some() || self.armed.is_some() {
            return Err("already recording or waiting to".to_string());
        }
        let fmt = want.validate()?;
        log::info!(target: "wavcraft", "recording armed: starts in {} s{}", delay.as_secs(), max_secs.map_or(String::new(), |m| format!(", stops after {m} s")));
        self.armed = Some(Armed { at: Instant::now() + delay, fmt, max_secs });
        Ok(())
    }

    /// Give up waiting for the start time. True when something was armed.
    pub fn disarm(&mut self) -> bool {
        self.armed.take().is_some()
    }

    /// The armed take when its time has come.
    pub fn due(&mut self) -> Option<(RecFormat, Option<u64>)> {
        if self.armed.as_ref().is_some_and(|a| Instant::now() >= a.at) {
            return self.armed.take().map(|a| (a.fmt, a.max_secs));
        }
        None
    }

    /// Pause (true) or resume (false) the take. Returns false when not recording or no change.
    pub fn set_paused(&mut self, pause: bool) -> bool {
        let Some(a) = self.active.as_mut() else { return false };
        if a.ring.is_paused() == pause {
            return false;
        }
        a.ring.set_paused(pause);
        if pause {
            log::info!(target: "wavcraft", "recording paused at {} frames", a.ring.accepted_frames());
        } else {
            let at = a.ring.accepted_frames();
            a.resumes.push(at);
            log::info!(target: "wavcraft", "recording resumed at frame {at} of the file");
        }
        true
    }

    pub fn is_paused(&self) -> bool {
        self.active.as_ref().is_some_and(|a| a.ring.is_paused())
    }

    /// True once a timed take has recorded its maximum.
    pub fn max_reached(&self) -> bool {
        self.active.as_ref().is_some_and(|a| a.max_frames.is_some_and(|m| a.recorder.status().frames >= m))
    }

    /// Start a take in the recordings folder. Returns the new file's path.
    pub fn start(&mut self, want: RecFormat, max_secs: Option<u64>) -> Result<PathBuf, String> {
        if self.active.is_some() {
            return Err("already recording".to_string());
        }
        self.armed = None;
        let want = want.validate()?;
        let dir = record_dir()?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let make = |fmt: RecFormat| record::create_take(&dir, fmt, now).map_err(|e| format!("cannot make a take in {}: {e}", dir.display()));
        let (recorder, source, ring, device) = match FakeSpec::from_env()? {
            Some(spec) => {
                let ring_frames = spec.ring_ms.map_or(want.rate as usize * 2, |ms| (want.rate as usize * ms as usize / 1000).max(1));
                let ring = Ring::new(ring_frames, want.channels);
                let recorder =
                    Recorder::start_stalled(make(want)?, ring.clone(), Duration::from_millis(u64::from(spec.stall_ms))).map_err(|e| e.to_string())?;
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
        let fmt = recorder.format();
        self.last_fmt = Some(fmt);
        log::info!(target: "wavcraft", "recording {} from {device}: {fmt:?}{}", path.display(), max_secs.map_or(String::new(), |m| format!(", stops after {m} s")));
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
            max_frames: max_secs.map(|m| m * u64::from(fmt.rate)),
            resumes: Vec::new(),
            logged_dropped: 0,
            warn_at: Instant::now() - Duration::from_secs(10),
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
        if !a.resumes.is_empty() {
            log::info!(target: "wavcraft", "take {} was paused and resumed at frames {:?}", a.recorder.path().display(), a.resumes);
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
        let dropped = a.ring.dropped_frames();
        if dropped > a.logged_dropped && a.warn_at.elapsed() >= Duration::from_secs(1) {
            a.warn_at = Instant::now();
            let ms = dropped * 1000 / u64::from(a.recorder.format().rate.max(1));
            log::warn!(target: "wavcraft", "recording: {dropped} frames (about {ms} ms) lost so far because the disk fell behind; the take is kept");
            a.logged_dropped = dropped;
        }
        if a.free_at.elapsed() >= Duration::from_secs(1) {
            a.free = free_space(&a.dir);
            a.free_at = Instant::now();
        }
        a.recorder.status().error
    }

    /// Whole seconds until an armed take starts, else -1.
    fn armed_in(&self) -> i64 {
        self.armed.as_ref().map_or(-1, |a| a.at.saturating_duration_since(Instant::now()).as_secs() as i64)
    }

    /// Seconds of recording that fit in the free space, if known.
    fn left_secs(a: &Active) -> Option<u64> {
        a.free.map(|f| f / a.recorder.format().bytes_per_second().max(1))
    }

    /// ` recording=<0|1> …` for the control channel's STATE (before ` undo=`).
    pub fn state_fields(&self) -> String {
        match &self.active {
            None => format!(
                " recording=0 rec_frames=0 rec_bytes=0 rec_dropped=0 rec_left_s=-1 rec_paused=0 rec_resumes=0 rec_max_s=-1 rec_armed={}",
                self.armed_in()
            ),
            Some(a) => {
                let s = a.recorder.status();
                let left = Self::left_secs(a).map_or(-1, |l| l.min(i64::MAX as u64) as i64);
                format!(
                    " recording=1 rec_frames={} rec_bytes={} rec_dropped={} rec_left_s={left} rec_paused={} rec_resumes={} rec_resume_at={} rec_max_s={} rec_armed=-1 rec_rate={} rec_ch={} rec_bits={}",
                    s.frames,
                    s.file_bytes,
                    s.dropped_frames,
                    u8::from(a.ring.is_paused()),
                    a.resumes.len(),
                    if a.resumes.is_empty() { "-".to_string() } else { a.resumes.iter().map(u64::to_string).collect::<Vec<_>>().join(",") },
                    a.max_frames.map_or(-1, |m| (m / u64::from(a.recorder.format().rate.max(1))) as i64),
                    a.recorder.format().rate,
                    a.recorder.format().channels,
                    a.recorder.format().bits
                )
            }
        }
    }

    /// The status bar while recording: (text, whether something is wrong or clipping).
    pub fn status_text(&self) -> Option<(String, bool)> {
        if let Some(w) = &self.armed {
            let secs = w.at.saturating_duration_since(Instant::now()).as_secs();
            return Some((format!("Recording starts in {} (Cancel to give up)", clock(secs)), false));
        }
        let a = self.active.as_ref()?;
        let (s, fmt) = (a.recorder.status(), a.recorder.format());
        let elapsed = s.frames / u64::from(fmt.rate.max(1));
        let left = Self::left_secs(a).map_or_else(|| "?".to_string(), clock);
        let db = if a.peak > 0.0 { format!("{:.1} dB", 20.0 * a.peak.log10()) } else { "-inf dB".to_string() };
        let lost_ms = s.dropped_frames * 1000 / u64::from(fmt.rate.max(1));
        let warn = if s.dropped_frames > 0 { format!("⚠ DROPPED {} frames ({lost_ms} ms) · ", s.dropped_frames) } else { String::new() };
        let state = if a.ring.is_paused() { "❚❚ PAUSED" } else { "● REC" };
        let limit = a.max_frames.map_or(String::new(), |m| format!(" of {}", clock(m / u64::from(fmt.rate.max(1)))));
        let mut text = format!(
            "{warn}{state} {}{limit} · {} · {left} left on disk · peak {db}{} · {} Hz {} ch {}-bit · {}",
            clock(elapsed),
            format_bytes(s.file_bytes),
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
    use super::{StartOpts, clock, parse_start_args};
    use wavcraft_core::record::RecFormat;

    #[test]
    fn start_arguments_split_the_timer_from_the_format() {
        let (f, o) = parse_start_args("", 1000).unwrap();
        assert_eq!((f, o), (RecFormat::default(), StartOpts::default()));
        let (f, o) = parse_start_args("ch=2 max=2 bits=16", 1000).unwrap();
        assert_eq!((f.channels, f.bits, o), (2, 16, StartOpts { max_secs: Some(2), delay_secs: None }));
        assert_eq!(parse_start_args("in=5", 1000).unwrap().1.delay_secs, Some(5));
        assert_eq!(parse_start_args("at=1030", 1000).unwrap().1.delay_secs, Some(30));
        assert_eq!(parse_start_args("at=900", 1000).unwrap().1.delay_secs, Some(0), "a start time already past means now");
        assert!(parse_start_args("max=0", 0).is_err());
        assert!(parse_start_args("max=x", 0).is_err());
        assert!(parse_start_args("rate=5", 0).is_err());
    }

    #[test]
    fn the_clock_reads_minutes_then_hours() {
        assert_eq!(clock(0), "0:00");
        assert_eq!(clock(65), "1:05");
        assert_eq!(clock(3723), "1:02:03");
    }
}
