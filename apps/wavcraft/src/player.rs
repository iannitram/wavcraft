//! Playback of the open file through the default output device.
//!
//! The audio callback never blocks or allocates: it `try_lock`s the shared buffer and plays
//! silence if the UI holds it. The file's rate is converted to the device's with linear
//! interpolation, which is fine for auditioning (an edit never goes through this path).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use wavcraft_core::AudioBuffer;

struct Shared {
    audio: Mutex<Option<Arc<AudioBuffer>>>,
    /// Play position in file frames, an `f64` stored as bits.
    pos: AtomicU64,
    /// Frame to stop at, an `f64` stored as bits.
    end: AtomicU64,
    /// Bumped by every play/stop; a render pass started under an older value must not write back.
    generation: AtomicU64,
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
}

impl Player {
    /// Open the default output device. Without one the player still works, silently.
    pub fn new() -> Player {
        let shared = Arc::new(Shared {
            audio: Mutex::new(None),
            pos: AtomicU64::new(0f64.to_bits()),
            end: AtomicU64::new(0f64.to_bits()),
            generation: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            failed: AtomicBool::new(false),
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
        if self.stream.is_none() || self.failed() {
            self.reopen();
        }
        if self.stream.is_none() {
            return;
        }
        self.shared.generation.fetch_add(1, Ordering::AcqRel);
        self.shared.pos.store((from as f64).to_bits(), Ordering::Relaxed);
        self.shared.end.store((to as f64).to_bits(), Ordering::Relaxed);
        self.shared.playing.store(to > from, Ordering::Relaxed);
    }

    pub fn stop(&self) {
        self.shared.generation.fetch_add(1, Ordering::AcqRel);
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
    render_with(shared, out, channels, device_rate, || {});
}

fn render_with<T: cpal::Sample + cpal::FromSample<f32>, F: FnOnce()>(
    shared: &Shared,
    out: &mut [T],
    channels: usize,
    device_rate: f64,
    before_commit: F,
) {
    let generation = shared.generation.load(Ordering::Acquire);
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
    let end = f64::from_bits(shared.end.load(Ordering::Relaxed)).min(audio.frames() as f64);
    let mut pos = f64::from_bits(shared.pos.load(Ordering::Relaxed)).max(0.0);
    let mut finished = false;
    for frame in out.chunks_mut(channels) {
        if pos >= end {
            finished = true;
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
    before_commit();
    if shared.generation.load(Ordering::Acquire) == generation {
        if finished {
            shared.playing.store(false, Ordering::Relaxed);
        }
        shared.pos.store(pos.to_bits(), Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared_with(audio: AudioBuffer) -> Shared {
        Shared {
            audio: Mutex::new(Some(Arc::new(audio))),
            pos: AtomicU64::new(0f64.to_bits()),
            end: AtomicU64::new(0f64.to_bits()),
            generation: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            failed: AtomicBool::new(false),
        }
    }

    fn play(shared: &Shared, from: f64, to: f64) {
        shared.generation.fetch_add(1, Ordering::AcqRel);
        shared.pos.store(from.to_bits(), Ordering::Relaxed);
        shared.end.store(to.to_bits(), Ordering::Relaxed);
        shared.playing.store(true, Ordering::Relaxed);
    }

    #[test]
    fn a_stale_callback_does_not_overwrite_a_restart() {
        let ramp: Vec<f32> = (0..1000).map(|i| i as f32).collect();
        let shared = shared_with(AudioBuffer { sample_rate: 48_000, channels: vec![ramp] });
        play(&shared, 0.0, 1000.0);
        let mut out = vec![0.0f32; 64];
        render_with(&shared, &mut out, 1, 48_000.0, || {
            shared.generation.fetch_add(1, Ordering::AcqRel);
            shared.pos.store(500.0f64.to_bits(), Ordering::Relaxed);
            shared.end.store(1000.0f64.to_bits(), Ordering::Relaxed);
            shared.playing.store(true, Ordering::Relaxed);
        });
        assert_eq!(f64::from_bits(shared.pos.load(Ordering::Relaxed)), 500.0);
        assert!(shared.playing.load(Ordering::Relaxed));
    }

    #[test]
    fn a_stale_callback_does_not_clear_a_restart_at_end_of_range() {
        let ramp = vec![0.0f32; 1000];
        let shared = shared_with(AudioBuffer { sample_rate: 48_000, channels: vec![ramp] });
        play(&shared, 10.0, 20.0);
        let mut out = vec![0.0f32; 64];
        render_with(&shared, &mut out, 1, 48_000.0, || {
            shared.generation.fetch_add(1, Ordering::AcqRel);
            shared.pos.store(500.0f64.to_bits(), Ordering::Relaxed);
            shared.end.store(1000.0f64.to_bits(), Ordering::Relaxed);
            shared.playing.store(true, Ordering::Relaxed);
        });
        assert!(shared.playing.load(Ordering::Relaxed));
        assert_eq!(f64::from_bits(shared.pos.load(Ordering::Relaxed)), 500.0);
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
}
