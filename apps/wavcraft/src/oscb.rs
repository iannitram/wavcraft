//! The system clipboard for Copy, Cut and Paste (roadmap A153). Copy puts the clip on the OS
//! clipboard as a 24-bit WAV file (a file entry, which is what Finder, Explorer and the Linux
//! file managers and most audio programs paste from), and Paste takes an audio file another
//! program put there. The files live in one temporary folder, removed on exit. Clips over
//! `wavcraft_core::clipboard::MAX_FILE_BYTES` stay in the internal clipboard.
//! Crate used: `arboard` (MIT OR Apache-2.0), for the file list on all three systems.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use wavcraft_core::clipboard;
use wavcraft_core::doc::edit::Clip;
use wavcraft_core::io::OpenOptions;

/// What was on the OS clipboard when we last looked: a file and when it was written.
type Seen = (PathBuf, Option<SystemTime>);

pub struct OsClipboard {
    board: Option<arboard::Clipboard>,
    dir: PathBuf,
    counter: u32,
    /// The file Copy last put there.
    ours: Option<PathBuf>,
    /// The last entry we acted on (put there, or pasted from), so a second Paste does not read it again.
    seen: Option<Seen>,
}

impl OsClipboard {
    pub fn new() -> OsClipboard {
        let dir = std::env::temp_dir().join(format!("wavcraft-clipboard-{}", std::process::id()));
        OsClipboard { board: None, dir, counter: 0, ours: None, seen: None }
    }

    fn board(&mut self) -> Result<&mut arboard::Clipboard, String> {
        if self.board.is_none() {
            self.board = Some(arboard::Clipboard::new().map_err(|e| format!("no system clipboard: {e}"))?);
        }
        self.board.as_mut().ok_or_else(|| "no system clipboard".to_string())
    }

    /// Put `clip` on the OS clipboard. An error says why it stayed internal.
    pub fn publish(&mut self, clip: &Clip) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        self.counter += 1;
        let path = self.dir.join(format!("WavCraft clip {}.wav", self.counter));
        clipboard::write_clip_wav(clip, &path).map_err(|e| e.to_string())?;
        let put = self.board()?.set().file_list(&[&path]).map_err(|e| format!("could not use the system clipboard: {e}"));
        if let Err(e) = put {
            let _ = std::fs::remove_file(&path);
            return Err(e);
        }
        if let Some(old) = self.ours.replace(path.clone()) {
            let _ = std::fs::remove_file(old);
        }
        self.seen = Some(stamp(&path));
        Ok(())
    }

    /// The audio file on the OS clipboard that another program put there and we have not
    /// taken yet, if any.
    pub fn incoming(&mut self) -> Option<PathBuf> {
        let files = self.board().ok()?.get().file_list().ok()?;
        let path = files.into_iter().find(|p| clipboard::is_audio_path(p) && p.is_file())?;
        let now = stamp(&path);
        (self.seen.as_ref() != Some(&now)).then_some(path)
    }

    /// Read `path` (from [`incoming`](Self::incoming)) into a clip, and remember it was taken.
    pub fn take(&mut self, path: &Path, opts: &OpenOptions) -> Result<Clip, String> {
        self.seen = Some(stamp(path));
        clipboard::clip_from_file(path, opts).map_err(|e| e.to_string())
    }

    /// Test hook (control channel): put any file on the OS clipboard as if another program had.
    pub fn put_foreign(&mut self, path: &Path) -> Result<(), String> {
        self.board()?.set().file_list(&[path]).map_err(|e| e.to_string())
    }

    /// Test hook: the first file on the OS clipboard, and whether it is ours.
    pub fn peek(&mut self) -> (Option<PathBuf>, bool) {
        let first = self.board().ok().and_then(|b| b.get().file_list().ok()).and_then(|f| f.into_iter().next());
        let ours = matches!((&first, &self.ours), (Some(a), Some(b)) if a.file_name() == b.file_name());
        (first, ours)
    }
}

impl Drop for OsClipboard {
    fn drop(&mut self) {
        // Only our own folder; the clipboard entry would point at a missing file after this.
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The file's identity: its resolved path (the system hands paths back resolved, e.g. `/private/var`
/// for `/var`) and when it was written.
fn stamp(path: &Path) -> Seen {
    (path.canonicalize().unwrap_or_else(|_| path.to_path_buf()), std::fs::metadata(path).and_then(|m| m.modified()).ok())
}
