//! The preview worker for an effect dialog (E6.1): a thread that owns its own copy of the
//! document, loops the selection through an effect with [`Preview`] in blocks, and writes the
//! result into a [`MonitorRing`] for the output to play. New settings arrive over a channel and
//! take effect at the current position. When the output rate differs from the file rate (E6.4),
//! the blocks are resampled with [`Resampler`] before they go into the ring. E6.2 starts it from
//! the control channel.

// Not called yet: E6.2 starts the worker.
#![allow(dead_code)]

use std::io;
use std::ops::Range;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use wavcraft_core::DocState;
use wavcraft_core::doc::edit::ChannelMask;
use wavcraft_core::effects::Effect;
use wavcraft_core::effects::preview::Preview;
use wavcraft_core::record::MonitorRing;
use wavcraft_core::resample::{Quality, Resampler};

/// The ring's size in frames. Small on purpose: the delay it can add stays short.
const RING_FRAMES: usize = 4096;
/// Frames per render call.
const BLOCK: usize = 1024;
/// The worker keeps at most this many frames waiting in the ring. The ring skips frames once
/// three quarters (3072) are waiting, so this keeps every frame and bounds the delay.
const TARGET_FILL_FRAMES: usize = 2048;

enum Msg {
    Params(Vec<f64>),
    Stop,
}

/// What the worker writes to: the ring it fills, the channel it takes settings from, and the
/// slot it reports a failure in.
struct Output {
    ring: Arc<MonitorRing>,
    rx: Receiver<Msg>,
    error: Arc<Mutex<Option<String>>>,
    out_rate: u32,
}

/// A running preview. Dropping it stops the worker and waits for it to end.
pub struct EffectPreview {
    tx: Sender<Msg>,
    ring: Arc<MonitorRing>,
    error: Arc<Mutex<Option<String>>>,
    out_rate: u32,
    handle: Option<JoinHandle<()>>,
}

impl EffectPreview {
    /// Start a worker that loops `range` (the whole file when empty) through `effect` with
    /// `params`, on its own copy of `doc`. A spawn failure is returned; a bad selection or
    /// settings are reported through [`EffectPreview::error`] once the worker has run.
    pub fn start(
        doc: DocState,
        effect: &'static dyn Effect,
        params: Vec<f64>,
        range: Range<u64>,
        channels: ChannelMask,
    ) -> io::Result<EffectPreview> {
        let rate = doc.sample_rate();
        EffectPreview::start_at_rate(doc, effect, params, range, channels, rate)
    }

    /// Like [`EffectPreview::start`], but the ring holds frames at `out_rate` (the output
    /// device's rate): the blocks are resampled when it differs from the file's rate. An
    /// `out_rate` of 0 means the file rate.
    pub fn start_at_rate(
        doc: DocState,
        effect: &'static dyn Effect,
        params: Vec<f64>,
        range: Range<u64>,
        channels: ChannelMask,
        out_rate: u32,
    ) -> io::Result<EffectPreview> {
        let out_rate = if out_rate == 0 { doc.sample_rate() } else { out_rate };
        let doc_channels = doc.channel_count();
        let ring = MonitorRing::new(RING_FRAMES, u16::try_from(doc_channels).unwrap_or(u16::MAX));
        let (tx, rx) = mpsc::channel();
        let error = Arc::new(Mutex::new(None));
        let output = Output { ring: Arc::clone(&ring), rx, error: Arc::clone(&error), out_rate };
        let handle = thread::Builder::new().name("effect-preview".into()).spawn(move || run(doc, effect, params, range, channels, output))?;
        Ok(EffectPreview { tx, ring, error, out_rate, handle: Some(handle) })
    }

    /// The ring the worker fills, for the output to read.
    pub fn ring(&self) -> Arc<MonitorRing> {
        Arc::clone(&self.ring)
    }

    /// The rate of the frames in the ring, for `Player::set_monitor`.
    pub fn out_rate(&self) -> u32 {
        self.out_rate
    }

    /// Send new settings. They take effect at the current position; a dead worker is reported
    /// through [`EffectPreview::error`].
    pub fn set_params(&self, params: Vec<f64>) {
        let _ = self.tx.send(Msg::Params(params));
    }

    /// The first failure the worker reported, if any.
    pub fn error(&self) -> Option<String> {
        self.error.lock().ok().and_then(|slot| slot.clone())
    }

    /// Whether the worker thread is still running.
    pub fn is_running(&self) -> bool {
        self.handle.as_ref().is_some_and(|h| !h.is_finished())
    }

    /// Stop the worker and wait for it to end; the same as dropping the preview.
    pub fn stop(self) {
        drop(self);
    }
}

impl Drop for EffectPreview {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Record a failure in the slot; a poisoned slot is left as it is.
fn set_error(slot: &Mutex<Option<String>>, message: String) {
    if let Ok(mut current) = slot.lock() {
        *current = Some(message);
    }
}

/// Converts file-rate blocks to the output rate, keeping only the file frames still needed.
struct Converter {
    resampler: Resampler,
    /// Per channel, the file-rate frames from absolute file frame `base` onward.
    src: Vec<Vec<f32>>,
    base: u64,
    next_out: u64,
}

impl Converter {
    fn new(file_rate: u32, out_rate: u32, channels: usize) -> Converter {
        Converter {
            resampler: Resampler::new(file_rate, out_rate, Quality::High),
            src: (0..channels).map(|_| Vec::new()).collect(),
            base: 0,
            next_out: 0,
        }
    }

    /// The file frame count held so far.
    fn have_until(&self) -> u64 {
        self.base + self.src.first().map_or(0, Vec::len) as u64
    }

    /// The file frame count that must be held before `count` (at least 1) output frames can be made.
    fn need_until(&self, count: usize) -> u64 {
        let last_x = self.resampler.doc_position(0.0, self.next_out + count as u64 - 1);
        (last_x + self.resampler.half_width()).floor() as u64 + 1
    }

    fn push_source(&mut self, bufs: &[Vec<f32>]) {
        for (held, block) in self.src.iter_mut().zip(bufs) {
            held.extend_from_slice(block);
        }
    }

    /// Make `count` output frames into `inter` (interleaved), then drop file frames no longer needed.
    fn make(&mut self, count: usize, inter: &mut [f32]) {
        let channels = self.src.len();
        for i in 0..count {
            let x = self.resampler.doc_position(0.0, self.next_out + i as u64);
            for c in 0..channels {
                let value = self.resampler.sample_with(x, |k| {
                    usize::try_from(k - self.base as i64).ok().and_then(|j| self.src.get(c).and_then(|ch| ch.get(j))).copied().unwrap_or(0.0)
                });
                if let Some(slot) = inter.get_mut(i * channels + c) {
                    *slot = value;
                }
            }
        }
        self.next_out += count as u64;
        let keep = ((self.resampler.doc_position(0.0, self.next_out) - self.resampler.half_width()).floor() as i64 - 1).max(0) as u64;
        if keep > self.base {
            let n = (keep - self.base) as usize;
            for ch in &mut self.src {
                ch.drain(..n.min(ch.len()));
            }
            self.base += n as u64;
        }
    }
}

/// Render one block into `bufs`; on failure record it and return `None`.
fn render_block(preview: &mut Preview, bufs: &mut [Vec<f32>], error: &Mutex<Option<String>>) -> Option<()> {
    let mut refs: Vec<&mut [f32]> = bufs.iter_mut().map(Vec::as_mut_slice).collect();
    match preview.render(&mut refs) {
        Ok(_) => Some(()),
        Err(err) => {
            set_error(error, err.to_string());
            None
        }
    }
}

/// Interleave per-channel blocks into `inter`.
fn interleave(bufs: &[Vec<f32>], inter: &mut [f32], channels: usize) {
    for (c, buf) in bufs.iter().enumerate() {
        for (f, &sample) in buf.iter().enumerate() {
            if let Some(slot) = inter.get_mut(f * channels + c) {
                *slot = sample;
            }
        }
    }
}

/// The worker's loop: take settings, then render one block into the ring when there is room.
fn run(doc: DocState, effect: &dyn Effect, params: Vec<f64>, range: Range<u64>, channels: ChannelMask, output: Output) {
    let Output { ring, rx, error, out_rate } = output;
    let doc_channels = doc.channel_count();
    let mut preview = match Preview::new(effect, params, &doc, range, channels) {
        Ok(preview) => preview,
        Err(err) => {
            set_error(&error, err.to_string());
            return;
        }
    };
    let mut bufs: Vec<Vec<f32>> = (0..doc_channels).map(|_| vec![0.0; BLOCK]).collect();
    let mut inter: Vec<f32> = vec![0.0; BLOCK * doc_channels];
    let file_rate = doc.sample_rate();
    let mut conv = if out_rate == file_rate { None } else { Some(Converter::new(file_rate, out_rate, doc_channels)) };
    loop {
        loop {
            match rx.try_recv() {
                Ok(Msg::Params(new_params)) => {
                    if let Err(err) = preview.set_params(new_params) {
                        set_error(&error, err.to_string());
                    }
                }
                Ok(Msg::Stop) | Err(TryRecvError::Disconnected) => return,
                Err(TryRecvError::Empty) => break,
            }
        }
        if ring.waiting_frames() + BLOCK <= TARGET_FILL_FRAMES {
            match conv.as_mut() {
                None => {
                    if render_block(&mut preview, &mut bufs, &error).is_none() {
                        return;
                    }
                    interleave(&bufs, &mut inter, doc_channels);
                }
                Some(conv) => {
                    while conv.have_until() < conv.need_until(BLOCK) {
                        if render_block(&mut preview, &mut bufs, &error).is_none() {
                            return;
                        }
                        conv.push_source(&bufs);
                    }
                    conv.make(BLOCK, &mut inter);
                }
            }
            ring.push(&inter);
        } else {
            thread::sleep(Duration::from_millis(2));
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::time::Instant;
    use wavcraft_core::effects::Amplify;
    use wavcraft_core::{AudioBuffer, Document};

    fn doc_at(rate: u32, channels: Vec<Vec<f32>>) -> DocState {
        Document::from_buffer(AudioBuffer { sample_rate: rate, channels }).unwrap().state
    }

    fn doc_of(channels: Vec<Vec<f32>>) -> DocState {
        doc_at(48_000, channels)
    }

    fn sine(freq: f64, rate: u32, frames: usize) -> Vec<f32> {
        (0..frames).map(|i| (2.0 * std::f64::consts::PI * freq * i as f64 / f64::from(rate)).sin() as f32).collect()
    }

    fn crossings(x: &[f32]) -> usize {
        x.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count()
    }

    /// Read `want` frames from a mono ring, waiting for the worker. Panics after 10 seconds.
    fn read_frames(ring: &MonitorRing, want: usize) -> Vec<f32> {
        let mut out = Vec::new();
        let mut buf = vec![0.0f32; 480];
        let start = Instant::now();
        while out.len() < want {
            let got = ring.pop_into(&mut buf);
            out.extend_from_slice(&buf[..got]);
            if got == 0 {
                thread::sleep(Duration::from_millis(1));
            }
            if start.elapsed() > Duration::from_secs(10) {
                panic!("the preview worker produced {} of {} frames in 10 s", out.len(), want);
            }
        }
        out.truncate(want);
        out
    }

    #[test]
    fn e6_worker_loops_selection() {
        let ramp: Vec<f32> = (0..4800).map(|i| i as f32 / 4800.0).collect();
        let doc = doc_of(vec![ramp.clone()]);
        let worker = EffectPreview::start(doc, &Amplify, vec![0.0, 0.0, 1.0], 0..4800, ChannelMask::ALL).unwrap();
        let ring = worker.ring();
        let got = read_frames(&ring, 9600);
        let mut want = ramp.clone();
        want.extend_from_slice(&ramp);
        assert_eq!(got, want);
        assert_eq!(ring.overflow_frames(), 0);
    }

    #[test]
    fn e6_worker_applies_new_params_within_100_ms() {
        let doc = doc_of(vec![vec![0.5f32; 4800]]);
        let worker = EffectPreview::start(doc, &Amplify, vec![0.0, 0.0, 1.0], 0..4800, ChannelMask::ALL).unwrap();
        let ring = worker.ring();
        let before = read_frames(&ring, 480);
        assert!(before.iter().all(|&v| v == 0.5));
        worker.set_params(vec![-6.020_599_913_279_624, -6.020_599_913_279_624, 1.0]);
        let after = read_frames(&ring, 4800);
        let k = after.iter().position(|&v| (v - 0.25).abs() < 0.001).expect("the new gain never reached 0.25");
        assert!(k <= 4800, "the change took {k} frames");
        assert!(after[k..].iter().all(|&v| (v - 0.25).abs() < 0.001));
    }

    #[test]
    fn e6_worker_reports_a_bad_selection() {
        let doc = doc_of(vec![vec![0.5f32; 100]]);
        let worker = EffectPreview::start(doc, &Amplify, vec![0.0, 0.0, 1.0], 0..1_000_000, ChannelMask::ALL).unwrap();
        let start = Instant::now();
        while worker.error().is_none() || worker.is_running() {
            assert!(start.elapsed() < Duration::from_secs(2), "no error reported and worker ended within 2 s");
            thread::sleep(Duration::from_millis(10));
        }
        assert!(worker.error().is_some());
        assert!(!worker.is_running());
    }

    #[test]
    fn e6_worker_stops_on_drop() {
        let doc = doc_of(vec![vec![0.5f32; 4800]]);
        let worker = EffectPreview::start(doc, &Amplify, vec![0.0, 0.0, 1.0], 0..4800, ChannelMask::ALL).unwrap();
        let ring = worker.ring();
        thread::sleep(Duration::from_millis(50));
        // The worker's own clone of the ring goes away only when its thread ends, so the count
        // is 1 once drop has joined the thread. Without the join the worker still holds it.
        drop(worker);
        assert_eq!(Arc::strong_count(&ring), 1, "drop returned before the worker thread ended");
    }

    #[test]
    fn e6_preview_resamples_44100_to_48000() {
        let doc = doc_at(44_100, vec![sine(1000.0, 44_100, 44_100)]);
        let worker = EffectPreview::start_at_rate(doc, &Amplify, vec![0.0, 0.0, 1.0], 0..44_100, ChannelMask::ALL, 48_000).unwrap();
        let got = read_frames(&worker.ring(), 480 + 48_000);
        let n = crossings(&got[480..]);
        assert!(n.abs_diff(2000) <= 2, "{n} zero crossings");
        assert_eq!(worker.out_rate(), 48_000);
    }

    #[test]
    fn e6_preview_resamples_48000_to_44100() {
        let doc = doc_at(48_000, vec![sine(1000.0, 48_000, 48_000)]);
        let worker = EffectPreview::start_at_rate(doc, &Amplify, vec![0.0, 0.0, 1.0], 0..48_000, ChannelMask::ALL, 44_100).unwrap();
        let got = read_frames(&worker.ring(), 441 + 44_100);
        let n = crossings(&got[441..]);
        assert!(n.abs_diff(2000) <= 2, "{n} zero crossings");
    }

    #[test]
    fn e6_preview_resampled_level_is_kept() {
        let doc = doc_at(44_100, vec![sine(1000.0, 44_100, 44_100)]);
        let worker = EffectPreview::start_at_rate(doc, &Amplify, vec![0.0, 0.0, 1.0], 0..44_100, ChannelMask::ALL, 48_000).unwrap();
        let ring = worker.ring();
        let got = read_frames(&ring, 480 + 48_000);
        let peak = got[480..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak > 0.98 && peak < 1.02, "peak {peak}");
        assert_eq!(ring.overflow_frames(), 0);
    }

    #[test]
    fn e6_preview_same_rate_is_not_resampled() {
        let ramp: Vec<f32> = (0..4800).map(|i| i as f32 / 4800.0).collect();
        let doc = doc_at(48_000, vec![ramp.clone()]);
        let worker = EffectPreview::start_at_rate(doc, &Amplify, vec![0.0, 0.0, 1.0], 0..4800, ChannelMask::ALL, 48_000).unwrap();
        assert_eq!(read_frames(&worker.ring(), 4800), ramp);
    }
}
