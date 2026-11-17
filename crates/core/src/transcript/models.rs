//! Catalogue of speech model files (L1.7, #1536).

use std::fs::File;
#[cfg(feature = "whisper")]
use std::io::Write;
use std::io::{self, Read};
use std::path::Path;
#[cfg(feature = "whisper")]
use std::path::PathBuf;
#[cfg(feature = "whisper")]
use std::sync::atomic::{AtomicBool, Ordering};

use sha2::{Digest, Sha256};

/// A file published as part of a speech model repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelFile {
    /// The file name inside the repository.
    pub name: &'static str,
    /// The expected SHA-256, as 64 lower-case hexadecimal digits.
    pub sha256: &'static str,
}

/// A pinned speech model repository and its files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Model {
    pub id: &'static str,
    pub repo: &'static str,
    pub revision: &'static str,
    pub files: &'static [ModelFile],
}

/// OpenAI Whisper base, pinned to the published repository revision.
pub const WHISPER_BASE: Model = Model {
    id: "whisper-base",
    repo: "openai/whisper-base",
    revision: "e37978b90ca9030d5170a5c07aadb050351a65bb",
    files: &[
        ModelFile { name: "config.json", sha256: "a153c53883a6799b6f056b4a8d1a515c9926d03994682ba88a7616618d7da0c1" },
        ModelFile { name: "tokenizer.json", sha256: "27fc476bfe7f17299480be2273fc0608e4d5a99aba2ab5dec5374b4482d1a566" },
        ModelFile { name: "model.safetensors", sha256: "07cadb9f25677c8d50df603e66a98fbd842cce45047139baeb16e6219a1e807b" },
    ],
};

impl Model {
    /// The URL of `file` at this model's pinned revision.
    pub fn url(&self, file: &ModelFile) -> String {
        format!("https://huggingface.co/{}/resolve/{}/{}", self.repo, self.revision, file.name)
    }

    /// A downloadable file from this model.
    #[cfg(feature = "whisper")]
    pub fn entry(&self, file: &ModelFile) -> DownloadEntry {
        DownloadEntry { name: file.name, url: self.url(file), sha256: file.sha256 }
    }
}

/// One model file to download and verify.
#[cfg(feature = "whisper")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadEntry {
    pub name: &'static str,
    pub url: String,
    pub sha256: &'static str,
}

/// How long to wait for a connection, and for each read, before giving up. The read timeout
/// bounds how long a stalled server can delay a cancel.
#[cfg(feature = "whisper")]
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
#[cfg(feature = "whisper")]
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Download an entry to `folder`, publishing it only after its SHA-256 matches.
///
/// The response is streamed to a temporary file beside the destination. Cancellation, network
/// errors, and a bad hash remove that temporary file and never leave a partial destination.
/// An existing destination is replaced by the verified file.
///
/// `progress` receives fractions in 0..=1 as bytes arrive, then 1.0 on success. When the server
/// sends no usable `Content-Length` the total is unknown, so no intermediate progress is
/// reported and only the final 1.0 arrives.
///
/// `cancel` is checked before each read. The read timeout applies to each read, not to the
/// whole download, so a slow but live server never times out; a stalled one delays a cancel by
/// at most one read timeout, and cancel takes effect between reads.
#[cfg(feature = "whisper")]
pub fn download(entry: &DownloadEntry, folder: &Path, cancel: &AtomicBool, progress: &mut dyn FnMut(f64)) -> io::Result<PathBuf> {
    if cancel.load(Ordering::Relaxed) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "model download cancelled"));
    }
    let name_path = Path::new(entry.name);
    if entry.name.is_empty() || name_path.components().count() != 1 || name_path.file_name().and_then(|name| name.to_str()) != Some(entry.name) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "model file name must be a single path component"));
    }
    std::fs::create_dir_all(folder)?;
    let destination = folder.join(entry.name);
    let temporary = folder.join(format!(".{}.{}.part", entry.name, crate::random::hex(16)?));
    let mut output = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;

    let result = (|| {
        let response = ureq::AgentBuilder::new()
            .timeout_connect(CONNECT_TIMEOUT)
            .timeout_read(READ_TIMEOUT)
            .build()
            .get(&entry.url)
            .call()
            .map_err(io::Error::other)?;
        let total = response.header("Content-Length").and_then(|length| length.parse::<u64>().ok());
        let mut reader = response.into_reader();
        let mut buffer = [0u8; 64 * 1024];
        let mut downloaded = 0u64;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "model download cancelled"));
            }
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            output.write_all(&buffer[..count])?;
            downloaded = downloaded.saturating_add(count as u64);
            if let Some(total) = total.filter(|total| *total > 0) {
                progress((downloaded as f64 / total as f64).clamp(0.0, 1.0));
            }
        }
        output.sync_all()?;
        drop(output);
        verify(&temporary, entry.sha256)?;
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "model download cancelled"));
        }
        std::fs::rename(&temporary, &destination)?;
        progress(1.0);
        Ok(destination.clone())
    })();

    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

/// Verify a file's SHA-256, reading it in bounded 64 KiB pieces.
pub fn verify(path: &Path, sha256: &str) -> io::Result<()> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    let actual = format!("{:x}", hash.finalize());
    if actual.eq_ignore_ascii_case(sha256) { Ok(()) } else { Err(bad(format!("{} has SHA-256 {actual}; expected {sha256}", path.display()))) }
}

fn bad(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("wavcraft-models-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn lower_hex(value: &str) -> bool {
        value.len() == 64 && value.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
    }

    #[test]
    fn catalogue_hashes_are_64_hex() {
        assert_eq!(WHISPER_BASE.revision.len(), 40);
        assert!(WHISPER_BASE.revision.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)));
        assert_eq!(WHISPER_BASE.files.len(), 3);
        assert!(WHISPER_BASE.files.iter().all(|file| lower_hex(file.sha256)));
        assert_eq!(WHISPER_BASE.files.iter().map(|file| file.name).collect::<Vec<_>>(), ["config.json", "tokenizer.json", "model.safetensors"]);
        assert_eq!(
            WHISPER_BASE.url(&WHISPER_BASE.files[2]),
            format!("https://huggingface.co/openai/whisper-base/resolve/{}/model.safetensors", WHISPER_BASE.revision)
        );
    }

    #[test]
    fn a_wrong_hash_is_refused() {
        let dir = temp_dir("wrong");
        let path = dir.join("model.bin");
        std::fs::write(&path, b"abd").unwrap();
        let error = verify(&path, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(verify(&dir.join("missing"), "00").is_err_and(|e| e.kind() == io::ErrorKind::NotFound));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_right_hash_passes() {
        let dir = temp_dir("right");
        let path = dir.join("model.bin");
        std::fs::write(&path, b"abc").unwrap();
        let hash = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(verify(&path, hash).is_ok());
        assert!(verify(&path, &hash.to_ascii_uppercase()).is_ok());
        std::fs::write(&path, vec![b'a'; 200_000]).unwrap();
        assert!(verify(&path, "2287d207f24a941ff3b56c04c8a25ad56b63e3023207b3bb5b4ac0c9869d74be").is_ok());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
