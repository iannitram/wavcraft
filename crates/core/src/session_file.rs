//! The multitrack session file (f117, roadmap C075). It is the SoundCraft model's own session
//! JSON (`Session::to_json`, `format_version` included), so no format is invented here; WavCraft's
//! extension for it is [`EXTENSION`].
//!
//! What the file holds: tracks, clips (positions, lengths, gains, fades), track settings, and for
//! each source its file path. A path is stored **relative to the session file** (with `/`) when
//! the source lies at most [`MAX_UPS`] folders away from it, otherwise absolute, so a folder
//! holding a session and its media can be moved or synced to another machine. Audio that has no
//! file behind it (a clip made from an open document, or whose file has since gone) is written
//! beside the session, in `<name>-media/`, as WAV, so a save never loses audio.
//!
//! **Safety** is the WAV save's ([`crate::save::write_file_atomic`]): an exclusive temporary
//! file, owner-only on Unix, synced before the rename; a failed save leaves the old file.
//!
//! **Opening never fails because media is missing.** A source whose file cannot be found, read,
//! or that no longer matches what was saved opens *offline* (its clips stay, with no audio), and
//! the open reports one message per such file.

use std::path::{Path, PathBuf};

use soundcraft_audio_io::{BitDepth, EncodeOptions};
use soundcraft_model::{Session, SourceId};

use crate::multitrack::{MtError, Multitrack, read_audio_file};
use crate::save::write_file_atomic;

/// WavCraft's session file extension (not shared with any other program's).
pub const EXTENSION: &str = "wcsession";
/// Sources further than this many folders up from the session file are stored absolute.
pub const MAX_UPS: usize = 3;
/// The largest session file read: 64 MiB.
pub const MAX_SESSION_FILE_BYTES: u64 = 64 * 1024 * 1024;

fn err<T>(s: impl Into<String>) -> Result<T, MtError> {
    Err(MtError(s.into()))
}

/// `target` as a `/`-separated path from the folder `base` (both absolute and normal), or `None`
/// when they share no root (another drive) or the way is more than [`MAX_UPS`] folders up.
pub fn relative_path(base: &Path, target: &Path) -> Option<String> {
    let b: Vec<_> = base.components().collect();
    let t: Vec<_> = target.components().collect();
    let common = b.iter().zip(&t).take_while(|(x, y)| x == y).count();
    if common == 0 || b.len() - common > MAX_UPS {
        return None;
    }
    let mut parts: Vec<&str> = vec![".."; b.len() - common];
    for c in &t[common..] {
        match c {
            std::path::Component::Normal(s) => parts.push(s.to_str()?),
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// A stored path as a real one: absolute stays, relative is taken from the session's folder.
pub fn resolve_path(session_dir: &Path, stored: &str) -> PathBuf {
    let p = Path::new(stored);
    if p.is_absolute() {
        return p.to_path_buf();
    }
    let mut out = session_dir.to_path_buf();
    for part in stored.split(['/', '\\']).filter(|s| !s.is_empty() && *s != ".") {
        if part == ".." {
            out.pop();
        } else {
            out.push(part);
        }
    }
    out
}

fn media_name(id: SourceId, name: &str) -> String {
    let clean: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).take(40).collect();
    format!("{}-{}.wav", id.0, if clean.is_empty() { "audio".into() } else { clean })
}

/// What a save did.
#[derive(Debug, Clone, PartialEq)]
pub struct SaveReport {
    pub path: PathBuf,
    /// Sources written into the media folder because no file was behind them.
    pub exported: usize,
    /// Sources with no audio and no file (offline): kept by path only.
    pub offline: usize,
}

impl Multitrack {
    /// Source ids with no audio loaded (their file was missing or did not match).
    pub fn offline_sources(&self) -> Vec<SourceId> {
        self.session.sources.iter().map(|s| s.id).filter(|id| !self.session.pool.contains(*id)).collect()
    }

    /// Whether the clip's source is offline.
    pub fn clip_offline(&self, clip: &soundcraft_model::Clip) -> bool {
        clip.source().is_some_and(|s| !self.session.pool.contains(s))
    }

    /// Save to `path` (the extension [`EXTENSION`] is added when there is none).
    pub fn save_to(&mut self, path: &Path) -> Result<SaveReport, MtError> {
        let mut path = std::path::absolute(path).map_err(|e| MtError(format!("bad path {}: {e}", path.display())))?;
        if path.extension().is_none() {
            path.set_extension(EXTENSION);
        }
        let Some(dir) = path.parent().map(Path::to_path_buf) else { return err("no folder to save in") };
        std::fs::create_dir_all(&dir).map_err(|e| MtError(format!("cannot make {}: {e}", dir.display())))?;
        let dir_c = dir.canonicalize().map_err(|e| MtError(format!("cannot reach {}: {e}", dir.display())))?;
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("session").to_string();
        let media_dir = dir_c.join(format!("{stem}-media"));

        let mut out = self.session.clone();
        let mut new_paths: Vec<(SourceId, String)> = Vec::new();
        let (mut exported, mut offline) = (0, 0);
        for src in &mut out.sources {
            let current = PathBuf::from(&src.path);
            let backed = current.is_absolute() && current.is_file();
            let real = if backed {
                current.canonicalize().unwrap_or(current)
            } else if let Some(audio) = self.session.pool.get(src.id) {
                std::fs::create_dir_all(&media_dir).map_err(|e| MtError(format!("cannot make {}: {e}", media_dir.display())))?;
                let opts = EncodeOptions { bit_depth: BitDepth::Float32, ..EncodeOptions::default() };
                let bytes = soundcraft_audio_io::encode(&audio.buffer, &opts).map_err(|e| MtError(format!("cannot write {}: {e}", src.name)))?;
                let target = media_dir.join(media_name(src.id, &src.name));
                write_file_atomic(&target, &bytes, true).map_err(|e| MtError(format!("cannot write {}: {e}", target.display())))?;
                exported += 1;
                target
            } else {
                offline += 1;
                resolve_path(&dir_c, &src.path)
            };
            let stored = relative_path(&dir_c, &real).unwrap_or_else(|| real.to_string_lossy().into_owned());
            src.path = stored;
            src.unsaved = false;
            new_paths.push((src.id, real.to_string_lossy().into_owned()));
        }
        let json = out.to_json().map_err(|e| MtError(e.to_string()))?;
        write_file_atomic(&path, json.as_bytes(), true).map_err(|e| MtError(format!("cannot save {}: {e}", path.display())))?;
        for (id, real) in new_paths {
            if let Some(s) = self.session.sources.iter_mut().find(|s| s.id == id) {
                s.path = real;
                s.unsaved = false;
            }
        }
        log::info!(target: "wavcraft_core::session_file", "saved session {:?}: {} source(s), {exported} written to the media folder, {offline} offline", self.session.name, self.session.sources.len());
        self.file = Some(path.clone());
        Ok(SaveReport { path, exported, offline })
    }

    /// Open a session file. Returns the session and one message per source left offline.
    pub fn open_from(path: &Path) -> Result<(Multitrack, Vec<String>), MtError> {
        let path = std::path::absolute(path).map_err(|e| MtError(format!("bad path {}: {e}", path.display())))?;
        let meta = std::fs::metadata(&path).map_err(|e| MtError(format!("cannot read {}: {e}", path.display())))?;
        if meta.len() > MAX_SESSION_FILE_BYTES {
            return err(format!("{} is too big to be a session file", path.display()));
        }
        let text = std::fs::read_to_string(&path).map_err(|e| MtError(format!("cannot read {}: {e}", path.display())))?;
        let mut session: Session =
            Session::from_json(&text).map_err(|e| MtError(format!("{} is not a session this version can open: {e}", path.display())))?;
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let dir = dir.canonicalize().unwrap_or(dir);
        let mut messages = Vec::new();
        let rate = session.sample_rate.hz();
        let mut loaded = Vec::new();
        for src in &mut session.sources {
            let real = resolve_path(&dir, &src.path);
            src.path = real.to_string_lossy().into_owned();
            let problem = match read_audio_file(&real) {
                Err(e) => Some(e.0),
                Ok(a) if a.sample_rate != rate => Some(format!("it is {} Hz and the session {rate} Hz", a.sample_rate)),
                Ok(a) if a.num_channels() != usize::from(src.channels) || a.frames() as u64 != src.frames => {
                    Some("it has changed since the session was saved".to_string())
                }
                Ok(a) => {
                    loaded.push((src.id, a));
                    None
                }
            };
            if let Some(p) = problem {
                messages.push(format!("\"{}\" is offline: {} ({p})", src.name, real.display()));
            }
        }
        for (id, a) in loaded {
            session.pool.insert(id, std::sync::Arc::new(soundcraft_model::SourceAudio::new(a)));
        }
        log::info!(target: "wavcraft_core::session_file", "opened session {:?}: {} source(s), {} offline", session.name, session.sources.len(), messages.len());
        let punch = crate::mtrecord::Punch::new(session.sample_rate.samples(crate::mtrecord::DEFAULT_PREROLL_S));
        let mt = Multitrack {
            session,
            takes: Vec::new(),
            punch,
            tempo_meta: Default::default(),
            file: Some(path),
            clip_types: Default::default(),
            type_filter: None,
        };
        Ok((mt, messages))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soundcraft_audio_io::AudioBuffer;
    use soundcraft_model::BitDepthSetting;

    fn tone(rate: u32, secs: f32, ch: usize, hz: f32) -> AudioBuffer {
        let n = (rate as f32 * secs) as usize;
        AudioBuffer { sample_rate: rate, channels: (0..ch).map(|_| (0..n).map(|i| (i as f32 * hz).sin() * 0.5).collect()).collect() }
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wavcraft-sf-{tag}-{}-{}", std::process::id(), crate::random::hex(4).unwrap_or_default()));
        std::fs::create_dir_all(&d).unwrap_or_else(|e| panic!("{e}"));
        d.canonicalize().unwrap_or(d)
    }

    fn write_wav(path: &Path, buf: &AudioBuffer) {
        let bytes = soundcraft_audio_io::encode(buf, &EncodeOptions { bit_depth: BitDepth::Float32, ..EncodeOptions::default() })
            .unwrap_or_else(|e| panic!("{e}"));
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new("."))).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(path, bytes).unwrap_or_else(|e| panic!("{e}"));
    }

    /// A session with a file clip (in `root/media`) and a buffer clip with no file.
    fn build(root: &Path) -> Multitrack {
        write_wav(&root.join("media/a.wav"), &tone(48_000, 1.0, 2, 0.05));
        let mut m = Multitrack::new("Demo", 48_000, BitDepthSetting::Int24).unwrap_or_else(|e| panic!("{e}"));
        let a = m.add_track(Some("Music"), 2).unwrap_or_else(|e| panic!("{e}"));
        let b = m.add_track(Some("Voice"), 1).unwrap_or_else(|e| panic!("{e}"));
        m.add_clip_file(a, &root.join("media/a.wav"), 1.5).unwrap_or_else(|e| panic!("{e}"));
        m.add_clip_buffer(b, "Mem", "mem", tone(48_000, 0.5, 1, 0.1), 0.25).unwrap_or_else(|e| panic!("{e}"));
        m.set_mute(b, true).unwrap_or_else(|e| panic!("{e}"));
        m.set_volume_db(a, -4.5).unwrap_or_else(|e| panic!("{e}"));
        if let Some(c) = m.session.track_mut(a).and_then(|t| t.playlist_mut()).and_then(|p| p.clips.first_mut()) {
            c.gain_db = 3.0;
        }
        m
    }

    #[test]
    fn a_session_round_trips_with_clips_gains_and_settings() {
        let root = scratch("rt");
        let mut m = build(&root);
        let report = m.save_to(&root.join("proj/demo")).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(report.path, root.join("proj/demo.wcsession"));
        assert_eq!((report.exported, report.offline), (1, 0), "the buffer clip has no file, so it is written beside the session");
        let (back, msgs) = Multitrack::open_from(&report.path).unwrap_or_else(|e| panic!("{e}"));
        assert!(msgs.is_empty(), "{msgs:?}");
        assert_eq!(back.state_json(), m.state_json());
        assert_eq!(back.session.tracks, m.session.tracks, "tracks, clips, gains, mute and volume are all kept");
        assert!(back.offline_sources().is_empty());
        for s in &back.session.sources {
            let (x, y) = (back.session.pool.get(s.id), m.session.pool.get(s.id));
            assert_eq!(x.map(|a| a.buffer.channels.clone()), y.map(|a| a.buffer.channels.clone()), "audio for {}", s.name);
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_file_stores_relative_paths_and_is_owner_only() {
        let root = scratch("rel");
        let mut m = build(&root);
        let r = m.save_to(&root.join("demo.wcsession")).unwrap_or_else(|e| panic!("{e}"));
        let text = std::fs::read_to_string(&r.path).unwrap_or_else(|e| panic!("{e}"));
        let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}"));
        let paths: Vec<_> = v["sources"].as_array().unwrap_or(&vec![]).iter().filter_map(|s| s["path"].as_str().map(String::from)).collect();
        assert!(paths.contains(&"media/a.wav".to_string()), "{paths:?}");
        assert!(paths.iter().any(|p| p.starts_with("demo-media/")), "{paths:?}");
        assert!(!text.contains(&root.to_string_lossy().into_owned()), "no absolute path of this folder in the file");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&r.path).map(|m| m.permissions().mode() & 0o777).unwrap_or(0);
            assert_eq!(mode, 0o600);
        }
        assert!(crate::save::temp_files(&r.path).is_empty(), "no temporary file is left");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_media_opens_offline_with_a_message() {
        let root = scratch("off");
        let mut m = build(&root);
        let r = m.save_to(&root.join("demo.wcsession")).unwrap_or_else(|e| panic!("{e}"));
        std::fs::remove_file(root.join("media/a.wav")).unwrap_or_else(|e| panic!("{e}"));
        let (back, msgs) = Multitrack::open_from(&r.path).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(msgs.len(), 1, "{msgs:?}");
        assert!(msgs[0].contains("offline") && msgs[0].contains("a.wav"), "{msgs:?}");
        assert_eq!(back.clip_count(), 2, "the clips stay");
        assert_eq!(back.offline_sources().len(), 1);
        let offline: Vec<_> = back.session.tracks.iter().flat_map(|t| t.clips().to_vec()).filter(|c| back.clip_offline(c)).collect();
        assert_eq!(offline.len(), 1);
        // Saving it again keeps the offline source by its path, and the file comes back when relinked in place.
        let mut back = back;
        back.save_to(&r.path).unwrap_or_else(|e| panic!("{e}"));
        write_wav(&root.join("media/a.wav"), &tone(48_000, 1.0, 2, 0.05));
        let (again, msgs) = Multitrack::open_from(&r.path).unwrap_or_else(|e| panic!("{e}"));
        assert!(msgs.is_empty() && again.offline_sources().is_empty(), "{msgs:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_changed_file_is_offline_not_wrong_audio() {
        let root = scratch("chg");
        let mut m = build(&root);
        let r = m.save_to(&root.join("demo.wcsession")).unwrap_or_else(|e| panic!("{e}"));
        write_wav(&root.join("media/a.wav"), &tone(48_000, 2.0, 2, 0.05));
        let (back, msgs) = Multitrack::open_from(&r.path).unwrap_or_else(|e| panic!("{e}"));
        assert!(msgs.len() == 1 && msgs[0].contains("changed"), "{msgs:?}");
        assert_eq!(back.offline_sources().len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn relative_paths_still_resolve_after_the_session_folder_moves() {
        let root = scratch("mv");
        let mut m = build(&root);
        // Media next to the session, so the whole folder is self-contained.
        std::fs::create_dir_all(root.join("proj")).unwrap_or_else(|e| panic!("{e}"));
        std::fs::rename(root.join("media"), root.join("proj/media")).unwrap_or_else(|e| panic!("{e}"));
        let mut m2 = {
            // Rebuild against the new place so the stored path is under the session folder.
            let mut x = Multitrack::new("Demo", 48_000, BitDepthSetting::Int24).unwrap_or_else(|e| panic!("{e}"));
            let a = x.add_track(Some("Music"), 2).unwrap_or_else(|e| panic!("{e}"));
            x.add_clip_file(a, &root.join("proj/media/a.wav"), 1.5).unwrap_or_else(|e| panic!("{e}"));
            x
        };
        let _ = &mut m;
        let r = m2.save_to(&root.join("proj/demo.wcsession")).unwrap_or_else(|e| panic!("{e}"));
        let moved = root.join("elsewhere/deeper/renamed-folder");
        std::fs::create_dir_all(moved.parent().unwrap_or(&root)).unwrap_or_else(|e| panic!("{e}"));
        std::fs::rename(root.join("proj"), &moved).unwrap_or_else(|e| panic!("{e}"));
        assert!(!r.path.exists());
        let (back, msgs) = Multitrack::open_from(&moved.join("demo.wcsession")).unwrap_or_else(|e| panic!("{e}"));
        assert!(msgs.is_empty(), "{msgs:?}");
        assert!(back.offline_sources().is_empty());
        assert_eq!(back.clip_count(), 1);
        assert!(back.session.sources[0].path.starts_with(&moved.to_string_lossy().into_owned()), "{}", back.session.sources[0].path);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_failed_save_leaves_the_old_file_and_no_temporary() {
        let root = scratch("fail");
        let mut m = build(&root);
        let r = m.save_to(&root.join("demo.wcsession")).unwrap_or_else(|e| panic!("{e}"));
        let before = std::fs::read(&r.path).unwrap_or_else(|e| panic!("{e}"));
        // The target is a folder, so the rename cannot replace it.
        let blocked = root.join("blocked.wcsession");
        std::fs::create_dir_all(blocked.join("x")).unwrap_or_else(|e| panic!("{e}"));
        assert!(m.save_to(&blocked).is_err());
        assert!(crate::save::temp_files(&blocked).is_empty());
        assert_eq!(std::fs::read(&r.path).unwrap_or_default(), before);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bad_files_are_refused_with_a_message() {
        let root = scratch("bad");
        assert!(Multitrack::open_from(&root.join("nope.wcsession")).is_err());
        std::fs::write(root.join("junk.wcsession"), "not json").unwrap_or_else(|e| panic!("{e}"));
        let e = Multitrack::open_from(&root.join("junk.wcsession")).err().map(|e| e.0).unwrap_or_default();
        assert!(e.contains("not a session"), "{e}");
        std::fs::write(root.join("future.wcsession"), r#"{"format_version": 9999}"#).unwrap_or_else(|e| panic!("{e}"));
        assert!(Multitrack::open_from(&root.join("future.wcsession")).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn relative_path_goes_up_a_few_folders_and_gives_up_far_away() {
        assert_eq!(relative_path(Path::new("/a/b"), Path::new("/a/b/c/d.wav")).as_deref(), Some("c/d.wav"));
        assert_eq!(relative_path(Path::new("/a/b"), Path::new("/a/x/d.wav")).as_deref(), Some("../x/d.wav"));
        assert_eq!(relative_path(Path::new("/a/b/c/d/e"), Path::new("/x.wav")), None);
        assert_eq!(resolve_path(Path::new("/a/b"), "../x/d.wav"), PathBuf::from("/a/x/d.wav"));
    }

    #[test]
    fn an_edited_file_source_is_saved_with_its_new_audio_not_the_old_file() {
        let root = scratch("edit");
        let mut m = build(&root);
        let a = m.track_ids()[0];
        let (_, src) = m.clip_source(a, 1).unwrap_or_else(|e| panic!("{e}"));
        let mut audio = m.source_buffer(src).unwrap_or_else(|e| panic!("{e}"));
        for ch in &mut audio.channels {
            ch.truncate(24_000);
        }
        audio.channels[0][0] = 0.75;
        m.replace_source_audio(src, audio).unwrap_or_else(|e| panic!("{e}"));
        let r = m.save_to(&root.join("edit.wcsession")).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((r.exported, r.offline), (2, 0), "the edited file and the buffer clip are both written");
        let (back, msgs) = Multitrack::open_from(&r.path).unwrap_or_else(|e| panic!("{e}"));
        assert!(msgs.is_empty() && back.offline_sources().is_empty(), "{msgs:?}");
        let (_, back_src) = back.clip_source(a, 1).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(back_src, src);
        assert_eq!(back.session.source(src).map(|s| s.frames), Some(24_000));
        assert_eq!(back.session.track(a).map(|t| t.clips()[0].length), Some(24_000), "the clip is trimmed to the new audio");
        assert_eq!(back.source_buffer(src).map(|b| (b.frames(), b.channels[0][0])), Ok((24_000, 0.75)));
        let _ = std::fs::remove_dir_all(&root);
    }
}
