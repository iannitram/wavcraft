//! The Video panel (f122): the picture of a video file (MOV/MP4) at the playhead or cursor.
//!
//! Decoding is `soundcraft-video` (MIT OR Apache-2.0: pure Rust H.264, ProRes and Motion JPEG,
//! ISO base media demux), on a worker thread that owns the `Movie` (positioned reads of the file,
//! never the whole file in memory, and a cache of [`CACHE_FRAMES`] decoded frames). The window
//! thread only sends times and receives finished pictures, so it never waits for a decode. Frames
//! are scaled down on the worker to at most [`MAX_W`] x [`MAX_H`] before they become a texture.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

use egui::{ColorImage, TextureHandle, TextureOptions, vec2};
use soundcraft_video::{Movie, VideoError};

/// Decoded frames the worker keeps (a 1080p frame is 8 MB).
pub const CACHE_FRAMES: usize = 3;
/// The largest picture sent to the window.
pub const MAX_W: u32 = 960;
pub const MAX_H: u32 = 540;

/// The frame shown at `secs` for a clip of `frames` frames at a constant `fps`: the one whose
/// interval holds the time, so frame `i` is shown from `i / fps`. A time at or past the end shows
/// the last frame; a negative or non-finite time, or a clip without frames, shows none.
pub fn frame_index(secs: f64, fps: f64, frames: usize) -> Option<usize> {
    if frames == 0 || !secs.is_finite() || secs < 0.0 || !fps.is_finite() || fps <= 0.0 {
        return None;
    }
    // A hair of slack so a time computed from a sample position (k / rate) does not fall just
    // under its frame's start.
    let i = (secs * fps + 1e-6).floor();
    Some(if i >= frames as f64 { frames - 1 } else { i as usize })
}

/// What the attached file is.
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub frames: usize,
    pub duration: f64,
    pub codec: String,
}

/// Where the attached file stands.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Loading,
    Ready(Info),
    NoVideo,
    Unsupported(String),
    Failed(String),
}

impl Status {
    /// The line the panel (and the control channel) shows.
    pub fn text(&self) -> String {
        match self {
            Status::Loading => "Opening the video…".to_string(),
            Status::Ready(i) => format!("{}x{} {} {:.3} fps, {} frames, {:.2} s", i.width, i.height, i.codec, i.fps, i.frames, i.duration),
            Status::NoVideo => "This file has no video track.".to_string(),
            Status::Unsupported(c) => format!("This video's codec is not supported ({c})."),
            Status::Failed(e) => format!("Could not open the video: {e}"),
        }
    }
}

/// One decoded picture, already scaled for the window.
pub struct Shown {
    pub index: usize,
    pub time: f64,
    pub src_width: u32,
    pub src_height: u32,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

enum Req {
    /// `must`: answer even when a newer request is waiting (a control-channel request).
    Show { seq: u64, secs: f64, must: bool },
}

enum Msg {
    Opened(Status),
    Frame { seq: u64, result: Result<Shown, String> },
}

/// The panel: a worker, the newest picture as a texture, and the file's status.
pub struct VideoPanel {
    pub path: PathBuf,
    pub status: Status,
    tx: Sender<Req>,
    rx: Receiver<Msg>,
    seq: u64,
    outstanding: Vec<u64>,
    /// The newest request sent for the playhead, and whether its picture is still to come.
    auto_seq: u64,
    in_flight: bool,
    /// The time last asked for from the playhead (frames are asked for again only on a change).
    last_asked: Option<f64>,
    shown_seq: u64,
    texture: Option<TextureHandle>,
    /// The index, time and size of the picture on screen.
    pub shown: Option<(usize, f64, u32, u32)>,
    /// The last decode error (a time that has no picture, say), shown under the picture.
    pub last_error: Option<String>,
}

impl VideoPanel {
    /// Attach `path`: a worker opens it and answers requests. Never blocks.
    pub fn attach(path: PathBuf, ctx: &egui::Context) -> VideoPanel {
        let (req_tx, req_rx) = channel();
        let (msg_tx, msg_rx) = channel();
        let (wp, wctx) = (path.clone(), ctx.clone());
        let spawned = std::thread::Builder::new().name("video decode".into()).spawn(move || worker(&wp, &req_rx, &msg_tx, &wctx));
        let status = if spawned.is_ok() { Status::Loading } else { Status::Failed("could not start the decoder".to_string()) };
        VideoPanel {
            path,
            status,
            tx: req_tx,
            rx: msg_rx,
            seq: 0,
            outstanding: Vec::new(),
            auto_seq: 0,
            in_flight: false,
            last_asked: None,
            shown_seq: 0,
            texture: None,
            shown: None,
            last_error: None,
        }
    }

    /// Ask for the picture at `secs`. `must` requests are always answered.
    pub fn request(&mut self, secs: f64, must: bool) -> u64 {
        self.seq += 1;
        if !must {
            self.auto_seq = self.seq;
            self.in_flight = true;
            self.last_asked = Some(secs);
        }
        self.outstanding.push(self.seq);
        let _ = self.tx.send(Req::Show { seq: self.seq, secs, must });
        self.seq
    }

    /// Follow the playhead: ask for its picture when it moved and the last ask is answered.
    pub fn follow(&mut self, secs: f64) {
        if !self.in_flight && self.last_asked != Some(secs) && matches!(self.status, Status::Ready(_) | Status::Loading) {
            self.request(secs, false);
        }
    }

    /// Take the worker's results. Returns the control-channel answer for every picture that came.
    pub fn poll(&mut self, ctx: &egui::Context) -> Vec<(u64, String)> {
        let mut answers = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(Msg::Opened(status)) => self.status = status,
                Ok(Msg::Frame { seq, result }) => {
                    self.outstanding.retain(|outstanding| *outstanding != seq);
                    if seq == self.auto_seq {
                        self.in_flight = false;
                    }
                    match result {
                        Ok(f) => {
                            answers.push((seq, format!("OK index={} size={}x{} time={:.4}", f.index, f.src_width, f.src_height, f.time)));
                            if seq >= self.shown_seq {
                                self.shown_seq = seq;
                                let image = ColorImage::from_rgba_unmultiplied([f.width as usize, f.height as usize], &f.rgba);
                                self.texture = Some(ctx.load_texture("video-frame", image, TextureOptions::LINEAR));
                                self.shown = Some((f.index, f.time, f.src_width, f.src_height));
                                self.last_error = None;
                            }
                        }
                        Err(e) => {
                            answers.push((seq, format!("ERR {e}")));
                            if seq >= self.shown_seq {
                                self.last_error = Some(e);
                            }
                        }
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    answers.extend(self.outstanding.drain(..).map(|seq| (seq, "ERR video detached".to_string())));
                    if matches!(self.status, Status::Loading) {
                        self.status = Status::Failed("the decoder stopped unexpectedly".to_string());
                    }
                    break;
                }
            }
        }
        answers
    }

    /// Draw the panel body: the picture scaled to fit, or what is wrong with the file.
    pub fn ui(&self, ui: &mut egui::Ui) {
        ui.label(self.path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned()));
        ui.small(self.status.text());
        let avail = ui.available_size();
        match (&self.texture, &self.status) {
            (Some(tex), Status::Ready(_)) => {
                let t = tex.size_vec2();
                let scale = (avail.x / t.x).min((avail.y - 40.0).max(40.0) / t.y).max(0.01);
                ui.image((tex.id(), vec2(t.x * scale, t.y * scale)));
                if let Some((index, time, ..)) = self.shown {
                    ui.small(format!("Frame {index}  {time:.3} s"));
                }
            }
            (None, Status::Ready(_) | Status::Loading) => {
                ui.small("Waiting for the first picture…");
            }
            _ => {}
        }
        if let Some(e) = &self.last_error {
            ui.small(e);
        }
    }
}

/// Open the file and describe it, or say why it cannot be shown.
fn open_movie(path: &Path) -> Result<(Movie, Info), Status> {
    let file = std::fs::File::open(path).map_err(|e| Status::Failed(e.to_string()))?;
    let mut movie = match Movie::open_source(Arc::new(file)) {
        Ok(m) => m,
        Err(VideoError::NoVideoTrack) => return Err(Status::NoVideo),
        Err(e) => return Err(Status::Failed(e.to_string())),
    };
    let i = movie.info().clone();
    if !i.decodable {
        return Err(Status::Unsupported(i.codec_detail));
    }
    movie.set_cache_frames(CACHE_FRAMES);
    let info = Info { width: i.width, height: i.height, fps: i.frame_rate, frames: i.frame_count, duration: i.duration, codec: i.codec_detail };
    Ok((movie, info))
}

/// Decode the frame shown at `secs` and scale it for the window.
fn decode(movie: &mut Movie, info: &Info, secs: f64) -> Result<Shown, String> {
    let index =
        movie.frame_index_at(secs).or_else(|| frame_index(secs, info.fps, info.frames)).ok_or_else(|| "no picture at that time".to_string())?;
    let frame = movie.frame(index).map_err(|e| e.to_string())?;
    let fitted = frame.fit(MAX_W, MAX_H);
    Ok(Shown {
        index: frame.index,
        time: frame.time,
        src_width: frame.width,
        src_height: frame.height,
        width: fitted.width,
        height: fitted.height,
        rgba: fitted.rgba,
    })
}

fn worker(path: &Path, rx: &Receiver<Req>, tx: &Sender<Msg>, ctx: &egui::Context) {
    let (mut movie, problem) = match open_movie(path) {
        Ok((m, info)) => {
            let _ = tx.send(Msg::Opened(Status::Ready(info.clone())));
            (Some((m, info)), String::new())
        }
        Err(status) => {
            let why = status.text();
            let _ = tx.send(Msg::Opened(status));
            (None, why)
        }
    };
    ctx.request_repaint();
    while let Ok(first) = rx.recv() {
        // Everything waiting: of the playhead's requests only the newest is worth decoding.
        let mut batch = vec![first];
        while let Ok(r) = rx.try_recv() {
            batch.push(r);
        }
        let last_auto = batch.iter().rposition(|Req::Show { must, .. }| !must);
        for (i, Req::Show { seq, secs, must }) in batch.into_iter().enumerate() {
            if !must && Some(i) != last_auto {
                continue;
            }
            let result = match &mut movie {
                Some((m, info)) => decode(m, info, secs),
                None => Err(problem.clone()),
            };
            if tx.send(Msg::Frame { seq, result }).is_err() {
                return;
            }
            ctx.request_repaint();
        }
    }
}

/// Control-channel bookkeeping: who is waiting for which picture.
#[derive(Default)]
pub struct Waiters(HashMap<u64, Sender<String>>);

impl Waiters {
    pub fn add(&mut self, seq: u64, reply: Sender<String>) {
        self.0.insert(seq, reply);
    }
    pub fn answer(&mut self, seq: u64, text: String) {
        if let Some(r) = self.0.remove(&seq) {
            let _ = r.send(text);
        }
    }
    pub fn fail_all(&mut self, text: &str) {
        for (_, r) in self.0.drain() {
            let _ = r.send(text.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn disconnect_answers_outstanding_requests_with_err() {
        let (request_tx, _request_rx) = channel();
        let (message_tx, message_rx) = channel();
        let mut panel = VideoPanel {
            path: PathBuf::new(),
            status: Status::Loading,
            tx: request_tx,
            rx: message_rx,
            seq: 0,
            outstanding: Vec::new(),
            auto_seq: 0,
            in_flight: false,
            last_asked: None,
            shown_seq: 0,
            texture: None,
            shown: None,
            last_error: None,
        };
        panel.request(0.0, true);
        drop(message_tx);

        assert!(panel.poll(&egui::Context::default()).contains(&(1, "ERR video detached".to_string())));
    }

    #[test]
    fn frame_index_maps_time_through_the_frame_rate() {
        assert_eq!(frame_index(0.0, 24.0, 48), Some(0));
        assert_eq!(frame_index(0.5, 24.0, 48), Some(12));
        assert_eq!(frame_index(0.0416, 24.0, 48), Some(0), "just before frame 1 is still frame 0");
        assert_eq!(frame_index(1.0 / 24.0, 24.0, 48), Some(1));
        assert_eq!(frame_index(1.999, 24.0, 48), Some(47));
        assert_eq!(frame_index(2.0, 24.0, 48), Some(47), "the end shows the last frame");
        assert_eq!(frame_index(99.0, 24.0, 48), Some(47));
        assert_eq!(frame_index(0.5, 29.97, 60), Some(14));
        // A time worked out from a sample position (k / rate) must not fall under its frame.
        assert_eq!(frame_index(12.0 / 24.0, 24.0, 48), Some(12));
        assert_eq!(frame_index(36_000.0 / 48_000.0, 24.0, 48), Some(18));
    }

    #[test]
    fn frame_index_refuses_what_has_no_picture() {
        assert_eq!(frame_index(-0.1, 24.0, 48), None);
        assert_eq!(frame_index(f64::NAN, 24.0, 48), None);
        assert_eq!(frame_index(f64::INFINITY, 24.0, 48), None);
        assert_eq!(frame_index(0.0, 24.0, 0), None);
        assert_eq!(frame_index(0.0, 0.0, 10), None);
        assert_eq!(frame_index(0.0, f64::NAN, 10), None);
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wavcraft-video-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A 2 s, 24 fps, 160x96 H.264 clip made by ffmpeg, or `None` (the test then skips).
    fn ffmpeg_clip(dir: &Path) -> Option<PathBuf> {
        let out = dir.join("clip.mp4");
        let ok = Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=2:size=160x96:rate=24",
                "-c:v",
                "libx264",
                "-profile:v",
                "baseline",
                "-pix_fmt",
                "yuv420p",
                "-g",
                "12",
            ])
            .arg(&out)
            .status()
            .is_ok_and(|s| s.success());
        ok.then_some(out)
    }

    #[test]
    fn decodes_one_frame_of_an_ffmpeg_h264_clip() {
        let dir = scratch("h264");
        let Some(clip) = ffmpeg_clip(&dir) else {
            eprintln!("skipped: ffmpeg (with libx264) is not available");
            return;
        };
        let (mut movie, info) = open_movie(&clip).unwrap_or_else(|s| panic!("open: {}", s.text()));
        assert_eq!((info.width, info.height, info.frames), (160, 96, 48));
        assert!((info.fps - 24.0).abs() < 1e-6 && (info.duration - 2.0).abs() < 0.01);
        let shown = decode(&mut movie, &info, 1.0).unwrap();
        assert_eq!((shown.index, shown.width, shown.height), (24, 160, 96));
        assert_eq!(shown.rgba.len(), 160 * 96 * 4);
        assert!(shown.rgba.chunks(4).any(|p| p[0] != p[1] || p[1] != p[2]), "testsrc has colour");
        // The pure mapping agrees with the demuxer's own index for this clip.
        for t in [0.0, 0.3, 1.0, 1.95] {
            assert_eq!(movie.frame_index_at(t), frame_index(t, info.fps, info.frames), "at {t} s");
        }
        // A time past the end falls back to the last frame.
        assert_eq!(decode(&mut movie, &info, 5.0).unwrap().index, 47);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_that_is_not_a_movie_is_reported_not_decoded() {
        let dir = scratch("junk");
        let junk = dir.join("junk.mp4");
        std::fs::write(&junk, b"not a movie at all, just some bytes").unwrap();
        assert!(matches!(open_movie(&junk), Err(Status::Failed(_))));
        assert!(matches!(open_movie(&dir.join("missing.mov")), Err(Status::Failed(_))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_movie_without_a_video_track_says_so() {
        let dir = scratch("novideo");
        let wav = dir.join("a.m4a");
        // An ISO file with only an ftyp box: a movie, with nothing to show.
        let mut bytes = Vec::new();
        bytes.extend(24u32.to_be_bytes());
        bytes.extend(b"ftypisom");
        bytes.extend(0u32.to_be_bytes());
        bytes.extend(b"isomiso2");
        std::fs::write(&wav, bytes).unwrap();
        let s = open_movie(&wav).err().map(|s| s.text()).unwrap_or_default();
        assert!(!s.is_empty());
        assert!(s.contains("no video track") || s.contains("Could not open"), "{s}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
