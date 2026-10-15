//! Disk space for the session store: how much opening a file will write, and how much room the
//! disk holding the session folder has.
//!
//! The store keeps a 16-bit source as 16-bit integers (2 bytes a sample), a 24-bit source as
//! 24-bit integers (3 bytes) and everything else as 32-bit floats (4 bytes)
//! ([`io::store_encoding`](crate::io::store_encoding)), so a file needs
//! `frames × channels × width` bytes: a 3-hour 48 kHz stereo 16-bit episode about 2.1 GB, its
//! own size. [`store_bytes`] adds a small margin on top. For a compressed file whose decoder can't say how many frames it holds,
//! [`frames_from_file_size`] guesses from the file's size.

use std::path::{Path, PathBuf};

/// A function that says how many bytes are free on the disk holding a folder (`None`: unknown).
/// [`free_space`] is the real one; tests pass their own.
pub type FreeSpaceFn = fn(&Path) -> Option<u64>;

/// Bytes added to every estimate on top of 1 %: the lock file, the file system's own
/// bookkeeping, and some room left over for the rest of the machine.
pub const MARGIN_BYTES: u64 = 64 << 20;

/// The bit rate assumed for a compressed file whose length the decoder can't tell (bits per
/// second). Spoken-word MP3s are mostly 64 to 192 kbit/s.
pub const ASSUMED_BITRATE: u64 = 128_000;

/// The bytes `frames` frames of `channels` channels take in the store at `width` bytes a sample
/// ([`Encoding::width`](crate::store::Encoding::width)), no margin.
pub fn raw_store_bytes(frames: u64, channels: u64, width: u64) -> u64 {
    frames.saturating_mul(channels).saturating_mul(width)
}

/// The space to ask for before writing `frames` frames of `channels` channels at `width` bytes
/// a sample: the store's bytes plus 1 % plus [`MARGIN_BYTES`].
pub fn store_bytes(frames: u64, channels: u64, width: u64) -> u64 {
    let raw = raw_store_bytes(frames, channels, width);
    raw.saturating_add(raw / 100).saturating_add(MARGIN_BYTES)
}

/// A guess at the frames in a compressed file of `file_bytes` bytes at `sample_rate`, taking it
/// to be [`ASSUMED_BITRATE`].
pub fn frames_from_file_size(file_bytes: u64, sample_rate: u32) -> u64 {
    let frames = u128::from(file_bytes) * 8 * u128::from(sample_rate) / u128::from(ASSUMED_BITRATE);
    u64::try_from(frames).unwrap_or(u64::MAX)
}

/// The bytes free (to this user) on the disk that holds `path`, which should exist. `None` when
/// the system won't say.
pub fn free_space(path: &Path) -> Option<u64> {
    let path = plain_path(&std::fs::canonicalize(path).ok()?);
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .filter(|d| path.starts_with(plain_path(d.mount_point())))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
}

/// `path` without Windows' `\\?\` prefix, which `canonicalize` adds and mount points lack.
fn plain_path(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

/// A byte count for people: "4.1 GB", "850 MB", "12 KB" (powers of 1000, as disks are sold).
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [(&str, f64); 4] = [("TB", 1e12), ("GB", 1e9), ("MB", 1e6), ("KB", 1e3)];
    let b = bytes as f64;
    for (unit, size) in UNITS {
        if b >= size {
            let v = b / size;
            return if v < 10.0 { format!("{v:.1} {unit}") } else { format!("{v:.0} {unit}") };
        }
    }
    format!("{bytes} bytes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbatim_prefixes_are_dropped() {
        assert_eq!(plain_path(Path::new(r"\\?\C:\Users\x")), PathBuf::from(r"C:\Users\x"));
        assert_eq!(plain_path(Path::new("/home/x")), PathBuf::from("/home/x"));
    }

    #[test]
    fn bytes_read_as_people_say_them() {
        assert_eq!(format_bytes(4_147_200_000), "4.1 GB");
        assert_eq!(format_bytes(850_000_000), "850 MB");
        assert_eq!(format_bytes(999), "999 bytes");
    }
}
