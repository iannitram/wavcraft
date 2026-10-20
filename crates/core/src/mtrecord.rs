//! Multitrack recording (f120, roadmap C037, C038, C043): arm a track and choose its input
//! channel, record while the other tracks play (overdub), and place each take as a clip at the
//! record start. Takes on a range that already holds clips are numbered and stacked: each new
//! take goes on a new playlist (SoundCraft's own device for passes, `soundcraft_engine::io`'s
//! `add_recording`), made the active one, so the newest sits on top and the older takes stay
//! on their playlists.
//!
//! Arming reuses SoundCraft's: `mixer.record_arm` plus `mixer.input` as `Route::Hardware("In 3")`
//! (stereo tracks "In 3-4"), the form `plan_takes` in the engine reads.
//!
//! Never lose a take: a take is written straight to its own file as it arrives, and the WAV
//! header's sizes are rewritten and synced every [`PATCH_EVERY`] seconds of audio, so a killed
//! process leaves a file that opens with everything up to the last patch. Takes are 32-bit float.
//!
//! Input: `WAVCRAFT_FAKE_INPUT=1` is a synthetic, clock-paced input (channel `c` is a sine at
//! `220 * (c + 1)` Hz, amplitude 0.25) so no device is opened and no microphone is asked for.
//! Capture from a real device is not built yet (see the PR).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use soundcraft_audio_io::AudioBuffer as ModelBuffer;
use soundcraft_model::time::Samples;
use soundcraft_model::{Clip, ClipContent, ClipId, Playlist, Route, Source, SourceAudio, SourceId, TrackId, TrackKind};

use crate::multitrack::{MAX_IMPORT_SAMPLES, MtError, Multitrack};

/// Seconds of audio between header patches (and syncs) of a take file being written.
pub const PATCH_EVERY: f64 = 0.25;
/// Frames the fake input delivers at a time.
pub const FAKE_BLOCK: usize = 480;
/// How many input channels the fake input has.
pub const FAKE_CHANNELS: usize = 8;

/// Whether `WAVCRAFT_FAKE_INPUT=1` is set.
pub fn fake_input() -> bool {
    std::env::var_os("WAVCRAFT_FAKE_INPUT").is_some_and(|v| v == "1")
}

/// Frames `[at, at + n)` of fake input channel `ch` (0-based).
pub fn fake_samples(ch: usize, at: u64, n: usize, rate: u32) -> Vec<f32> {
    let hz = 220.0 * (ch as f64 + 1.0);
    (0..n).map(|i| (0.25 * (2.0 * std::f64::consts::PI * hz * (at + i as u64) as f64 / f64::from(rate)).sin()) as f32).collect()
}

/// A track's input route as text: "In 3", "In 3-4", or "" (none).
pub fn input_label(r: &Route) -> String {
    match r {
        Route::Hardware(h) => h.clone(),
        _ => String::new(),
    }
}

/// The first input channel (0-based) a route names, 0 when it names none.
pub fn first_input(r: &Route) -> usize {
    match r {
        Route::Hardware(h) => {
            h.trim_start_matches("In ").split(['-', ' ']).next().and_then(|n| n.parse::<usize>().ok()).map_or(0, |n| n.saturating_sub(1))
        }
        _ => 0,
    }
}

/// One placed take.
#[derive(Debug, Clone, PartialEq)]
pub struct TakeInfo {
    pub track: TrackId,
    /// 1, 2, 3 ... per track.
    pub number: u32,
    pub start: Samples,
    pub length: Samples,
    pub path: PathBuf,
    /// The track's playlist the take's clip is on.
    pub playlist: usize,
    pub clip: ClipId,
}

/// What one armed track will record: its input channels (0-based) and its take file.
#[derive(Debug, Clone, PartialEq)]
pub struct TakePlan {
    pub track: TrackId,
    pub inputs: Vec<usize>,
    pub path: PathBuf,
}

/// What a stopped recording left: the takes whose files hold audio (placeable) and one message
/// per track that had a problem. A track can be in both: a disk-full mid-take still leaves
/// the audio written before it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StopReport {
    pub written: Vec<Written>,
    pub failures: Vec<String>,
}

/// A take file that was written: where, and how many frames.
#[derive(Debug, Clone, PartialEq)]
pub struct Written {
    pub track: TrackId,
    pub path: PathBuf,
    pub frames: u64,
}

fn err<T>(s: impl Into<String>) -> Result<T, MtError> {
    Err(MtError(s.into()))
}

/// The part of `c` between timeline frames `from..to` (inside the clip), as a new clip: same
/// source with the offset moved on, fades kept only at the end that is still the clip's own.
/// `None` for content that has no source offset to move (MIDI), so the caller drops it.
fn cut_piece(c: &Clip, from: Samples, to: Samples, is_left: bool, mut new_id: impl FnMut() -> ClipId) -> Option<Clip> {
    let skip = from - c.start;
    let adv = if (c.stretch - 1.0).abs() < f64::EPSILON { skip } else { (skip as f64 * c.stretch).round() as Samples };
    let mut n = c.clone();
    n.content = match &c.content {
        ClipContent::Audio { source, offset } => ClipContent::Audio { source: *source, offset: offset + adv },
        ClipContent::Video { source, offset } => ClipContent::Video { source: *source, offset: offset + adv },
        _ => return None,
    };
    n.id = new_id();
    n.start = from;
    n.length = (to - from).max(1);
    n.gain_env = c.gain_env.iter().filter(|(t, _)| *t >= skip && *t < skip + n.length).map(|(t, g)| (t - skip, *g)).collect();
    n.sync_point = (c.sync_point - skip).clamp(0, n.length);
    if is_left {
        n.fade_out = Default::default();
    } else {
        n.fade_in = Default::default();
    }
    Some(n)
}

fn file_stem(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).take(60).collect();
    if s.is_empty() { "track".into() } else { s }
}

impl Multitrack {
    /// Arm a track for recording, on input channel `input` (1-based, as the interface labels
    /// it; a stereo track takes `input` and the next one). `None` keeps the route it has
    /// (channel 1 when it has none).
    pub fn arm_track(&mut self, id: TrackId, input: Option<usize>) -> Result<(), MtError> {
        let Some(t) = self.session.track_mut(id) else { return err("no such track") };
        if t.kind != TrackKind::Audio {
            return err("only an audio track can be armed");
        }
        let ch = match input {
            Some(0) => return err("input channels count from 1"),
            Some(n) if n > FAKE_CHANNELS && fake_input() => return err(format!("the fake input has {FAKE_CHANNELS} channels, not {n}")),
            Some(n) => n,
            None => first_input(&t.mixer.input) + 1,
        };
        t.mixer.input = Route::Hardware(if t.channels() == 2 { format!("In {ch}-{}", ch + 1) } else { format!("In {ch}") });
        t.mixer.record_arm = true;
        Ok(())
    }

    pub fn disarm_track(&mut self, id: TrackId) -> Result<(), MtError> {
        let Some(t) = self.session.track_mut(id) else { return err("no such track") };
        t.mixer.record_arm = false;
        Ok(())
    }

    /// The armed audio tracks with their input channels (wrapped to `input_channels`) and a new
    /// take file `<track>_take<NN>.wav` in `dir`, a name no file and no other plan has.
    pub fn plan_takes(&self, dir: &Path, input_channels: usize) -> Vec<TakePlan> {
        let mut taken: Vec<PathBuf> = Vec::new();
        self.session
            .tracks
            .iter()
            .filter(|t| t.mixer.record_arm && t.kind == TrackKind::Audio && !t.inactive)
            .map(|t| {
                let first = first_input(&t.mixer.input);
                let inputs = (0..t.channels()).map(|k| (first + k) % input_channels.max(1)).collect();
                let stem = file_stem(&t.name);
                let mut n = self.takes.iter().filter(|k| k.track == t.id).map(|k| k.number).max().unwrap_or(0) + 1;
                let path = loop {
                    let p = dir.join(format!("{stem}_take{n:02}.wav"));
                    if !p.exists() && !taken.iter().any(|q| q.to_string_lossy().eq_ignore_ascii_case(&p.to_string_lossy())) {
                        break p;
                    }
                    n += 1;
                };
                taken.push(path.clone());
                TakePlan { track: t.id, inputs, path }
            })
            .collect()
    }

    /// Place a recorded take file on `track` as a clip at `start` (session frames). When the
    /// range already holds clips on the track's active playlist, the take goes on a new
    /// playlist (a copy of the active one without the clips it overlaps, plus the take) that
    /// becomes the active one: the newest take is on top. Returns the placed take.
    pub fn place_take(&mut self, track: TrackId, start: Samples, path: &Path) -> Result<TakeInfo, MtError> {
        let (rate, name) = match self.session.track(track) {
            Some(t) => (self.session.sample_rate.hz(), t.name.clone()),
            None => return err("no such track"),
        };
        let bytes = std::fs::read(path).map_err(|e| MtError(format!("cannot read the take {}: {e}", path.display())))?;
        let (_, audio) = soundcraft_audio_io::decode(&bytes, path.extension().and_then(|e| e.to_str()))
            .map_err(|e| MtError(format!("cannot decode the take {}: {e}", path.display())))?;
        drop(bytes);
        self.place_take_audio(track, start, path, audio, rate, &name)
    }

    fn place_take_audio(
        &mut self,
        track: TrackId,
        start: Samples,
        path: &Path,
        audio: ModelBuffer,
        rate: u32,
        name: &str,
    ) -> Result<TakeInfo, MtError> {
        let frames = audio.frames();
        if frames == 0 {
            return err("the take is empty");
        }
        if audio.sample_rate != rate {
            return err(format!("the take is {} Hz and the session {rate} Hz", audio.sample_rate));
        }
        if frames as u64 * audio.num_channels() as u64 > MAX_IMPORT_SAMPLES {
            return err("the take is too long to hold in memory as a clip (issue #302)");
        }
        let number = self.takes.iter().filter(|k| k.track == track).map(|k| k.number).max().unwrap_or(0) + 1;
        let length = frames as i64;
        let range = soundcraft_model::time::Range::new(start, start + length);
        let source = SourceId(self.session.alloc());
        let channels = u16::try_from(audio.num_channels()).unwrap_or(u16::MAX);
        let label = format!("{name} take {number:02}");
        self.session.sources.push(Source {
            id: source,
            name: label.clone(),
            path: path.to_string_lossy().into_owned(),
            channels,
            frames: frames as u64,
            sample_rate: rate,
            format: soundcraft_audio_io::FileFormat::Wav,
            time_reference: 0,
            unsaved: true,
        });
        self.session.pool.insert(source, Arc::new(SourceAudio::new(audio)));
        let clip_id = self.session.new_clip_id();
        let clip = Clip::audio(clip_id, label, source, 0, start, length);
        let active_clips: Vec<Clip> = match self.session.track(track).and_then(|t| t.playlist()) {
            Some(p) => p.clips.clone(),
            None => Vec::new(),
        };
        let overlaps = active_clips.iter().any(|c| c.range().overlaps(&range));
        // The new playlist keeps what the take does not cover: a clip the take covers only in
        // part is trimmed (or split in two when the take sits inside it), never dropped whole.
        let mut kept: Vec<Clip> = Vec::new();
        if overlaps {
            for c in &active_clips {
                if !c.range().overlaps(&range) {
                    kept.push(c.clone());
                    continue;
                }
                let (cs, ce) = (c.start, c.start + c.length);
                if cs < start
                    && let Some(left) = cut_piece(c, cs, start, true, || self.session.new_clip_id())
                {
                    kept.push(left);
                }
                if ce > start + length
                    && let Some(right) = cut_piece(c, start + length, ce, false, || self.session.new_clip_id())
                {
                    kept.push(right);
                }
            }
        }
        let Some(t) = self.session.track_mut(track) else { return err("no such track") };
        let playlist = if overlaps || t.playlists.is_empty() {
            let mut pl = Playlist::new(format!("{name}.take{number:02}"));
            pl.clips = kept;
            pl.clips.push(clip);
            pl.sort();
            t.playlists.push(pl);
            t.active_playlist = t.playlists.len() - 1;
            t.active_playlist
        } else {
            if let Some(pl) = t.playlist_mut() {
                pl.clips.push(clip);
                pl.sort();
            }
            t.active_playlist
        };
        let info = TakeInfo { track, number, start, length, path: path.to_path_buf(), playlist, clip: clip_id };
        self.takes.push(info.clone());
        Ok(info)
    }
}

/// A take file being written. The header is a complete 32-bit float WAV header from the first
/// byte; its sizes are patched and synced as audio arrives.
pub struct TakeWriter {
    file: File,
    channels: usize,
    rate: u32,
    frames: u64,
    since_patch: u64,
}

const HEADER: u64 = 44;

impl TakeWriter {
    pub fn create(path: &Path, channels: usize, rate: u32) -> std::io::Result<TakeWriter> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut file = std::fs::OpenOptions::new().write(true).read(true).create_new(true).open(path)?;
        let ch = u16::try_from(channels).map_err(|_| std::io::Error::other("too many channels"))?;
        let mut h = Vec::with_capacity(44);
        h.extend_from_slice(b"RIFF");
        h.extend_from_slice(&36u32.to_le_bytes());
        h.extend_from_slice(b"WAVEfmt ");
        h.extend_from_slice(&16u32.to_le_bytes());
        h.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
        h.extend_from_slice(&ch.to_le_bytes());
        h.extend_from_slice(&rate.to_le_bytes());
        h.extend_from_slice(&(rate * u32::from(ch) * 4).to_le_bytes());
        h.extend_from_slice(&(ch * 4).to_le_bytes());
        h.extend_from_slice(&32u16.to_le_bytes());
        h.extend_from_slice(b"data");
        h.extend_from_slice(&0u32.to_le_bytes());
        file.write_all(&h)?;
        file.sync_data()?;
        Ok(TakeWriter { file, channels, rate, frames: 0, since_patch: 0 })
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Append planar blocks (one slice per channel, equal lengths).
    pub fn write(&mut self, planar: &[Vec<f32>]) -> std::io::Result<()> {
        let n = planar.first().map_or(0, Vec::len);
        let mut buf = Vec::with_capacity(n * self.channels * 4);
        for i in 0..n {
            for ch in planar {
                buf.extend_from_slice(&ch.get(i).copied().unwrap_or(0.0).to_le_bytes());
            }
        }
        self.file.write_all(&buf)?;
        self.frames += n as u64;
        self.since_patch += n as u64;
        if self.since_patch as f64 >= PATCH_EVERY * f64::from(self.rate) {
            self.patch()?;
        }
        Ok(())
    }

    fn patch(&mut self) -> std::io::Result<()> {
        let data = (self.frames * self.channels as u64 * 4).min(u64::from(u32::MAX - 36)) as u32;
        self.file.seek(SeekFrom::Start(4))?;
        self.file.write_all(&(36 + data).to_le_bytes())?;
        self.file.seek(SeekFrom::Start(HEADER - 4))?;
        self.file.write_all(&data.to_le_bytes())?;
        self.file.seek(SeekFrom::End(0))?;
        self.file.sync_data()?;
        self.since_patch = 0;
        Ok(())
    }

    pub fn finish(mut self) -> std::io::Result<u64> {
        self.patch()?;
        Ok(self.frames)
    }
}

/// A recording under way: a thread feeding every plan's take file from the input, paced by the
/// clock like a device would be.
pub struct Recorder {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<StopReport>>,
    /// The session frame the takes start at.
    pub start: Samples,
    pub plans: Vec<TakePlan>,
}

impl Recorder {
    /// Start recording `plans` at session frame `start`. Without `WAVCRAFT_FAKE_INPUT=1` this
    /// is refused: capture from a real device is not built yet.
    pub fn start(plans: Vec<TakePlan>, rate: u32, start: Samples) -> Result<Recorder, MtError> {
        if plans.is_empty() {
            return err("no track is armed: TRACK arm <track> input=<ch>");
        }
        if !fake_input() {
            return err("recording from a real input device is not built yet; set WAVCRAFT_FAKE_INPUT=1 for the synthetic input");
        }
        Self::start_fake(plans, rate, start)
    }

    /// Start recording the synthetic input without looking at the environment (tests; `start`
    /// calls it once it has checked `WAVCRAFT_FAKE_INPUT`). Never opens a device.
    pub fn start_fake(plans: Vec<TakePlan>, rate: u32, start: Samples) -> Result<Recorder, MtError> {
        let mut writers = Vec::new();
        for p in &plans {
            let w = TakeWriter::create(&p.path, p.inputs.len(), rate)
                .map_err(|e| MtError(format!("cannot create the take file {}: {e}", p.path.display())))?;
            writers.push((p.clone(), w));
        }
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("wavcraft-fake-input".into())
            .spawn(move || run_fake(writers, rate, &flag))
            .map_err(|e| MtError(format!("cannot start the input thread: {e}")))?;
        Ok(Recorder { stop, thread: Some(thread), start, plans })
    }

    /// Stop, finish the files, and say what was written.
    pub fn stop(mut self) -> Result<StopReport, MtError> {
        self.stop.store(true, Ordering::SeqCst);
        match self.thread.take().map(std::thread::JoinHandle::join) {
            Some(Ok(r)) => Ok(r),
            _ => err("the input thread failed"),
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run_fake(writers: Vec<(TakePlan, TakeWriter)>, rate: u32, stop: &AtomicBool) -> StopReport {
    let mut writers: Vec<(TakePlan, TakeWriter, bool)> = writers.into_iter().map(|(p, w)| (p, w, true)).collect();
    let mut report = StopReport::default();
    let t0 = Instant::now();
    let mut at = 0u64;
    while !stop.load(Ordering::SeqCst) && writers.iter().any(|w| w.2) {
        let due = t0 + Duration::from_secs_f64((at + FAKE_BLOCK as u64) as f64 / f64::from(rate));
        let now = Instant::now();
        if due > now {
            std::thread::sleep((due - now).min(Duration::from_millis(20)));
            if Instant::now() < due {
                continue;
            }
        }
        // One track failing (disk full, file gone) stops that track only: the others go on.
        for (plan, w, live) in writers.iter_mut().filter(|w| w.2) {
            let planar: Vec<Vec<f32>> = plan.inputs.iter().map(|c| fake_samples(*c, at, FAKE_BLOCK, rate)).collect();
            if let Err(e) = w.write(&planar) {
                report.failures.push(format!("writing {}: {e}", plan.path.display()));
                *live = false;
            }
        }
        at += FAKE_BLOCK as u64;
    }
    // Finish every writer, whatever the others did: each result is kept on its own.
    for (plan, w, _) in writers {
        match w.finish() {
            Ok(f) => report.written.push(Written { track: plan.track, path: plan.path, frames: f }),
            Err(e) => report.failures.push(format!("finishing {}: {e}", plan.path.display())),
        }
    }
    report
}

/// Read a take file's header the way a crashed one is read: the frames its data size claims,
/// capped by what the file holds. `None` when it is not a WAV this module wrote.
pub fn readable_frames(path: &Path) -> Option<(usize, u32, u64)> {
    let mut f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let mut h = [0u8; 44];
    f.read_exact(&mut h).ok()?;
    if &h[0..4] != b"RIFF" || &h[8..16] != b"WAVEfmt " || &h[36..40] != b"data" {
        return None;
    }
    let ch = usize::from(u16::from_le_bytes([h[22], h[23]]));
    let rate = u32::from_le_bytes([h[24], h[25], h[26], h[27]]);
    let data = u64::from(u32::from_le_bytes([h[40], h[41], h[42], h[43]])).min(len.saturating_sub(HEADER));
    Some((ch, rate, data / (ch.max(1) as u64 * 4)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use soundcraft_model::BitDepthSetting;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wavcraft-rec-{}-{name}-{}", std::process::id(), crate::random::hex(4).unwrap_or_default()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap_or_else(|e| panic!("{e}"));
        d
    }

    fn session() -> (Multitrack, TrackId, TrackId) {
        let mut m = Multitrack::new("Rec", 48_000, BitDepthSetting::Int24).unwrap_or_else(|e| panic!("{e}"));
        let a = m.add_track(Some("Vox"), 1).unwrap_or_else(|e| panic!("{e}"));
        let b = m.add_track(Some("Gtr"), 2).unwrap_or_else(|e| panic!("{e}"));
        (m, a, b)
    }

    fn write_take(path: &Path, frames: usize, ch: usize) {
        let mut w = TakeWriter::create(path, ch, 48_000).unwrap_or_else(|e| panic!("{e}"));
        let block: Vec<Vec<f32>> = (0..ch).map(|c| fake_samples(c, 0, frames, 48_000)).collect();
        w.write(&block).unwrap_or_else(|e| panic!("{e}"));
        w.finish().unwrap_or_else(|e| panic!("{e}"));
    }

    #[test]
    fn arming_picks_the_input_channel_and_plans_take_files() {
        let (mut m, a, b) = session();
        assert!(m.arm_track(a, Some(0)).is_err(), "channels count from 1");
        m.arm_track(a, Some(3)).unwrap_or_else(|e| panic!("{e}"));
        m.arm_track(b, Some(5)).unwrap_or_else(|e| panic!("{e}"));
        let t = m.session.track(a).unwrap_or_else(|| panic!("t"));
        assert_eq!((t.mixer.record_arm, input_label(&t.mixer.input).as_str()), (true, "In 3"));
        assert_eq!(input_label(&m.session.track(b).unwrap_or_else(|| panic!("t")).mixer.input), "In 5-6");
        let dir = scratch("plan");
        let plan = m.plan_takes(&dir, 8);
        assert_eq!(plan.len(), 2);
        assert_eq!((plan[0].inputs.clone(), plan[1].inputs.clone()), (vec![2], vec![4, 5]));
        assert!(plan[0].path.ends_with("Vox_take01.wav"));
        assert_eq!(m.plan_takes(&dir, 4)[1].inputs, vec![0, 1], "wrapped to the device's channels");
        m.disarm_track(a).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(m.plan_takes(&dir, 8).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_take_is_placed_at_the_record_start() {
        let (mut m, a, _) = session();
        let dir = scratch("place");
        let p = dir.join("Vox_take01.wav");
        write_take(&p, 24_000, 1);
        let info = m.place_take(a, 96_000, &p).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((info.number, info.start, info.length, info.playlist), (1, 96_000, 24_000, 0));
        let c = &m.session.track(a).unwrap_or_else(|| panic!("t")).clips()[0];
        assert_eq!((c.start, c.length, c.name.as_str()), (96_000, 24_000, "Vox take 01"));
        // A take that doesn't overlap shares the playlist.
        let p2 = dir.join("Vox_take02.wav");
        write_take(&p2, 1_000, 1);
        let i2 = m.place_take(a, 0, &p2).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((i2.number, i2.playlist), (2, 0));
        assert_eq!(m.session.track(a).map(|t| t.clips().len()), Some(2));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn takes_over_the_same_range_are_numbered_and_stacked_newest_on_top() {
        let (mut m, a, b) = session();
        let dir = scratch("stack");
        let mut infos = Vec::new();
        for n in 1..=3 {
            let p = dir.join(format!("Vox_take{n:02}.wav"));
            write_take(&p, 12_000, 1);
            infos.push(m.place_take(a, 48_000, &p).unwrap_or_else(|e| panic!("{e}")));
        }
        assert_eq!(infos.iter().map(|i| (i.number, i.playlist)).collect::<Vec<_>>(), vec![(1, 0), (2, 1), (3, 2)]);
        let t = m.session.track(a).unwrap_or_else(|| panic!("t"));
        assert_eq!((t.playlists.len(), t.active_playlist), (3, 2));
        assert_eq!(t.clips().len(), 1);
        assert_eq!(t.clips()[0].name, "Vox take 03");
        assert_eq!(t.playlists[0].clips[0].name, "Vox take 01", "older takes stay on their playlists");
        // Numbering is per track.
        let p = dir.join("Gtr_take01.wav");
        write_take(&p, 12_000, 2);
        assert_eq!(m.place_take(b, 0, &p).unwrap_or_else(|e| panic!("{e}")).number, 1);
        // Planned file names continue the numbering.
        m.arm_track(a, Some(1)).unwrap_or_else(|e| panic!("{e}"));
        std::fs::remove_file(dir.join("Vox_take03.wav")).unwrap_or_else(|e| panic!("{e}"));
        assert!(m.plan_takes(&dir, 8)[0].path.ends_with("Vox_take04.wav"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stacking_keeps_the_clips_the_new_take_does_not_cover() {
        let (mut m, a, _) = session();
        let dir = scratch("keep");
        let (p1, p2, p3) = (dir.join("a.wav"), dir.join("b.wav"), dir.join("c.wav"));
        write_take(&p1, 1_000, 1);
        write_take(&p2, 1_000, 1);
        write_take(&p3, 1_000, 1);
        m.place_take(a, 0, &p1).unwrap_or_else(|e| panic!("{e}"));
        m.place_take(a, 10_000, &p2).unwrap_or_else(|e| panic!("{e}"));
        m.place_take(a, 500, &p3).unwrap_or_else(|e| panic!("{e}"));
        let t = m.session.track(a).unwrap_or_else(|| panic!("t"));
        assert_eq!(t.active_playlist, 1);
        // The first clip (0..1000) is covered from 500 on: its first half stays.
        assert_eq!(t.clips().iter().map(|c| (c.start, c.length)).collect::<Vec<_>>(), vec![(0, 500), (500, 1_000), (10_000, 1_000)]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_take_file_is_readable_after_a_kill_mid_write() {
        let dir = scratch("crash");
        let p = dir.join("t.wav");
        let mut w = TakeWriter::create(&p, 1, 48_000).unwrap_or_else(|e| panic!("{e}"));
        // 1 s of audio in 0.01 s blocks: patched every 0.25 s.
        for k in 0..100 {
            w.write(&[fake_samples(0, k * 480, 480, 48_000)]).unwrap_or_else(|e| panic!("{e}"));
        }
        // No finish: the process "dies" here (the writer is leaked, nothing more is written).
        std::mem::forget(w);
        let (ch, rate, frames) = readable_frames(&p).unwrap_or_else(|| panic!("header"));
        assert_eq!((ch, rate), (1, 48_000));
        assert!(frames >= 36_000, "everything up to the last patch is there: {frames}");
        let bytes = std::fs::read(&p).unwrap_or_else(|e| panic!("{e}"));
        let (_, buf) = soundcraft_audio_io::decode(&bytes, Some("wav")).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(buf.frames() as u64, frames);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_fake_input_is_deterministic_and_the_recorder_writes_a_take() {
        assert_eq!(fake_samples(2, 100, 8, 48_000), fake_samples(2, 100, 8, 48_000));
        assert_ne!(fake_samples(0, 0, 8, 48_000), fake_samples(1, 0, 8, 48_000));
        let dir = scratch("rec");
        let plan = TakePlan { track: TrackId(1), inputs: vec![0], path: dir.join("x.wav") };
        // Not through the environment (tests share it): run the loop directly.
        let w = TakeWriter::create(&plan.path, 1, 48_000).unwrap_or_else(|e| panic!("{e}"));
        let stop = AtomicBool::new(false);
        std::thread::scope(|s| {
            let h = s.spawn(|| run_fake(vec![(plan.clone(), w)], 48_000, &stop));
            std::thread::sleep(Duration::from_millis(300));
            stop.store(true, Ordering::SeqCst);
            let out = h.join().unwrap_or_else(|_| panic!("thread"));
            assert!(out.failures.is_empty());
            assert!(out.written[0].frames >= 9_600 && out.written[0].frames <= 24_000, "about 0.3 s: {}", out.written[0].frames);
        });
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_take_over_part_of_a_clip_keeps_the_rest_of_that_clip() {
        let (mut m, a, _) = session();
        let dir = scratch("partial");
        let (back, t1, t2) = (dir.join("back.wav"), dir.join("t1.wav"), dir.join("t2.wav"));
        write_take(&back, 10_000, 1);
        write_take(&t1, 1_000, 1);
        write_take(&t2, 1_000, 1);
        m.place_take(a, 0, &back).unwrap_or_else(|e| panic!("{e}"));
        // Over the start of the backing: the rest of it stays, from where the take ends.
        m.place_take(a, 0, &t1).unwrap_or_else(|e| panic!("{e}"));
        let clips = m.session.track(a).unwrap_or_else(|| panic!("t")).clips().to_vec();
        assert_eq!(clips.iter().map(|c| (c.start, c.length)).collect::<Vec<_>>(), vec![(0, 1_000), (1_000, 9_000)]);
        match &clips[1].content {
            ClipContent::Audio { offset, .. } => assert_eq!(*offset, 1_000, "the remainder plays the same audio it did"),
            other => panic!("{other:?}"),
        }
        // Inside the remainder: it is split in two around the take.
        m.place_take(a, 4_000, &t2).unwrap_or_else(|e| panic!("{e}"));
        let clips = m.session.track(a).unwrap_or_else(|| panic!("t")).clips().to_vec();
        assert_eq!(clips.iter().map(|c| (c.start, c.length)).collect::<Vec<_>>(), vec![(0, 1_000), (1_000, 3_000), (4_000, 1_000), (5_000, 5_000)]);
        match &clips[3].content {
            ClipContent::Audio { offset, .. } => assert_eq!(*offset, 5_000),
            other => panic!("{other:?}"),
        }
        let ids: std::collections::HashSet<_> = clips.iter().map(|c| c.id).collect();
        assert_eq!(ids.len(), clips.len(), "every clip has its own id");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn one_failing_track_does_not_cost_the_other_tracks_their_take() {
        let dir = scratch("fail");
        let (pa, pb) = (dir.join("a.wav"), dir.join("b.wav"));
        let plan_a = TakePlan { track: TrackId(1), inputs: vec![0], path: pa.clone() };
        let plan_b = TakePlan { track: TrackId(2), inputs: vec![1], path: pb.clone() };
        let wa = TakeWriter::create(&pa, 1, 48_000).unwrap_or_else(|e| panic!("{e}"));
        let mut wb = TakeWriter::create(&pb, 1, 48_000).unwrap_or_else(|e| panic!("{e}"));
        // A write that fails, like a full disk: the handle can only read.
        wb.file = File::open(&pb).unwrap_or_else(|e| panic!("{e}"));
        let stop = AtomicBool::new(false);
        let report = std::thread::scope(|s| {
            let h = s.spawn(|| run_fake(vec![(plan_b.clone(), wb), (plan_a.clone(), wa)], 48_000, &stop));
            std::thread::sleep(Duration::from_millis(300));
            stop.store(true, Ordering::SeqCst);
            h.join().unwrap_or_else(|_| panic!("thread"))
        });
        assert_eq!(report.failures.len(), 2, "the failed write, and the failed finish of that file: {:?}", report.failures);
        assert!(report.failures[0].contains("b.wav"));
        assert_eq!(report.written.len(), 1, "the good track's take is reported even though the other failed");
        assert_eq!(report.written[0].track, TrackId(1));
        assert!(report.written[0].frames >= 9_600);
        assert!(readable_frames(&pa).is_some_and(|(_, _, f)| f == report.written[0].frames), "its header was patched");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
