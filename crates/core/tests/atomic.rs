//! C324: crash-safe write helper.

use std::io::{self, Write};
use std::path::PathBuf;

use wavcraft_core::atomic::write_atomic;

/// A folder in the system's temp directory, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-atomic-test-{}-{name}", std::process::id()));
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

fn tmp_files(dir: &TempDir) -> Vec<String> {
    std::fs::read_dir(&dir.0).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".tmp")).collect()
}

#[test]
fn new_target_gets_the_bytes_and_no_temp_remains() {
    let dir = TempDir::new("new");
    let target = dir.0.join("out.bin");
    write_atomic(&target, |f| f.write_all(b"hello")).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"hello");
    assert!(tmp_files(&dir).is_empty());
}

#[test]
fn existing_target_is_replaced_whole() {
    let dir = TempDir::new("replace");
    let target = dir.0.join("out.bin");
    std::fs::write(&target, b"old").unwrap();
    write_atomic(&target, |f| f.write_all(b"new contents")).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"new contents");
    assert!(tmp_files(&dir).is_empty());
}

#[test]
fn failed_write_keeps_the_old_target_and_removes_the_temp() {
    let dir = TempDir::new("fail-existing");
    let target = dir.0.join("out.bin");
    std::fs::write(&target, b"old").unwrap();
    let result = write_atomic(&target, |f| {
        f.write_all(b"half")?;
        Err(io::Error::from(io::ErrorKind::Other))
    });
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Other);
    assert_eq!(std::fs::read(&target).unwrap(), b"old");
    assert!(tmp_files(&dir).is_empty());
}

#[test]
fn failed_write_on_a_new_target_leaves_nothing() {
    let dir = TempDir::new("fail-new");
    let target = dir.0.join("out.bin");
    let result = write_atomic(&target, |f| {
        f.write_all(b"half")?;
        Err(io::Error::from(io::ErrorKind::Other))
    });
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Other);
    assert!(!target.exists());
    assert!(tmp_files(&dir).is_empty());
}

#[test]
fn missing_folder_is_an_error_and_creates_nothing() {
    let dir = TempDir::new("missing-folder");
    let folder = dir.0.join("nope");
    let target = folder.join("out.bin");
    assert!(write_atomic(&target, |f| f.write_all(b"hello")).is_err());
    assert!(!folder.exists());
    assert!(!target.exists());
    assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 0);
}

#[test]
fn target_without_a_file_name_is_invalid_input() {
    let err = write_atomic(std::path::Path::new(""), |f| f.write_all(b"x")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}
