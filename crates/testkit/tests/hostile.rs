//! The Legion Go review's W8 (`verification-by-legiongo.md`, 2026-10-16): a session folder that
//! can't be written is named as such (not as an unreadable file), and hostile WAV headers are
//! refused or opened with a warning, never a panic.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::Document;
use wavcraft_core::doc::DocState;
use wavcraft_core::io::{MAX_SAMPLE_RATE_HZ, OpenError, OpenOptions, open};
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
    let err = result.expect_err("the open should fail");
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
    let err = open_at(&dir.0.join("missing.wav"), &fine.0).expect_err("a missing file fails");
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
    let err = result.expect_err("an unreadable file fails");
    assert!(matches!(err, OpenError::Io(_)), "{err:?}");
    assert!(err.to_string().starts_with("cannot read the file"), "{err}");
    // Nothing was left in the session folder by either failure.
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
}

/// Write `bytes` as `name` in `dir` and open it.
fn open_bytes(dir: &TempDir, name: &str, bytes: &[u8], root: &Path) -> Result<DocState, OpenError> {
    let path = dir.0.join(name);
    std::fs::write(&path, bytes).unwrap();
    open_at(&path, root)
}

/// The open failed with a decode error whose text holds `needle`.
fn assert_refused(result: Result<DocState, OpenError>, needle: &str, what: &str) {
    match result {
        Err(OpenError::Decode(text)) => assert!(text.contains(needle), "{what}: {text}"),
        Err(other) => panic!("{what}: expected a decode error, got {other:?}"),
        Ok(doc) => panic!("{what}: opened ({} frames at {} Hz) instead of being refused", doc.len_frames(), doc.sample_rate()),
    }
}

/// W8b: a sample rate of 0 or above `MAX_SAMPLE_RATE_HZ` is refused; the bound itself opens.
#[test]
fn hostile_sample_rates_are_refused() {
    let dir = TempDir::new("rates");
    let root = TempDir::new("rates-sessions");
    for rate in [0, MAX_SAMPLE_RATE_HZ + 1, 4_294_967_295] {
        let h = Header { rate, ..Header::good() };
        assert_refused(open_bytes(&dir, &format!("rate-{rate}.wav"), &wav(h, 4_000), &root.0), "sample rate", &format!("{rate} Hz"));
    }
    let h = Header { rate: MAX_SAMPLE_RATE_HZ, ..Header::good() };
    let doc = open_bytes(&dir, "rate-max.wav", &wav(h, 4_000), &root.0).unwrap();
    assert_eq!((doc.sample_rate(), doc.len_frames()), (MAX_SAMPLE_RATE_HZ, 1_000));
    assert!(doc.source().warnings.is_empty(), "{:?}", doc.source().warnings);
}

/// W8b: a block-align of 0, or one that disagrees with channels × bytes a sample, is refused.
#[test]
fn a_block_align_that_disagrees_with_the_format_is_refused() {
    let dir = TempDir::new("align");
    let root = TempDir::new("align-sessions");
    // Stereo 16-bit takes 4 bytes a frame.
    for block_align in [0u16, 2, 3, 8, u16::MAX] {
        let h = Header { block_align, ..Header::good() };
        assert_refused(
            open_bytes(&dir, &format!("align-{block_align}.wav"), &wav(h, 4_000), &root.0),
            "block-align",
            &format!("block-align {block_align}"),
        );
    }
    // 24-bit mono takes 3.
    let h = Header { channels: 1, bits: 24, block_align: 4, data_declared: 3_000, ..Header::good() };
    assert_refused(open_bytes(&dir, "align-24-in-4.wav", &wav(h, 3_000), &root.0), "block-align", "24-bit with block-align 4");
    let h = Header { channels: 1, bits: 24, block_align: 3, data_declared: 3_000, ..Header::good() };
    assert_eq!(open_bytes(&dir, "align-24-in-3.wav", &wav(h, 3_000), &root.0).unwrap().len_frames(), 1_000);
}

/// W8b: a sample size under 8 bits opens as the whole bytes it is stored in, with a warning;
/// 0 bits and more than 32 are refused.
#[test]
fn odd_sample_sizes_warn_or_are_refused() {
    let dir = TempDir::new("bits");
    let root = TempDir::new("bits-sessions");
    let h = Header { channels: 1, bits: 7, block_align: 1, data_declared: 1_000, ..Header::good() };
    let doc = open_bytes(&dir, "bits-7.wav", &wav(h, 1_000), &root.0).unwrap();
    assert_eq!(doc.len_frames(), 1_000);
    let warnings = &doc.source().warnings;
    assert!(warnings.len() == 1 && warnings[0].contains("7 bits per sample"), "{warnings:?}");
    // 12-bit in 2 bytes is how the format stores it: no warning.
    let h = Header { channels: 1, bits: 12, block_align: 2, data_declared: 2_000, ..Header::good() };
    let doc = open_bytes(&dir, "bits-12.wav", &wav(h, 2_000), &root.0).unwrap();
    assert!(doc.source().warnings.is_empty(), "{:?}", doc.source().warnings);
    for bits in [0u16, 33, 40] {
        let h = Header { channels: 1, bits, block_align: bits.div_ceil(8), ..Header::good() };
        assert_refused(open_bytes(&dir, &format!("bits-{bits}.wav"), &wav(h, 4_000), &root.0), "bits per sample", &format!("{bits} bits"));
    }
}

/// W8b: data shorter than the header claims (half of it, or a claim of 0xFFFFFFF0 bytes with
/// 1,000 present) opens what is there, and the warning reaches `Document::warnings`, which the
/// app shows in its status bar.
#[test]
fn data_shorter_than_its_header_opens_with_a_warning() {
    let dir = TempDir::new("short");
    let root = TempDir::new("short-sessions");
    for (name, declared, present, frames) in [("half.wav", 4_000u32, 2_000usize, 500u64), ("huge-claim.wav", 0xFFFF_FFF0, 1_000, 250)] {
        let h = Header { data_declared: declared, ..Header::good() };
        let doc = open_bytes(&dir, name, &wav(h, present), &root.0).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(doc.len_frames(), frames, "{name}");
        let warnings = &doc.source().warnings;
        assert!(warnings.len() == 1 && warnings[0].contains("shorter than its header says"), "{name}: {warnings:?}");

        let path = dir.0.join(name);
        let opened = Document::open_in(&path, &opts(&root.0), &mut |_| {}, &AtomicBool::new(false)).unwrap();
        assert_eq!(opened.warnings(), &warnings[..], "{name}");
    }
}

/// #374: a tiny WAV declaring 65,535 channels is refused with an `OpenError`, not an allocation
/// of gigabytes that aborts the process.
#[test]
fn a_huge_channel_count_is_an_open_error_not_an_abort() {
    let dir = TempDir::new("channels");
    let root = TempDir::new("channels-sessions");
    let h = Header { channels: u16::MAX, rate: 48_000, block_align: u16::MAX, bits: 8, data_declared: 65_535 };
    let result = open_bytes(&dir, "channels-65535.wav", &wav(h, 65_535), &root.0);
    assert!(result.is_err(), "a 65,535-channel WAV opened");
}
