//! Playback of the open file through the default output device.
//!
//! The audio callback never blocks or allocates: it `try_lock`s the shared buffer and plays
//! silence if the UI holds it. The file's rate is converted to the device's with linear
//! interpolation, which is fine for auditioning (an edit never goes through this path).
//!
//! Two things keep playback in step with the world (f48): an edit made while playing is handed
//! over by [`Player::replace_audio`] (the new buffer is swapped in under the same lock the
//! callback `try_lock`s, with the play position moved across the edit, so the callback sees the
//! old buffer and position or the new ones, never a mix), and a thread polls the system's default
//! output device once a second and flags a change, on which [`Player::maintain`] reopens the
//! stream on the new default and goes on from the same position.
//!
//! `WAVCRAFT_NO_AUDIO=1` swaps the sound card for a silent output thread that runs the same
//! callback at real-time pace (headless checks, scripted runs); the control verb `DEVICE_CHANGED`
//! simulates a change of the default device in both modes.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use wavcraft_core::AudioBuffer;
use wavcraft_core::doc::span::EditSpan;

/// How often the default output device is looked at.
const DEVICE_POLL: Duration = Duration::from_secs(1);

struct Shared {
    audio: Mutex<Option<Arc<AudioBuffer>>>,
    /// Play position in file frames, an `f64` stored as bits.
    pos: AtomicU64,
    /// Frame to stop at, an `f64` stored as bits.
    end: AtomicU64,
    playing: AtomicBool,
    /// Set by the stream's error callback (the device went away, for example).
    failed: AtomicBool,
    /// Set when the default output device is no longer the one the stream was opened on.
    device_changed: AtomicBool,
    /// The name of the device the stream was opened on (empty if it could not be opened).
    opened_device: Mutex<String>,
    /// How many times the stream was reopened because of a change or a failure.
    reopens: AtomicU32,
    /// `WAVCRAFT_NO_AUDIO`: a silent output thread instead of the sound card.
    silent: bool,
    /// The simulated default device in silent mode (`DEVICE_CHANGED` bumps it).
    sim_device: AtomicU32,
}

impl Shared {
    fn new(silent: bool) -> Shared {
        Shared {
            audio: Mutex::new(None),
            pos: AtomicU64::new(0f64.to_bits()),
            end: AtomicU64::new(0f64.to_bits()),
            playing: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            device_changed: AtomicBool::new(false),
            opened_device: Mutex::new(String::new()),
            reopens: AtomicU32::new(0),
            silent,
            sim_device: AtomicU32::new(0),
        }
    }

    /// The name of the system's default output device now (`None` if there is none).
    fn default_device_name(&self) -> Option<String> {
        if self.silent {
            return Some(format!("Silent output {}", self.sim_device.load(Ordering::Relaxed)));
        }
        let device = cpal::default_host().default_output_device()?;
        Some(device.name().unwrap_or_else(|_| "output".to_string()))
    }
}

/// What the player's output is: the sound card, or (headless) a thread that pulls the callback.
enum Output {
    Card(#[allow(dead_code)] cpal::Stream),
    Silent(#[allow(dead_code)] SilentOut),
}

/// A thread that calls [`render`] into a scratch buffer every 10 ms, as a device would.
struct SilentOut {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl SilentOut {
    fn start(shared: &Arc<Shared>) -> SilentOut {
        const RATE: usize = 48_000;
        let stop = Arc::new(AtomicBool::new(false));
        let (weak, flag): (Weak<Shared>, _) = (Arc::downgrade(shared), stop.clone());
        let thread = std::thread::Builder::new()
            .name("silent output".into())
            .spawn(move || {
                let mut buf = vec![0.0f32; RATE / 100 * 2];
                while !flag.load(Ordering::Relaxed) {
                    let Some(shared) = weak.upgrade() else { break };
                    render(&shared, &mut buf, 2, RATE as f64);
                    drop(shared);
                    std::thread::sleep(Duration::from_millis(10));
                }
            })
            .ok();
        SilentOut { stop, thread }
    }
}

impl Drop for SilentOut {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// The output stream and the play state shared with its callback.
pub struct Player {
    shared: Arc<Shared>,
    stream: Option<Output>,
    /// Why there is no sound, if the device could not be opened.
    pub error: Option<String>,
    /// The output device's name, for the status bar.
    pub device: String,
}

impl Player {
    /// Open the default output device. Without one the player still works, silently.
    pub fn new() -> Player {
        Player::with_silent(std::env::var("WAVCRAFT_NO_AUDIO").is_ok_and(|v| !v.is_empty() && v != "0"))
    }

    /// [`new`](Self::new) with the silent output chosen by the caller (tests).
    fn with_silent(silent: bool) -> Player {
        let shared = Arc::new(Shared::new(silent));
        let mut player = Player { shared, stream: None, error: None, device: String::new() };
        player.reopen();
        watch_default_device(&player.shared);
        player
    }

    /// (Re)open the default output device, replacing any stream there was.
    fn reopen(&mut self) {
        self.stream = None;
        self.shared.failed.store(false, Ordering::Relaxed);
        // Whatever the default is now is what this open is for, so a change is only reported once.
        self.shared.device_changed.store(false, Ordering::Relaxed);
        match open_stream(&self.shared) {
            Ok((stream, device)) => {
                self.set_opened(&device);
                self.stream = Some(stream);
                self.device = device;
                self.error = None;
            }
            Err(e) => {
                self.set_opened("");
                self.error = Some(e);
            }
        }
    }

    fn set_opened(&self, name: &str) {
        if let Ok(mut opened) = self.shared.opened_device.lock() {
            *opened = name.to_string();
        }
    }

    /// Call every frame, on the UI thread: if the default output device changed or the stream
    /// failed, reopen on the current default and carry on from the same position (playback that
    /// was running keeps running; if no output can be opened, it stops). Returns whether it reopened.
    pub fn maintain(&mut self) -> bool {
        if !(self.shared.device_changed.load(Ordering::Relaxed) || self.failed()) {
            return false;
        }
        let was_playing = self.is_playing();
        self.reopen();
        self.shared.reopens.fetch_add(1, Ordering::Relaxed);
        if self.stream.is_none() {
            self.shared.playing.store(false, Ordering::Relaxed);
        } else {
            self.shared.playing.store(was_playing, Ordering::Relaxed);
        }
        log::info!(target: "wavcraft", "audio output reopened on {:?} (playing: {was_playing})", self.device);
        true
    }

    /// Pretend the system's default output device changed (scripted checks): in silent mode the
    /// simulated default gets a new name, which the watcher also sees; either way the next
    /// [`maintain`](Self::maintain) reopens.
    pub fn simulate_device_change(&self) {
        if self.shared.silent {
            self.shared.sim_device.fetch_add(1, Ordering::Relaxed);
        }
        self.shared.device_changed.store(true, Ordering::Relaxed);
    }

    /// How many times the stream was reopened because of a device change or failure.
    pub fn reopens(&self) -> u32 {
        self.shared.reopens.load(Ordering::Relaxed)
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

    /// Hand over the audio after an edit without stopping: the play position (and the end of
    /// the range) move across the edit `span` (see [`EditSpan::map_position`]) in the same
    /// critical section that swaps the buffer, so the next block the callback reads is the new
    /// audio at the matching place. The callback never waits for this; if it runs while the lock
    /// is held, that one block is silence (the same hand-off `set_audio` always had).
    pub fn replace_audio(&self, audio: Option<Arc<AudioBuffer>>, span: &EditSpan) {
        if let Ok(mut slot) = self.shared.audio.lock() {
            for cell in [&self.shared.pos, &self.shared.end] {
                let at = f64::from_bits(cell.load(Ordering::Relaxed));
                cell.store(span.map_position(at).to_bits(), Ordering::Relaxed);
            }
            if audio.is_none() {
                self.shared.playing.store(false, Ordering::Relaxed);
            }
            *slot = audio;
        }
    }

    /// Where the player is now, for `STATE` (file frames, with a fraction).
    pub fn position_exact(&self) -> f64 {
        f64::from_bits(self.shared.pos.load(Ordering::Relaxed))
    }

    /// Play `[from, to)` in file frames. Without a working output (none at start-up, or one that
    /// has since failed) the device is tried again first; if there still is none, nothing starts,
    /// so the playhead does not sit there "playing" with no callback to move it.
    pub fn play(&mut self, from: usize, to: usize) {
        self.maintain();
        if self.stream.is_none() {
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

fn open_stream(shared: &Arc<Shared>) -> Result<(Output, String), String> {
    if shared.silent {
        let name = shared.default_device_name().unwrap_or_default();
        return Ok((Output::Silent(SilentOut::start(shared)), name));
    }
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
    Ok((Output::Card(stream), name))
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
                // Playback is not cleared: `Player::maintain` reopens and goes on (or stops if it can't).
                on_error.failed.store(true, Ordering::Relaxed);
            },
            None,
        )
        .map_err(|e| e.to_string())
}

/// Poll the default output device once a second, on a thread of its own (not the audio thread),
/// and flag a change. The thread ends when the player is dropped.
fn watch_default_device(shared: &Arc<Shared>) {
    let weak = Arc::downgrade(shared);
    let _ = std::thread::Builder::new().name("audio device watch".into()).spawn(move || {
        loop {
            std::thread::sleep(DEVICE_POLL);
            let Some(shared) = weak.upgrade() else { break };
            let now = shared.default_device_name().unwrap_or_default();
            let opened = shared.opened_device.lock().map(|o| o.clone()).unwrap_or_default();
            if now != opened {
                log::info!(target: "wavcraft", "default output device is now {now:?} (was {opened:?})");
                shared.device_changed.store(true, Ordering::Relaxed);
            }
        }
    });
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
        let shared = Shared::new(true);
        *shared.audio.lock().unwrap() = Some(Arc::new(audio));
        shared
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

    fn ramp_audio(frames: usize) -> Arc<AudioBuffer> {
        Arc::new(AudioBuffer { sample_rate: 48_000, channels: vec![(0..frames).map(|i| i as f32).collect()] })
    }

    /// Edit while playing: the swap moves the position across the edit and the very next block
    /// is the new audio at the matching place.
    #[test]
    fn replace_audio_moves_the_playhead_and_the_next_block_is_the_new_audio() {
        let p = Player::with_silent(true);
        p.set_audio(Some(ramp_audio(1000)));
        let shared = &p.shared;
        play(shared, 600.0, 1000.0);
        // The edit deleted frames 100..300 of the ramp; the new audio is the ramp with a gap closed.
        let new: Vec<f32> = (0..1000).filter(|i| !(100..300).contains(i)).map(|i| i as f32).collect();
        p.replace_audio(Some(Arc::new(AudioBuffer { sample_rate: 48_000, channels: vec![new] })), &EditSpan { at: 100, removed: 200, inserted: 0 });
        assert_eq!(p.position_exact(), 400.0);
        assert_eq!(f64::from_bits(shared.end.load(Ordering::Relaxed)), 800.0);
        let mut out = vec![0.0f32; 8];
        render(shared, &mut out, 2, 48_000.0);
        // New frame 400 is old frame 600: the same sample is heard as before the edit.
        assert_eq!(out[0], 600.0);
        assert!(p.is_playing());
    }

    #[test]
    fn replacing_with_nothing_stops_playback() {
        let p = Player::with_silent(true);
        p.set_audio(Some(ramp_audio(1000)));
        play(&p.shared, 10.0, 1000.0);
        p.replace_audio(None, &EditSpan { at: 0, removed: 1000, inserted: 0 });
        assert!(!p.is_playing());
    }

    /// A change of the default device reopens the stream and playback goes on from where it was.
    #[test]
    fn a_device_change_reopens_and_playback_continues_from_the_same_position() {
        let mut p = Player::with_silent(true);
        p.set_audio(Some(ramp_audio(48_000 * 600)));
        p.play(1000, 48_000 * 600);
        assert!(!p.maintain(), "nothing changed");
        let before = p.device.clone();
        p.simulate_device_change();
        // The stream is reopened on the next maintain; the position is not reset by it.
        std::thread::sleep(Duration::from_millis(60));
        let at = p.position_exact();
        assert!(at > 1000.0, "the silent output was playing: {at}");
        assert!(p.maintain());
        assert_ne!(p.device, before, "now on the new default");
        assert_eq!(p.reopens(), 1);
        assert!(p.is_playing());
        std::thread::sleep(Duration::from_millis(60));
        assert!(p.position_exact() > at, "still advancing after the reopen");
        assert!(!p.maintain(), "reported once");
    }

    #[test]
    fn the_watcher_sees_a_changed_default_on_its_own() {
        let mut p = Player::with_silent(true);
        p.shared.sim_device.fetch_add(1, Ordering::Relaxed);
        let t0 = std::time::Instant::now();
        while !p.maintain() {
            assert!(t0.elapsed() < Duration::from_secs(4), "the watcher flagged nothing within 4 s");
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(p.device, "Silent output 1");
    }
}
