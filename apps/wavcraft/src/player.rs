//! Playback of the open file through the default output device.
//!
//! The audio callback never blocks or allocates: it `try_lock`s the shared buffer and plays
//! silence if the UI holds it. The file's rate is converted to the device's with linear
//! interpolation, which is fine for auditioning (an edit never goes through this path).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use cpal::Sample as _;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use wavcraft_core::AudioBuffer;
use wavcraft_core::rack::{MAX_BLOCK, RackConfig, RackProcessor};

struct Shared {
    audio: Mutex<Option<Arc<AudioBuffer>>>,
    /// Play position in file frames, an `f64` stored as bits.
    pos: AtomicU64,
    /// Frame to stop at, an `f64` stored as bits.
    end: AtomicU64,
    playing: AtomicBool,
    /// Set by the stream's error callback (the device went away, for example).
    failed: AtomicBool,
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
    /// The output stream's channel count and sample rate (what a rack is built for).
    out_channels: usize,
    out_rate: u32,
}

impl Player {
    /// Open the default output device. Without one the player still works, silently.
    pub fn new() -> Player {
        let shared = Arc::new(Shared {
            audio: Mutex::new(None),
            pos: AtomicU64::new(0f64.to_bits()),
            end: AtomicU64::new(0f64.to_bits()),
            playing: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            rack_mailbox: Mutex::new(None),
            rack_new: AtomicBool::new(false),
        });
        let mut player = Player { shared, stream: None, error: None, device: String::new(), out_channels: 2, out_rate: 48_000 };
        player.reopen();
        player
    }

    /// (Re)open the default output device, replacing any stream there was.
    fn reopen(&mut self) {
        self.stream = None;
        self.shared.failed.store(false, Ordering::Relaxed);
        match open_stream(&self.shared) {
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
        self.shared.pos.store((from as f64).to_bits(), Ordering::Relaxed);
        self.shared.end.store((to as f64).to_bits(), Ordering::Relaxed);
        self.shared.playing.store(to > from, Ordering::Relaxed);
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

fn open_stream(shared: &Arc<Shared>) -> Result<(cpal::Stream, String, usize, u32), String> {
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
            failed: AtomicBool::new(false),
            rack_mailbox: Mutex::new(None),
            rack_new: AtomicBool::new(false),
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
