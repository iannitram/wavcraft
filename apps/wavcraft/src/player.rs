//! Playback of the open file through the default output device.
//!
//! The audio callback never blocks or allocates: it `try_lock`s the shared buffer and plays
//! silence if the UI holds it. The file's rate is converted to the device's with linear
//! interpolation, which is fine for auditioning (an edit never goes through this path).

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use wavcraft_core::AudioBuffer;
use wavcraft_core::record::MonitorRing;

struct Shared {
    audio: Mutex<Option<Arc<AudioBuffer>>>,
    /// Play position in file frames, an `f64` stored as bits.
    pos: AtomicU64,
    /// Frame to stop at, an `f64` stored as bits.
    end: AtomicU64,
    playing: AtomicBool,
    /// Nothing plays while this is set (a take is being recorded; roadmap C024).
    muted: AtomicBool,
    /// Set by the stream's error callback (the device went away, for example).
    failed: AtomicBool,
    /// The input being heard through the output while recording (C025). The callback only
    /// `try_lock`s it; the UI thread swaps it in and out.
    monitor: Mutex<Option<Arc<MonitorRing>>>,
    /// The output device's sample rate (0 until a stream is open).
    device_rate: AtomicU32,
}

/// The output stream and the play state shared with its callback.
pub struct Player {
    shared: Arc<Shared>,
    stream: Option<cpal::Stream>,
    /// Why there is no sound, if the device could not be opened.
    pub error: Option<String>,
    /// The output device's name, for the status bar.
    pub device: String,
}

impl Player {
    /// Open the default output device. Without one the player still works, silently.
    pub fn new() -> Player {
        let shared = Arc::new(Shared {
            audio: Mutex::new(None),
            pos: AtomicU64::new(0f64.to_bits()),
            end: AtomicU64::new(0f64.to_bits()),
            playing: AtomicBool::new(false),
            muted: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            monitor: Mutex::new(None),
            device_rate: AtomicU32::new(0),
        });
        let mut player = Player { shared, stream: None, error: None, device: String::new() };
        player.reopen();
        player
    }

    /// (Re)open the default output device, replacing any stream there was.
    fn reopen(&mut self) {
        self.stream = None;
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

    /// Hear `ring` (input at `rate` Hz) through the output (C025), or stop hearing it with
    /// `None`. Refused, with the reason, when there is no working output or its rate differs from
    /// the input's (no resampling here); the recording itself is not affected.
    pub fn set_monitor(&mut self, ring: Option<(Arc<MonitorRing>, u32)>) -> Result<(), String> {
        let Some((ring, rate)) = ring else {
            if let Ok(mut slot) = self.shared.monitor.lock() {
                *slot = None;
            }
            return Ok(());
        };
        if self.stream.is_none() || self.failed() {
            self.reopen();
        }
        if self.stream.is_none() {
            return Err(format!("input monitoring needs an output device: {}", self.error.as_deref().unwrap_or("none")));
        }
        let out = self.shared.device_rate.load(Ordering::Relaxed);
        if out != rate {
            return Err(format!("input monitoring needs the input and output at the same rate (input {rate} Hz, output {out} Hz)"));
        }
        if let Ok(mut slot) = self.shared.monitor.lock() {
            *slot = Some(ring);
        }
        Ok(())
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

    /// Play `[from, to)` in file frames. Without a working output (none at start-up, or one that
    /// has since failed) the device is tried again first; if there still is none, nothing starts,
    /// so the playhead does not sit there "playing" with no callback to move it.
    pub fn play(&mut self, from: usize, to: usize) {
        if self.is_muted() {
            return;
        }
        if self.stream.is_none() || self.failed() {
            self.reopen();
        }
        if self.stream.is_none() {
            return;
        }
        self.shared.pos.store((from as f64).to_bits(), Ordering::Relaxed);
        self.shared.end.store((to as f64).to_bits(), Ordering::Relaxed);
        self.shared.playing.store(to > from, Ordering::Relaxed);
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

    pub fn stop(&self) {
        self.shared.playing.store(false, Ordering::Relaxed);
    }

    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }

    /// The frame being played right now.
    pub fn position(&self) -> usize {
        f64::from_bits(self.shared.pos.load(Ordering::Relaxed)).max(0.0) as usize
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
    shared.device_rate.store(config.sample_rate.0, Ordering::Relaxed);
    Ok((stream, name))
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

/// Fill one device buffer: the file being played, plus the input when it is being monitored. Runs
/// on the audio thread.
fn render<T: cpal::Sample + cpal::FromSample<f32>>(shared: &Shared, out: &mut [T], channels: usize, device_rate: f64)
where
    f32: cpal::FromSample<T>,
{
    render_playback(shared, out, channels, device_rate);
    if let Ok(slot) = shared.monitor.try_lock()
        && let Some(ring) = slot.as_ref()
    {
        mix_monitor(ring, out, channels);
    }
}

/// What output channel `c` of `out_channels` hears of one input frame: a mono input on every
/// output channel, otherwise channel for channel (a one-channel output gets the average).
fn monitor_sample(input: &[f32], c: usize, out_channels: usize) -> f32 {
    match input.len() {
        0 => 0.0,
        1 => input[0],
        n if out_channels == 1 => input.iter().sum::<f32>() / n as f32,
        _ => input.get(c).copied().unwrap_or(0.0),
    }
}

/// Add the monitored input to `out` (already holding the file's playback). Takes the frames the
/// ring has; the rest of the buffer hears silence from it. Stack space only.
fn mix_monitor<T: cpal::Sample + cpal::FromSample<f32>>(ring: &MonitorRing, out: &mut [T], channels: usize)
where
    f32: cpal::FromSample<T>,
{
    let in_ch = ring.channels();
    let mut tmp = [0.0f32; 2048];
    let per_chunk = (tmp.len() / in_ch).max(1);
    if in_ch > tmp.len() {
        return;
    }
    for chunk in out.chunks_mut(per_chunk * channels) {
        let frames = chunk.len() / channels;
        let got = ring.pop_into(&mut tmp[..frames * in_ch]);
        for (f, frame) in chunk.chunks_mut(channels).take(got).enumerate() {
            let input = &tmp[f * in_ch..(f + 1) * in_ch];
            for (c, slot) in frame.iter_mut().enumerate() {
                let mixed = cpal::Sample::to_sample::<f32>(*slot) + monitor_sample(input, c, channels);
                *slot = T::from_sample(mixed.clamp(-1.0, 1.0));
            }
        }
    }
}

/// The file's playback alone.
fn render_playback<T: cpal::Sample + cpal::FromSample<f32>>(shared: &Shared, out: &mut [T], channels: usize, device_rate: f64) {
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
    let end = f64::from_bits(shared.end.load(Ordering::Relaxed)).min(audio.frames() as f64);
    let mut pos = f64::from_bits(shared.pos.load(Ordering::Relaxed)).max(0.0);
    for frame in out.chunks_mut(channels) {
        if pos >= end {
            shared.playing.store(false, Ordering::Relaxed);
            break;
        }
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
    shared.pos.store(pos.to_bits(), Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared_with(audio: AudioBuffer) -> Shared {
        Shared {
            audio: Mutex::new(Some(Arc::new(audio))),
            pos: AtomicU64::new(0f64.to_bits()),
            end: AtomicU64::new(0f64.to_bits()),
            playing: AtomicBool::new(false),
            muted: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            monitor: Mutex::new(None),
            device_rate: AtomicU32::new(0),
        }
    }

    fn play(shared: &Shared, from: f64, to: f64) {
        shared.pos.store(from.to_bits(), Ordering::Relaxed);
        shared.end.store(to.to_bits(), Ordering::Relaxed);
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

    /// C025: the input is added to the output; a mono input is on every channel.
    #[test]
    fn the_monitored_input_is_heard_with_the_playback_and_never_blocks() {
        let shared = shared_with(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.25; 100]] });
        let ring = MonitorRing::new(64, 1);
        ring.push(&[0.5, 0.5, 0.5]);
        *shared.monitor.lock().unwrap() = Some(ring.clone());
        let mut out = vec![9.0f32; 8];
        render(&shared, &mut out, 2, 48_000.0);
        // Nothing plays, so the first three frames are the input alone, then silence (underrun).
        assert_eq!(out, [0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.0, 0.0]);
        assert_eq!(ring.underrun_frames(), 1);
        // Playing and monitoring add; stereo input goes channel for channel.
        let st = MonitorRing::new(64, 2);
        st.push(&[0.1, 0.2, 0.1, 0.2]);
        *shared.monitor.lock().unwrap() = Some(st);
        play(&shared, 0.0, 100.0);
        let mut out = vec![0.0f32; 4];
        render(&shared, &mut out, 2, 48_000.0);
        for (got, want) in out.iter().zip([0.35, 0.45, 0.35, 0.45]) {
            assert!((got - want).abs() < 1e-6, "{out:?}");
        }
        // A busy slot (the UI is swapping the monitor) is skipped, not waited for.
        let _held = shared.monitor.lock().unwrap();
        let mut out = vec![0.0f32; 4];
        render(&shared, &mut out, 2, 48_000.0);
    }

    #[test]
    fn monitoring_maps_channels_and_clamps_integer_output() {
        assert_eq!(monitor_sample(&[0.5], 3, 4), 0.5);
        assert_eq!(monitor_sample(&[0.2, 0.4], 0, 1), (0.2 + 0.4) / 2.0);
        assert_eq!(monitor_sample(&[0.2, 0.4, 0.6], 2, 2), 0.6);
        assert_eq!(monitor_sample(&[0.2, 0.4], 3, 4), 0.0, "no input channel for an output channel");
        let shared = shared_with(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.0; 10]] });
        let ring = MonitorRing::new(16, 1);
        ring.push(&[2.0]);
        *shared.monitor.lock().unwrap() = Some(ring);
        let mut out = vec![0i16; 2];
        render(&shared, &mut out, 1, 48_000.0);
        assert_eq!(out[0], i16::MAX, "clamped, not wrapped");
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
}
