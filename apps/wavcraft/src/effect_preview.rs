//! The preview worker for an effect dialog (E6.1): a thread that owns its own copy of the
//! document, loops the selection through an effect with [`Preview`] in blocks, and writes the
//! result into a [`MonitorRing`] for the output to play. New settings arrive over a channel and
//! take effect at the current position. E6.2 starts it from the control channel; E6.4 resamples
//! its blocks for the device rate.

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
}

/// A running preview. Dropping it stops the worker and waits for it to end.
pub struct EffectPreview {
    tx: Sender<Msg>,
    ring: Arc<MonitorRing>,
    error: Arc<Mutex<Option<String>>>,
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
        let doc_channels = doc.channel_count();
        let ring = MonitorRing::new(RING_FRAMES, u16::try_from(doc_channels).unwrap_or(u16::MAX));
        let (tx, rx) = mpsc::channel();
        let error = Arc::new(Mutex::new(None));
        let output = Output { ring: Arc::clone(&ring), rx, error: Arc::clone(&error) };
        let handle = thread::Builder::new().name("effect-preview".into()).spawn(move || run(doc, effect, params, range, channels, output))?;
        Ok(EffectPreview { tx, ring, error, handle: Some(handle) })
    }

    /// The ring the worker fills, for the output to read.
    pub fn ring(&self) -> Arc<MonitorRing> {
        Arc::clone(&self.ring)
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

/// The worker's loop: take settings, then render one block into the ring when there is room.
fn run(doc: DocState, effect: &dyn Effect, params: Vec<f64>, range: Range<u64>, channels: ChannelMask, output: Output) {
    let Output { ring, rx, error } = output;
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
            let mut refs: Vec<&mut [f32]> = bufs.iter_mut().map(Vec::as_mut_slice).collect();
            if let Err(err) = preview.render(&mut refs) {
                set_error(&error, err.to_string());
                return;
            }
            for (c, buf) in bufs.iter().enumerate() {
                for (f, &sample) in buf.iter().enumerate() {
                    inter[f * doc_channels + c] = sample;
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

    fn doc_of(channels: Vec<Vec<f32>>) -> DocState {
        Document::from_buffer(AudioBuffer { sample_rate: 48_000, channels }).unwrap().state
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
        while worker.error().is_none() {
            assert!(start.elapsed() < Duration::from_secs(2), "no error reported within 2 s");
            thread::sleep(Duration::from_millis(10));
        }
        assert!(!worker.is_running());
    }

    #[test]
    fn e6_worker_stops_on_drop() {
        let doc = doc_of(vec![vec![0.5f32; 4800]]);
        let worker = EffectPreview::start(doc, &Amplify, vec![0.0, 0.0, 1.0], 0..4800, ChannelMask::ALL).unwrap();
        let ring = worker.ring();
        thread::sleep(Duration::from_millis(50));
        drop(worker);
        let waiting = ring.waiting_frames();
        thread::sleep(Duration::from_millis(50));
        assert_eq!(ring.waiting_frames(), waiting);
    }
}
