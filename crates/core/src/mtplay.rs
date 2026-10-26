//! Playing a multitrack session (f114, roadmap C074): offline rendering, the playback snapshot, and
//! a silent transport for headless runs. The mix engine (`soundcraft-mix`, the same code the
//! device callback runs) does all the rendering; this module owns no mixing of its own.
//!
//! * [`render`]: the main mix over a frame range, planar (the unit tests' reference).
//! * [`PlaybackSession`]: the session as the audio thread plays it. Every source goes behind a
//!   `ReadAheadSource` (a background thread fills a lock-free ring; the audio callback only pops
//!   and copies, and counts an underrun if the data is late), so a source that reads from disk can
//!   never block the callback. Sources are in RAM today (the disk-backed adapter, issue #315, is
//!   not on this branch); the wrapper is the same for both.
//! * [`SilentPlayer`]: a transport with no device (`WAVCRAFT_NO_AUDIO=1`): a thread that renders
//!   the session through the mix engine at real time and throws the samples away, so position,
//!   stop and underrun counts behave as with a device. It never opens any audio device.

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use soundcraft_mix::{DEFAULT_RING_FRAMES, MixEngine, ReadAheadSource, main_channels, mark_audio_thread};
use soundcraft_model::time::{Range, Samples};
use soundcraft_model::{SampleSource, Session};

use crate::multitrack::Multitrack;

/// The block the silent transport renders at a time (frames).
pub const SILENT_BLOCK: usize = 512;

/// The main mix of `frames` frames starting at `start` (in session frames), planar. Offline, on
/// the calling thread: sources are read directly, not through read-ahead.
pub fn render(mt: &Multitrack, start: Samples, frames: usize) -> Vec<Vec<f32>> {
    soundcraft_mix::render_range(&mt.session, Range::new(start, start + frames as i64), 1024)
}

/// The session as the audio thread plays it, with the read-ahead sources it holds.
pub struct PlaybackSession {
    pub session: Arc<Session>,
    readers: Vec<Arc<ReadAheadSource>>,
}

impl PlaybackSession {
    /// A snapshot of `mt` with every audio source behind a `ReadAheadSource`.
    pub fn new(mt: &Multitrack) -> PlaybackSession {
        let mut session = mt.session.clone();
        let mut readers = Vec::new();
        let ids: Vec<_> = session.pool.ids().collect();
        for id in ids {
            if let Some(src) = session.pool.get_source(id) {
                let ra = Arc::new(ReadAheadSource::new(src, DEFAULT_RING_FRAMES));
                session.pool.insert_stream(id, Arc::clone(&ra) as Arc<dyn SampleSource>);
                readers.push(ra);
            }
        }
        PlaybackSession { session: Arc::new(session), readers }
    }

    /// Audio-thread reads that came up short, over all sources.
    pub fn underruns(&self) -> u64 {
        self.readers.iter().map(|r| r.underruns()).sum()
    }

    /// The session's rate in Hz.
    pub fn rate(&self) -> u32 {
        self.session.sample_rate.hz()
    }
}

struct Shared {
    position: AtomicI64,
    playing: AtomicBool,
    /// Set by `play`: the start position and a fresh clock reference.
    restart: AtomicBool,
    quit: AtomicBool,
}

/// A transport with no audio device: see the module docs.
pub struct SilentPlayer {
    shared: Arc<Shared>,
    snapshot: Arc<Mutex<Arc<PlaybackSession>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl SilentPlayer {
    pub fn new(ps: Arc<PlaybackSession>) -> SilentPlayer {
        let shared = Arc::new(Shared {
            position: AtomicI64::new(0),
            playing: AtomicBool::new(false),
            restart: AtomicBool::new(false),
            quit: AtomicBool::new(false),
        });
        let snapshot = Arc::new(Mutex::new(ps));
        let (sh, sn) = (Arc::clone(&shared), Arc::clone(&snapshot));
        let thread = std::thread::Builder::new().name("wavcraft-silent-clock".into()).spawn(move || run(&sh, &sn)).ok();
        SilentPlayer { shared, snapshot, thread }
    }

    /// Replace the session being played (takes effect at the next `play`).
    pub fn set_session(&self, ps: Arc<PlaybackSession>) {
        *self.snapshot.lock().unwrap_or_else(PoisonError::into_inner) = ps;
    }

    pub fn play(&self, from: Samples) {
        self.shared.position.store(from.max(0), Ordering::SeqCst);
        self.shared.restart.store(true, Ordering::SeqCst);
        self.shared.playing.store(true, Ordering::SeqCst);
    }

    pub fn stop(&self) {
        self.shared.playing.store(false, Ordering::SeqCst);
    }

    pub fn position(&self) -> Samples {
        self.shared.position.load(Ordering::SeqCst)
    }

    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::SeqCst)
    }
}

impl Drop for SilentPlayer {
    fn drop(&mut self) {
        self.shared.quit.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(sh: &Shared, snapshot: &Mutex<Arc<PlaybackSession>>) {
    mark_audio_thread();
    let mut ps = Arc::clone(&snapshot.lock().unwrap_or_else(PoisonError::into_inner));
    let mut engine = MixEngine::new(ps.rate() as f32, SILENT_BLOCK);
    let mut out: Vec<Vec<f32>> = vec![vec![0.0; SILENT_BLOCK]; main_channels(&ps.session).max(2)];
    let (mut origin, mut t0, mut done) = (0_i64, Instant::now(), 0_i64);
    while !sh.quit.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(5));
        if !sh.playing.load(Ordering::SeqCst) {
            continue;
        }
        if sh.restart.swap(false, Ordering::SeqCst) {
            ps = Arc::clone(&snapshot.lock().unwrap_or_else(PoisonError::into_inner));
            engine = MixEngine::new(ps.rate() as f32, SILENT_BLOCK);
            out = vec![vec![0.0; SILENT_BLOCK]; main_channels(&ps.session).max(2)];
            origin = sh.position.load(Ordering::SeqCst);
            t0 = Instant::now();
            done = 0;
        }
        let due = (t0.elapsed().as_secs_f64() * f64::from(ps.rate())) as i64;
        while done + (SILENT_BLOCK as i64) <= due && sh.playing.load(Ordering::SeqCst) {
            engine.render(&ps.session, origin + done, SILENT_BLOCK, &mut out);
            done += SILENT_BLOCK as i64;
            sh.position.store(origin + done, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soundcraft_audio_io::AudioBuffer;
    use soundcraft_model::BitDepthSetting;

    fn tone(rate: u32, secs: f32, freq: f32, amp: f32) -> AudioBuffer {
        let n = (rate as f32 * secs) as usize;
        let ch: Vec<f32> = (0..n).map(|i| (i as f32 / rate as f32 * freq * std::f32::consts::TAU).sin() * amp).collect();
        AudioBuffer { sample_rate: rate, channels: vec![ch.clone(), ch] }
    }

    fn two_tone_session() -> Multitrack {
        let mut mt = Multitrack::new("T", 48_000, BitDepthSetting::Float32).unwrap();
        let a = mt.add_track(None, 2).unwrap();
        let b = mt.add_track(None, 2).unwrap();
        mt.add_clip_buffer(a, "a", "a.wav", tone(48_000, 1.0, 440.0, 0.25), 0.0).unwrap();
        mt.add_clip_buffer(b, "b", "b.wav", tone(48_000, 1.0, 1000.0, 0.125), 0.0).unwrap();
        mt
    }

    #[test]
    fn two_tracks_render_to_the_sum_of_their_tones() {
        let mt = two_tone_session();
        let out = render(&mt, 1000, 4000);
        assert!(out.len() >= 2 && out[0].len() == 4000);
        let (ta, tb) = (tone(48_000, 1.0, 440.0, 0.25), tone(48_000, 1.0, 1000.0, 0.125));
        for (ch, plane) in out.iter().enumerate().take(2) {
            for (i, got) in plane.iter().enumerate() {
                let want = ta.channels[ch][1000 + i] + tb.channels[ch][1000 + i];
                assert!((got - want).abs() < 1e-4, "ch {ch} frame {i}: {got} vs {want}");
            }
        }
    }

    #[test]
    fn a_muted_track_drops_out_of_the_sum() {
        let mut mt = two_tone_session();
        let id = mt.find_track("1").unwrap();
        mt.set_mute(id, true).unwrap();
        let out = render(&mt, 0, 2000);
        let tb = tone(48_000, 1.0, 1000.0, 0.125);
        assert!((0..2000).all(|i| (out[0][i] - tb.channels[0][i]).abs() < 1e-4));
    }

    #[test]
    fn the_playback_session_plays_the_same_samples_through_read_ahead() {
        let mt = two_tone_session();
        let ps = PlaybackSession::new(&mt);
        std::thread::sleep(Duration::from_millis(300)); // let the rings fill
        let mut engine = MixEngine::new(48_000.0, 512);
        let mut out = vec![vec![0.0; 512]; 2];
        let mut got = Vec::new();
        mark_audio_thread();
        for blk in 0..4 {
            engine.render(&ps.session, blk * 512, 512, &mut out);
            got.extend_from_slice(&out[0]);
        }
        let want = render(&mt, 0, 2048);
        assert!(got.iter().zip(&want[0]).all(|(a, b)| (a - b).abs() < 1e-4));
        assert_eq!(ps.underruns(), 0);
    }

    #[test]
    fn the_silent_player_advances_in_real_time_and_stops() {
        let mt = two_tone_session();
        let p = SilentPlayer::new(Arc::new(PlaybackSession::new(&mt)));
        std::thread::sleep(Duration::from_millis(300));
        p.play(0);
        std::thread::sleep(Duration::from_millis(500));
        let at = p.position();
        assert!(p.is_playing());
        assert!((0.3..0.7).contains(&(at as f64 / 48_000.0)), "position {at}");
        p.stop();
        std::thread::sleep(Duration::from_millis(50));
        let held = p.position();
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(p.position(), held);
    }
}
