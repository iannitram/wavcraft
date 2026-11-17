#![cfg(feature = "whisper")]

use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

use wavcraft_core::transcript::models::{self, DownloadEntry};

const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

fn temp_dir(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("wavcraft-transcript-models-{}-{label}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn serve(body: Vec<u8>) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let thread = thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let header = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
        let _ = stream.write_all(header.as_bytes());
        let _ = stream.write_all(&body);
    });
    (format!("http://{address}/model"), thread)
}

fn entry(url: String, sha256: &'static str) -> DownloadEntry {
    DownloadEntry { name: "model.bin", url, sha256 }
}

fn files(folder: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(folder).unwrap().map(|entry| entry.unwrap().path()).collect()
}

#[test]
fn download_verifies_then_keeps_the_file() {
    // Catches: a response is published only after its bytes match the expected SHA-256.
    let folder = temp_dir("verified");
    let (url, server) = serve(b"abc".to_vec());
    let mut progress = Vec::new();
    let path = models::download(&entry(url, ABC_SHA256), &folder, &AtomicBool::new(false), &mut |p| progress.push(p)).unwrap();

    server.join().unwrap();
    assert_eq!(path, folder.join("model.bin"));
    assert_eq!(std::fs::read(&path).unwrap(), b"abc");
    assert_eq!(files(&folder), vec![path]);
    assert_eq!(progress.last(), Some(&1.0));
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn a_wrong_hash_leaves_no_file() {
    // Catches: a hash mismatch removes the temporary download and never publishes the destination.
    let folder = temp_dir("wrong-hash");
    let (url, server) = serve(b"abd".to_vec());
    let result = models::download(&entry(url, ABC_SHA256), &folder, &AtomicBool::new(false), &mut |_| {});

    server.join().unwrap();
    assert!(result.is_err_and(|error| error.kind() == std::io::ErrorKind::InvalidData));
    assert!(files(&folder).is_empty());
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn cancel_leaves_no_file() {
    // Catches: cancellation during a streamed response removes the temporary file.
    let folder = temp_dir("cancelled");
    let (url, server) = serve(vec![b'x'; 256 * 1024]);
    let cancel = AtomicBool::new(false);
    let result = models::download(&entry(url, ABC_SHA256), &folder, &cancel, &mut |p| {
        if p > 0.0 {
            cancel.store(true, Ordering::Relaxed);
        }
    });

    server.join().unwrap();
    assert!(result.is_err_and(|error| error.kind() == std::io::ErrorKind::Interrupted));
    assert!(files(&folder).is_empty());
    std::fs::remove_dir_all(folder).unwrap();
}
