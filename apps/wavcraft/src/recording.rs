//! Recording a take in the window (roadmap C020-C024): the dialog, the live input (a real device
//! or the fake one chosen by `WAVCRAFT_FAKE_INPUT`), and the status readout. The audio path
//! itself is `wavcraft_core::record`.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use egui::vec2;
use wavcraft_core::DocState;
use wavcraft_core::doc::rec_edit::{RecMode, RecPlan, RecTarget};
use wavcraft_core::record::{self, FakeInput, FakeSpec, MonitorRing, RecFormat, Recorder, Ring, Take};
use wavcraft_core::space::{format_bytes, free_space};

/// Where the audio comes from while a take runs.
enum Source {
    Fake(FakeInput),
    Device(cpal::Stream),
}

/// Software input monitoring for the take that is running (C025).
struct Mon {
    ring: Arc<MonitorRing>,
    rate: u32,
    /// False when the output could not take it (`set_monitor_audible`).
    audible: bool,
    /// With `WAVCRAFT_MONITOR_SINK=null`, a thread that drains the queue instead of the output
    /// device, so a scripted run hears nothing.
    sink: Option<NullSink>,
}

/// Drains a [`MonitorRing`] in real time into nowhere.
struct NullSink {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl NullSink {
    fn spawn(ring: Arc<MonitorRing>) -> NullSink {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let handle = std::thread::spawn(move || {
            let mut buf = vec![0.0f32; ring.capacity_frames() * ring.channels()];
            while !flag.load(Ordering::Relaxed) {
                let n = ring.waiting_frames();
                if n > 0 {
                    ring.pop_into(&mut buf[..n * ring.channels()]);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        NullSink { stop, handle: Some(handle) }
    }
}

impl Drop for NullSink {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// What monitoring did during a take.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct MonStats {
    on: bool,
    played: u64,
    overflow: u64,
    underrun: u64,
}

impl MonStats {
    fn of(m: &Mon) -> MonStats {
        MonStats { on: m.audible, played: m.ring.played_frames(), overflow: m.ring.overflow_frames(), underrun: m.ring.underrun_frames() }
    }

    fn fields(&self) -> String {
        format!(" mon={} mon_played={} mon_overflow={} mon_underrun={}", u8::from(self.on), self.played, self.overflow, self.underrun)
    }
}

struct Active {
    monitor: Option<Mon>,
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

/// A limit on a take's length.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Max {
    Secs(u64),
    Frames(u64),
}

impl std::fmt::Display for Max {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Max::Secs(s) => write!(f, "{s} s"),
            Max::Frames(n) => write!(f, "{n} frames"),
        }
    }
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
    /// `mode=insert|replace|punch`: record into the open document instead of into a new file
    /// (C026, C027).
    pub mode: Option<RecMode>,
    /// `channel=L|R|both`: which channels of the document get the take (C030).
    pub target: Option<RecTarget>,
    /// `preroll=<ms>`: how long a punch-in plays before the selection (default 2000).
    pub preroll_ms: Option<u64>,
    /// `monitor=1|0`: hear the input through the output during this take, whatever the
    /// preference says (C025).
    pub monitor: Option<bool>,
}

/// How long a punch-in plays before the selection, unless `preroll=` says otherwise.
pub const DEFAULT_PREROLL_MS: u64 = 2000;

/// A recording that goes into the open document (C026, C027, C030) instead of a new file.
pub struct IntoDoc {
    pub plan: RecPlan,
    /// The document state the recording started from; the take is only brought in if the
    /// document is still exactly this when it ends.
    pub base: Arc<DocState>,
    pub doc_id: u64,
    punch: Option<Punch>,
}

/// A punch-in waiting for the playhead to reach the selection.
struct Punch {
    /// Where playback began (frames), for the clock used when nothing is playing.
    from: u64,
    began: Instant,
    rate: u32,
    open: bool,
}

/// `1`/`on`/`true`/`yes` and `0`/`off`/`false`/`no`.
pub fn parse_flag(v: &str) -> Option<bool> {
    match v {
        "1" | "on" | "true" | "yes" => Some(true),
        "0" | "off" | "false" | "no" => Some(false),
        _ => None,
    }
}

/// Split `max=<s>`, `in=<s>`, `at=<unix s>`, `mode=`, `channel=` and `preroll=<ms>` off a `RECORD start` argument line; the rest is
/// the format's arguments. `now_unix` turns `at=` into a delay.
pub fn parse_start_args(args: &str, now_unix: u64) -> Result<(RecFormat, StartOpts), String> {
    let (mut opts, mut rest) = (StartOpts::default(), Vec::new());
    for w in args.split_whitespace() {
        let num = |v: &str| v.parse::<u64>().map_err(|_| format!("{w:?}: expected a whole number of seconds"));
        match w.split_once('=') {
            Some(("max", v)) => opts.max_secs = Some(num(v)?).filter(|m| *m > 0).ok_or("max=0: a take needs at least a second")?.into(),
            Some(("in", v)) => opts.delay_secs = Some(num(v)?),
            Some(("mode", v)) => opts.mode = Some(RecMode::parse(v).ok_or_else(|| format!("{w:?}: expected mode=insert, replace or punch"))?),
            Some(("channel", v)) => opts.target = Some(RecTarget::parse(v).ok_or_else(|| format!("{w:?}: expected channel=L, R or both"))?),
            Some(("preroll", v)) => opts.preroll_ms = Some(num(v)?),
            Some(("monitor", v)) => opts.monitor = Some(parse_flag(v).ok_or_else(|| format!("{w:?}: expected monitor=1 or monitor=0"))?),
            Some(("at", v)) => opts.delay_secs = Some(num(v)?.saturating_sub(now_unix)),
            _ => rest.push(w),
        }
    }
    if opts.mode.is_none() && (opts.target.is_some() || opts.preroll_ms.is_some()) {
        return Err("channel= and preroll= go with mode=insert, replace or punch".to_string());
    }
    if opts.mode.is_some() && (opts.delay_secs.is_some() || opts.max_secs.is_some()) {
        return Err("mode= cannot be combined with in=, at= or max=".to_string());
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
    /// The dialog's "Record to": `None` is a new file.
    pub dialog_mode: Option<RecMode>,
    pub dialog_target: RecTarget,
    /// Set while a take is being recorded into the open document.
    into: Option<IntoDoc>,
    /// The preference: hear the input while recording (C025). Off unless the owner turns it on.
    pub monitor_default: bool,
    /// `RECORD start monitor=`: this take only; taken by the next start.
    pub monitor_override: Option<bool>,
    /// What monitoring did during the last take, and the input latency the device reported for it.
    last_monitor: MonStats,
    last_latency_us: Option<u64>,
    /// The record offset applied to the last take put into a document: (ms, frames).
    pub last_offset: Option<(u32, u64)>,
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

    /// Remember that the take now starting goes into the document (`plan`, checked by the caller).
    /// For a punch-in, `from` is where playback begins.
    pub fn set_into(&mut self, plan: RecPlan, base: Arc<DocState>, doc_id: u64, punch_from: Option<u64>, rate: u32) {
        let punch = punch_from.map(|from| Punch { from, began: Instant::now(), rate, open: false });
        self.into = Some(IntoDoc { plan, base, doc_id, punch });
    }

    pub fn going_into_doc(&self) -> Option<&IntoDoc> {
        self.into.as_ref()
    }

    /// The plan of the take that just stopped (or was never started), if it was going into the
    /// document.
    pub fn take_into(&mut self) -> Option<IntoDoc> {
        self.into.take()
    }

    /// A punch-in lets the input through once the playhead (`playing_at` when something plays,
    /// else a clock from where playback began) reaches the selection. Call it every frame.
    pub fn punch_tick(&mut self, playing_at: Option<usize>) {
        let (Some(a), Some(into)) = (self.active.as_ref(), self.into.as_mut()) else { return };
        let Some(p) = into.punch.as_mut().filter(|p| !p.open) else { return };
        let clock = p.from as f64 + p.began.elapsed().as_secs_f64() * f64::from(p.rate);
        let at = playing_at.map_or(clock, |x| x as f64);
        if at >= into.plan.selection.start as f64 {
            p.open = true;
            a.ring.set_paused(false);
            log::info!(target: "wavcraft", "punch-in: recording from the selection's start (playhead at frame {at:.0})");
        }
    }

    /// True while a punch-in is still playing its preroll.
    pub fn punch_waiting(&self) -> bool {
        self.into.as_ref().and_then(|i| i.punch.as_ref()).is_some_and(|p| !p.open)
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
        if self.punch_waiting() {
            return false;
        }
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
        self.start_with(want, max_secs.map(Max::Secs), false)
    }

    /// [`start`](Self::start) with a limit in seconds or frames, and optionally with the input
    /// held back (discarded, not counted as dropped) until [`punch_tick`](Self::punch_tick) lets
    /// it through.
    pub fn start_with(&mut self, want: RecFormat, max: Option<Max>, held: bool) -> Result<PathBuf, String> {
        if self.active.is_some() {
            return Err("already recording".to_string());
        }
        self.armed = None;
        let want = want.validate()?;
        let dir = record_dir()?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let make = |fmt: RecFormat| record::create_take(&dir, fmt, now).map_err(|e| format!("cannot make a take in {}: {e}", dir.display()));
        let want_monitor = self.monitor_override.take().unwrap_or(self.monitor_default);
        let monitor;
        let (recorder, source, ring, device) = match FakeSpec::from_env()? {
            Some(spec) => {
                let ring_frames = spec.ring_ms.map_or(want.rate as usize * 2, |ms| (want.rate as usize * ms as usize / 1000).max(1));
                let mon = want_monitor.then(|| MonitorRing::new(crate::input::monitor_frames(want.rate), want.channels));
                let ring = Ring::with_monitor(ring_frames, want.channels, mon.clone());
                monitor = mon.map(|ring| (ring, want.rate));
                ring.set_paused(held);
                let recorder =
                    Recorder::start_stalled(make(want)?, ring.clone(), Duration::from_millis(u64::from(spec.stall_ms))).map_err(|e| e.to_string())?;
                let fake = FakeInput::spawn(spec, want, ring.clone()).map_err(|e| e.to_string())?;
                (recorder, Source::Fake(fake), ring, format!("fake sine {} Hz", spec.hz))
            }
            None => {
                let opened = crate::input::open(want, want_monitor)?;
                monitor = opened.monitor.clone().map(|m| (m, opened.fmt.rate));
                opened.ring.set_paused(held);
                let recorder = Recorder::start(make(opened.fmt)?, opened.ring.clone()).map_err(|e| e.to_string())?;
                cpal::traits::StreamTrait::play(&opened.stream).map_err(|e| format!("cannot start the input: {e}"))?;
                (recorder, Source::Device(opened.stream), opened.ring, opened.device)
            }
        };
        let path = recorder.path().to_path_buf();
        let fmt = recorder.format();
        self.last_fmt = Some(fmt);
        log::info!(target: "wavcraft", "recording {} from {device}: {fmt:?}{}", path.display(), max.map_or(String::new(), |m| format!(", stops after {m}")));
        let null_sink = std::env::var("WAVCRAFT_MONITOR_SINK").is_ok_and(|v| v == "null");
        let monitor = monitor.map(|(ring, rate)| Mon { sink: null_sink.then(|| NullSink::spawn(ring.clone())), ring, rate, audible: true });
        if monitor.is_some() {
            log::info!(target: "wavcraft", "input monitoring on{}", if null_sink { " (to a null sink)" } else { "" });
        }
        self.active = Some(Active {
            monitor,
            recorder,
            source,
            ring,
            dir,
            device,
            free: None,
            free_at: Instant::now() - Duration::from_secs(10),
            peak: 0.0,
            clipped: false,
            max_frames: max.map(|m| match m {
                Max::Secs(s) => s * u64::from(fmt.rate),
                Max::Frames(f) => f,
            }),
            resumes: Vec::new(),
            logged_dropped: 0,
            warn_at: Instant::now() - Duration::from_secs(10),
        });
        self.refresh();
        Ok(path)
    }

    /// The monitoring queue and its rate when the output should play it (not with the null sink).
    pub fn monitor_for_output(&self) -> Option<(Arc<MonitorRing>, u32)> {
        let m = self.active.as_ref()?.monitor.as_ref()?;
        (m.audible && m.sink.is_none()).then(|| (m.ring.clone(), m.rate))
    }

    /// The output could not take the monitor: say it is not heard.
    pub fn set_monitor_audible(&mut self, audible: bool) {
        if let Some(m) = self.active.as_mut().and_then(|a| a.monitor.as_mut()) {
            m.audible = audible;
        }
    }

    /// The input latency the device reported for the last take, if it did (C010).
    pub fn last_reported_latency_us(&self) -> Option<u64> {
        self.last_latency_us
    }

    /// Stop the input, then finish the take (queue drained, header final, file synced).
    pub fn stop(&mut self) -> Option<Take> {
        let a = self.active.take()?;
        self.last_latency_us = a.ring.reported_latency_us();
        if let Some(m) = &a.monitor {
            self.last_monitor = MonStats::of(m);
            log::info!(target: "wavcraft", "input monitoring: {} frames heard, {} dropped, {} underrun", self.last_monitor.played, self.last_monitor.overflow, self.last_monitor.underrun);
        } else {
            self.last_monitor = MonStats::default();
        }
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
                " recording=0 rec_frames=0 rec_bytes=0 rec_dropped=0 rec_left_s=-1 rec_paused=0 rec_resumes=0 rec_max_s=-1 rec_armed={} rec_mode=none rec_target=- rec_gate=-1{} rec_offset_ms={} rec_offset_frames={}",
                self.armed_in(),
                self.last_monitor.fields(),
                self.last_offset.map_or(-1, |o| i64::from(o.0)),
                self.last_offset.map_or(-1, |o| o.1 as i64)
            ),
            Some(a) => {
                let s = a.recorder.status();
                let left = Self::left_secs(a).map_or(-1, |l| l.min(i64::MAX as u64) as i64);
                format!(
                    " recording=1 rec_frames={} rec_bytes={} rec_dropped={} rec_left_s={left} rec_paused={} rec_resumes={} rec_resume_at={} rec_max_s={} rec_armed=-1 rec_rate={} rec_ch={} rec_bits={} rec_mode={} rec_target={} rec_gate={}{} rec_offset_ms=-1 rec_offset_frames=-1",
                    s.frames,
                    s.file_bytes,
                    s.dropped_frames,
                    u8::from(a.ring.is_paused()),
                    a.resumes.len(),
                    if a.resumes.is_empty() { "-".to_string() } else { a.resumes.iter().map(u64::to_string).collect::<Vec<_>>().join(",") },
                    a.max_frames.map_or(-1, |m| (m / u64::from(a.recorder.format().rate.max(1))) as i64),
                    a.recorder.format().rate,
                    a.recorder.format().channels,
                    a.recorder.format().bits,
                    self.into.as_ref().map_or("none", |i| i.plan.mode.name()),
                    self.into.as_ref().map_or("-", |i| i.plan.target.name()),
                    self.into.as_ref().and_then(|i| i.punch.as_ref()).map_or(-1, |p| i64::from(p.open)),
                    a.monitor.as_ref().map_or(MonStats::default(), MonStats::of).fields()
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
        let state = match self.into.as_ref() {
            Some(i) if self.punch_waiting() => {
                format!("PUNCH-IN: playing the preroll, records from the selection's start ({})", i.plan.target.name())
            }
            Some(i) => format!("{} {}", if i.plan.mode == RecMode::Punch { "● PUNCH-IN" } else { "● REC into the file" }, i.plan.target.name()),
            None if a.ring.is_paused() => "❚❚ PAUSED".to_string(),
            None => "● REC".to_string(),
        };
        let state = if a.monitor.as_ref().is_some_and(|m| m.audible) { format!("{state} (monitoring)") } else { state };
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
    pub fn dialog(&mut self, ctx: &egui::Context, doc_open: bool) -> Option<RecFormat> {
        let fmt = self.dialog.as_mut()?;
        let (mode, target) = (&mut self.dialog_mode, &mut self.dialog_target);
        if !doc_open {
            *mode = None;
        }
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
                if doc_open {
                    ui.label("Record to:");
                    let name = |m: Option<RecMode>| match m {
                        None => "A new file",
                        Some(RecMode::Insert) => "Insert at the cursor",
                        Some(RecMode::Replace) => "Replace the selection (or from the cursor)",
                        Some(RecMode::Punch) => "Punch in over the selection",
                    };
                    egui::ComboBox::from_id_salt("rec-into").selected_text(name(*mode)).show_ui(ui, |ui| {
                        for m in [None, Some(RecMode::Insert), Some(RecMode::Replace), Some(RecMode::Punch)] {
                            ui.selectable_value(mode, m, name(m));
                        }
                    });
                    ui.end_row();
                    if mode.is_some() {
                        ui.label("Channels:");
                        egui::ComboBox::from_id_salt("rec-target")
                            .selected_text(match target {
                                RecTarget::Both => "Both",
                                RecTarget::Left => "Left only",
                                RecTarget::Right => "Right only",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(target, RecTarget::Both, "Both");
                                ui.selectable_value(target, RecTarget::Left, "Left only");
                                ui.selectable_value(target, RecTarget::Right, "Right only");
                            });
                        ui.end_row();
                    }
                }
            });
            if mode.is_some() {
                ui.label("The sample rate and channels follow the file; the take is saved first, then brought in as one undo step.");
            }
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
    use super::{Duration, Instant, NullSink, StartOpts, clock, parse_start_args};
    use wavcraft_core::doc::rec_edit::{RecMode, RecTarget};
    use wavcraft_core::record::{MonitorRing, RecFormat};

    #[test]
    fn start_arguments_split_the_timer_from_the_format() {
        let (f, o) = parse_start_args("", 1000).unwrap();
        assert_eq!((f, o), (RecFormat::default(), StartOpts::default()));
        let (f, o) = parse_start_args("ch=2 max=2 bits=16", 1000).unwrap();
        assert_eq!((f.channels, f.bits, o), (2, 16, StartOpts { max_secs: Some(2), ..StartOpts::default() }));
        assert_eq!(parse_start_args("in=5", 1000).unwrap().1.delay_secs, Some(5));
        assert_eq!(parse_start_args("at=1030", 1000).unwrap().1.delay_secs, Some(30));
        assert_eq!(parse_start_args("at=900", 1000).unwrap().1.delay_secs, Some(0), "a start time already past means now");
        assert!(parse_start_args("max=0", 0).is_err());
        assert!(parse_start_args("max=x", 0).is_err());
        assert!(parse_start_args("rate=5", 0).is_err());
    }

    #[test]
    fn start_arguments_take_the_record_into_file_options() {
        let o = parse_start_args("mode=insert channel=L", 0).unwrap().1;
        assert_eq!((o.mode, o.target), (Some(RecMode::Insert), Some(RecTarget::Left)));
        let o = parse_start_args("mode=punch preroll=500 channel=both bits=16", 0).unwrap();
        assert_eq!((o.1.mode, o.1.target, o.1.preroll_ms, o.0.bits), (Some(RecMode::Punch), Some(RecTarget::Both), Some(500), 16));
        assert!(parse_start_args("mode=splice", 0).is_err());
        assert!(parse_start_args("mode=insert channel=C", 0).is_err());
        assert!(parse_start_args("channel=L", 0).is_err(), "a channel without a mode means nothing");
        assert!(parse_start_args("mode=insert max=2", 0).is_err());
        assert!(parse_start_args("mode=replace in=3", 0).is_err());
    }

    #[test]
    fn start_arguments_take_monitor_on_or_off() {
        assert_eq!(parse_start_args("mode=punch monitor=1", 0).unwrap().1.monitor, Some(true));
        assert_eq!(parse_start_args("monitor=0 rate=44100", 0).unwrap().1.monitor, Some(false));
        assert_eq!(parse_start_args("", 0).unwrap().1.monitor, None);
        assert!(parse_start_args("monitor=maybe", 0).is_err());
    }

    #[test]
    fn the_null_sink_drains_the_monitor_queue_and_stops() {
        let ring = MonitorRing::new(4800, 1);
        let sink = NullSink::spawn(ring.clone());
        ring.push(&[0.5; 1000]);
        let t = Instant::now();
        while ring.played_frames() < 1000 && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(ring.played_frames(), 1000);
        assert_eq!((ring.overflow_frames(), ring.underrun_frames()), (0, 0));
        drop(sink);
    }

    #[test]
    fn the_clock_reads_minutes_then_hours() {
        assert_eq!(clock(0), "0:00");
        assert_eq!(clock(65), "1:05");
        assert_eq!(clock(3723), "1:02:03");
    }
}
