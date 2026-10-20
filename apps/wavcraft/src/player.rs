//! Playback of the open file through the default output device.
//!
//! The audio callback never blocks or allocates: it `try_lock`s the shared buffer and plays
//! silence if the UI holds it. The file's rate is converted to the device's with linear
//! interpolation, which is fine for auditioning (an edit never goes through this path).

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use wavcraft_core::AudioBuffer;
use wavcraft_core::metronome::{ClickTrack, click_sound};
use wavcraft_core::tempo::Tempo;

struct Shared {
    audio: Mutex<Option<Arc<AudioBuffer>>>,
    /// Play position in file frames, an `f64` stored as bits.
    pos: AtomicU64,
    /// Frame to stop at, an `f64` stored as bits.
    end: AtomicU64,
    playing: AtomicBool,
    /// Set by the stream's error callback (the device went away, for example).
    failed: AtomicBool,
    /// Metronome (roadmap C201). Mixed into the output here and nowhere else.
    metro_on: AtomicBool,
    /// Click volume, an `f32` stored as bits.
    metro_volume: AtomicU32,
    /// Frames in one beat of the file's rate, an `f64` stored as bits.
    metro_beat: AtomicU64,
    metro_numerator: AtomicU32,
    /// File frame beat 0 of the click grid falls on (before the start when there is a count-in), an `f64` stored as bits.
    metro_origin: AtomicU64,
    /// The file's own audio starts sounding at this frame (after a count-in), an `f64` stored as bits.
    audio_from: AtomicU64,
    /// Bumped when the play position jumps or the metronome changes: the callback finds its next beat again.
    generation: AtomicU32,
}

impl Shared {
    fn new(audio: Option<Arc<AudioBuffer>>) -> Shared {
        Shared {
            audio: Mutex::new(audio),
            pos: AtomicU64::new(0f64.to_bits()),
            end: AtomicU64::new(0f64.to_bits()),
            playing: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            metro_on: AtomicBool::new(false),
            metro_volume: AtomicU32::new(0.5f32.to_bits()),
            metro_beat: AtomicU64::new(0f64.to_bits()),
            metro_numerator: AtomicU32::new(4),
            metro_origin: AtomicU64::new(0f64.to_bits()),
            audio_from: AtomicU64::new(0f64.to_bits()),
            generation: AtomicU32::new(0),
        }
    }
}

/// The metronome's settings as the app keeps them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metronome {
    pub on: bool,
    /// 0 to 1.
    pub volume: f32,
    /// Bars of clicks before playback starts.
    pub count_in_bars: u32,
}

impl Default for Metronome {
    fn default() -> Self {
        Metronome { on: false, volume: 0.5, count_in_bars: 0 }
    }
}

/// The audio thread's own state for the clicks: the sounds (made when the stream opens, so the
/// callback allocates nothing), the click now sounding and the next beat to start.
struct Clicker {
    accent: Vec<f32>,
    normal: Vec<f32>,
    /// (accent, frames already played) of the click sounding now.
    voice: Option<(bool, usize)>,
    seen: u32,
    next_beat: u64,
    next_frame: f64,
}

impl Clicker {
    fn new(device_rate: f64) -> Clicker {
        let rate = device_rate.round().max(1.0) as u32;
        Clicker { accent: click_sound(rate, true), normal: click_sound(rate, false), voice: None, seen: u32::MAX, next_beat: 0, next_frame: 0.0 }
    }
}

/// The output stream and the play state shared with its callback.
pub struct Player {
    shared: Arc<Shared>,
    stream: Option<cpal::Stream>,
    /// Why there is no sound, if the device could not be opened.
    pub error: Option<String>,
    /// The output device's name, for the status bar.
    pub device: String,
    metronome: Metronome,
    tempo: Tempo,
}

impl Player {
    /// Open the default output device. Without one the player still works, silently.
    pub fn new() -> Player {
        let shared = Arc::new(Shared::new(None));
        let mut player =
            Player { shared, stream: None, error: None, device: String::new(), metronome: Metronome::default(), tempo: Tempo::default() };
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
        self.publish_metronome();
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
        let lead = self.count_in_frames();
        self.shared.audio_from.store((from as f64).to_bits(), Ordering::Relaxed);
        // With a count-in the click grid starts at the count-in's first beat, so bar 1 of it is accented
        // and the bars after it carry on the same grid; otherwise the grid is the document's.
        let origin = if lead > 0.0 { from as f64 - lead } else { 0.0 };
        self.shared.metro_origin.store(origin.to_bits(), Ordering::Relaxed);
        self.shared.pos.store((from as f64 - lead).to_bits(), Ordering::Relaxed);
        self.shared.end.store((to as f64).to_bits(), Ordering::Relaxed);
        self.shared.generation.fetch_add(1, Ordering::Relaxed);
        self.shared.playing.store(to > from, Ordering::Relaxed);
    }

    /// Set the metronome and the document's tempo it follows (takes effect at once, also while playing).
    pub fn set_metronome(&mut self, metronome: Metronome, tempo: Tempo) {
        self.metronome = metronome;
        self.tempo = tempo;
        self.publish_metronome();
    }

    pub fn metronome(&self) -> Metronome {
        self.metronome
    }

    fn file_rate(&self) -> u32 {
        self.shared.audio.lock().ok().and_then(|a| a.as_ref().map(|a| a.sample_rate)).unwrap_or(0)
    }

    /// Frames of count-in before a Play, in file frames: 0 unless the metronome is on.
    fn count_in_frames(&self) -> f64 {
        let rate = self.file_rate();
        if !self.metronome.on || self.metronome.count_in_bars == 0 || rate == 0 {
            return 0.0;
        }
        ClickTrack::new(self.tempo, rate).bars_frames(self.metronome.count_in_bars) as f64
    }

    fn publish_metronome(&self) {
        let rate = self.file_rate();
        let s = &self.shared;
        s.metro_on.store(self.metronome.on, Ordering::Relaxed);
        s.metro_volume.store(self.metronome.volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        s.metro_numerator.store(self.tempo.numerator, Ordering::Relaxed);
        s.metro_beat.store((ClickTrack::new(self.tempo, rate.max(1)).beat_frames()).to_bits(), Ordering::Relaxed);
        s.generation.fetch_add(1, Ordering::Relaxed);
    }

    /// The frames (from `from`, in the file's rate) at which the metronome's clicks sound, found by
    /// running the output mixer itself into a buffer over silence. Used by the control channel to check
    /// the mix without a sound device; at most two minutes.
    pub fn render_clicks(&self, from: usize, to: usize) -> Vec<usize> {
        let rate = self.file_rate();
        let to = to.min(from + 120 * rate as usize);
        if rate == 0 || to <= from {
            return Vec::new();
        }
        let len = to - from;
        let silent = Arc::new(AudioBuffer { sample_rate: rate, channels: vec![vec![0.0; len]] });
        let shared = Shared::new(Some(silent));
        let s = &self.shared;
        shared.metro_volume.store(s.metro_volume.load(Ordering::Relaxed), Ordering::Relaxed);
        shared.metro_beat.store(s.metro_beat.load(Ordering::Relaxed), Ordering::Relaxed);
        shared.metro_numerator.store(s.metro_numerator.load(Ordering::Relaxed), Ordering::Relaxed);
        shared.metro_on.store(s.metro_on.load(Ordering::Relaxed), Ordering::Relaxed);
        // Positions are counted from `from`, so the grid's origin sits `from` frames before 0.
        shared.metro_origin.store((-(from as f64)).to_bits(), Ordering::Relaxed);
        shared.audio_from.store(0f64.to_bits(), Ordering::Relaxed);
        shared.pos.store(0f64.to_bits(), Ordering::Relaxed);
        shared.end.store((len as f64).to_bits(), Ordering::Relaxed);
        shared.playing.store(true, Ordering::Relaxed);
        let mut clicker = Clicker::new(f64::from(rate));
        let mut onsets = Vec::new();
        let mut quiet = 1_000usize;
        let mut at = from;
        let mut buf = vec![0.0f32; 2 * 4096];
        while shared.playing.load(Ordering::Relaxed) {
            render(&shared, &mut buf, 2, f64::from(rate), &mut clicker);
            for (k, frame) in buf.chunks(2).enumerate() {
                if frame[0] != 0.0 {
                    if quiet >= 256 {
                        onsets.push(at + k);
                    }
                    quiet = 0;
                } else {
                    quiet += 1;
                }
            }
            at += 4096;
            if at > to + 4096 {
                break;
            }
        }
        onsets
    }

    pub fn stop(&self) {
        self.shared.playing.store(false, Ordering::Relaxed);
    }

    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }

    /// The frame being played right now.
    pub fn position(&self) -> usize {
        // During a count-in the playhead waits at the start.
        let from = f64::from_bits(self.shared.audio_from.load(Ordering::Relaxed));
        f64::from_bits(self.shared.pos.load(Ordering::Relaxed)).max(from).max(0.0) as usize
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
    // The click sounds are made here, before the stream runs, so the callback allocates nothing.
    let mut clicker = Clicker::new(device_rate);
    device
        .build_output_stream(
            config,
            move |out: &mut [T], _: &cpal::OutputCallbackInfo| render(&shared, out, channels, device_rate, &mut clicker),
            move |err| {
                log::warn!("audio output: {err}");
                on_error.failed.store(true, Ordering::Relaxed);
                on_error.playing.store(false, Ordering::Relaxed);
            },
            None,
        )
        .map_err(|e| e.to_string())
}

/// Fill one device buffer. Runs on the audio thread: no allocation, no blocking.
fn render<T: cpal::Sample + cpal::FromSample<f32>>(shared: &Shared, out: &mut [T], channels: usize, device_rate: f64, clicker: &mut Clicker) {
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
    let mut pos = f64::from_bits(shared.pos.load(Ordering::Relaxed));
    let audio_from = f64::from_bits(shared.audio_from.load(Ordering::Relaxed));

    // The metronome's settings and, when the position jumped or the tempo changed, its next beat.
    let metro = shared.metro_on.load(Ordering::Relaxed);
    let beat = f64::from_bits(shared.metro_beat.load(Ordering::Relaxed));
    let metro = metro && beat > 0.0;
    let origin = f64::from_bits(shared.metro_origin.load(Ordering::Relaxed));
    let numerator = u64::from(shared.metro_numerator.load(Ordering::Relaxed).max(1));
    let volume = f32::from_bits(shared.metro_volume.load(Ordering::Relaxed));
    let generation = shared.generation.load(Ordering::Relaxed);
    let beat_frame = |n: u64| origin + (n as f64 * beat).round();
    if metro && clicker.seen != generation {
        clicker.seen = generation;
        clicker.voice = None;
        let mut n = (((pos - origin) / beat).floor().max(0.0)) as u64;
        while n > 0 && beat_frame(n - 1) >= pos {
            n -= 1;
        }
        while beat_frame(n) < pos {
            n += 1;
        }
        clicker.next_beat = n;
        clicker.next_frame = beat_frame(n);
    }

    for frame in out.chunks_mut(channels) {
        if pos >= end {
            shared.playing.store(false, Ordering::Relaxed);
            break;
        }
        if metro {
            while clicker.next_frame <= pos {
                clicker.voice = Some((clicker.next_beat.is_multiple_of(numerator), 0));
                clicker.next_beat += 1;
                clicker.next_frame = beat_frame(clicker.next_beat);
            }
        }
        // The click sounding now, if any, scaled by the volume; the same on the first two channels.
        let mut click = 0.0f32;
        if let Some((accent, at)) = clicker.voice {
            let sound = if accent { &clicker.accent } else { &clicker.normal };
            match sound.get(at) {
                Some(v) => {
                    click = v * volume;
                    clicker.voice = Some((accent, at + 1));
                }
                None => clicker.voice = None,
            }
        }
        let sounding = pos >= audio_from;
        let i = pos.max(0.0) as usize;
        let frac = (pos.max(0.0) - i as f64) as f32;
        let sample = |source: &Vec<f32>| {
            if !sounding {
                return 0.0;
            }
            let a = source.get(i).copied().unwrap_or(0.0);
            let b = source.get(i + 1).copied().unwrap_or(a);
            a + (b - a) * frac
        };
        if channels == 1 && file_channels > 1 {
            // A one-channel device gets the average of the file's channels, not just the left
            // one (a host-left, guest-right recording would lose the guest).
            let sum: f32 = audio.channels.iter().map(sample).sum();
            if let Some(slot) = frame.first_mut() {
                *slot = T::from_sample(sum / file_channels as f32 + click);
            }
        } else {
            for (c, slot) in frame.iter_mut().enumerate() {
                // Mono plays on both speakers; a stereo file leaves any further device channels silent.
                let source = if file_channels == 1 { audio.channels.first() } else { audio.channels.get(c) };
                let value = source.filter(|_| c < file_channels.max(2)).map_or(0.0, sample);
                let click = if c < 2 { click } else { 0.0 };
                if value != 0.0 || click != 0.0 {
                    *slot = T::from_sample(value + click);
                }
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
        Shared::new(Some(Arc::new(audio)))
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
        render(&shared, &mut out, 2, 48_000.0, &mut Clicker::new(48_000.0));
        assert!(out.iter().all(|v| *v == 0.0), "stopped: silence");
        play(&shared, 10.0, 20.0);
        render(&shared, &mut out, 2, 48_000.0, &mut Clicker::new(48_000.0));
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
        render(&shared, &mut out, 4, 48_000.0, &mut Clicker::new(48_000.0));
        for k in 0..8 {
            let want = k as f32 * 0.5;
            assert_eq!(out[4 * k..4 * k + 4], [want, want, 0.0, 0.0], "frame {k}");
        }
        // The end of the range is clamped to the file, and integer formats convert.
        play(&shared, 998.0, 1e12);
        let mut ints = vec![7i16; 8];
        render(&shared, &mut ints, 1, 24_000.0, &mut Clicker::new(24_000.0));
        assert!(ints[0] != 0 && ints[1] != 0 && ints[2..].iter().all(|v| *v == 0), "{ints:?}");
    }

    /// Regression: a one-channel device played only the file's left channel.
    #[test]
    fn a_mono_device_gets_every_channel_of_the_file() {
        let shared = shared_with(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.0; 100], vec![0.5; 100]] });
        play(&shared, 0.0, 100.0);
        let mut out = vec![9.0f32; 16];
        render(&shared, &mut out, 1, 48_000.0, &mut Clicker::new(48_000.0));
        assert!(out.iter().all(|v| *v == 0.25), "left silent, right at 0.5: {out:?}");
    }

    #[test]
    fn a_busy_or_empty_buffer_is_silence() {
        let shared = shared_with(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.5; 100]] });
        play(&shared, 0.0, 100.0);
        let mut out = vec![1.0f32; 16];
        {
            let _held = shared.audio.lock().unwrap();
            render(&shared, &mut out, 2, 48_000.0, &mut Clicker::new(48_000.0));
        }
        assert!(out.iter().all(|v| *v == 0.0), "the UI held the buffer: silence, no blocking");
        *shared.audio.lock().unwrap() = None;
        render(&shared, &mut out, 2, 48_000.0, &mut Clicker::new(48_000.0));
        assert!(out.iter().all(|v| *v == 0.0));
    }

    fn metro(shared: &Shared, bpm: f64, numerator: u32, rate: u32, volume: f32) {
        let beat = ClickTrack::new(Tempo::new(bpm, numerator, 4).unwrap(), rate).beat_frames();
        shared.metro_on.store(true, Ordering::Relaxed);
        shared.metro_beat.store(beat.to_bits(), Ordering::Relaxed);
        shared.metro_numerator.store(numerator, Ordering::Relaxed);
        shared.metro_volume.store(volume.to_bits(), Ordering::Relaxed);
    }

    #[test]
    fn the_mixer_puts_an_accent_on_beat_1_and_a_click_on_each_beat_at_the_tempo() {
        // 120 bpm 4/4 at 48 kHz: a beat every 24,000 frames, the accent on beats 0 and 4.
        let shared = shared_with(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.0; 6 * 24_000]] });
        metro(&shared, 120.0, 4, 48_000, 1.0);
        play(&shared, 0.0, 144_000.0);
        let mut out = vec![0.0f32; 2 * 144_000];
        let mut clicker = Clicker::new(48_000.0);
        for chunk in out.chunks_mut(2 * 512) {
            render(&shared, chunk, 2, 48_000.0, &mut clicker);
        }
        let (accent, normal) = (click_sound(48_000, true), click_sound(48_000, false));
        for beat in 0..6usize {
            let sound = if beat.is_multiple_of(4) { &accent } else { &normal };
            for (k, v) in sound.iter().enumerate() {
                let at = 2 * (beat * 24_000 + k);
                assert_eq!((out[at], out[at + 1]), (*v, *v), "beat {beat} frame {k}");
            }
        }
        let loud: Vec<usize> = (0..144_000).filter(|f| out[2 * f] != 0.0).collect();
        assert!(loud.iter().all(|f| f % 24_000 < accent.len()), "sound only where a click is");
    }

    #[test]
    fn volume_scales_the_click_and_off_is_silence_and_the_file_is_untouched() {
        let shared = shared_with(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.0; 1000]] });
        metro(&shared, 120.0, 4, 48_000, 0.5);
        play(&shared, 0.0, 1000.0);
        let mut out = vec![0.0f32; 2 * 1000];
        render(&shared, &mut out, 2, 48_000.0, &mut Clicker::new(48_000.0));
        assert_eq!(out[0], click_sound(48_000, true)[0] * 0.5);
        shared.metro_on.store(false, Ordering::Relaxed);
        play(&shared, 0.0, 1000.0);
        render(&shared, &mut out, 2, 48_000.0, &mut Clicker::new(48_000.0));
        assert!(out.iter().all(|v| *v == 0.0), "off: nothing");
        // The document's samples never change: the click is added to the output only.
        let guard = shared.audio.lock().unwrap();
        assert!(guard.as_ref().unwrap().channels[0].iter().all(|v| *v == 0.0));
    }

    #[test]
    fn a_click_is_added_to_the_audio_and_starts_where_play_starts() {
        let shared = shared_with(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.25; 48_000]] });
        metro(&shared, 120.0, 4, 48_000, 1.0);
        // Start part-way through beat 0 at frame 100: the next click is beat 1 at 24,000.
        play(&shared, 100.0, 30_000.0);
        let mut out = vec![0.0f32; 2 * 30_000];
        render(&shared, &mut out, 2, 48_000.0, &mut Clicker::new(48_000.0));
        assert_eq!(out[0], 0.25, "no click before the next beat");
        let at = 2 * (24_000 - 100);
        assert_eq!(out[at], 0.25 + click_sound(48_000, false)[0]);
    }

    #[test]
    fn a_count_in_clicks_before_the_audio_which_starts_after_it() {
        // 120 bpm 4/4, 1 bar of count-in = 96,000 frames at 48 kHz; play from frame 0.
        let bar = 96_000.0f64;
        let shared = shared_with(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.5; 200_000]] });
        metro(&shared, 120.0, 4, 48_000, 1.0);
        shared.metro_origin.store((-bar).to_bits(), Ordering::Relaxed);
        shared.audio_from.store(0f64.to_bits(), Ordering::Relaxed);
        shared.pos.store((-bar).to_bits(), Ordering::Relaxed);
        shared.end.store(100_000.0f64.to_bits(), Ordering::Relaxed);
        shared.playing.store(true, Ordering::Relaxed);
        let mut out = vec![0.0f32; 2 * 196_000];
        let mut clicker = Clicker::new(48_000.0);
        for chunk in out.chunks_mut(2 * 1000) {
            render(&shared, chunk, 2, 48_000.0, &mut clicker);
        }
        // Count-in: only clicks (first is the accent), four of them, 24,000 apart.
        let accent = click_sound(48_000, true);
        let normal = click_sound(48_000, false);
        for beat in 0..4usize {
            let sound = if beat == 0 { &accent } else { &normal };
            assert_eq!(out[2 * beat * 24_000], sound[0], "count-in beat {beat}");
        }
        assert!((0..96_000).all(|f| out[2 * f].abs() <= 0.8), "no file audio in the count-in");
        assert!(out[2 * 90_000..2 * 95_000].iter().all(|v| *v == 0.0), "quiet between the last click and the audio");
        // Then the audio at 0.5 plus the accent of the bar the count-in leads into.
        assert_eq!(out[2 * 96_000], 0.5 + accent[0]);
    }
}
