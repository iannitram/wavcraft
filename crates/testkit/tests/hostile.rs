//! The Legion Go review's W8 (`verification-by-legiongo.md`, 2026-10-16): a session folder that
//! can't be written is named as such (not as an unreadable file), and hostile WAV headers are
//! refused or opened with a warning, never a panic.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::doc::DocState;
use wavcraft_core::io::{OpenError, OpenOptions, open};
use wavcraft_core::settings::HOW_TO_CHANGE;

/// A folder in the system's temp directory, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-hostile-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The fmt and data fields of a plain PCM WAV, each settable to a hostile value.
#[derive(Debug, Clone, Copy)]
struct Header {
    channels: u16,
    rate: u32,
    block_align: u16,
    bits: u16,
    /// What the data chunk's size field claims.
    data_declared: u32,
}

impl Header {
    /// A sound header: stereo 16-bit at 48 kHz, data claimed as 1,000 frames.
    fn good() -> Header {
        Header { channels: 2, rate: 48_000, block_align: 4, bits: 16, data_declared: 4_000 }
    }
}

/// A WAV with `h`'s fields and `data_len` bytes of data after the header, whatever it claims.
fn wav(h: Header, data_len: usize) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(b"RIFF");
    out.extend((36 + data_len as u32).to_le_bytes());
    out.extend(b"WAVE");
    out.extend(b"fmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(h.channels.to_le_bytes());
    out.extend(h.rate.to_le_bytes());
    out.extend(h.rate.wrapping_mul(u32::from(h.block_align)).to_le_bytes());
    out.extend(h.block_align.to_le_bytes());
    out.extend(h.bits.to_le_bytes());
    out.extend(b"data");
    out.extend(h.data_declared.to_le_bytes());
    out.extend((0..data_len).map(|i| (i % 251) as u8));
    out
}

fn opts(root: &Path) -> OpenOptions {
    OpenOptions { sessions_root: Some(root.to_path_buf()), ..OpenOptions::default() }
}

/// Open `path` with session folders in `root`; a panic fails the test with the file's name.
fn open_at(path: &Path, root: &Path) -> Result<DocState, OpenError> {
    catch_unwind(AssertUnwindSafe(|| open(path, &opts(root), &mut |_| {}, &AtomicBool::new(false))))
        .unwrap_or_else(|_| panic!("{}: open panicked", path.display()))
}

/// The error is the session folder's: it names `folder` and says how to choose another, and does
/// not blame the file.
fn assert_session_folder_error(result: Result<DocState, OpenError>, folder: &Path) {
    let err = result.err().expect("the open should fail");
    let text = err.to_string();
    match &err {
        OpenError::SessionFolder { folder: f, .. } => assert_eq!(f, folder, "{text}"),
        other => panic!("expected a session folder error, got {other:?}: {text}"),
    }
    assert!(text.contains("session folder") && text.contains(&folder.display().to_string()), "{text}");
    assert!(text.contains(HOW_TO_CHANGE), "{text}");
    assert!(!text.contains("cannot read the file"), "{text}");
}

/// W8a, on every platform: a session folder that can't be made (its parent is a file) is named
/// as the problem; a file that isn't there is still "cannot read the file".
#[test]
fn a_session_folder_that_cannot_be_made_is_named_not_the_file() {
    let dir = TempDir::new("folder-made");
    let good = dir.0.join("good.wav");
    std::fs::write(&good, wav(Header::good(), 4_000)).unwrap();
    let blocker = dir.0.join("a-file");
    std::fs::write(&blocker, b"not a folder").unwrap();
    let root = blocker.join("sessions");

    assert_session_folder_error(open_at(&good, &root), &root);

    // The other case: the source can't be read, the session folder is fine.
    let fine = TempDir::new("folder-made-sessions");
    let err = open_at(&dir.0.join("missing.wav"), &fine.0).err().expect("a missing file fails");
    assert!(matches!(err, OpenError::Io(_)), "{err:?}");
    assert!(err.to_string().starts_with("cannot read the file"), "{err}");
    // And it opens once the session folder is one that works.
    assert_eq!(open_at(&good, &fine.0).unwrap().len_frames(), 1_000);
}

/// W8a with permissions (Unix only: Windows' read-only attribute doesn't stop files being made in
/// a folder). A read-only session folder is named; an unreadable source file is "cannot read the
/// file". Running as root bypasses permissions, so the test checks that first and skips.
#[cfg(unix)]
#[test]
fn a_read_only_session_folder_and_an_unreadable_file_are_told_apart() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new("perms");
    let good = dir.0.join("good.wav");
    std::fs::write(&good, wav(Header::good(), 4_000)).unwrap();
    let root = dir.0.join("read-only-sessions");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o555)).unwrap();
    let probe = root.join("probe");
    if std::fs::write(&probe, b"x").is_ok() {
        let _ = std::fs::remove_file(&probe);
        let _ = std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755));
        eprintln!("skipped: a read-only folder can be written here (running as root?)");
        return;
    }

    let result = open_at(&good, &root);
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_session_folder_error(result, &root);

    // An unreadable source with a writable session folder.
    let secret = dir.0.join("secret.wav");
    std::fs::write(&secret, wav(Header::good(), 4_000)).unwrap();
    std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o000)).unwrap();
    let result = open_at(&secret, &root);
    std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o644)).unwrap();
    let err = result.err().expect("an unreadable file fails");
    assert!(matches!(err, OpenError::Io(_)), "{err:?}");
    assert!(err.to_string().starts_with("cannot read the file"), "{err}");
    // Nothing was left in the session folder by either failure.
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
}
