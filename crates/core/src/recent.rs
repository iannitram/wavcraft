//! The "Open Recent" list: the last [`MAX_RECENT`] files opened, newest first, kept in a small
//! text file in the config folder (`recent.txt` next to `settings.txt`), one path per line.
//! Also the config folder's file naming and an atomic write, shared with other small lists the
//! app remembers (the Files panel's columns).

use std::io;
use std::path::{Path, PathBuf};

/// How many files the list keeps.
pub const MAX_RECENT: usize = 10;

/// The config folder's file `name` (`<config dir>/wavcraft/<name>`), when the system has one.
pub fn config_file(name: &str) -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("wavcraft").join(name))
}

/// Write `text` to `path` (its folder made if missing) by writing a neighbour and renaming it
/// over, so a crash never leaves half a file.
pub fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".new");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// The recent files, newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recent {
    paths: Vec<PathBuf>,
}

impl Recent {
    pub fn new() -> Recent {
        Recent::default()
    }

    /// The list in the file's text: one path a line; blank lines and `#` comments are skipped,
    /// repeats dropped, and only the first [`MAX_RECENT`] kept.
    pub fn parse(text: &str) -> Recent {
        let mut r = Recent::new();
        for line in text.lines() {
            let line = line.trim_end_matches('\r');
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let p = PathBuf::from(line);
            if r.paths.len() < MAX_RECENT && !r.paths.contains(&p) {
                r.paths.push(p);
            }
        }
        r
    }

    pub fn to_text(&self) -> String {
        let mut out = String::from("# WavCraft: the files Open Recent lists, newest first. One path a line.\n");
        for p in &self.paths {
            out.push_str(&p.to_string_lossy());
            out.push('\n');
        }
        out
    }

    /// Read `path`; a missing or unreadable file is an empty list.
    pub fn load(path: &Path) -> Recent {
        match std::fs::read_to_string(path) {
            Ok(text) => Recent::parse(&text),
            Err(e) => {
                if e.kind() != io::ErrorKind::NotFound {
                    log::warn!("could not read the recent files list {}: {e}", path.display());
                }
                Recent::new()
            }
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        write_atomic(path, &self.to_text())
    }

    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// `path` was opened: it goes to the front (once), and the oldest fall off. A path with a
    /// line break in it can't be kept in the file and is not listed.
    pub fn push(&mut self, path: &Path) {
        if path.as_os_str().is_empty() || path.to_string_lossy().contains(['\n', '\r']) {
            return;
        }
        self.paths.retain(|p| p != path);
        self.paths.insert(0, path.to_path_buf());
        self.paths.truncate(MAX_RECENT);
    }

    /// Take entry `i` out of the list, giving its path back.
    pub fn remove(&mut self, i: usize) -> Option<PathBuf> {
        (i < self.paths.len()).then(|| self.paths.remove(i))
    }

    pub fn clear(&mut self) {
        self.paths.clear();
    }
}
