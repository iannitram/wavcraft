//! Playback of the open document through the chosen output device (the system default unless the
//! preferences name one).
//!
//! The audio callback never blocks or allocates. It reads the play state with `try_lock` only
//! (silence if another thread holds it), then plays from a **read-ahead ring** of about 2.7
//! seconds of the file's frames that a separate thread keeps filled from the document store
//! ([`DocState::read`], chunk by chunk). If the ring runs dry the callback plays silence for that
//! buffer and counts an underrun, instead of waiting for the disk. The file's rate is converted to
//! the device's by windowed-sinc interpolation (f74, `wavcraft_core::resample`, `fast` or `high`
//! per the `playback_quality` setting) read straight out of the ring; the kernel table is built
//! off the audio thread and handed over under a `try_lock` (until it is there, and for a ratio
//! too wide for the ring's margin, the callback interpolates linearly). The playhead stays in
//! file frames. At most [`RING_CHANNELS`] channels of the file are buffered and played.
//!
//! The ring holds the file **in play order**, not file order (f23, f36): the read-ahead thread walks
//! the play's [`Plan`] (loop, skipped part, forward or backward) and writes the frames it plays one
//! after the other, so the callback only ever reads ahead of one counter and a loop wrap, a skip
//! or a backward play costs it nothing. The play speed is the callback's step through that
//! counter (linear interpolation; the sinc kernel is for normal speed). A change of direction, of
//! the plan or of the place (a seek) starts a new play under a new generation, so the guard that
//! keeps a stale callback from overwriting a restart (#376) covers them too.
//!
//! `WAVCRAFT_NO_AUDIO=1` swaps the sound card for a silent real-time sink (stereo, 48 kHz unless
//! `WAVCRAFT_NO_AUDIO_RATE`), so the whole path can be driven without a device.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::Sample as _;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use wavcraft_core::DocState;
use wavcraft_core::levels::LevelTap;
use wavcraft_core::rack::{MAX_BLOCK, RackConfig, RackProcessor};
use wavcraft_core::chanmap::{ChannelMap, choose_buffer};
use wavcraft_core::resample::{Quality, Resampler};
use wavcraft_core::transport::{self, Plan};

/// Frames the read-ahead ring holds (a power of two).
const RING_FRAMES: usize = 1 << 17;
/// Channels the ring buffers.
const RING_CHANNELS: usize = 8;
/// Frames read from the store at a time.
const FILL_CHUNK: usize = 8192;
/// Frames that must be in the ring before a play starts sounding.
const PRIME_FRAMES: u64 = 16_384;
/// Frames of the ring kept clear behind the play position, so a write never touches one the
/// callback is still reading (the sinc kernel looks that far back; a resampler wider than this
/// is not used, see [`rebuild_resampler`]).
const RING_MARGIN: u64 = 1024;

#[derive(Copy, Clone)]
struct Playback {
    /// Bumped by every play/stop; a render pass started under an older value must not write back.
    generation: u64,
    /// Play position, counted in frames of play order from `from` (equal to the file frame while
    /// nothing loops, skips or runs backward; see [`Playback::file_position`]).
    pos: f64,
    /// Where playback finishes in the same count (clamped to the document by the read-ahead
    /// thread; infinite in a loop).
    end: f64,
    playing: bool,
    /// The part of the file being played and the file frame the play began at.
    plan: Plan,
    from: f64,
    /// Playing backward.
    rev: bool,
    /// The speed (> 0; the direction is `rev`).
    speed: f64,
    /// A scrub grain: its ends are faded.
    grain: bool,
}

impl Default for Playback {
    fn default() -> Playback {
        Playback { generation: 0, pos: 0.0, end: 0.0, playing: false, plan: Plan::new(0.0, 0.0), from: 0.0, rev: false, speed: 1.0, grain: false }
    }
}

impl Playback {
    /// The file frame the play is at (where it finished, once it has).
    fn file_position(&self) -> f64 {
        let mut wraps = 0;
        self.plan
            .position_after(self.from, (self.pos - self.from).max(0.0), self.rev, &mut wraps)
            .unwrap_or(if self.rev { self.plan.start } else { self.plan.end })
    }

    /// How many times a loop has gone round since the play began.
    fn wraps(&self) -> u32 {
        let mut wraps = 0;
        let _ = self.plan.position_after(self.from, (self.pos - self.from).max(0.0), self.rev, &mut wraps);
        wraps
    }
}

struct Shared {
    /// The ring: frame `f` of channel `c` lives at `(f % RING_FRAMES) * RING_CHANNELS + c`, as
    /// `f32` bits.
    ring: Vec<AtomicU32>,
    /// File frames `[.., filled_to)` have been written to the ring (the newest `RING_FRAMES` of
    /// them are still there).
    filled_to: AtomicU64,
    /// The document's sample rate and channel count (0: no document).
    rate: AtomicU32,
    channels: AtomicU32,
    /// The play generation the ring was primed for: the callback sounds only when it equals the
    /// current generation (a play sounds once its ring is primed, never before).
    ready_generation: AtomicU64,
    /// Callbacks that found the ring dry (for the log and the tests).
    underruns: AtomicU64,
    /// The first file frame of the current play: the ring holds an older play's audio before it,
    /// which the resampler's kernel must read as silence.
    origin: AtomicU64,
    /// The output stream's sample rate (0: none yet) and the conversion quality (0 fast, 1 high).
    device_rate: AtomicU32,
    quality: AtomicU8,
    /// The sinc resampler for (document rate -> device rate), built by the read-ahead thread; the
    /// callback `try_lock`s it for the length of a buffer, and the builder swaps it under the lock
    /// and drops the old one outside it, so the callback never frees it.
    resampler: Mutex<Option<Arc<Resampler>>>,
    /// The play state (lock held only briefly; the callback uses `try_lock`).
    state: Mutex<Playback>,
    /// Set by the stream's error callback (the device went away, for example).
    failed: AtomicBool,
    /// The peaks of what is sent, for the Levels panel (lock-free; see `wavcraft_core::levels`).
    levels: LevelTap,
    /// A new Effects Rack for the callback to pick up (f31): the UI builds it (all memory
    /// allocated) and parks it here; the callback swaps it with the one it runs, using
    /// `try_lock` only, so it never waits, allocates or frees. The old one is left here for the
    /// UI to drop.
    rack_mailbox: Mutex<Option<Box<RackProcessor>>>,
    rack_new: AtomicBool,
    /// Document channel -> device output channel (empty: the automatic layout). Changing it needs
    /// no new stream; the callback `try_lock`s it like the rest.
    map: Mutex<ChannelMap>,
}

impl Shared {
    fn new() -> Shared {
        Shared {
            ring: (0..RING_FRAMES * RING_CHANNELS).map(|_| AtomicU32::new(0)).collect(),
            filled_to: AtomicU64::new(0),
            rate: AtomicU32::new(0),
            channels: AtomicU32::new(0),
            ready_generation: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
            origin: AtomicU64::new(0),
            device_rate: AtomicU32::new(0),
            quality: AtomicU8::new(quality_code(Quality::default())),
            resampler: Mutex::new(None),
            state: Mutex::new(Playback::default()),
            failed: AtomicBool::new(false),
            levels: LevelTap::default(),
            rack_mailbox: Mutex::new(None),
            rack_new: AtomicBool::new(false),
            map: Mutex::new(ChannelMap::default()),
        }
    }

    fn quality(&self) -> Quality {
        if self.quality.load(Ordering::Relaxed) == 0 { Quality::Fast } else { Quality::High }
    }

    fn slot(frame: u64, ch: usize) -> usize {
        (frame as usize % RING_FRAMES) * RING_CHANNELS + ch
    }

    /// The play state, copied out.
    fn snapshot(&self) -> Playback {
        *self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[cfg(test)]
    fn start(&self, from: f64, to: f64) -> u64 {
        self.start_with(from, to, || {})
    }

    #[cfg(test)]
    fn start_with<F: FnOnce()>(&self, from: f64, to: f64, between: F) -> u64 {
        self.start_plan_with(Plan::new(from, to), from, false, 1.0, false, between)
    }

    /// Start a play of `plan` from file frame `from` (backward when `rev`, at `speed` times normal,
    /// `grain` fading its ends) and return its generation. The ring must be primed for it before
    /// it sounds.
    fn start_plan(&self, plan: Plan, from: f64, rev: bool, speed: f64, grain: bool) -> u64 {
        self.start_plan_with(plan, from, rev, speed, grain, || {})
    }

    fn start_plan_with<F: FnOnce()>(&self, plan: Plan, from: f64, rev: bool, speed: f64, grain: bool, between: F) -> u64 {
        let from = from.floor().max(0.0);
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.generation += 1;
        // Test seam: keep the state lock held so a callback cannot see a half-applied start.
        between();
        let frames = plan.frames_from(from, rev);
        state.plan = plan;
        state.from = from;
        state.rev = rev;
        state.speed = speed;
        state.grain = grain;
        state.pos = from;
        state.end = from + frames;
        state.playing = frames > 0.0;
        state.generation
    }

    /// Change the speed (and the direction it is shown with) without starting a new play.
    fn set_speed(&self, speed: f64, rev: bool) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.speed = speed;
        if !state.playing {
            state.rev = rev;
        }
    }

    fn halt(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.generation += 1;
        state.playing = false;
    }
}

fn quality_code(q: Quality) -> u8 {
    u8::from(q == Quality::High)
}

/// Build the resampler for the current document rate, device rate and quality, and park it for the
/// callback. None when no conversion is needed or the kernel is wider than the ring's margin.
fn rebuild_resampler(shared: &Shared) {
    let (doc, device) = (shared.rate.load(Ordering::Acquire), shared.device_rate.load(Ordering::Acquire));
    let new = (doc != 0 && device != 0 && doc != device)
        .then(|| Resampler::new(doc, device, shared.quality()))
        .filter(|r| r.half_width() + 1.0 < RING_MARGIN as f64)
        .map(Arc::new);
    let old = std::mem::replace(&mut *shared.resampler.lock().unwrap_or_else(|e| e.into_inner()), new);
    drop(old);
}

/// What the read-ahead thread is sent.
enum Command {
    /// The device rate or the conversion quality changed: rebuild the resampler.
    Rebuild,
    /// The document to play (`None`: nothing).
    Document(Option<DocState>),
    /// Prime the ring for the play begun as `generation` (its plan is in the play state).
    Play { generation: u64 },
}

/// What the preferences ask of the output: the device, the audio host and the buffer size.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct OutputChoice {
    /// The device's name (`None`: the host's default).
    pub device: Option<String>,
    /// The audio host's name (`None`: the system's default).
    pub host: Option<String>,
    /// The buffer size in frames (`None`: the device's default).
    pub buffer: Option<u32>,
}

/// The output stream, the read-ahead thread and the play state shared with the callback.
pub struct Player {
    shared: Arc<Shared>,
    commands: Sender<Command>,
    stream: Option<cpal::Stream>,
    /// The silent test sink standing in for a device (`WAVCRAFT_NO_AUDIO=1`).
    sink: Option<Sink>,
    /// Why there is no sound, if the device could not be opened.
    pub error: Option<String>,
    /// The output device's name, for the status bar.
    pub device: String,
    /// The audio host in use (CoreAudio, WASAPI, ALSA...).
    pub host: String,
    /// The buffer size in use in frames (`None`: the device's default).
    pub buffer: Option<u32>,
    /// The buffer sizes the device supports, when it says.
    pub buffer_range: Option<(u32, u32)>,
    /// The output stream's channel count and sample rate (what a rack is built for).
    pub out_channels: usize,
    out_rate: u32,
    /// What the preferences asked for.
    wanted: OutputChoice,
    /// The play Pause stopped, to carry on from with Resume (f23).
    held: Mutex<Option<Held>>,
    /// Loop passes of the plays this one replaced (a seek or a speed change starts a new play).
    wraps_base: AtomicU32,
}

/// A paused play: what Resume starts again.
#[derive(Debug, Clone, Copy)]
struct Held {
    plan: Plan,
    pos: f64,
    rev: bool,
    speed: f64,
    grain: bool,
}

/// The audio hosts this build offers (more than one on Windows with ASIO compiled in, or Linux
/// with JACK; one elsewhere).
pub fn host_names() -> Vec<String> {
    cpal::available_hosts().iter().map(|h| h.name().to_string()).collect()
}

/// The host named `wanted`, or the system default (with a warning when the name is not there).
fn host_for(wanted: Option<&str>) -> cpal::Host {
    if let Some(w) = wanted {
        match cpal::available_hosts().into_iter().find(|h| h.name() == w).map(cpal::host_from_id) {
            Some(Ok(host)) => return host,
            _ => log::warn!("audio host {w:?} is not available; using the system default"),
        }
    }
    cpal::default_host()
}

impl Player {
    /// Open the output device named `wanted` (the system default for `None`, or when that name is
    /// not there). Without a device the player still works, silently.
    pub fn new(wanted: OutputChoice, map: ChannelMap, quality: Quality) -> Player {
        let shared = Arc::new(Shared::new());
        shared.quality.store(quality_code(quality), Ordering::Relaxed);
        *shared.map.lock().unwrap_or_else(|e| e.into_inner()) = map;
        let (commands, inbox) = channel();
        let fill_shared = Arc::clone(&shared);
        if let Err(e) = std::thread::Builder::new().name("wavcraft-readahead".to_string()).spawn(move || read_ahead(&fill_shared, &inbox)) {
            log::warn!(target: "wavcraft", "could not start the playback read-ahead thread: {e}");
        }
        let mut player = Player {
            shared,
            commands,
            stream: None,
            sink: None,
            error: None,
            device: String::new(),
            host: String::new(),
            buffer: None,
            buffer_range: None,
            wanted,
            out_channels: 2,
            out_rate: 48_000,
            held: Mutex::new(None),
            wraps_base: AtomicU32::new(0),
        };
        player.reopen();
        player
    }

    /// Play through the device named `wanted` from now on (`None`: the system default): stops
    /// playback and reopens the output.
    pub fn set_output(&mut self, wanted: OutputChoice) {
        self.stop();
        self.wanted = wanted;
        self.reopen();
    }

    /// Route the document's channels to the device's from now on (takes effect at once).
    pub fn set_map(&self, map: ChannelMap) {
        if let Ok(mut slot) = self.shared.map.lock() {
            *slot = map;
        }
    }

    /// (Re)open the wanted output device, replacing any stream there was.
    fn reopen(&mut self) {
        self.stream = None;
        self.sink = None;
        self.shared.failed.store(false, Ordering::Relaxed);
        if no_audio() {
            let sink = Sink::start(&self.shared);
            self.device = "test sink (no audio)".to_string();
            self.host = String::new();
            self.buffer = None;
            self.buffer_range = None;
            self.out_channels = SINK_CHANNELS;
            self.out_rate = sink.rate;
            self.sink = Some(sink);
            self.error = None;
        } else {
            match open_stream(&self.shared, &self.wanted) {
                Ok(o) => {
                    self.stream = Some(o.stream);
                    self.device = o.device;
                    self.host = o.host;
                    self.buffer = o.buffer;
                    self.buffer_range = o.range;
                    self.out_channels = o.channels;
                    self.out_rate = o.rate;
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            }
        }
        if self.has_output() {
            self.shared.device_rate.store(self.out_rate, Ordering::Release);
            let _ = self.commands.send(Command::Rebuild);
        }
    }

    /// Whether an output (a device or the silent test sink) is moving the position.
    pub fn has_output(&self) -> bool {
        self.stream.is_some() || self.sink.is_some()
    }

    /// Use conversion quality `quality` from now on (the resampler is rebuilt off the audio thread).
    pub fn set_quality(&self, quality: Quality) {
        self.shared.quality.store(quality_code(quality), Ordering::Relaxed);
        let _ = self.commands.send(Command::Rebuild);
    }

    /// The conversion state, for the status bar and the control channel's `PLAYBACK`.
    pub fn info(&self) -> PlaybackInfo {
        let doc_rate = self.shared.rate.load(Ordering::Acquire);
        PlaybackInfo {
            device_rate: self.out_rate,
            doc_rate,
            resampling: self.has_output() && doc_rate != 0 && doc_rate != self.out_rate,
            quality: self.shared.quality(),
            playing: self.is_playing(),
            position: self.position(),
            underruns: self.underruns(),
            test_sink: self.sink.is_some(),
        }
    }

    /// Set the Effects Rack the player runs on what it plays (f31). An inactive rack (power off,
    /// nothing switched on) is removed. The swap into the audio callback is lock-free; calling
    /// this while playing is fine. Takes effect for the next audio buffer.
    pub fn set_rack(&self, config: &RackConfig) -> Result<(), String> {
        let new = if config.is_active() { Some(Box::new(RackProcessor::new(config, self.out_channels.min(2), self.out_rate)?)) } else { None };
        if let Ok(mut slot) = self.shared.rack_mailbox.lock() {
            *slot = new;
            self.shared.rack_new.store(true, Ordering::Release);
        }
        Ok(())
    }

    /// Whether the output stream has reported an error since it was opened.
    pub fn failed(&self) -> bool {
        self.shared.failed.load(Ordering::Relaxed)
    }

    /// Hand over the document to play (stops playback). Nothing is copied: the read-ahead thread
    /// reads it from the store as it plays.
    pub fn set_audio(&self, doc: Option<DocState>) {
        self.stop();
        let _ = self.commands.send(Command::Document(doc));
    }

    /// Play `[from, to)` in file frames. Without a working output (none at start-up, or one that
    /// has since failed) the device is tried again first; if there still is none, nothing starts,
    /// so the playhead does not sit there "playing" with no callback to move it.
    pub fn play(&mut self, from: usize, to: usize) {
        self.start(&Plan::new(from as f64, to as f64), from as f64);
    }

    /// Play `plan` from file frame `from` at normal speed. Returns whether it started (it does
    /// not when there is no output, nothing in the plan, or `from` is past its end).
    pub fn start(&mut self, plan: &Plan, from: f64) -> bool {
        self.wraps_base.store(0, Ordering::Relaxed);
        self.begin_play(*plan, from, false, 1.0, false)
    }

    /// Play a short scrub grain: `plan` from `from` at `speed` (negative runs backward), with
    /// faded ends. Replaces whatever was playing.
    pub fn grain(&mut self, plan: &Plan, from: f64, speed: f64) -> bool {
        self.wraps_base.store(0, Ordering::Relaxed);
        self.begin_play(*plan, from, speed < 0.0, speed.abs().max(MIN_SPEED), true)
    }

    fn begin_play(&mut self, plan: Plan, from: f64, rev: bool, speed: f64, grain: bool) -> bool {
        *self.held.lock().unwrap_or_else(|e| e.into_inner()) = None;
        if !self.has_output() || self.failed() {
            self.reopen();
        }
        if !self.has_output() {
            return false;
        }
        let generation = self.shared.start_plan(plan, from, rev, speed, grain);
        let started = self.shared.snapshot().playing;
        if started {
            let _ = self.commands.send(Command::Play { generation });
        }
        started
    }

    /// Start a new play from where the current one is (a change that the ring cannot follow: a
    /// seek, another plan, the other direction).
    fn restart(&mut self, plan: Plan, from: f64, rev: bool, speed: f64, grain: bool) -> bool {
        let now = self.shared.snapshot();
        self.wraps_base.fetch_add(now.wraps(), Ordering::Relaxed);
        self.begin_play(plan, from, rev, speed, grain)
    }

    /// Change the speed (negative: backward), taking effect at once while playing. Turning round
    /// starts a new play from the same place.
    pub fn set_speed(&mut self, speed: f64) {
        if !speed.is_finite() || speed == 0.0 {
            return;
        }
        let (rev, mag) = (speed < 0.0, speed.abs().max(MIN_SPEED));
        if let Some(h) = self.held.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            h.rev = rev;
            h.speed = mag;
        }
        let now = self.shared.snapshot();
        if now.playing && now.rev != rev {
            self.restart(now.plan, now.file_position(), rev, mag, now.grain);
        } else {
            self.shared.set_speed(mag, rev);
        }
    }

    /// The speed, negative when going backward.
    pub fn speed(&self) -> f64 {
        if let Some(h) = *self.held.lock().unwrap_or_else(|e| e.into_inner()) {
            return if h.rev { -h.speed } else { h.speed };
        }
        let now = self.shared.snapshot();
        if now.rev { -now.speed } else { now.speed }
    }

    /// Swap the plan while playing or paused, keeping the place (a shuttle that changes direction
    /// needs the other half of the file).
    pub fn replace_plan(&mut self, plan: &Plan) {
        if let Some(h) = self.held.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            h.plan = *plan;
            return;
        }
        let now = self.shared.snapshot();
        if now.playing {
            self.restart(*plan, now.file_position(), now.rev, now.speed, false);
        }
    }

    /// Move the play position (Rewind / Fast Forward while playing or paused).
    pub fn seek(&mut self, frame: f64) {
        if !frame.is_finite() {
            return;
        }
        let frame = frame.max(0.0);
        if let Some(h) = self.held.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            h.pos = frame;
            return;
        }
        let now = self.shared.snapshot();
        if now.playing {
            self.restart(now.plan, frame, now.rev, now.speed, now.grain);
        }
    }

    /// Stop where it is, keeping the place and the plan for [`Player::resume`].
    pub fn pause(&self) {
        let now = self.shared.snapshot();
        if now.playing {
            *self.held.lock().unwrap_or_else(|e| e.into_inner()) =
                Some(Held { plan: now.plan, pos: now.file_position(), rev: now.rev, speed: now.speed, grain: now.grain });
        }
        self.shared.halt();
    }

    /// Carry on from the place `pause` left. False when there is nothing to resume.
    pub fn resume(&mut self) -> bool {
        let held = self.held.lock().unwrap_or_else(|e| e.into_inner()).take();
        let Some(h) = held else { return false };
        // The paused play's loop passes are kept; the new play counts from zero.
        let base = self.wraps_base.load(Ordering::Relaxed);
        let ok = self.begin_play(h.plan, h.pos, h.rev, h.speed, h.grain);
        self.wraps_base.store(base, Ordering::Relaxed);
        ok
    }

    pub fn stop(&self) {
        *self.held.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.shared.halt();
    }

    /// Loop on or off, taking effect at once while playing.
    pub fn set_looping(&mut self, on: bool) {
        if let Some(h) = self.held.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            h.plan.looping = on;
            return;
        }
        let mut plan = self.shared.snapshot().plan;
        plan.looping = on;
        self.replace_plan(&plan);
    }

    /// Whether a play is running (a paused one is not).
    pub fn is_playing(&self) -> bool {
        self.shared.snapshot().playing
    }

    /// Times the loop has gone round since playback started.
    pub fn wraps(&self) -> u32 {
        self.wraps_base.load(Ordering::Relaxed).saturating_add(self.shared.snapshot().wraps())
    }

    /// How many audio callbacks found the read-ahead ring dry since start-up (each played silence
    /// for that buffer).
    pub fn underruns(&self) -> u64 {
        self.shared.underruns.load(Ordering::Relaxed)
    }

    /// The largest sample magnitude sent on the first two channels since the last call, and start over.
    pub fn take_levels(&self) -> [f32; 2] {
        self.shared.levels.take()
    }

    /// Record a block's peaks as if the callback had sent them (the control channel's `METERS PUSH`).
    pub fn push_levels(&self, peaks: [f32; 2]) {
        self.shared.levels.record(0, peaks[0]);
        self.shared.levels.record(1, peaks[1]);
    }

    /// The frame being played right now (where a paused play will carry on).
    pub fn position(&self) -> usize {
        self.position_f64() as usize
    }

    fn position_f64(&self) -> f64 {
        if let Some(h) = *self.held.lock().unwrap_or_else(|e| e.into_inner()) {
            return h.pos.max(0.0);
        }
        self.shared.snapshot().file_position().max(0.0)
    }
}

/// The slowest play speed (a crawl); anything below is raised to it.
const MIN_SPEED: f64 = 0.05;

/// What [`Player::info`] reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackInfo {
    pub device_rate: u32,
    pub doc_rate: u32,
    /// The document's rate is not the device's, so the sinc resampler is converting.
    pub resampling: bool,
    pub quality: Quality,
    pub playing: bool,
    pub position: usize,
    pub underruns: u64,
    pub test_sink: bool,
}

/// True under `WAVCRAFT_NO_AUDIO=1`: no sound card, a silent real-time sink instead.
fn no_audio() -> bool {
    std::env::var("WAVCRAFT_NO_AUDIO").is_ok_and(|v| !matches!(v.trim(), "" | "0" | "false"))
}

const SINK_CHANNELS: usize = 2;
/// Frames the sink asks the callback for at a time (10 ms at 48 kHz).
const SINK_BUFFER: usize = 480;

/// A thread that plays the part of the audio device: it runs the same callback at real-time speed
/// into nowhere. The rate is 48 kHz unless `WAVCRAFT_NO_AUDIO_RATE` says otherwise.
struct Sink {
    rate: u32,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Sink {
    fn start(shared: &Arc<Shared>) -> Sink {
        let rate = std::env::var("WAVCRAFT_NO_AUDIO_RATE")
            .ok()
            .and_then(|v| v.trim().parse::<u32>().ok())
            .filter(|r| (8_000..=768_000).contains(r))
            .unwrap_or(48_000);
        let stop = Arc::new(AtomicBool::new(false));
        let (flag, shared) = (Arc::clone(&stop), Arc::clone(shared));
        let thread = std::thread::Builder::new()
            .name("wavcraft-test-sink".to_string())
            .spawn(move || {
                let mut rack: Option<Box<RackProcessor>> = None;
                let mut out = vec![0.0f32; SINK_BUFFER * SINK_CHANNELS];
                let period = Duration::from_secs_f64(SINK_BUFFER as f64 / f64::from(rate));
                let mut next = Instant::now();
                while !flag.load(Ordering::Relaxed) {
                    device_callback(&shared, &mut out, SINK_CHANNELS, f64::from(rate), &mut rack);
                    next += period;
                    let now = Instant::now();
                    if next > now {
                        std::thread::sleep(next - now);
                    } else {
                        next = now;
                    }
                }
            })
            .ok();
        Sink { rate, stop, thread }
    }
}

impl Drop for Sink {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// The names of the output and input devices the system offers, for the Audio Hardware page.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Devices {
    pub output: Vec<String>,
    pub input: Vec<String>,
}

/// List the devices on a worker thread (the system can block for seconds while it asks each
/// device whether it does output; one stuck device must not freeze the window). The answer comes
/// through the receiver: the output and input devices, or, if that takes too long, every device
/// the system names under both headings.
pub fn list_devices_in_background(host: Option<String>) -> std::sync::mpsc::Receiver<Devices> {
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = std::thread::Builder::new().name("audio devices".into()).spawn(move || {
        let (probe_tx, probe_rx) = std::sync::mpsc::channel();
        let probe_host = host.clone();
        let _ = std::thread::Builder::new().name("audio device probe".into()).spawn(move || {
            let host = host_for(probe_host.as_deref());
            let names = |devices: Vec<cpal::Device>| -> Vec<String> { devices.iter().filter_map(|d| d.name().ok()).collect() };
            let _ = probe_tx.send(Devices {
                output: names(host.output_devices().map(Iterator::collect).unwrap_or_default()),
                input: names(host.input_devices().map(Iterator::collect).unwrap_or_default()),
            });
        });
        let devices = probe_rx.recv_timeout(std::time::Duration::from_secs(3)).unwrap_or_else(|_| {
            log::warn!("the audio system is slow to list output and input devices; listing every device under both");
            let all: Vec<String> = host_for(host.as_deref()).devices().map(|d| d.filter_map(|d| d.name().ok()).collect()).unwrap_or_default();
            Devices { output: all.clone(), input: all }
        });
        let _ = tx.send(devices);
    });
    rx
}

/// [`list_devices_in_background`], waited for (the control channel's `DEVICES`).
pub fn list_devices(host: Option<String>) -> Devices {
    list_devices_in_background(host).recv_timeout(std::time::Duration::from_secs(10)).unwrap_or_default()
}

/// The names of every device the host knows (no device is asked whether it does output, which
/// can block; none is opened), on a worker thread. This keys the audio hardware profile.
pub fn list_device_names_in_background(host: Option<String>) -> std::sync::mpsc::Receiver<Vec<String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = std::thread::Builder::new().name("audio device names".into()).spawn(move || {
        let host = host_for(host.as_deref());
        let names: Vec<String> = host.devices().map(|d| d.filter_map(|d| d.name().ok()).collect()).unwrap_or_default();
        let _ = tx.send(names);
    });
    rx
}

/// Which of `available` to open for the wanted name: its position, or `None` (take the system
/// default) when nothing was asked for or the name isn't there (the device was unplugged).
fn pick(wanted: Option<&str>, available: &[String]) -> Option<usize> {
    let wanted = wanted?;
    available.iter().position(|n| n == wanted)
}

/// An opened output stream and what it turned out to be.
struct Opened {
    stream: cpal::Stream,
    device: String,
    host: String,
    buffer: Option<u32>,
    range: Option<(u32, u32)>,
    channels: usize,
    rate: u32,
}

fn open_stream(shared: &Arc<Shared>, wanted: &OutputChoice) -> Result<Opened, String> {
    let host = host_for(wanted.host.as_deref());
    let host_name = host.id().name().to_string();
    let chosen = wanted.device.as_deref().and_then(|w| {
        // Not `output_devices()`: that asks every device about output, which can block.
        let devices: Vec<cpal::Device> = host.devices().ok()?.collect();
        let names: Vec<String> = devices.iter().map(|d| d.name().unwrap_or_default()).collect();
        let found = pick(Some(w), &names).and_then(|i| devices.into_iter().nth(i));
        if found.is_none() {
            log::warn!("audio output {w:?} is not available; using the system default");
        }
        found
    });
    let device = match chosen {
        Some(d) => d,
        None => host.default_output_device().ok_or("no audio output device")?,
    };
    let name = device.name().unwrap_or_else(|_| "output".to_string());
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let range = match supported.buffer_size() {
        cpal::SupportedBufferSize::Range { min, max } => Some((*min, *max)),
        cpal::SupportedBufferSize::Unknown => None,
    };
    let format = supported.sample_format();
    let mut config: cpal::StreamConfig = supported.into();
    let channels = usize::from(config.channels).max(1);
    let rate = config.sample_rate.0.max(1);
    let build_with = |config: &cpal::StreamConfig| match format {
        cpal::SampleFormat::F32 => build::<f32>(&device, config, shared),
        cpal::SampleFormat::I16 => build::<i16>(&device, config, shared),
        cpal::SampleFormat::U16 => build::<u16>(&device, config, shared),
        cpal::SampleFormat::I32 => build::<i32>(&device, config, shared),
        other => Err(format!("unsupported output sample format {other:?}")),
    };
    let mut buffer = choose_buffer(wanted.buffer, range);
    if let (Some(asked), Some(got)) = (wanted.buffer, buffer)
        && asked != got
    {
        log::warn!("buffer size {asked} is outside what {name:?} supports; using {got}");
    }
    if let Some(n) = buffer {
        config.buffer_size = cpal::BufferSize::Fixed(n);
    }
    let stream = match build_with(&config) {
        Ok(s) => s,
        Err(e) if buffer.is_some() => {
            log::warn!("{name:?} refused a buffer of {buffer:?} frames ({e}); using its default");
            config.buffer_size = cpal::BufferSize::Default;
            buffer = None;
            build_with(&config)?
        }
        Err(e) => return Err(e),
    };
    stream.play().map_err(|e| e.to_string())?;
    log::info!("audio output: host {host_name}, device {name:?}, {channels} channels, buffer {buffer:?}");
    Ok(Opened { stream, device: name, host: host_name, buffer, range, channels, rate })
}

fn build<T>(device: &cpal::Device, config: &cpal::StreamConfig, shared: &Arc<Shared>) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
    f32: cpal::FromSample<T>,
{
    let on_error = Arc::clone(shared);
    let shared = Arc::clone(shared);
    let channels = usize::from(config.channels).max(1);
    let device_rate = f64::from(config.sample_rate.0.max(1));
    let mut rack: Option<Box<RackProcessor>> = None;
    device
        .build_output_stream(
            config,
            move |out: &mut [T], _: &cpal::OutputCallbackInfo| device_callback(&shared, out, channels, device_rate, &mut rack),
            move |err| {
                log::warn!("audio output: {err}");
                on_error.failed.store(true, Ordering::Relaxed);
                on_error.state.lock().unwrap_or_else(|e| e.into_inner()).playing = false;
            },
            None,
        )
        .map_err(|e| e.to_string())
}

/// One device buffer: render what plays, then run the Effects Rack over it. Runs on the audio thread
/// (the cpal callback, or the test sink's stand-in for it).
fn device_callback<T>(shared: &Shared, out: &mut [T], channels: usize, device_rate: f64, rack: &mut Option<Box<RackProcessor>>)
where
    T: cpal::Sample + cpal::FromSample<f32>,
    f32: cpal::FromSample<T>,
{
    let was_playing = shared.state.try_lock().map(|s| s.playing).unwrap_or(false);
    render(shared, out, channels, device_rate);
    take_rack(shared, rack);
    if let (true, Some(r)) = (was_playing, rack.as_mut()) {
        run_rack(r, out, channels);
    }
}

/// Pick up a rack the UI parked in the mailbox (see [`Shared::rack_mailbox`]). Never blocks.
fn take_rack(shared: &Shared, rack: &mut Option<Box<RackProcessor>>) {
    if shared.rack_new.load(Ordering::Acquire)
        && let Ok(mut parked) = shared.rack_mailbox.try_lock()
    {
        std::mem::swap(&mut *parked, rack);
        shared.rack_new.store(false, Ordering::Release);
    }
}

/// Run the rack over one device buffer in place, in chunks of at most [`MAX_BLOCK`] frames. No
/// allocation: the rack's own buffers hold each chunk. Channels beyond the rack's are untouched.
fn run_rack<T>(rack: &mut RackProcessor, out: &mut [T], channels: usize)
where
    T: cpal::Sample + cpal::FromSample<f32>,
    f32: cpal::FromSample<T>,
{
    let rack_channels = rack.channels().min(channels);
    for chunk in out.chunks_mut(channels * MAX_BLOCK) {
        let n = chunk.len() / channels;
        for c in 0..rack_channels {
            for (f, slot) in rack.chunk_in(c).iter_mut().take(n).enumerate() {
                *slot = f32::from_sample(chunk[f * channels + c]);
            }
        }
        rack.run_chunk(n);
        for c in 0..rack_channels {
            for (f, v) in rack.chunk_out(c).iter().take(n).enumerate() {
                chunk[f * channels + c] = T::from_sample(*v);
            }
        }
    }
}

/// Fill one device buffer. Runs on the audio thread.
fn render<T: cpal::Sample + cpal::FromSample<f32>>(shared: &Shared, out: &mut [T], channels: usize, device_rate: f64) {
    render_with(shared, out, channels, device_rate, || {}, || {});
}

fn render_with<T: cpal::Sample + cpal::FromSample<f32>, F: FnOnce(), G: FnOnce()>(
    shared: &Shared,
    out: &mut [T],
    channels: usize,
    device_rate: f64,
    before_commit: F,
    after_check: G,
) {
    let silence = T::from_sample(0.0f32);
    let Ok(state) = shared.state.try_lock() else {
        out.iter_mut().for_each(|s| *s = silence);
        return;
    };
    let playback = *state;
    drop(state);
    out.iter_mut().for_each(|s| *s = silence);
    // A play sounds only once the read-ahead thread has primed the ring for its generation.
    if !playback.playing || shared.ready_generation.load(Ordering::Acquire) != playback.generation {
        return;
    }
    let file_channels = shared.channels.load(Ordering::Acquire) as usize;
    let rate = shared.rate.load(Ordering::Acquire);
    if file_channels == 0 || rate == 0 {
        return;
    }
    let Ok(map) = shared.map.try_lock() else { return };
    let step = f64::from(rate) / device_rate * playback.speed;
    let end = playback.end;
    let last = end.ceil().max(0.0) as u64;
    let mut pos = playback.pos.max(0.0);
    let mut filled = shared.filled_to.load(Ordering::Acquire);
    let origin = shared.origin.load(Ordering::Acquire);
    let sample = |frame: u64, c: usize| shared.ring.get(Shared::slot(frame, c)).map_or(0.0, |v| f32::from_bits(v.load(Ordering::Relaxed)));
    // The sinc resampler, when one is parked for exactly this (file rate, device rate) pair. A busy
    // lock or a stale pair (a rebuild is on its way) falls back to linear interpolation.
    let parked = shared.resampler.try_lock().ok();
    // The kernel is for normal speed; shuttle and scrub speeds interpolate linearly.
    let sinc = parked
        .as_deref()
        .and_then(|p| p.as_deref())
        .filter(|r| playback.speed == 1.0 && r.in_rate() == rate && f64::from(r.out_rate()) == device_rate && !r.is_identity());
    let fade = transport::GRAIN_FADE_SECS * f64::from(rate);
    // The sinc kernel's end of the play, as a signed frame (an unbounded loop has none).
    let last_k = last.min(i64::MAX as u64) as i64;
    let reach = sinc.map_or(0, |r| r.half_width().ceil() as u64);
    let mut finished = false;
    let mut peak = [0.0f32; 2];
    for frame in out.chunks_mut(channels) {
        if pos >= end {
            finished = true;
            break;
        }
        let i = pos as u64;
        // This frame and the next, or what the kernel reaches (unless the play ends first), must be in the ring.
        let need = (i + 2 + reach).min(last);
        if filled < need {
            filled = shared.filled_to.load(Ordering::Acquire);
            if filled < need {
                shared.underruns.fetch_add(1, Ordering::Relaxed);
                break;
            }
        }
        let frac = (pos - i as f64) as f32;
        // A scrub grain fades in and out; the file frame it is at follows from the play order.
        let gain = if playback.grain {
            let at = if playback.rev { playback.from - (pos - playback.from) } else { pos };
            transport::grain_gain(at, playback.plan.start, playback.plan.end, fade)
        } else {
            1.0
        };
        let interp = |c: usize| {
            if let Some(r) = sinc {
                // Frames before this play's start and at or past its end are silence.
                return gain * r.sample_with(pos, |k| if k < origin as i64 || k >= last_k { 0.0 } else { sample(k as u64, c) });
            }
            let a = sample(i, c);
            let b = if i + 1 < last { sample(i + 1, c) } else { a };
            gain * (a + (b - a) * frac)
        };
        // Level tap: the first two file channels (a mono file shows on both), before the mix-down.
        let l = interp(0);
        let r = if file_channels > 1 { interp(1) } else { l };
        peak = [peak[0].max(l.abs()), peak[1].max(r.abs())];
        if !map.is_default() {
            // An explicit map: each device channel plays the document channels mapped to it
            // (summed), and nothing else.
            for (c, slot) in frame.iter_mut().enumerate() {
                let sum: f32 = map.sources(c).filter(|src| *src < file_channels).map(|src| interp(src)).sum();
                *slot = T::from_sample(sum);
            }
        } else if channels == 1 && file_channels > 1 {
            // A one-channel device gets the average of the file's channels, not just the left
            // one (a host-left, guest-right recording would lose the guest).
            let sum: f32 = (0..file_channels).map(|c| interp(c)).sum();
            if let Some(slot) = frame.first_mut() {
                *slot = T::from_sample(sum / file_channels as f32);
            }
        } else {
            for (c, slot) in frame.iter_mut().enumerate() {
                // Mono plays on both speakers; a stereo file leaves any further device channels silent.
                let source = if file_channels == 1 { 0 } else { c };
                if source >= file_channels || c >= file_channels.max(2) {
                    continue;
                }
                *slot = T::from_sample(interp(source));
            }
        }
        pos += step;
    }
    shared.levels.record(0, peak[0]);
    shared.levels.record(1, peak[1]);
    before_commit();
    let Ok(mut state) = shared.state.try_lock() else { return };
    if state.generation == playback.generation {
        // Test seam: keep the state lock held so a concurrent restart applies after this commit.
        after_check();
        if finished {
            state.playing = false;
        }
        state.pos = pos;
    }
}

/// What the read-ahead thread is feeding.
struct Feed {
    state: DocState,
    /// Channels buffered (the document's, at most [`RING_CHANNELS`]).
    channels: usize,
    /// The play: its plan (the end clamped to the document), the file frame it began at (also
    /// the ring's first slot) and whether it runs backward.
    plan: Plan,
    from: f64,
    rev: bool,
    /// One past the last slot of the ring this play needs (`u64::MAX` in a loop).
    last: u64,
}

/// Start feeding the play `generation` of `state`: reset the ring, clamp the end of the plan to the
/// document, and fill enough to start. Marks the ring ready for `generation` unless a newer play
/// has begun since.
fn begin(shared: &Shared, state: &DocState, generation: u64) -> Feed {
    let play = {
        let mut play = shared.state.lock().unwrap_or_else(|e| e.into_inner());
        if play.generation == generation {
            play.plan.end = play.plan.end.min(state.len_frames() as f64);
            play.end = play.from + play.plan.frames_from(play.from, play.rev);
            play.playing &= play.end > play.from;
        }
        *play
    };
    let from = play.from as u64;
    let last = play.end.ceil() as u64;
    shared.origin.store(from, Ordering::Release);
    shared.filled_to.store(from, Ordering::Release);
    let feed = Feed { state: state.clone(), channels: state.channel_count().min(RING_CHANNELS), plan: play.plan, from: play.from, rev: play.rev, last };
    let want = (from + PRIME_FRAMES).min(last);
    while shared.filled_to.load(Ordering::Relaxed) < want {
        if !top_up(shared, &feed) {
            break;
        }
    }
    if shared.snapshot().generation == generation {
        shared.ready_generation.store(generation, Ordering::Release);
    }
    feed
}

/// Read the next chunk into the ring if there is room ahead of the play position. Returns whether
/// it wrote anything.
fn top_up(shared: &Shared, feed: &Feed) -> bool {
    let filled = shared.filled_to.load(Ordering::Relaxed);
    let pos = shared.snapshot().pos.max(0.0) as u64;
    let room_to = (pos + RING_FRAMES as u64).saturating_sub(RING_MARGIN);
    let target = feed.last.min(room_to);
    if filled >= target {
        return false;
    }
    let n = (target - filled).min(FILL_CHUNK as u64) as usize;
    let mut buf = vec![0.0f32; n];
    let mut wraps = 0;
    for c in 0..feed.channels {
        // The chunk is `n` frames of play order: copy each run of consecutive file frames in one read.
        let mut done = 0;
        while done < n {
            let at = feed.plan.position_after(feed.from, (filled + done as u64 - feed.from as u64) as f64, feed.rev, &mut wraps);
            let Some(at) = at else {
                buf[done..].fill(0.0);
                break;
            };
            let run = (feed.plan.run_len(at, feed.rev) as usize).min(n - done);
            let first = if feed.rev { (at.floor() as u64 + 1).saturating_sub(run as u64) } else { at.floor() as u64 };
            let Some(dst) = buf.get_mut(done..done + run) else { break };
            let got = match feed.state.read(c, first, dst) {
                Ok(got) => got,
                Err(e) => {
                    log::warn!(target: "wavcraft", "playback read from the store failed: {e}");
                    0
                }
            };
            // A short or failed read plays as silence for the rest of the run.
            if let Some(rest) = dst.get_mut(got..) {
                rest.fill(0.0);
            }
            if feed.rev {
                dst.reverse();
            }
            done += run;
        }
        for (k, v) in buf.iter().enumerate() {
            if let Some(slot) = shared.ring.get(Shared::slot(filled + k as u64, c)) {
                slot.store(v.to_bits(), Ordering::Relaxed);
            }
        }
    }
    shared.filled_to.store(filled + n as u64, Ordering::Release);
    true
}

/// Fill the ring as far as there is room.
fn fill_all(shared: &Shared, feed: &Feed) {
    while top_up(shared, feed) {}
}

/// The read-ahead thread: owns the document, primes the ring on a play and keeps it topped up
/// while playing. Ends when the player is dropped.
fn read_ahead(shared: &Shared, inbox: &Receiver<Command>) {
    let mut state: Option<DocState> = None;
    let mut feed: Option<Feed> = None;
    loop {
        let wait = if feed.is_some() && shared.snapshot().playing { Duration::from_millis(3) } else { Duration::from_secs(3600) };
        match inbox.recv_timeout(wait) {
            Ok(Command::Document(doc)) => {
                feed = None;
                shared.rate.store(doc.as_ref().map_or(0, DocState::sample_rate), Ordering::Release);
                shared.channels.store(doc.as_ref().map_or(0, |d| d.channel_count().min(RING_CHANNELS) as u32), Ordering::Release);
                state = doc;
                rebuild_resampler(shared);
            }
            Ok(Command::Rebuild) => rebuild_resampler(shared),
            Ok(Command::Play { generation }) => {
                feed = state.as_ref().map(|s| begin(shared, s, generation));
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if shared.snapshot().playing
            && let Some(f) = &feed
        {
            fill_all(shared, f);
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use wavcraft_core::{AudioBuffer, Document};

    /// A document holding `audio` in the store, as the app opens it.
    fn doc_of(audio: AudioBuffer) -> DocState {
        Document::from_buffer(audio).unwrap().state
    }

    /// A shared player state with `state` loaded, as the read-ahead thread leaves it.
    fn shared_with(state: &DocState) -> Shared {
        let shared = Shared::new();
        shared.rate.store(state.sample_rate(), Ordering::Relaxed);
        shared.channels.store(state.channel_count().min(RING_CHANNELS) as u32, Ordering::Relaxed);
        shared
    }

    /// Start a play the way `Player::play` and the read-ahead thread do, without the threads: prime
    /// the ring for it, then fill it.
    fn play(shared: &Shared, state: &DocState, from: f64, to: f64) {
        let generation = shared.start(from, to);
        let feed = begin(shared, state, generation);
        fill_all(shared, &feed);
    }

    fn playback(shared: &Shared) -> Playback {
        shared.snapshot()
    }

    #[test]
    fn a_stale_callback_does_not_overwrite_a_restart() {
        let ramp: Vec<f32> = (0..1000).map(|i| i as f32).collect();
        let state = doc_of(AudioBuffer { sample_rate: 48_000, channels: vec![ramp] });
        let shared = shared_with(&state);
        play(&shared, &state, 0.0, 1000.0);
        let mut out = vec![0.0f32; 64];
        render_with(
            &shared,
            &mut out,
            1,
            48_000.0,
            || {
                shared.start(500.0, 1000.0);
            },
            || {},
        );
        assert_eq!(playback(&shared).pos, 500.0);
        assert!(playback(&shared).playing);
    }

    #[test]
    fn a_stale_callback_does_not_clear_a_restart_at_end_of_range() {
        let state = doc_of(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.0f32; 1000]] });
        let shared = shared_with(&state);
        play(&shared, &state, 10.0, 20.0);
        let mut out = vec![0.0f32; 64];
        render_with(
            &shared,
            &mut out,
            1,
            48_000.0,
            || {
                shared.start(500.0, 1000.0);
            },
            || {},
        );
        assert!(playback(&shared).playing);
        assert_eq!(playback(&shared).pos, 500.0);
    }

    #[test]
    fn play_bump_window_is_not_clobbered() {
        let ramp: Vec<f32> = (0..1000).map(|i| (i + 1) as f32).collect();
        let state = doc_of(AudioBuffer { sample_rate: 48_000, channels: vec![ramp] });
        let shared = shared_with(&state);
        play(&shared, &state, 0.0, 1000.0);
        let mut out = vec![0.0f32; 64];
        shared.start_with(500.0, 1000.0, || {
            render_with(&shared, &mut out, 1, 48_000.0, || {}, || {});
            assert!(out.iter().all(|v| *v == 0.0));
        });
        assert_eq!(playback(&shared).pos, 500.0);
        assert!(playback(&shared).playing);
    }

    #[test]
    fn restart_between_check_and_commit_is_not_clobbered() {
        let ramp: Vec<f32> = (0..1000).map(|i| i as f32).collect();
        let state = doc_of(AudioBuffer { sample_rate: 48_000, channels: vec![ramp] });
        let shared = shared_with(&state);
        play(&shared, &state, 0.0, 1000.0);
        let mut out = vec![0.0f32; 64];
        std::thread::scope(|scope| {
            render_with(
                &shared,
                &mut out,
                1,
                48_000.0,
                || {},
                || {
                    scope.spawn(|| shared.start(500.0, 1000.0));
                    std::thread::sleep(std::time::Duration::from_millis(50));
                },
            );
        });
        assert_eq!(playback(&shared).pos, 500.0);
        assert!(playback(&shared).playing);
    }

    #[test]
    fn restart_between_check_and_commit_is_not_clobbered_at_end_of_range() {
        let state = doc_of(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.0f32; 1000]] });
        let shared = shared_with(&state);
        play(&shared, &state, 10.0, 20.0);
        let mut out = vec![0.0f32; 64];
        std::thread::scope(|scope| {
            render_with(
                &shared,
                &mut out,
                1,
                48_000.0,
                || {},
                || {
                    scope.spawn(|| shared.start(500.0, 1000.0));
                    std::thread::sleep(std::time::Duration::from_millis(50));
                },
            );
        });
        assert!(playback(&shared).playing);
        assert_eq!(playback(&shared).pos, 500.0);
    }

    #[test]
    fn a_wanted_device_is_found_by_name_or_the_default_is_used() {
        let names: Vec<String> = ["Built-in", "USB Interface", "HDMI"].iter().map(|s| s.to_string()).collect();
        assert_eq!(pick(Some("USB Interface"), &names), Some(1));
        assert_eq!(pick(Some("Gone"), &names), None, "an unplugged device falls back to the default");
        assert_eq!(pick(None, &names), None);
    }

    #[test]
    fn plays_the_range_then_stops_with_silence() {
        let ramp: Vec<f32> = (0..100).map(|i| i as f32 / 100.0).collect();
        let state = doc_of(AudioBuffer { sample_rate: 48_000, channels: vec![ramp.clone(), ramp.iter().map(|v| -v).collect()] });
        let shared = shared_with(&state);
        let mut out = vec![9.0f32; 40];
        render(&shared, &mut out, 2, 48_000.0);
        assert!(out.iter().all(|v| *v == 0.0), "stopped: silence");
        play(&shared, &state, 10.0, 20.0);
        render(&shared, &mut out, 2, 48_000.0);
        // Frames 10..20, left and right, then silence for the rest of the buffer.
        for k in 0..10 {
            assert_eq!((out[2 * k], out[2 * k + 1]), (ramp[10 + k], -ramp[10 + k]), "frame {k}");
        }
        assert!(out[20..].iter().all(|v| *v == 0.0));
        assert!(!playback(&shared).playing, "stops at the end of the range");
        assert_eq!(playback(&shared).pos, 20.0);
    }

    #[test]
    fn a_play_is_silent_until_its_ring_is_primed() {
        let ramp: Vec<f32> = (0..100).map(|i| 0.5 + i as f32 / 200.0).collect();
        let state = doc_of(AudioBuffer { sample_rate: 48_000, channels: vec![ramp.clone()] });
        let shared = shared_with(&state);
        let generation = shared.start(0.0, 100.0);
        let mut out = vec![9.0f32; 8];
        render(&shared, &mut out, 1, 48_000.0);
        assert!(out.iter().all(|v| *v == 0.0), "not primed for this play: silence");
        let feed = begin(&shared, &state, generation);
        render(&shared, &mut out, 1, 48_000.0);
        assert_eq!(out[0], ramp[0]);
        drop(feed);
    }

    #[test]
    fn mono_plays_on_both_speakers_and_rates_are_converted() {
        let ramp: Vec<f32> = (0..1000).map(|i| i as f32).collect();
        let state = doc_of(AudioBuffer { sample_rate: 24_000, channels: vec![ramp] });
        let shared = shared_with(&state);
        play(&shared, &state, 0.0, 1000.0);
        // A 48 kHz device plays a 24 kHz file at half a file frame per device frame, on 4 channels.
        let mut out = vec![0.0f32; 4 * 8];
        render(&shared, &mut out, 4, 48_000.0);
        for k in 0..8 {
            let want = k as f32 * 0.5;
            assert_eq!(out[4 * k..4 * k + 4], [want, want, 0.0, 0.0], "frame {k}");
        }
        // The end of the range is clamped to the file, and integer formats convert.
        play(&shared, &state, 998.0, 1e12);
        let mut ints = vec![7i16; 8];
        render(&shared, &mut ints, 1, 24_000.0);
        assert!(ints[0] != 0 && ints[1] != 0 && ints[2..].iter().all(|v| *v == 0), "{ints:?}");
    }

    /// Regression: a one-channel device played only the file's left channel.
    #[test]
    fn a_mono_device_gets_every_channel_of_the_file() {
        let state = doc_of(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.0; 100], vec![0.5; 100]] });
        let shared = shared_with(&state);
        play(&shared, &state, 0.0, 100.0);
        let mut out = vec![9.0f32; 16];
        render(&shared, &mut out, 1, 48_000.0);
        assert!(out.iter().all(|v| *v == 0.25), "left silent, right at 0.5: {out:?}");
    }

    /// f26: what the callback sends is what the Levels panel gets, per channel, without a lock.
    /// Set the channel map a play is rendered through (the text form, as the settings hold it).
    fn set_map(shared: &Shared, text: &str) {
        let map = ChannelMap::parse(text).unwrap_or_default();
        *shared.map.lock().unwrap_or_else(|e| e.into_inner()) = map;
    }

    #[test]
    fn a_channel_map_routes_stereo_to_outputs_three_and_four() {
        let state = doc_of(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.25; 100], vec![-0.5; 100]] });
        let shared = shared_with(&state);
        set_map(&shared, "1:3,2:4");
        play(&shared, &state, 0.0, 100.0);
        let mut out = vec![9.0f32; 4 * 3];
        render(&shared, &mut out, 4, 48_000.0);
        for k in 0..3 {
            assert_eq!(out[4 * k..4 * k + 4], [0.0, 0.0, 0.25, -0.5], "frame {k}");
        }
    }

    #[test]
    fn a_map_sums_channels_sharing_an_output_and_drops_those_off_the_device() {
        let state = doc_of(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.25; 100], vec![0.5; 100]] });
        let shared = shared_with(&state);
        // Both on output 2 (summed); a mapping to output 9 on a 2-channel device is dropped.
        set_map(&shared, "1:2,2:2");
        play(&shared, &state, 0.0, 100.0);
        let mut out = vec![9.0f32; 4];
        render(&shared, &mut out, 2, 48_000.0);
        assert_eq!(out, [0.0, 0.75, 0.0, 0.75]);
        set_map(&shared, "1:9");
        play(&shared, &state, 0.0, 100.0);
        render(&shared, &mut out, 2, 48_000.0);
        assert!(out.iter().all(|v| *v == 0.0), "{out:?}");
    }

    #[test]
    fn the_callback_records_the_peaks_it_sends() {
        let state = doc_of(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.0, 0.25, -0.5, 0.0], vec![0.0, 0.125, 0.0, -1.0]] });
        let shared = shared_with(&state);
        play(&shared, &state, 0.0, 4.0);
        let mut out = vec![0.0f32; 8];
        render(&shared, &mut out, 2, 48_000.0);
        assert_eq!(shared.levels.take(), [0.5, 1.0]);
        assert_eq!(shared.levels.take(), [0.0, 0.0], "taking starts over");
        // A mono file shows on both bars; a stopped player records nothing.
        let mono = doc_of(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.0, 0.75]] });
        let shared = shared_with(&mono);
        play(&shared, &mono, 0.0, 2.0);
        render(&shared, &mut out, 2, 48_000.0);
        assert_eq!(shared.levels.take(), [0.75, 0.75]);
        render(&shared, &mut out, 2, 48_000.0);
        assert_eq!(shared.levels.take(), [0.0, 0.0]);
    }

    /// Render `frames` device frames of mono output into one vector, buffer by buffer.
    fn render_run(shared: &Shared, frames: usize, device_rate: f64) -> Vec<f32> {
        let mut all = Vec::new();
        while all.len() < frames {
            let mut out = vec![0.0f32; 64];
            render(shared, &mut out, 1, device_rate);
            all.extend(out);
        }
        all
    }

    /// f74: with the sinc resampler parked, a 44.1 kHz sine plays on a 48 kHz device at its own
    /// frequency and level, the playhead stays in file frames, and both qualities do it.
    #[test]
    fn the_parked_sinc_resampler_converts_a_sine_out_of_the_ring() {
        let sine: Vec<f32> = (0..20_000).map(|i| (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / 44_100.0).sin() as f32).collect();
        let state = doc_of(AudioBuffer { sample_rate: 44_100, channels: vec![sine] });
        for q in [Quality::High, Quality::Fast] {
            let shared = shared_with(&state);
            shared.quality.store(quality_code(q), Ordering::Relaxed);
            shared.device_rate.store(48_000, Ordering::Relaxed);
            rebuild_resampler(&shared);
            assert!(shared.resampler.lock().unwrap().is_some(), "{q:?}: a resampler is parked");
            play(&shared, &state, 0.0, 20_000.0);
            let out = render_run(&shared, 9_600, 48_000.0);
            assert_eq!(shared.underruns.load(Ordering::Relaxed), 0);
            let tol = if q == Quality::High { 2e-3 } else { 3e-2 };
            // Past the kernel's start-up (the first taps see silence before frame 0).
            let worst = out
                .iter()
                .enumerate()
                .skip(100)
                .map(|(n, v)| (f64::from(*v) - (2.0 * std::f64::consts::PI * 1000.0 * n as f64 / 48_000.0).sin()).abs())
                .fold(0.0, f64::max);
            assert!(worst < tol, "{q:?}: worst error {worst}");
            // 9,600 device frames are 8,820 file frames (the playhead is in file frames).
            assert!((shared.snapshot().pos - 8_820.0).abs() < 1e-6, "{}", shared.snapshot().pos);
        }
    }

    /// A play starting mid-file does not hear the previous play's audio through the kernel's tail:
    /// frames before the play's start read as silence.
    #[test]
    fn the_kernel_does_not_read_an_earlier_plays_ring_contents() {
        let state = doc_of(AudioBuffer { sample_rate: 24_000, channels: vec![(0..4_000).map(|i| if i < 2_000 { -1.0 } else { 1.0 }).collect()] });
        let shared = shared_with(&state);
        shared.device_rate.store(48_000, Ordering::Relaxed);
        rebuild_resampler(&shared);
        play(&shared, &state, 0.0, 4_000.0);
        let _ = render_run(&shared, 64, 48_000.0);
        play(&shared, &state, 2_000.0, 4_000.0);
        let out = render_run(&shared, 192, 48_000.0);
        // The ring still holds -1.0 before frame 2,000 from the first play. Half a frame in, the
        // kernel's mass left of frame 2,000 (about 0.5) must read as silence: the sample is about
        // 1.13 (the step's overshoot), where that stale -1.0 would drag it to about 0.1.
        assert!(out[1] > 0.9, "{:?}", &out[..8]);
        assert!(out[150] > 0.99, "{}", out[150]);
    }

    /// Until a resampler for exactly this rate pair is parked, the callback interpolates linearly.
    #[test]
    fn a_stale_or_missing_resampler_falls_back_to_linear() {
        let ramp: Vec<f32> = (0..1000).map(|i| i as f32).collect();
        let state = doc_of(AudioBuffer { sample_rate: 24_000, channels: vec![ramp] });
        let shared = shared_with(&state);
        // Built for a 44.1 kHz device, played on a 48 kHz one.
        shared.device_rate.store(44_100, Ordering::Relaxed);
        rebuild_resampler(&shared);
        play(&shared, &state, 0.0, 1000.0);
        let out = render_run(&shared, 8, 48_000.0);
        assert_eq!(out[..8], [0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5]);
    }

    #[test]
    fn a_resampler_wider_than_the_ring_margin_is_not_parked() {
        let state = doc_of(AudioBuffer { sample_rate: 384_000, channels: vec![vec![0.0; 100]] });
        let shared = shared_with(&state);
        shared.device_rate.store(8_000, Ordering::Relaxed);
        rebuild_resampler(&shared);
        assert!(shared.resampler.lock().unwrap().is_none());
    }

    #[test]
    fn a_busy_or_empty_buffer_is_silence() {
        let state = doc_of(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.5; 100]] });
        let shared = shared_with(&state);
        play(&shared, &state, 0.0, 100.0);
        let mut out = vec![1.0f32; 16];
        {
            let _held = shared.state.lock().unwrap();
            render(&shared, &mut out, 2, 48_000.0);
        }
        assert!(out.iter().all(|v| *v == 0.0), "the state is held: silence, no blocking");
        shared.channels.store(0, Ordering::Relaxed);
        render(&shared, &mut out, 2, 48_000.0);
        assert!(out.iter().all(|v| *v == 0.0), "no document: silence");
    }

    fn rack_of(slots: &[(&str, Option<Vec<f64>>)]) -> RackConfig {
        let mut c = RackConfig::new();
        for (i, (id, p)) in slots.iter().enumerate() {
            c.set_slot(i, id, p.clone()).unwrap();
        }
        c
    }

    #[test]
    fn a_rack_parked_in_the_mailbox_processes_what_plays() {
        let state = doc_of(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.5; 5000], vec![-0.25; 5000]] });
        let shared = shared_with(&state);
        let mut local = None;
        // Nothing parked: the callback keeps no rack.
        take_rack(&shared, &mut local);
        assert!(local.is_none());
        // Park Invert + Gain -6.0206 dB (two slots), as Player::set_rack does.
        let cfg = rack_of(&[("invert", None), ("gain", Some(vec![-6.0206]))]);
        *shared.rack_mailbox.lock().unwrap() = Some(Box::new(RackProcessor::new(&cfg, 2, 48_000).unwrap()));
        shared.rack_new.store(true, Ordering::Release);
        // A busy mailbox is skipped, not waited for.
        {
            let _held = shared.rack_mailbox.lock().unwrap();
            take_rack(&shared, &mut local);
            assert!(local.is_none() && shared.rack_new.load(Ordering::Acquire));
        }
        take_rack(&shared, &mut local);
        assert!(local.is_some() && !shared.rack_new.load(Ordering::Acquire));
        // 3000 frames: more than one rack chunk.
        play(&shared, &state, 0.0, 5000.0);
        let mut out = vec![0.0f32; 6000];
        render(&shared, &mut out, 2, 48_000.0);
        run_rack(local.as_mut().unwrap(), &mut out, 2);
        for f in [0usize, 1, 1023, 1024, 2999] {
            assert!((out[2 * f] + 0.25).abs() < 1e-4 && (out[2 * f + 1] - 0.125).abs() < 1e-4, "frame {f}: {} {}", out[2 * f], out[2 * f + 1]);
        }
        // Swapping in "no rack" leaves the old one in the mailbox for the UI to drop.
        shared.rack_new.store(true, Ordering::Release);
        take_rack(&shared, &mut local);
        assert!(local.is_none() && shared.rack_mailbox.lock().unwrap().is_some());
    }

    #[test]
    fn the_rack_runs_on_integer_output_and_leaves_extra_channels_alone() {
        let cfg = rack_of(&[("invert", None)]);
        let mut rack = RackProcessor::new(&cfg, 2, 48_000).unwrap();
        let mut out = vec![0i16; 4 * 3];
        for f in 0..3 {
            out[4 * f] = 8192;
            out[4 * f + 1] = -16384;
            out[4 * f + 2] = 1234;
        }
        run_rack(&mut rack, &mut out, 4);
        for f in 0..3 {
            assert!((i32::from(out[4 * f]) + 8192).abs() <= 1 && (i32::from(out[4 * f + 1]) - 16384).abs() <= 1, "{out:?}");
            assert_eq!((out[4 * f + 2], out[4 * f + 3]), (1234, 0), "channels 3 and 4 are not the rack's");
        }
    }

    /// Start `plan` the way `Player::start` / `Player::grain` and the read-ahead thread do.
    fn play_plan(shared: &Shared, state: &DocState, plan: &Plan, from: f64, rev: bool, speed: f64, grain: bool) {
        let generation = shared.start_plan(*plan, from, rev, speed, grain);
        let feed = begin(shared, state, generation);
        fill_all(shared, &feed);
    }

    fn ramp_state(rate: u32, n: usize) -> DocState {
        doc_of(AudioBuffer { sample_rate: rate, channels: vec![(0..n).map(|i| i as f32).collect()] })
    }

    fn left(out: &[f32]) -> Vec<f32> {
        out.chunks(2).map(|f| f[0]).collect()
    }

    #[test]
    fn a_loop_goes_round_through_the_ring() {
        let state = ramp_state(48_000, 100);
        let shared = shared_with(&state);
        play_plan(&shared, &state, &Plan { looping: true, ..Plan::new(10.0, 14.0) }, 12.0, false, 1.0, false);
        let mut out = vec![0.0f32; 2 * 10];
        render(&shared, &mut out, 2, 48_000.0);
        assert_eq!(left(&out), [12.0, 13.0, 10.0, 11.0, 12.0, 13.0, 10.0, 11.0, 12.0, 13.0]);
        assert!(playback(&shared).playing, "a loop does not stop");
        assert_eq!(playback(&shared).wraps(), 3, "two wraps inside the buffer and one at its end");
        assert_eq!(playback(&shared).file_position(), 10.0);
    }

    #[test]
    fn the_skipped_part_is_not_heard() {
        let state = ramp_state(48_000, 100);
        let shared = shared_with(&state);
        let plan = Plan { skip_start: 5.0, skip_end: 8.0, ..Plan::new(2.0, 11.0) };
        play_plan(&shared, &state, &plan, 2.0, false, 1.0, false);
        let mut out = vec![0.0f32; 2 * 10];
        render(&shared, &mut out, 2, 48_000.0);
        assert_eq!(left(&out), [2.0, 3.0, 4.0, 8.0, 9.0, 10.0, 0.0, 0.0, 0.0, 0.0]);
        assert!(!playback(&shared).playing);
    }

    #[test]
    fn backward_playback_reads_the_file_in_reverse() {
        let state = ramp_state(48_000, 100);
        let shared = shared_with(&state);
        play_plan(&shared, &state, &Plan::new(10.0, 20.0), 19.0, true, 1.0, false);
        let mut out = vec![0.0f32; 2 * 14];
        render(&shared, &mut out, 2, 48_000.0);
        let l = left(&out);
        assert_eq!(&l[..10], [19.0, 18.0, 17.0, 16.0, 15.0, 14.0, 13.0, 12.0, 11.0, 10.0]);
        assert!(l[10..].iter().all(|v| *v == 0.0), "then silence at the start");
        assert!(!playback(&shared).playing, "stopped at the start");
        assert_eq!(playback(&shared).file_position(), 10.0);
    }

    #[test]
    fn a_backward_loop_wraps_to_the_end() {
        let state = ramp_state(48_000, 100);
        let shared = shared_with(&state);
        play_plan(&shared, &state, &Plan { looping: true, ..Plan::new(10.0, 14.0) }, 11.0, true, 1.0, false);
        let mut out = vec![0.0f32; 2 * 6];
        render(&shared, &mut out, 2, 48_000.0);
        assert_eq!(left(&out), [11.0, 10.0, 13.0, 12.0, 11.0, 10.0]);
        assert!(playback(&shared).playing);
    }

    #[test]
    fn double_speed_skips_every_other_frame_and_a_half_speed_interpolates() {
        let state = ramp_state(48_000, 100);
        let shared = shared_with(&state);
        play_plan(&shared, &state, &Plan::new(0.0, 100.0), 0.0, false, 2.0, false);
        let mut out = vec![0.0f32; 2 * 4];
        render(&shared, &mut out, 2, 48_000.0);
        assert_eq!(left(&out), [0.0, 2.0, 4.0, 6.0]);
        play_plan(&shared, &state, &Plan::new(0.0, 100.0), 0.0, false, 0.5, false);
        render(&shared, &mut out, 2, 48_000.0);
        assert_eq!(left(&out), [0.0, 0.5, 1.0, 1.5]);
    }

    #[test]
    fn a_grain_fades_in_and_out() {
        let state = doc_of(AudioBuffer { sample_rate: 1000, channels: vec![vec![1.0; 2000]] });
        let shared = shared_with(&state);
        play_plan(&shared, &state, &Plan::new(100.0, 400.0), 100.0, false, 1.0, true);
        let mut out = vec![0.0f32; 2 * 300];
        render(&shared, &mut out, 2, 1000.0);
        let l = left(&out);
        assert_eq!(l[0], 0.0, "starts silent");
        assert!(l[3] > 0.0 && l[3] < 1.0, "fading in: {}", l[3]);
        assert_eq!(l[150], 1.0);
        assert!(l[298] < 1.0, "fading out");
    }

    /// Pause keeps the place; resume carries on from it (the silent test sink moves the position).
    #[test]
    fn pause_keeps_the_place_and_resume_carries_on() {
        let mut p = Player::new(OutputChoice::default(), ChannelMap::default(), Quality::default());
        // The silent sink stands in for the device (what `WAVCRAFT_NO_AUDIO=1` does at start-up).
        p.stream = None;
        let sink = Sink::start(&p.shared);
        p.out_rate = sink.rate;
        p.out_channels = SINK_CHANNELS;
        p.shared.device_rate.store(sink.rate, Ordering::Release);
        p.sink = Some(sink);
        let _ = p.commands.send(Command::Rebuild);
        assert!(p.has_output());
        p.set_audio(Some(ramp_state(48_000, 480_000)));
        assert!(p.start(&Plan::new(0.0, 480_000.0), 1000.0));
        std::thread::sleep(Duration::from_millis(150));
        let moved = p.position();
        assert!(p.is_playing() && moved > 1000, "the sink moves it: {moved}");
        p.pause();
        let held = p.position();
        assert!(!p.is_playing() && held >= moved);
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(p.position(), held, "paused: still");
        assert!(p.resume());
        std::thread::sleep(Duration::from_millis(150));
        assert!(p.position() > held, "resumed from the same place");
        p.stop();
        assert!(!p.is_playing());
        assert!(!p.resume(), "nothing to resume after a stop");
    }
}
