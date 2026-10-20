//! Playback of the open file through the default output device.
//!
//! Three parts, so that the audio callback does almost nothing:
//!
//! - **Producer** (a worker thread, not real-time): renders the play range into the device's
//!   rate (windowed-sinc resampling, `wavcraft_core::resample`; an exact copy when the rates
//!   match) and device channel layout, and pushes it into a ring buffer of about 0.4 s.
//! - **Ring** (`Mutex<Ring>`, filled by the producer, drained by the callback): the callback
//!   `try_lock`s it, so it never waits; if the producer holds it for that instant, or has not
//!   kept up, that buffer is silence and counted as an underrun.
//! - **Callback** ([`consume`]): copies frames out of the ring and converts the sample format.
//!   It never resamples, allocates or blocks.
//!
//! The playhead stays in **document frames**: the callback counts device frames taken from the
//! ring, and the position is `start + taken * doc_rate / device_rate`, recomputed from the
//! integer count each time, so it cannot drift and it only moves for audio that was really played.
//!
//! With `WAVCRAFT_NO_AUDIO=1` there is no sound card: a thread plays the part of the callback at
//! real-time speed into nowhere (a "test sink", 48 kHz stereo unless `WAVCRAFT_NO_AUDIO_RATE`
//! says another rate), so playback can be checked on a machine without audio.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use wavcraft_core::AudioBuffer;
use wavcraft_core::resample::{Quality, Resampler};

/// Environment variable: play into a silent test sink instead of the sound card.
pub const ENV_NO_AUDIO: &str = "WAVCRAFT_NO_AUDIO";
/// Environment variable: the test sink's rate in Hz (default 48,000).
pub const ENV_NO_AUDIO_RATE: &str = "WAVCRAFT_NO_AUDIO_RATE";
/// How much audio the ring holds, in milliseconds of device time.
const RING_MS: u32 = 400;
/// Device frames the producer renders at a time.
const CHUNK_FRAMES: usize = 1024;

/// The ring buffer and the play session it belongs to.
struct Ring {
    /// Interleaved device-rate samples waiting for the callback.
    buf: VecDeque<f32>,
    /// Bumped by every play and stop; the producer drops work for an older one.
    session: u64,
    /// Whether a play is under way (the producer works only while it is).
    active: bool,
    /// Document frame under device frame 0 of this session.
    start: f64,
    /// Document frame where the session ends.
    end: f64,
    /// Document frames per device frame.
    ratio: f64,
    /// Device frames the callback has taken since the session began.
    consumed: u64,
    /// Device frames the whole session has, once the producer knows it has pushed the last.
    total: Option<u64>,
}

struct Shared {
    audio: Mutex<Option<Arc<AudioBuffer>>>,
    ring: Mutex<Ring>,
    /// Play position in document frames, an `f64` stored as bits.
    pos: AtomicU64,
    playing: AtomicBool,
    /// Set by the stream's error callback (the device went away, for example).
    failed: AtomicBool,
    /// Tells the worker threads to end.
    quit: AtomicBool,
    /// `Quality` as 0 (fast) or 1 (high).
    quality: AtomicU8,
    device_rate: AtomicU32,
    device_channels: AtomicU32,
    /// Callbacks that wanted audio the ring did not have.
    underruns: AtomicU64,
}

impl Shared {
    fn new(quality: Quality) -> Shared {
        Shared {
            audio: Mutex::new(None),
            ring: Mutex::new(Ring { buf: VecDeque::new(), session: 0, active: false, start: 0.0, end: 0.0, ratio: 1.0, consumed: 0, total: None }),
            pos: AtomicU64::new(0f64.to_bits()),
            playing: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            quit: AtomicBool::new(false),
            quality: AtomicU8::new(quality_code(quality)),
            device_rate: AtomicU32::new(48_000),
            device_channels: AtomicU32::new(2),
            underruns: AtomicU64::new(0),
        }
    }

    fn set_device(&self, rate: u32, channels: usize) {
        self.device_rate.store(rate.max(1), Ordering::Relaxed);
        self.device_channels.store(channels.max(1) as u32, Ordering::Relaxed);
        if let Ok(mut ring) = self.ring.lock() {
            ring.buf = VecDeque::with_capacity((rate.max(1) as usize * RING_MS as usize / 1000 + CHUNK_FRAMES) * channels.max(1));
        }
    }

    fn quality(&self) -> Quality {
        if self.quality.load(Ordering::Relaxed) == 0 { Quality::Fast } else { Quality::High }
    }

    /// Ring capacity in samples.
    fn capacity(&self) -> usize {
        let rate = self.device_rate.load(Ordering::Relaxed) as usize;
        let ch = self.device_channels.load(Ordering::Relaxed) as usize;
        (rate * RING_MS as usize / 1000).max(CHUNK_FRAMES) * ch
    }
}

fn quality_code(q: Quality) -> u8 {
    u8::from(q == Quality::High)
}

/// What the control channel and the status bar report about playback.
#[derive(Debug, Clone)]
pub struct PlaybackInfo {
    pub device_rate: u32,
    /// The open file's rate (0 with no file).
    pub doc_rate: u32,
    /// Whether the file's rate differs from the device's, so the resampler is converting.
    pub resampling: bool,
    pub quality: Quality,
    pub playing: bool,
    /// The playhead in document frames.
    pub position: usize,
    pub underruns: u64,
    pub test_sink: bool,
}

/// The output stream and the play state shared with its callback.
pub struct Player {
    shared: Arc<Shared>,
    stream: Option<cpal::Stream>,
    /// True while the test sink (`WAVCRAFT_NO_AUDIO`) stands in for a device.
    sink: bool,
    /// Why there is no sound, if the device could not be opened.
    pub error: Option<String>,
    /// The output device's name, for the status bar.
    pub device: String,
}

impl Drop for Player {
    fn drop(&mut self) {
        self.shared.quit.store(true, Ordering::Relaxed);
    }
}

fn no_audio_requested() -> bool {
    std::env::var(ENV_NO_AUDIO).is_ok_and(|v| !matches!(v.trim(), "" | "0" | "false"))
}

impl Player {
    /// Open the default output device. Without one the player still works, silently.
    pub fn new(quality: Quality) -> Player {
        let shared = Arc::new(Shared::new(quality));
        let worker = Arc::clone(&shared);
        let _ = std::thread::Builder::new().name("playback producer".into()).spawn(move || {
            let mut producer = Producer::default();
            while !worker.quit.load(Ordering::Relaxed) {
                if !producer.step(&worker) {
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        });
        let mut player = Player { shared, stream: None, sink: false, error: None, device: String::new() };
        player.reopen();
        player
    }

    /// (Re)open the default output device, replacing any stream there was.
    fn reopen(&mut self) {
        self.stream = None;
        self.shared.failed.store(false, Ordering::Relaxed);
        if no_audio_requested() {
            if !self.sink {
                let rate = std::env::var(ENV_NO_AUDIO_RATE)
                    .ok()
                    .and_then(|v| v.trim().parse::<u32>().ok())
                    .filter(|r| (1_000..=768_000).contains(r))
                    .unwrap_or(48_000);
                self.shared.set_device(rate, 2);
                start_sink(&self.shared, rate, 2);
                self.sink = true;
            }
            self.device = format!("test sink ({} Hz, no audio)", self.shared.device_rate.load(Ordering::Relaxed));
            self.error = None;
            return;
        }
        match open_stream(&self.shared) {
            Ok((stream, device)) => {
                self.stream = Some(stream);
                self.device = device;
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn has_output(&self) -> bool {
        self.sink || self.stream.is_some()
    }

    /// Whether the output stream has reported an error since it was opened.
    pub fn failed(&self) -> bool {
        self.shared.failed.load(Ordering::Relaxed)
    }

    /// Hand over the audio to play (stops playback).
    pub fn set_audio(&self, audio: Option<Arc<AudioBuffer>>) {
        self.stop();
        if let Ok(mut slot) = self.shared.audio.lock() {
            *slot = audio;
        }
    }

    /// Set the sample-rate conversion quality; it applies from the next play.
    pub fn set_quality(&self, quality: Quality) {
        self.shared.quality.store(quality_code(quality), Ordering::Relaxed);
    }

    /// Play `[from, to)` in file frames. Without a working output (none at start-up, or one that
    /// has since failed) the device is tried again first; if there still is none, nothing starts,
    /// so the playhead does not sit there "playing" with no callback to move it.
    pub fn play(&mut self, from: usize, to: usize) {
        if !self.has_output() || self.failed() {
            self.reopen();
        }
        if !self.has_output() {
            return;
        }
        start_session(&self.shared, from, to);
    }

    pub fn stop(&self) {
        self.shared.playing.store(false, Ordering::Relaxed);
        if let Ok(mut ring) = self.shared.ring.lock() {
            ring.session += 1;
            ring.active = false;
            ring.buf.clear();
        }
    }

    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }

    /// The frame being played right now.
    pub fn position(&self) -> usize {
        f64::from_bits(self.shared.pos.load(Ordering::Relaxed)).max(0.0) as usize
    }

    /// Device rate, file rate, whether the resampler is converting, and so on.
    pub fn info(&self) -> PlaybackInfo {
        let doc_rate = self.shared.audio.lock().ok().and_then(|a| a.as_ref().map(|a| a.sample_rate)).unwrap_or(0);
        let device_rate = self.shared.device_rate.load(Ordering::Relaxed);
        PlaybackInfo {
            device_rate,
            doc_rate,
            resampling: doc_rate != 0 && doc_rate != device_rate,
            quality: self.shared.quality(),
            playing: self.is_playing(),
            position: self.position(),
            underruns: self.shared.underruns.load(Ordering::Relaxed),
            test_sink: self.sink,
        }
    }
}

/// Begin a play session of `[from, to)` document frames (clamped to the audio).
fn start_session(shared: &Shared, from: usize, to: usize) {
    let Some(audio) = shared.audio.lock().ok().and_then(|a| a.clone()) else { return };
    if audio.sample_rate == 0 {
        return;
    }
    let to = (to as f64).min(audio.frames() as f64);
    let from = from as f64;
    let active = to > from;
    let device_rate = f64::from(shared.device_rate.load(Ordering::Relaxed).max(1));
    if let Ok(mut ring) = shared.ring.lock() {
        ring.session += 1;
        ring.buf.clear();
        ring.active = active;
        ring.start = from;
        ring.end = to;
        ring.ratio = f64::from(audio.sample_rate) / device_rate;
        ring.consumed = 0;
        ring.total = None;
    }
    shared.pos.store(from.to_bits(), Ordering::Relaxed);
    shared.playing.store(active, Ordering::Relaxed);
}

/// The worker that renders into the ring.
#[derive(Default)]
struct Producer {
    session: u64,
    audio: Option<Arc<AudioBuffer>>,
    resampler: Option<Resampler>,
    start: f64,
    /// Device frames pushed so far in this session, and how many the session has.
    pushed: u64,
    total: u64,
    done: bool,
    scratch: Vec<f32>,
    frame: Vec<f32>,
}

impl Producer {
    /// Do one chunk of work if the ring has room. Returns whether it did any.
    fn step(&mut self, shared: &Shared) -> bool {
        let (session, active, start, end, used) = {
            let Ok(ring) = shared.ring.lock() else { return false };
            (ring.session, ring.active, ring.start, ring.end, ring.buf.len())
        };
        if session != self.session {
            self.session = session;
            self.done = !active;
            self.pushed = 0;
            self.start = start;
            self.audio = shared.audio.lock().ok().and_then(|a| a.clone());
            self.resampler = self.audio.as_ref().map(|a| Resampler::new(a.sample_rate, shared.device_rate.load(Ordering::Relaxed), shared.quality()));
            self.total = self.resampler.as_ref().map_or(0, |r| r.out_frames(start, end));
        }
        if self.done {
            return false;
        }
        let channels = shared.device_channels.load(Ordering::Relaxed) as usize;
        let room = shared.capacity().saturating_sub(used) / channels.max(1);
        let count = (self.total - self.pushed).min(CHUNK_FRAMES as u64) as usize;
        if count == 0 || room < count {
            if count == 0 {
                self.finish(shared);
            }
            return count == 0;
        }
        let (Some(audio), Some(rs)) = (self.audio.as_ref(), self.resampler.as_ref()) else {
            self.done = true;
            return false;
        };
        render_device(audio, rs, self.start, self.pushed, count, channels, &mut self.frame, &mut self.scratch);
        let Ok(mut ring) = shared.ring.lock() else { return false };
        if ring.session != self.session {
            return true; // stopped or restarted while rendering: drop this chunk
        }
        ring.buf.extend(self.scratch.iter().copied());
        self.pushed += count as u64;
        if self.pushed >= self.total {
            ring.total = Some(self.total);
            self.done = true;
        }
        true
    }

    fn finish(&mut self, shared: &Shared) {
        if let Ok(mut ring) = shared.ring.lock()
            && ring.session == self.session
        {
            ring.total = Some(self.pushed);
        }
        self.done = true;
    }
}

/// Render `count` device frames, from frame `first` of the run that starts at document frame
/// `start`, into `out` (interleaved, `channels` per frame). Channel layout: a mono file plays on
/// the first two device channels; a one-channel device gets the average of the file's channels;
/// otherwise file channel `c` goes to device channel `c` and further device channels are silent.
#[allow(clippy::too_many_arguments)]
fn render_device(
    audio: &AudioBuffer,
    rs: &Resampler,
    start: f64,
    first: u64,
    count: usize,
    channels: usize,
    per_file: &mut Vec<f32>,
    out: &mut Vec<f32>,
) {
    let file_channels = audio.num_channels();
    out.clear();
    out.resize(count * channels, 0.0);
    per_file.clear();
    per_file.resize(file_channels, 0.0);
    if file_channels == 0 {
        return;
    }
    for i in 0..count {
        let x = rs.doc_position(start, first + i as u64);
        for (c, v) in per_file.iter_mut().enumerate() {
            *v = rs.sample_at(&audio.channels[c], x);
        }
        let frame = &mut out[i * channels..(i + 1) * channels];
        if channels == 1 {
            // A one-channel device gets the average of the file's channels, not just the left one.
            frame[0] = per_file.iter().sum::<f32>() / file_channels as f32;
        } else {
            for (c, slot) in frame.iter_mut().enumerate() {
                *slot = if file_channels == 1 { if c < 2 { per_file[0] } else { 0.0 } } else { per_file.get(c).copied().unwrap_or(0.0) };
            }
        }
    }
}

/// Fill one device buffer from the ring. Runs on the audio thread: no allocation, no waiting.
fn consume<T: cpal::Sample + cpal::FromSample<f32>>(shared: &Shared, out: &mut [T], channels: usize) {
    let silence = T::from_sample(0.0f32);
    out.iter_mut().for_each(|s| *s = silence);
    if !shared.playing.load(Ordering::Relaxed) || channels == 0 {
        return;
    }
    let Ok(mut ring) = shared.ring.try_lock() else {
        shared.underruns.fetch_add(1, Ordering::Relaxed);
        return;
    };
    if !ring.active {
        return;
    }
    let wanted = out.len() / channels;
    let take = wanted.min(ring.buf.len() / channels);
    for slot in out.iter_mut().take(take * channels) {
        *slot = T::from_sample(ring.buf.pop_front().unwrap_or(0.0));
    }
    ring.consumed += take as u64;
    let pos = ring.start + ring.consumed as f64 * ring.ratio;
    if ring.total.is_some_and(|t| ring.consumed >= t) {
        ring.active = false;
        shared.playing.store(false, Ordering::Relaxed);
        shared.pos.store(ring.end.to_bits(), Ordering::Relaxed);
    } else {
        shared.pos.store(pos.to_bits(), Ordering::Relaxed);
        if take < wanted {
            shared.underruns.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// The test sink: takes audio from the ring at real-time speed and discards it.
fn start_sink(shared: &Arc<Shared>, rate: u32, channels: usize) {
    let shared = Arc::clone(shared);
    let _ = std::thread::Builder::new().name("test sink".into()).spawn(move || {
        let mut buf = vec![0.0f32; 4096 * channels];
        let (t0, mut taken) = (Instant::now(), 0u64);
        while !shared.quit.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(5));
            let due = (t0.elapsed().as_secs_f64() * f64::from(rate)) as u64;
            while taken < due {
                let n = (due - taken).min(4096) as usize;
                consume(&shared, &mut buf[..n * channels], channels);
                taken += n as u64;
            }
        }
    });
}

fn open_stream(shared: &Arc<Shared>) -> Result<(cpal::Stream, String), String> {
    let device = cpal::default_host().default_output_device().ok_or("no audio output device")?;
    let name = device.name().unwrap_or_else(|_| "output".to_string());
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let config: cpal::StreamConfig = supported.clone().into();
    shared.set_device(config.sample_rate.0, usize::from(config.channels));
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(&device, &config, shared),
        cpal::SampleFormat::I16 => build::<i16>(&device, &config, shared),
        cpal::SampleFormat::U16 => build::<u16>(&device, &config, shared),
        cpal::SampleFormat::I32 => build::<i32>(&device, &config, shared),
        other => Err(format!("unsupported output sample format {other:?}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, name))
}

fn build<T>(device: &cpal::Device, config: &cpal::StreamConfig, shared: &Arc<Shared>) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let on_error = Arc::clone(shared);
    let shared = Arc::clone(shared);
    let channels = usize::from(config.channels).max(1);
    device
        .build_output_stream(
            config,
            move |out: &mut [T], _: &cpal::OutputCallbackInfo| consume(&shared, out, channels),
            move |err| {
                log::warn!("audio output: {err}");
                on_error.failed.store(true, Ordering::Relaxed);
                on_error.playing.store(false, Ordering::Relaxed);
            },
            None,
        )
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    /// A player without threads or a device: the test plays producer and callback by hand.
    struct Rig {
        shared: Shared,
        producer: Producer,
    }

    impl Rig {
        fn new(audio: AudioBuffer, device_rate: u32, channels: usize) -> Rig {
            let shared = Shared::new(Quality::High);
            shared.set_device(device_rate, channels);
            *shared.audio.lock().unwrap() = Some(Arc::new(audio));
            Rig { shared, producer: Producer::default() }
        }

        fn play(&mut self, from: usize, to: usize) {
            start_session(&self.shared, from, to);
            while self.producer.step(&self.shared) {}
        }

        fn pump(&mut self) {
            while self.producer.step(&self.shared) {}
        }

        fn pos(&self) -> f64 {
            f64::from_bits(self.shared.pos.load(Ordering::Relaxed))
        }

        fn playing(&self) -> bool {
            self.shared.playing.load(Ordering::Relaxed)
        }
    }

    fn sine(freq: f64, rate: u32, frames: usize) -> Vec<f32> {
        (0..frames).map(|i| (2.0 * PI * freq * i as f64 / f64::from(rate)).sin() as f32).collect()
    }

    #[test]
    fn plays_the_range_then_stops_with_silence() {
        let ramp: Vec<f32> = (0..100).map(|i| i as f32 / 100.0).collect();
        let mut rig = Rig::new(AudioBuffer { sample_rate: 48_000, channels: vec![ramp.clone(), ramp.iter().map(|v| -v).collect()] }, 48_000, 2);
        let mut out = vec![9.0f32; 40];
        consume(&rig.shared, &mut out, 2);
        assert!(out.iter().all(|v| *v == 0.0), "stopped: silence");
        rig.play(10, 20);
        consume(&rig.shared, &mut out, 2);
        // Frames 10..20, left and right, then silence for the rest of the buffer; equal rates copy exactly.
        for k in 0..10 {
            assert_eq!((out[2 * k], out[2 * k + 1]), (ramp[10 + k], -ramp[10 + k]), "frame {k}");
        }
        assert!(out[20..].iter().all(|v| *v == 0.0));
        assert!(!rig.playing(), "stops at the end of the range");
        assert_eq!(rig.pos(), 20.0);
    }

    #[test]
    fn mono_plays_on_both_speakers_and_the_position_follows_the_rate() {
        let mut rig = Rig::new(AudioBuffer { sample_rate: 24_000, channels: vec![vec![0.5; 1000]] }, 48_000, 4);
        rig.play(100, 900);
        // A 48 kHz device plays a 24 kHz file at half a file frame per device frame, on 4 channels.
        let mut out = vec![0.0f32; 4 * 8];
        consume(&rig.shared, &mut out, 4);
        for k in 0..8 {
            assert!((out[4 * k] - 0.5).abs() < 1e-3 && (out[4 * k + 1] - 0.5).abs() < 1e-3, "frame {k}: {:?}", &out[4 * k..4 * k + 4]);
            assert_eq!(out[4 * k + 2..4 * k + 4], [0.0, 0.0]);
        }
        assert_eq!(rig.pos(), 104.0, "8 device frames at 0.5 document frames each");
        // Integer formats convert, and the end of the range is clamped to the file.
        let mut rig = Rig::new(AudioBuffer { sample_rate: 24_000, channels: vec![vec![0.5; 1000]] }, 48_000, 1);
        rig.play(998, 1_000_000);
        let mut ints = vec![7i16; 8];
        consume(&rig.shared, &mut ints, 1);
        assert!(ints[0] != 0 || ints[1] != 0, "{ints:?}");
        assert!(ints[4..].iter().all(|v| *v == 0), "two document frames are four device frames: {ints:?}");
        assert!(!rig.playing());
        assert_eq!(rig.pos(), 1000.0);
    }

    /// Regression: a one-channel device played only the file's left channel.
    #[test]
    fn a_mono_device_gets_every_channel_of_the_file() {
        let mut rig = Rig::new(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.0; 100], vec![0.5; 100]] }, 48_000, 1);
        rig.play(0, 100);
        let mut out = vec![9.0f32; 16];
        consume(&rig.shared, &mut out, 1);
        assert!(out.iter().all(|v| *v == 0.25), "left silent, right at 0.5: {out:?}");
    }

    #[test]
    fn a_busy_ring_is_silence_and_the_position_waits() {
        let mut rig = Rig::new(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.5; 1000]] }, 48_000, 2);
        rig.play(0, 1000);
        let mut out = vec![1.0f32; 16];
        {
            let _held = rig.shared.ring.lock().unwrap();
            consume(&rig.shared, &mut out, 2);
        }
        assert!(out.iter().all(|v| *v == 0.0), "the producer held the ring: silence, no blocking");
        assert_eq!((rig.pos(), rig.shared.underruns.load(Ordering::Relaxed)), (0.0, 1));
        consume(&rig.shared, &mut out, 2);
        assert!(out.iter().all(|v| *v == 0.5) && rig.pos() == 8.0, "{out:?}");
    }

    #[test]
    fn an_unfilled_ring_underruns_without_moving_the_playhead() {
        let mut rig = Rig::new(AudioBuffer { sample_rate: 44_100, channels: vec![vec![0.5; 1000]] }, 48_000, 2);
        start_session(&rig.shared, 500, 1000); // the producer has not run
        let mut out = vec![1.0f32; 16];
        consume(&rig.shared, &mut out, 2);
        assert!(out.iter().all(|v| *v == 0.0));
        assert_eq!((rig.pos(), rig.shared.underruns.load(Ordering::Relaxed)), (500.0, 1));
        rig.pump();
        consume(&rig.shared, &mut out, 2);
        assert!(out.iter().all(|v| (*v - 0.5).abs() < 1e-3));
    }

    #[test]
    fn a_stopped_session_is_not_refilled() {
        let mut rig = Rig::new(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.5; 5000]] }, 48_000, 2);
        rig.play(0, 5000);
        assert!(!rig.shared.ring.lock().unwrap().buf.is_empty());
        rig.shared.playing.store(false, Ordering::Relaxed);
        {
            let mut ring = rig.shared.ring.lock().unwrap();
            ring.session += 1;
            ring.active = false;
            ring.buf.clear();
        }
        rig.pump();
        assert!(rig.shared.ring.lock().unwrap().buf.is_empty(), "the producer must not refill a stopped session");
    }

    /// Measured: a 1 kHz 44.1 kHz file through a 48 kHz device keeps 1 kHz, the playhead (in
    /// document frames) advances 44,100 per second of device time, and a longer run does not drift.
    #[test]
    fn a_44k_file_on_a_48k_device_keeps_its_pitch_and_position() {
        let frames = 44_100 * 30;
        let tone = sine(1_000.0, 44_100, frames);
        let mut rig = Rig::new(AudioBuffer { sample_rate: 44_100, channels: vec![tone.clone(), tone] }, 48_000, 2);
        rig.play(0, frames);
        let mut left = Vec::new();
        let mut buf = vec![0.0f32; 480 * 2];
        let mut checkpoints = Vec::new();
        for call in 1..=(100 * 30) {
            rig.pump();
            consume(&rig.shared, &mut buf, 2);
            left.extend(buf.chunks(2).map(|f| f[0]));
            if call == 100 || call == 1_000 {
                checkpoints.push(rig.pos());
            }
        }
        assert_eq!(checkpoints[0], 44_100.0, "1 s of device time is 44,100 document frames");
        assert_eq!(checkpoints[1], 441_000.0, "10 s: no drift");
        assert_eq!(left.len(), 48_000 * 30);
        // Frequency by rising zero crossings over the middle 20 s.
        let mid = &left[5 * 48_000..25 * 48_000];
        let mut crossings = Vec::new();
        for i in 1..mid.len() {
            if mid[i - 1] < 0.0 && mid[i] >= 0.0 {
                crossings.push(i as f64 - 1.0 + f64::from(-mid[i - 1]) / f64::from(mid[i] - mid[i - 1]));
            }
        }
        let n = crossings.len();
        let hz = (n - 1) as f64 * 48_000.0 / (crossings[n - 1] - crossings[0]);
        assert!((hz - 1_000.0).abs() < 0.01, "measured {hz} Hz");
        assert_eq!(rig.shared.underruns.load(Ordering::Relaxed), 0);
        // Near the end the playhead lands on the end of the range.
        for _ in 0..50 {
            rig.pump();
            consume(&rig.shared, &mut buf, 2);
        }
        assert!(!rig.playing());
        assert_eq!(rig.pos(), frames as f64);
    }
}
