//! Playback of the open file through the default output device.
//!
//! The audio callback never blocks or allocates: it `try_lock`s the shared buffer and plays
//! silence if the UI holds it. The file's rate is converted to the device's with linear
//! interpolation, which is fine for auditioning (an edit never goes through this path).

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use wavcraft_core::AudioBuffer;
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
    /// Set by the stream's error callback (the device went away, for example).
    failed: AtomicBool,
}

/// The output stream and the play state shared with its callback.
pub struct Player {
    shared: Arc<Shared>,
    stream: Option<cpal::Stream>,
    /// Why there is no sound, if the device could not be opened.
    pub error: Option<String>,
    /// The output device's name, for the status bar.
    pub device: String,
    /// The file's sample rate, for the silent clock.
    rate: u32,
    /// Without an output stream the position still advances, by this wall clock, so the
    /// transport can be used and scripted with no sound device ("test mode"). `None` while the
    /// clock is not running.
    sim_last: Option<Instant>,
    /// `WAVCRAFT_NO_AUDIO` is set: never open a device (scripted runs).
    silent: bool,
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
            failed: AtomicBool::new(false),
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
    /// Open the default output device. Without one the player still works, silently.
    pub fn new() -> Player {
        let shared = Arc::new(Shared::new());
        let silent = std::env::var_os("WAVCRAFT_NO_AUDIO").is_some();
        let mut player = Player { shared, stream: None, error: None, device: String::new(), rate: 0, sim_last: None, silent };
        player.reopen();
        player
    }

    /// (Re)open the default output device, replacing any stream there was.
    fn reopen(&mut self) {
        self.stream = None;
        if self.silent {
            self.error = Some("audio off (WAVCRAFT_NO_AUDIO)".to_string());
            return;
        }
        self.shared.failed.store(false, Ordering::Relaxed);
        match open_stream(&self.shared) {
            Ok((stream, device)) => {
                self.stream = Some(stream);
                self.device = device;
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
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

fn open_stream(shared: &Arc<Shared>) -> Result<(cpal::Stream, String), String> {
    let device = cpal::default_host().default_output_device().ok_or("no audio output device")?;
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
    Ok((stream, name))
}

fn build<T>(device: &cpal::Device, config: &cpal::StreamConfig, shared: &Arc<Shared>) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let on_error = Arc::clone(shared);
    let shared = Arc::clone(shared);
    let channels = usize::from(config.channels).max(1);
    let device_rate = f64::from(config.sample_rate.0.max(1));
    device
        .build_output_stream(
            config,
            move |out: &mut [T], _: &cpal::OutputCallbackInfo| render(&shared, out, channels, device_rate),
            move |err| {
                log::warn!("audio output: {err}");
                on_error.failed.store(true, Ordering::Relaxed);
                on_error.playing.store(false, Ordering::Relaxed);
            },
            None,
        )
        .map_err(|e| e.to_string())
}

/// Fill one device buffer. Runs on the audio thread.
fn render<T: cpal::Sample + cpal::FromSample<f32>>(shared: &Shared, out: &mut [T], channels: usize, device_rate: f64) {
    let silence = T::from_sample(0.0f32);
    out.iter_mut().for_each(|s| *s = silence);
    if !shared.playing.load(Ordering::Relaxed) {
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
        let mut p = Player::new();
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
}
