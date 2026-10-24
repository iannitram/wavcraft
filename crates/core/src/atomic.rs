//! Crash-safe file writing: write to a temporary file beside the target, sync it, then rename it
//! into place, so a crash never leaves a half-written output.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

/// Write `target` atomically. `write` fills a temporary file made in the same folder; the file is
/// synced, closed and renamed over `target`. On unix the folder is synced too, but that sync is
/// best effort: its failure is ignored once the rename has succeeded. If `write`, the file sync or
/// the rename fails, the temporary file is removed, `target` is left as it was, and the error is
/// returned. A missing target is created; an existing one is replaced whole.
pub fn write_atomic<F>(target: &Path, write: F) -> io::Result<()>
where
    F: FnOnce(&mut File) -> io::Result<()>,
{
    let file_name =
        target.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target has no file name"))?.to_string_lossy().into_owned();
    // Create the temp file exclusively (O_EXCL: never follows a symlink, never truncates) under a
    // unique name; if the name is taken, try another.
    for _ in 0..16 {
        let tmp = target.with_file_name(format!("{file_name}.{}.{}.tmp", std::process::id(), NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed)));
        match OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(mut file) => {
                let written = write(&mut file).and_then(|()| file.sync_all());
                drop(file);
                if let Err(error) = written {
                    let _ = std::fs::remove_file(&tmp);
                    return Err(error);
                }
                if let Err(error) = std::fs::rename(&tmp, target) {
                    let _ = std::fs::remove_file(&tmp);
                    return Err(error);
                }
                #[cfg(unix)]
                {
                    let parent = match target.parent() {
                        Some(p) if !p.as_os_str().is_empty() => p,
                        _ => Path::new("."),
                    };
                    if let Ok(dir) = File::open(parent) {
                        let _ = dir.sync_all();
                    }
                }
                return Ok(());
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "no unused temporary file name found"))
}
