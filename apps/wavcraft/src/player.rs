//! Playback of the open file through the chosen output device (the system default unless the
//! preferences name one).
//!
//! The audio callback never blocks or allocates: it `try_lock`s the shared buffer and plays
//! silence if the UI holds it. The file's rate is converted to the device's with linear
//! interpolation, which is fine for auditioning (an edit never goes through this path).

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use cpal::Sample as _;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use wavcraft_core::AudioBuffer;
use wavcraft_core::levels::LevelTap;
use wavcraft_core::rack::{MAX_BLOCK, RackConfig, RackProcessor};
use wavcraft_core::transport::Plan;

struct Shared {
    audio: Mutex<Option<Arc<AudioBuffer>>>,
    /// Play position in file frames, an `f64` stored as bits.
    pos: AtomicU64,
    /// Frame to stop at (or wrap at, in a loop), an `f64` stored as bits.
    end: AtomicU64,
    /// Frame a loop wraps to, an `f64` stored as bits.
    start: AtomicU64,
    /// The skipped part (`Plan::skip_start/skip_end`), `f64` bits; empty when nothing is skipped.
    skip_start: AtomicU64,
    skip_end: AtomicU64,
    looping: AtomicBool,
    /// Times a loop has gone round since the last `start`.
    wraps: AtomicU32,
    playing: AtomicBool,
    /// Nothing plays while this is set (a take is being recorded; roadmap C024).
    muted: AtomicBool,
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
}

/// The output stream and the play state shared with its callback.
pub struct Player {
    shared: Arc<Shared>,
    stream: Option<cpal::Stream>,
    /// Why there is no sound, if the device could not be opened.
    pub error: Option<String>,
    /// The output device's name, for the status bar.
    pub device: String,
    /// The device the preferences asked for (`None`: the system default).
    wanted: Option<String>,
    /// The file's sample rate, for the silent clock.
    rate: u32,
    /// Without an output stream the position still advances, by this wall clock, so the
    /// transport can be used and scripted with no sound device ("test mode"). `None` while the
    /// clock is not running.
    sim_last: Option<Instant>,
    /// `WAVCRAFT_NO_AUDIO` is set: never open a device (scripted runs).
    silent: bool,
    /// The output stream's channel count and sample rate (what a rack is built for).
    out_channels: usize,
    out_rate: u32,
}

impl Shared {
    fn new() -> Shared {
        let zero = || AtomicU64::new(0f64.to_bits());
        Shared {
            audio: Mutex::new(None),
            pos: zero(),
            end: zero(),
            start: zero(),
            skip_start: zero(),
            skip_end: zero(),
            looping: AtomicBool::new(false),
            wraps: AtomicU32::new(0),
            playing: AtomicBool::new(false),
            muted: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            levels: LevelTap::default(),
            rack_mailbox: Mutex::new(None),
            rack_new: AtomicBool::new(false),
        }
    }

    fn plan(&self) -> Plan {
        let get = |a: &AtomicU64| f64::from_bits(a.load(Ordering::Relaxed));
        Plan {
            start: get(&self.start),
            end: get(&self.end),
            looping: self.looping.load(Ordering::Relaxed),
            skip_start: get(&self.skip_start),
            skip_end: get(&self.skip_end),
        }
    }

    fn set_plan(&self, plan: &Plan) {
        let put = |a: &AtomicU64, v: f64| a.store(v.to_bits(), Ordering::Relaxed);
        put(&self.start, plan.start);
        put(&self.end, plan.end);
        put(&self.skip_start, plan.skip_start);
        put(&self.skip_end, plan.skip_end);
        self.looping.store(plan.looping, Ordering::Relaxed);
    }
}

impl Player {
    /// Open the output device named `wanted` (the system default for `None`, or when that name is
    /// not there). Without a device the player still works, silently.
    pub fn new(wanted: Option<String>) -> Player {
        let shared = Arc::new(Shared::new());
        let silent = std::env::var_os("WAVCRAFT_NO_AUDIO").is_some();
        let mut player = Player {
            shared,
            stream: None,
            error: None,
            device: String::new(),
            wanted,
            rate: 0,
            sim_last: None,
            silent,
            out_channels: 2,
            out_rate: 48_000,
        };
        player.reopen();
        player
    }

    /// Play through the device named `wanted` from now on (`None`: the system default): stops
    /// playback and reopens the output.
    pub fn set_output(&mut self, wanted: Option<String>) {
        self.stop();
        self.wanted = wanted;
        self.reopen();
    }

    /// (Re)open the wanted output device, replacing any stream there was.
    fn reopen(&mut self) {
        self.stream = None;
        if self.silent {
            self.error = Some("audio off (WAVCRAFT_NO_AUDIO)".to_string());
            return;
        }
        self.shared.failed.store(false, Ordering::Relaxed);
        match open_stream(&self.shared, self.wanted.as_deref()) {
            Ok((stream, device, channels, rate)) => {
                self.stream = Some(stream);
                self.device = device;
                self.out_channels = channels;
                self.out_rate = rate;
                self.error = None;
            }
            Err(e) => self.error = Some(e),
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

    /// Hand over the audio to play (stops playback).
    pub fn set_audio(&mut self, audio: Option<Arc<AudioBuffer>>) {
        self.stop();
        self.rate = audio.as_ref().map_or(0, |a| a.sample_rate);
        if let Ok(mut slot) = self.shared.audio.lock() {
            *slot = audio;
        }
    }

    /// Play `[from, to)` in file frames. Without a working output (none at start-up, or one that
    /// has since failed) the device is tried again first; if there still is none, nothing starts,
    /// so the playhead does not sit there "playing" with no callback to move it.
    pub fn play(&mut self, from: usize, to: usize) {
        if self.is_muted() {
            return;
        }
    }
    /// Play `plan` from frame `from`. Without a working output (none at start-up, or one that
    /// has since failed) the device is tried again first; if there still is none the position
    /// advances silently by the wall clock (see [`Player::tick`]). Returns whether it started.
    pub fn start(&mut self, plan: &Plan, from: f64) -> bool {
        if self.stream.is_none() || self.failed() {
            self.reopen();
        }
        let has_audio = self.shared.audio.lock().is_ok_and(|a| a.is_some());
        if !has_audio || plan.normalize(from).is_none() {
            return false;
        }
        self.shared.set_plan(plan);
        self.shared.wraps.store(0, Ordering::Relaxed);
        self.shared.pos.store(from.to_bits(), Ordering::Relaxed);
        self.begin();
        true
    }

    /// Mute or unmute: while muted, nothing starts and a running play stops (recording a take).
    pub fn set_muted(&self, muted: bool) {
        self.shared.muted.store(muted, Ordering::Relaxed);
        if muted {
            self.stop();
        }
    }

    pub fn is_muted(&self) -> bool {
        self.shared.muted.load(Ordering::Relaxed)
    }

    pub fn stop(&self) {}
    fn begin(&mut self) {
        self.sim_last = self.stream.is_none().then(Instant::now);
        self.shared.playing.store(true, Ordering::Relaxed);
    }

    /// Stop where it is, keeping the place and the plan for [`Player::resume`].
    pub fn pause(&mut self) {
        self.tick();
        self.shared.playing.store(false, Ordering::Relaxed);
    }

    /// Carry on from the place `pause` left. False when there is nothing to resume.
    pub fn resume(&mut self) -> bool {
        let plan = self.shared.plan();
        if plan.normalize(self.position_f64()).is_none() {
            return false;
        }
        self.begin();
        true
    }

    pub fn stop(&mut self) {
        self.shared.playing.store(false, Ordering::Relaxed);
        self.sim_last = None;
    }

    /// Loop on or off, taking effect at once while playing.
    pub fn set_looping(&self, on: bool) {
        self.shared.looping.store(on, Ordering::Relaxed);
    }

    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
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
    /// Whether the position is moved by the output device (false: the silent clock).
    pub fn has_output(&self) -> bool {
        self.stream.is_some()
    }

    /// Times the loop has gone round since playback started.
    pub fn wraps(&self) -> u32 {
        self.shared.wraps.load(Ordering::Relaxed)
    }

    /// Advance the silent clock (no output device). The window and the control channel call this
    /// before reading the position; with a device it does nothing.
    pub fn tick(&mut self) {
        let Some(last) = self.sim_last else { return };
        if self.stream.is_some() || !self.is_playing() {
            return;
        }
        let now = Instant::now();
        self.sim_last = Some(now);
        let delta = now.duration_since(last).as_secs_f64() * f64::from(self.rate);
        let plan = self.shared.plan();
        let mut wraps = self.shared.wraps.load(Ordering::Relaxed);
        match plan.advance(self.position_f64(), delta, &mut wraps) {
            Some(pos) => self.shared.pos.store(pos.to_bits(), Ordering::Relaxed),
            None => {
                self.shared.pos.store(plan.end.to_bits(), Ordering::Relaxed);
                self.shared.playing.store(false, Ordering::Relaxed);
            }
        }
        self.shared.wraps.store(wraps, Ordering::Relaxed);
    }

    fn position_f64(&self) -> f64 {
        f64::from_bits(self.shared.pos.load(Ordering::Relaxed)).max(0.0)
    }

    /// The frame being played right now.
    pub fn position(&self) -> usize {
        self.position_f64() as usize
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
pub fn list_devices_in_background() -> std::sync::mpsc::Receiver<Devices> {
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = std::thread::Builder::new().name("audio devices".into()).spawn(move || {
        let (probe_tx, probe_rx) = std::sync::mpsc::channel();
        let _ = std::thread::Builder::new().name("audio device probe".into()).spawn(move || {
            let host = cpal::default_host();
            let names = |devices: Vec<cpal::Device>| -> Vec<String> { devices.iter().filter_map(|d| d.name().ok()).collect() };
            let _ = probe_tx.send(Devices {
                output: names(host.output_devices().map(Iterator::collect).unwrap_or_default()),
                input: names(host.input_devices().map(Iterator::collect).unwrap_or_default()),
            });
        });
        let devices = probe_rx.recv_timeout(std::time::Duration::from_secs(3)).unwrap_or_else(|_| {
            log::warn!("the audio system is slow to list output and input devices; listing every device under both");
            let all: Vec<String> = cpal::default_host().devices().map(|d| d.filter_map(|d| d.name().ok()).collect()).unwrap_or_default();
            Devices { output: all.clone(), input: all }
        });
        let _ = tx.send(devices);
    });
    rx
}

/// [`list_devices_in_background`], waited for (the control channel's `DEVICES`).
pub fn list_devices() -> Devices {
    list_devices_in_background().recv_timeout(std::time::Duration::from_secs(10)).unwrap_or_default()
}

/// Which of `available` to open for the wanted name: its position, or `None` (take the system
/// default) when nothing was asked for or the name isn't there (the device was unplugged).
fn pick(wanted: Option<&str>, available: &[String]) -> Option<usize> {
    let wanted = wanted?;
    available.iter().position(|n| n == wanted)
}

fn open_stream(shared: &Arc<Shared>, wanted: Option<&str>) -> Result<(cpal::Stream, String, usize, u32), String> {
    let host = cpal::default_host();
    let chosen = wanted.and_then(|w| {
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
    let config: cpal::StreamConfig = supported.clone().into();
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(&device, &config, shared),
        cpal::SampleFormat::I16 => build::<i16>(&device, &config, shared),
        cpal::SampleFormat::U16 => build::<u16>(&device, &config, shared),
        cpal::SampleFormat::I32 => build::<i32>(&device, &config, shared),
        other => Err(format!("unsupported output sample format {other:?}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, name, usize::from(config.channels).max(1), config.sample_rate.0.max(1)))
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
            move |out: &mut [T], _: &cpal::OutputCallbackInfo| {
                let was_playing = shared.playing.load(Ordering::Relaxed);
                render(&shared, out, channels, device_rate);
                take_rack(&shared, &mut rack);
                if let (true, Some(r)) = (was_playing, rack.as_mut()) {
                    run_rack(r, out, channels);
                }
            },
            move |err| {
                log::warn!("audio output: {err}");
                on_error.failed.store(true, Ordering::Relaxed);
                on_error.playing.store(false, Ordering::Relaxed);
            },
            None,
        )
        .map_err(|e| e.to_string())
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
    let silence = T::from_sample(0.0f32);
    out.iter_mut().for_each(|s| *s = silence);
    if !shared.playing.load(Ordering::Relaxed) || shared.muted.load(Ordering::Relaxed) {
        return;
    }
    let Ok(slot) = shared.audio.try_lock() else { return };
    let Some(audio) = slot.as_ref() else { return };
    let file_channels = audio.num_channels();
    if file_channels == 0 || audio.sample_rate == 0 {
        return;
    }
    let step = f64::from(audio.sample_rate) / device_rate;
    let mut plan = shared.plan();
    plan.end = plan.end.min(audio.frames() as f64);
    let mut pos = f64::from_bits(shared.pos.load(Ordering::Relaxed)).max(0.0);
    let mut peak = [0.0f32; 2];
    let mut wraps = 0u32;
    for frame in out.chunks_mut(channels) {
        // The loop's wrap and the skipped part are settled here, a frame at a time.
        let Some(at) = plan.normalize(pos) else {
            shared.playing.store(false, Ordering::Relaxed);
            pos = plan.end;
            break;
        };
        if at < pos {
            wraps += 1;
        }
        pos = at;
        let i = pos as usize;
        let frac = (pos - i as f64) as f32;
        let sample = |source: &Vec<f32>| {
            let a = source.get(i).copied().unwrap_or(0.0);
            let b = source.get(i + 1).copied().unwrap_or(a);
            a + (b - a) * frac
        };
        // Level tap: the first two file channels (a mono file shows on both), before the mix-down.
        if let Some(first) = audio.channels.first() {
            let l = sample(first);
            let r = audio.channels.get(1).map_or(l, sample);
            peak = [peak[0].max(l.abs()), peak[1].max(r.abs())];
        }
        if channels == 1 && file_channels > 1 {
            // A one-channel device gets the average of the file's channels, not just the left
            // one (a host-left, guest-right recording would lose the guest).
            let sum: f32 = audio.channels.iter().map(sample).sum();
            if let Some(slot) = frame.first_mut() {
                *slot = T::from_sample(sum / file_channels as f32);
            }
        } else {
            for (c, slot) in frame.iter_mut().enumerate() {
                // Mono plays on both speakers; a stereo file leaves any further device channels silent.
                let source = if file_channels == 1 { audio.channels.first() } else { audio.channels.get(c) };
                let Some(source) = source.filter(|_| c < file_channels.max(2)) else { continue };
                *slot = T::from_sample(sample(source));
            }
        }
        pos += step;
    }
    shared.levels.record(0, peak[0]);
    shared.levels.record(1, peak[1]);
    // A step that ended exactly on the end or the skip is settled now, so the position shown
    // never sits in the skipped part or past the end of a loop.
    if shared.playing.load(Ordering::Relaxed)
        && let Some(at) = plan.normalize(pos)
    {
        if at < pos {
            wraps += 1;
        }
        pos = at;
    }
    if wraps > 0 {
        shared.wraps.fetch_add(wraps, Ordering::Relaxed);
    }
    shared.pos.store(pos.to_bits(), Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared_with(audio: AudioBuffer) -> Shared {
        let shared = Shared::new();
        if let Ok(mut slot) = shared.audio.lock() {
            *slot = Some(Arc::new(audio));
        }
        shared
    }

    fn play(shared: &Shared, from: f64, to: f64) {
        play_plan(shared, &Plan::new(from, to), from);
    }

    fn play_plan(shared: &Shared, plan: &Plan, from: f64) {
        shared.set_plan(plan);
        shared.wraps.store(0, Ordering::Relaxed);
        shared.pos.store(from.to_bits(), Ordering::Relaxed);
        shared.playing.store(true, Ordering::Relaxed);
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
        let shared = shared_with(AudioBuffer { sample_rate: 48_000, channels: vec![ramp.clone(), ramp.iter().map(|v| -v).collect()] });
        let mut out = vec![9.0f32; 40];
        render(&shared, &mut out, 2, 48_000.0);
        assert!(out.iter().all(|v| *v == 0.0), "stopped: silence");
        play(&shared, 10.0, 20.0);
        render(&shared, &mut out, 2, 48_000.0);
        // Frames 10..20, left and right, then silence for the rest of the buffer.
        for k in 0..10 {
            assert_eq!((out[2 * k], out[2 * k + 1]), (ramp[10 + k], -ramp[10 + k]), "frame {k}");
        }
        assert!(out[20..].iter().all(|v| *v == 0.0));
        assert!(!shared.playing.load(Ordering::Relaxed), "stops at the end of the range");
        assert_eq!(f64::from_bits(shared.pos.load(Ordering::Relaxed)), 20.0);
    }

    #[test]
    fn mono_plays_on_both_speakers_and_rates_are_converted() {
        let ramp: Vec<f32> = (0..1000).map(|i| i as f32).collect();
        let shared = shared_with(AudioBuffer { sample_rate: 24_000, channels: vec![ramp] });
        play(&shared, 0.0, 1000.0);
        // A 48 kHz device plays a 24 kHz file at half a file frame per device frame, on 4 channels.
        let mut out = vec![0.0f32; 4 * 8];
        render(&shared, &mut out, 4, 48_000.0);
        for k in 0..8 {
            let want = k as f32 * 0.5;
            assert_eq!(out[4 * k..4 * k + 4], [want, want, 0.0, 0.0], "frame {k}");
        }
        // The end of the range is clamped to the file, and integer formats convert.
        play(&shared, 998.0, 1e12);
        let mut ints = vec![7i16; 8];
        render(&shared, &mut ints, 1, 24_000.0);
        assert!(ints[0] != 0 && ints[1] != 0 && ints[2..].iter().all(|v| *v == 0), "{ints:?}");
    }

    /// Regression: a one-channel device played only the file's left channel.
    #[test]
    fn a_mono_device_gets_every_channel_of_the_file() {
        let shared = shared_with(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.0; 100], vec![0.5; 100]] });
        play(&shared, 0.0, 100.0);
        let mut out = vec![9.0f32; 16];
        render(&shared, &mut out, 1, 48_000.0);
        assert!(out.iter().all(|v| *v == 0.25), "left silent, right at 0.5: {out:?}");
    }

    /// f26: what the callback sends is what the Levels panel gets, per channel, without a lock.
    #[test]
    fn the_callback_records_the_peaks_it_sends() {
        let shared = shared_with(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.0, 0.25, -0.5, 0.0], vec![0.0, 0.125, 0.0, -1.0]] });
        play(&shared, 0.0, 4.0);
        let mut out = vec![0.0f32; 8];
        render(&shared, &mut out, 2, 48_000.0);
        assert_eq!(shared.levels.take(), [0.5, 1.0]);
        assert_eq!(shared.levels.take(), [0.0, 0.0], "taking starts over");
        // A mono file shows on both bars; a stopped player records nothing.
        *shared.audio.lock().unwrap() = Some(Arc::new(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.0, 0.75]] }));
        play(&shared, 0.0, 2.0);
        render(&shared, &mut out, 2, 48_000.0);
        assert_eq!(shared.levels.take(), [0.75, 0.75]);
        render(&shared, &mut out, 2, 48_000.0);
        assert_eq!(shared.levels.take(), [0.0, 0.0]);
    }

    /// C024: while muted (recording) the callback writes silence even if a play was started.
    #[test]
    fn muted_is_silence() {
        let shared = shared_with(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.5; 100]] });
        play(&shared, 0.0, 100.0);
        shared.muted.store(true, Ordering::Relaxed);
        let mut out = vec![1.0f32; 16];
        render(&shared, &mut out, 2, 48_000.0);
        assert!(out.iter().all(|v| *v == 0.0));
        assert_eq!(f64::from_bits(shared.pos.load(Ordering::Relaxed)), 0.0, "the playhead does not move");
        shared.muted.store(false, Ordering::Relaxed);
        render(&shared, &mut out, 2, 48_000.0);
        assert!(out.iter().any(|v| *v != 0.0));
    }

    #[test]
    fn a_busy_or_empty_buffer_is_silence() {
        let shared = shared_with(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.5; 100]] });
        play(&shared, 0.0, 100.0);
        let mut out = vec![1.0f32; 16];
        {
            let _held = shared.audio.lock().unwrap();
            render(&shared, &mut out, 2, 48_000.0);
        }
        assert!(out.iter().all(|v| *v == 0.0), "the UI held the buffer: silence, no blocking");
        *shared.audio.lock().unwrap() = None;
        render(&shared, &mut out, 2, 48_000.0);
        assert!(out.iter().all(|v| *v == 0.0));
    }

    #[test]
    fn a_loop_goes_round_inside_the_callback() {
        let ramp: Vec<f32> = (0..100).map(|i| i as f32).collect();
        let shared = shared_with(AudioBuffer { sample_rate: 48_000, channels: vec![ramp] });
        play_plan(&shared, &Plan { looping: true, ..Plan::new(10.0, 14.0) }, 12.0);
        let mut out = vec![0.0f32; 2 * 10];
        render(&shared, &mut out, 2, 48_000.0);
        let left: Vec<f32> = out.chunks(2).map(|f| f[0]).collect();
        assert_eq!(left, [12.0, 13.0, 10.0, 11.0, 12.0, 13.0, 10.0, 11.0, 12.0, 13.0]);
        assert!(shared.playing.load(Ordering::Relaxed), "a loop does not stop");
        assert_eq!(shared.wraps.load(Ordering::Relaxed), 3, "two wraps inside the buffer and one at its end");
        assert_eq!(f64::from_bits(shared.pos.load(Ordering::Relaxed)), 10.0);
    }

    #[test]
    fn the_skipped_part_is_not_heard() {
        let ramp: Vec<f32> = (0..100).map(|i| i as f32).collect();
        let shared = shared_with(AudioBuffer { sample_rate: 48_000, channels: vec![ramp] });
        let plan = Plan { skip_start: 5.0, skip_end: 8.0, ..Plan::new(2.0, 11.0) };
        play_plan(&shared, &plan, 2.0);
        let mut out = vec![0.0f32; 2 * 10];
        render(&shared, &mut out, 2, 48_000.0);
        let left: Vec<f32> = out.chunks(2).map(|f| f[0]).collect();
        assert_eq!(left, [2.0, 3.0, 4.0, 8.0, 9.0, 10.0, 0.0, 0.0, 0.0, 0.0]);
        assert!(!shared.playing.load(Ordering::Relaxed));
    }

    /// Pause keeps the place; resume carries on from it. Without a device the silent clock moves it.
    #[test]
    fn the_silent_clock_advances_pauses_and_resumes() {
        let mut p = Player::new(None);
        p.silent = true;
        p.reopen();
        assert!(!p.has_output());
        p.set_audio(Some(Arc::new(AudioBuffer { sample_rate: 1000, channels: vec![vec![0.0; 100_000]] })));
        assert!(p.start(&Plan::new(0.0, 100_000.0), 100.0));
        std::thread::sleep(std::time::Duration::from_millis(60));
        p.tick();
        let moved = p.position();
        assert!(p.is_playing() && (150..400).contains(&moved), "about 60 ms at 1 kHz from 100: {moved}");
        p.pause();
        let held = p.position();
        std::thread::sleep(std::time::Duration::from_millis(40));
        p.tick();
        assert_eq!(p.position(), held, "paused: still");
        assert!(p.resume());
        std::thread::sleep(std::time::Duration::from_millis(40));
        p.tick();
        assert!(p.position() > held, "resumed from the same place");
        p.stop();
        assert!(!p.is_playing());
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
        let shared = shared_with(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.5; 5000], vec![-0.25; 5000]] });
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
        play(&shared, 0.0, 5000.0);
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
}
