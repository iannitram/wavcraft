//! Playback of the open document through the default output device.
//!
//! The audio callback never blocks: it takes no lock, does no disk read and allocates nothing.
//! It plays from a **read-ahead ring** (about 2.7 seconds of the file's frames) that a separate
//! thread keeps filled from the document store ([`DocState::read`], chunk by chunk). If the
//! ring runs dry the callback plays silence for that buffer and tries again on the next, instead
//! of waiting for the disk. The file's rate is converted to the device's with linear
//! interpolation, which is fine for auditioning (an edit never goes through this path).
//!
//! At most [`RING_CHANNELS`] channels of the file are buffered and played.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use wavcraft_core::DocState;

/// Frames the read-ahead ring holds (a power of two).
const RING_FRAMES: usize = 1 << 17;
/// Channels the ring buffers.
const RING_CHANNELS: usize = 8;
/// Frames read from the store at a time.
const FILL_CHUNK: usize = 8192;
/// Frames that must be in the ring before a play starts sounding.
const PRIME_FRAMES: u64 = 16_384;
/// Frames of the ring kept clear ahead of the play position, so a write never touches one the
/// callback is still reading (its interpolation looks one frame ahead).
const RING_MARGIN: u64 = 8;

struct Shared {
    /// The ring: frame `f` of channel `c` lives at `(f % RING_FRAMES) * RING_CHANNELS + c`, as
    /// `f32` bits.
    ring: Vec<AtomicU32>,
    /// File frames `[.., filled_to)` have been written to the ring (the newest `RING_FRAMES` of
    /// them are still there).
    filled_to: AtomicU64,
    /// Play position in file frames, an `f64` stored as bits.
    pos: AtomicU64,
    /// Frame to stop at, an `f64` stored as bits (clamped to the document by the fill thread).
    end: AtomicU64,
    playing: AtomicBool,
    /// Bumped by every play and every change of document.
    epoch: AtomicU64,
    /// The `epoch` the ring was primed for: the callback sounds only when it equals `epoch`.
    ready_epoch: AtomicU64,
    /// The document's sample rate and channel count (0: no document).
    rate: AtomicU32,
    channels: AtomicU32,
    /// Callbacks that found the ring dry (for the log and the tests).
    underruns: AtomicU64,
    /// Set by the stream's error callback (the device went away, for example).
    failed: AtomicBool,
}

impl Shared {
    fn new() -> Shared {
        Shared {
            ring: (0..RING_FRAMES * RING_CHANNELS).map(|_| AtomicU32::new(0)).collect(),
            filled_to: AtomicU64::new(0),
            pos: AtomicU64::new(0f64.to_bits()),
            end: AtomicU64::new(0f64.to_bits()),
            playing: AtomicBool::new(false),
            epoch: AtomicU64::new(1),
            ready_epoch: AtomicU64::new(0),
            rate: AtomicU32::new(0),
            channels: AtomicU32::new(0),
            underruns: AtomicU64::new(0),
            failed: AtomicBool::new(false),
        }
    }

    fn slot(frame: u64, ch: usize) -> usize {
        (frame as usize % RING_FRAMES) * RING_CHANNELS + ch
    }
}

enum Command {
    Document(Option<DocState>),
    Play { from: u64, to: u64, epoch: u64 },
}

/// The output stream, the read-ahead thread and the play state shared with the callback.
pub struct Player {
    shared: Arc<Shared>,
    commands: Sender<Command>,
    stream: Option<cpal::Stream>,
    /// Why there is no sound, if the device could not be opened.
    pub error: Option<String>,
    /// The output device's name, for the status bar.
    pub device: String,
}

impl Player {
    /// Open the default output device. Without one the player still works, silently.
    pub fn new() -> Player {
        let shared = Arc::new(Shared::new());
        let (commands, inbox) = channel();
        let fill_shared = Arc::clone(&shared);
        if let Err(e) = std::thread::Builder::new().name("wavcraft-readahead".to_string()).spawn(move || read_ahead(&fill_shared, &inbox)) {
            log::warn!(target: "wavcraft", "could not start the playback read-ahead thread: {e}");
        }
        let mut player = Player { shared, commands, stream: None, error: None, device: String::new() };
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

    /// Hand over the document to play (stops playback). Nothing is copied: the read-ahead thread
    /// reads it from the store as it plays.
    pub fn set_audio(&self, doc: Option<DocState>) {
        self.stop();
        self.shared.epoch.fetch_add(1, Ordering::Release);
        let _ = self.commands.send(Command::Document(doc));
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
        // The new epoch silences the callback until the read-ahead thread has primed the ring.
        let epoch = self.shared.epoch.fetch_add(1, Ordering::AcqRel) + 1;
        self.shared.playing.store(to > from, Ordering::Release);
        if to > from {
            let _ = self.commands.send(Command::Play { from: from as u64, to: to as u64, epoch });
        }
    }

    pub fn stop(&self) {
        self.shared.playing.store(false, Ordering::Relaxed);
    }

    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }

    /// How many audio callbacks found the read-ahead ring dry since start-up (each played
    /// silence for that buffer).
    pub fn underruns(&self) -> u64 {
        self.shared.underruns.load(Ordering::Relaxed)
    }

    /// The frame being played right now.
    pub fn position(&self) -> usize {
        f64::from_bits(self.shared.pos.load(Ordering::Relaxed)).max(0.0) as usize
    }
}

/// What the read-ahead thread is feeding.
struct Feed {
    state: DocState,
    /// Channels buffered (the document's, at most [`RING_CHANNELS`]).
    channels: usize,
    /// One past the last frame this play needs.
    last: u64,
}

/// Start feeding `[from, to)` of `state`: reset the ring, clamp the end to the document, and fill
/// enough to start. Marks the ring ready for `epoch` unless a newer play has already begun.
fn begin(shared: &Shared, state: &DocState, from: u64, to: u64, epoch: u64) -> Feed {
    let len = state.len_frames();
    let last = to.min(len);
    shared.end.store((last as f64).min(f64::from_bits(shared.end.load(Ordering::Relaxed))).to_bits(), Ordering::Relaxed);
    shared.filled_to.store(from, Ordering::Release);
    let feed = Feed { state: state.clone(), channels: state.channel_count().min(RING_CHANNELS), last };
    let want = (from + PRIME_FRAMES).min(last);
    while shared.filled_to.load(Ordering::Relaxed) < want {
        if !top_up(shared, &feed) {
            break;
        }
    }
    if shared.epoch.load(Ordering::Acquire) == epoch {
        shared.ready_epoch.store(epoch, Ordering::Release);
    }
    feed
}

/// Read the next chunk into the ring if there is room ahead of the play position. Returns whether
/// it wrote anything.
fn top_up(shared: &Shared, feed: &Feed) -> bool {
    let filled = shared.filled_to.load(Ordering::Relaxed);
    let pos = f64::from_bits(shared.pos.load(Ordering::Relaxed)).max(0.0) as u64;
    let room_to = (pos + RING_FRAMES as u64).saturating_sub(RING_MARGIN);
    let target = feed.last.min(room_to);
    if filled >= target {
        return false;
    }
    let n = (target - filled).min(FILL_CHUNK as u64) as usize;
    let mut buf = vec![0.0f32; n];
    for c in 0..feed.channels {
        let got = match feed.state.read(c, filled, &mut buf) {
            Ok(got) => got,
            Err(e) => {
                log::warn!(target: "wavcraft", "playback read from the store failed: {e}");
                0
            }
        };
        // A short or failed read plays as silence for the rest of the chunk.
        if let Some(rest) = buf.get_mut(got..) {
            rest.fill(0.0);
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
    loop {
        if !top_up(shared, feed) {
            break;
        }
    }
}

/// The read-ahead thread: owns the document, primes the ring on a play and keeps it topped up
/// while playing. Ends when the player is dropped.
fn read_ahead(shared: &Shared, inbox: &Receiver<Command>) {
    let mut state: Option<DocState> = None;
    let mut feed: Option<Feed> = None;
    loop {
        let wait = if feed.is_some() && shared.playing.load(Ordering::Relaxed) { Duration::from_millis(3) } else { Duration::from_secs(3600) };
        match inbox.recv_timeout(wait) {
            Ok(Command::Document(doc)) => {
                feed = None;
                shared.rate.store(doc.as_ref().map_or(0, DocState::sample_rate), Ordering::Release);
                shared.channels.store(doc.as_ref().map_or(0, |d| d.channel_count().min(RING_CHANNELS) as u32), Ordering::Release);
                state = doc;
            }
            Ok(Command::Play { from, to, epoch }) => {
                feed = state.as_ref().map(|s| begin(shared, s, from, to, epoch));
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if shared.playing.load(Ordering::Relaxed)
            && let Some(f) = &feed
        {
            fill_all(shared, f);
        }
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

/// Fill one device buffer. Runs on the audio thread: no locks, no disk, no allocation.
fn render<T: cpal::Sample + cpal::FromSample<f32>>(shared: &Shared, out: &mut [T], channels: usize, device_rate: f64) {
    let silence = T::from_sample(0.0f32);
    out.iter_mut().for_each(|s| *s = silence);
    if !shared.playing.load(Ordering::Acquire) {
        return;
    }
    let epoch = shared.epoch.load(Ordering::Acquire);
    if shared.ready_epoch.load(Ordering::Acquire) != epoch {
        return;
    }
    let file_channels = shared.channels.load(Ordering::Acquire) as usize;
    let rate = shared.rate.load(Ordering::Acquire);
    if file_channels == 0 || rate == 0 {
        return;
    }
    let step = f64::from(rate) / device_rate;
    let end = f64::from_bits(shared.end.load(Ordering::Relaxed));
    let last = end.ceil().max(0.0) as u64;
    let start_bits = shared.pos.load(Ordering::Relaxed);
    let mut pos = f64::from_bits(start_bits).max(0.0);
    let mut filled = shared.filled_to.load(Ordering::Acquire);
    let sample = |frame: u64, c: usize| shared.ring.get(Shared::slot(frame, c)).map_or(0.0, |v| f32::from_bits(v.load(Ordering::Relaxed)));
    for frame in out.chunks_mut(channels) {
        if pos >= end {
            shared.playing.store(false, Ordering::Release);
            break;
        }
        let i = pos as u64;
        // This frame and the next (unless the play ends first) must be in the ring.
        let need = (i + 2).min(last);
        if filled < need {
            filled = shared.filled_to.load(Ordering::Acquire);
            if filled < need {
                shared.underruns.fetch_add(1, Ordering::Relaxed);
                break;
            }
        }
        let frac = (pos - i as f64) as f32;
        let interp = |c: usize| {
            let a = sample(i, c);
            let b = if i + 1 < last { sample(i + 1, c) } else { a };
            a + (b - a) * frac
        };
        if channels == 1 && file_channels > 1 {
            // A one-channel device gets the average of the file's channels, not just the left
            // one (a host-left, guest-right recording would lose the guest).
            let sum: f32 = (0..file_channels).map(interp).sum();
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
    // A play started meanwhile has set its own position: only advance the one this callback began at.
    let _ = shared.pos.compare_exchange(start_bits, pos.to_bits(), Ordering::AcqRel, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use wavcraft_core::{AudioBuffer, Document};

    fn doc_of(audio: AudioBuffer) -> DocState {
        Document::from_buffer(audio).unwrap().state
    }

    /// A shared player state with `state` loaded, as the read-ahead thread would leave it.
    fn shared_with(state: &DocState) -> Shared {
        let shared = Shared::new();
        shared.rate.store(state.sample_rate(), Ordering::Relaxed);
        shared.channels.store(state.channel_count().min(RING_CHANNELS) as u32, Ordering::Relaxed);
        shared
    }

    /// Start a play the way `Player::play` and the read-ahead thread do, without the threads.
    fn play(shared: &Shared, state: &DocState, from: f64, to: f64) -> Feed {
        shared.pos.store(from.to_bits(), Ordering::Relaxed);
        shared.end.store(to.to_bits(), Ordering::Relaxed);
        let epoch = shared.epoch.fetch_add(1, Ordering::AcqRel) + 1;
        shared.playing.store(true, Ordering::Release);
        begin(shared, state, from as u64, to as u64, epoch)
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
        assert!(!shared.playing.load(Ordering::Relaxed), "stops at the end of the range");
        assert_eq!(f64::from_bits(shared.pos.load(Ordering::Relaxed)), 20.0);
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

    /// A play longer than the ring, fed in pieces as the callback consumes it, comes out sample
    /// for sample: the ring wraps several times and nothing is skipped or repeated.
    #[test]
    fn a_play_longer_than_the_ring_is_exact() {
        let frames = RING_FRAMES * 3 + 1_234;
        let left: Vec<f32> = (0..frames).map(|i| (i % 4_001) as f32 / 4_001.0).collect();
        let right: Vec<f32> = left.iter().map(|v| 1.0 - v).collect();
        let state = doc_of(AudioBuffer { sample_rate: 48_000, channels: vec![left.clone(), right.clone()] });
        let shared = shared_with(&state);
        let feed = play(&shared, &state, 0.0, frames as f64);
        let mut heard = 0usize;
        let mut buf = vec![0.0f32; 2 * 1024];
        while shared.playing.load(Ordering::Relaxed) {
            render(&shared, &mut buf, 2, 48_000.0);
            let n = (frames - heard).min(1024);
            for k in 0..n {
                assert_eq!((buf[2 * k], buf[2 * k + 1]), (left[heard + k], right[heard + k]), "frame {}", heard + k);
            }
            heard += n;
            fill_all(&shared, &feed);
        }
        assert_eq!(heard, frames);
        assert_eq!(shared.underruns.load(Ordering::Relaxed), 0);
    }

    /// The callback does not wait for the disk: a ring that is not filled in time is silence for
    /// that buffer, the position does not move, and playing carries on once it is filled.
    #[test]
    fn a_dry_ring_is_silence_not_a_wait() {
        let frames = PRIME_FRAMES as usize * 4;
        let ramp: Vec<f32> = (0..frames).map(|i| (i as f32 + 1.0) / frames as f32).collect();
        let state = doc_of(AudioBuffer { sample_rate: 48_000, channels: vec![ramp.clone()] });
        let shared = shared_with(&state);
        let feed = play(&shared, &state, 0.0, frames as f64);
        // Drain what was primed, then ask for more than is there.
        let mut out = vec![0.0f32; PRIME_FRAMES as usize * 2];
        render(&shared, &mut out, 1, 48_000.0);
        let filled = shared.filled_to.load(Ordering::Relaxed) as usize;
        assert!(filled < frames, "only part of the file is in the ring");
        assert!(out[..filled - 1].iter().zip(&ramp).all(|(a, b)| a == b), "what was there is played exactly");
        assert!(out[filled..].iter().all(|v| *v == 0.0), "the rest is silence");
        assert!(shared.underruns.load(Ordering::Relaxed) >= 1);
        let pos = f64::from_bits(shared.pos.load(Ordering::Relaxed)) as usize;
        assert!(pos < filled && pos + 2 > filled - 1, "the position stopped at the end of the filled part: {pos} of {filled}");
        // Filled again: the next buffer carries on from exactly where it stopped.
        fill_all(&shared, &feed);
        let mut next = vec![0.0f32; 64];
        render(&shared, &mut next, 1, 48_000.0);
        assert!(next.iter().zip(&ramp[pos..]).all(|(a, b)| a == b), "carries on at {pos}");
    }

    /// A buffer still being rendered when a new play starts must not drag the position back.
    #[test]
    fn the_callback_does_not_overwrite_a_newer_position() {
        let state = doc_of(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.5; 40_000]] });
        let shared = shared_with(&state);
        play(&shared, &state, 0.0, 40_000.0);
        let begun = shared.pos.load(Ordering::Relaxed);
        shared.pos.store(30_000f64.to_bits(), Ordering::Relaxed);
        assert!(shared.pos.compare_exchange(begun, 10f64.to_bits(), Ordering::AcqRel, Ordering::Relaxed).is_err());
        assert_eq!(f64::from_bits(shared.pos.load(Ordering::Relaxed)), 30_000.0);
    }

    #[test]
    fn no_document_or_a_new_play_not_yet_primed_is_silence() {
        let state = doc_of(AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.5; 100]] });
        let shared = Shared::new();
        shared.pos.store(0f64.to_bits(), Ordering::Relaxed);
        shared.end.store(100f64.to_bits(), Ordering::Relaxed);
        shared.playing.store(true, Ordering::Relaxed);
        let mut out = vec![1.0f32; 16];
        render(&shared, &mut out, 2, 48_000.0);
        assert!(out.iter().all(|v| *v == 0.0), "no document");
        let shared = shared_with(&state);
        play(&shared, &state, 0.0, 100.0);
        shared.epoch.fetch_add(1, Ordering::AcqRel); // a newer play whose ring is not primed yet
        render(&shared, &mut out, 2, 48_000.0);
        assert!(out.iter().all(|v| *v == 0.0), "not primed");
    }

    /// The same, through the real thread: play, wait, and see the position move.
    #[test]
    fn the_read_ahead_thread_primes_and_tops_up() {
        let frames = RING_FRAMES * 2;
        let ramp: Vec<f32> = (0..frames).map(|i| (i % 1000) as f32).collect();
        let state = doc_of(AudioBuffer { sample_rate: 48_000, channels: vec![ramp.clone()] });
        let shared = Arc::new(shared_with(&state));
        let (tx, rx) = channel();
        let thread_shared = Arc::clone(&shared);
        let handle = std::thread::spawn(move || read_ahead(&thread_shared, &rx));
        tx.send(Command::Document(Some(state))).unwrap();
        shared.pos.store(0f64.to_bits(), Ordering::Relaxed);
        shared.end.store((frames as f64).to_bits(), Ordering::Relaxed);
        let epoch = shared.epoch.fetch_add(1, Ordering::AcqRel) + 1;
        shared.playing.store(true, Ordering::Release);
        tx.send(Command::Play { from: 0, to: frames as u64, epoch }).unwrap();
        let mut heard = Vec::new();
        let mut buf = vec![0.0f32; 2048];
        let started = std::time::Instant::now();
        while shared.playing.load(Ordering::Relaxed) {
            assert!(started.elapsed() < Duration::from_secs(20), "stuck at {} frames", heard.len());
            let before = f64::from_bits(shared.pos.load(Ordering::Relaxed)) as usize;
            render(&shared, &mut buf, 1, 48_000.0);
            let after = f64::from_bits(shared.pos.load(Ordering::Relaxed)) as usize;
            heard.extend_from_slice(&buf[..after - before]);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(heard.len(), frames);
        assert!(heard == ramp, "the whole file came out exactly, through the ring");
        drop(tx);
        handle.join().unwrap();
    }
}
