//! Disk space for the session store: how much opening a file will write, and how much room the
//! disk holding the session folder has.
//!
//! The store keeps every sample as a 32-bit float, so a file needs
//! `frames × channels × 4` bytes whatever its own sample format (a 16-bit file takes twice its
//! size, a 3-hour 48 kHz stereo episode about 4.1 GB). [`store_bytes`] adds a small margin on
//! top. For a compressed file whose decoder can't say how many frames it holds,
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

/// The bytes `frames` frames of `channels` channels take in the store (no margin).
pub fn raw_store_bytes(frames: u64, channels: u64) -> u64 {
    frames.saturating_mul(channels).saturating_mul(4)
}

/// The space to ask for before writing `frames` frames of `channels` channels: the store's
/// bytes plus 1 % plus [`MARGIN_BYTES`].
pub fn store_bytes(frames: u64, channels: u64) -> u64 {
    let raw = raw_store_bytes(frames, channels);
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
///
/// On Unix the file system holding the folder itself is asked (`statvfs`), so a folder on a
/// tmpfs such as `/tmp` or `/dev/shm` gets that file system's figure (the Linux handheld review's W7:
/// sysinfo's disk list has no tmpfs, so `/tmp` used to be matched to `/`). Elsewhere, or when
/// that call fails, the folder is matched to the longest mount point in sysinfo's disk list.
pub fn free_space(path: &Path) -> Option<u64> {
    let path = std::fs::canonicalize(path).ok()?;
    folder_free_space(&path).or_else(|| mount_free_space(&path))
}

/// The bytes free to this user on the file system holding `path`, asked of the OS directly.
#[cfg(unix)]
fn folder_free_space(path: &Path) -> Option<u64> {
    let st = rustix::fs::statvfs(path).ok()?;
    // Blocks are counted in fragments (`f_frsize`); a system that leaves it 0 means `f_bsize`.
    let unit = if st.f_frsize > 0 { st.f_frsize } else { st.f_bsize };
    Some(st.f_bavail.saturating_mul(unit))
}

/// Not asked directly here: Windows' call needs `unsafe`, which the workspace forbids, and
/// sysinfo's volume list there covers every drive and mounted folder.
#[cfg(not(unix))]
fn folder_free_space(_path: &Path) -> Option<u64> {
    None
}

/// The bytes free on the longest mount point in sysinfo's disk list that holds `path`.
fn mount_free_space(path: &Path) -> Option<u64> {
    let path = plain_path(path);
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

    /// W7 (the Linux handheld review): a folder on a tmpfs reports that file system's free space, not
    /// that of `/`. Linux only (it runs on a Linux handheld); skipped when neither
    /// `/dev/shm` nor `/tmp` is a tmpfs, or when `/` is one itself.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_tmpfs_folder_reports_its_own_free_space() {
        let mounts = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
        // The last line for a mount point wins, as the kernel lists them in mount order.
        let fs_type = |mount: &str| {
            mounts.lines().rev().find_map(|l| {
                let mut f = l.split_whitespace();
                let (_, at, kind) = (f.next()?, f.next()?, f.next()?);
                (at == mount).then(|| kind.to_string())
            })
        };
        if fs_type("/").as_deref() == Some("tmpfs") {
            eprintln!("skipped: / is a tmpfs here");
            return;
        }
        let Some(folder) = ["/dev/shm", "/tmp"].into_iter().find(|m| fs_type(m).as_deref() == Some("tmpfs")) else {
            eprintln!("skipped: neither /dev/shm nor /tmp is a tmpfs here");
            return;
        };
        let got = free_space(Path::new(folder)).expect("free space of a tmpfs folder");
        let st = rustix::fs::statvfs(folder).expect("statvfs");
        let total = st.f_blocks.saturating_mul(if st.f_frsize > 0 { st.f_frsize } else { st.f_bsize });
        assert!(got <= total, "{folder}: {got} bytes free, more than the tmpfs holds ({total})");
        let root = free_space(Path::new("/")).expect("free space of /");
        assert_ne!(got, root, "{folder} reported the same free space as /, as when it was matched to /");
        // The sysinfo match this replaced gives `/`'s figure for it (no tmpfs in its list).
        println!("{folder}: {got} bytes free (tmpfs of {total}); / has {root}; sysinfo's match says {:?}", mount_free_space(Path::new(folder)));
    }

    #[test]
    fn bytes_read_as_people_say_them() {
        assert_eq!(format_bytes(4_147_200_000), "4.1 GB");
        assert_eq!(format_bytes(850_000_000), "850 MB");
        assert_eq!(format_bytes(999), "999 bytes");
    }
}
