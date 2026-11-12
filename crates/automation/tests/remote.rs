//! The remote backend's protocol, against a stand-in control channel that answers from a script.
//! It checks what goes on the wire and how replies are read; it does not run the app. The
//! app-level check (the same steps through a running window, sample for sample) is still to do.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::mpsc;

use wavcraft_automation::remote::Remote;
use wavcraft_automation::{Backend, Request};

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// A stand-in channel: checks the token, then answers each line with the next scripted reply
/// (or `ERR` when the script has run out), and sends every line it received back to the test.
fn stand_in(script: Vec<&'static str>) -> (u16, mpsc::Receiver<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else { return };
        let mut lines = BufReader::new(stream.try_clone().expect("clone")).lines();
        let mut seen = Vec::new();
        let first = lines.next().and_then(Result::ok).unwrap_or_default();
        if first != format!("TOKEN {TOKEN}") {
            let _ = writeln!(stream, "ERR the first line must be TOKEN <token>");
            let _ = tx.send(vec![first]);
            return;
        }
        let _ = writeln!(stream, "OK");
        let mut replies = script.into_iter();
        seen.push(first);
        for line in lines.map_while(Result::ok) {
            seen.push(line);
            let reply = replies.next().unwrap_or("ERR script ended");
            if writeln!(stream, "{reply}").is_err() {
                break;
            }
        }
        let _ = tx.send(seen);
    });
    (port, rx)
}

#[test]
fn requests_go_out_as_the_channel_spells_them_and_replies_are_read() {
    let (port, seen) = stand_in(vec!["OK selection=1-2", "OK selection=1-2 changed=1", "ERR no file is open"]);
    let mut remote = Remote::connect(port, TOKEN).expect("the token is accepted");
    assert_eq!(remote.execute(&Request::Select { start: 1, end: 2 }), Ok("selection=1-2".to_string()));
    assert_eq!(remote.execute(&Request::Action("edit.delete".to_string())), Ok("selection=1-2 changed=1".to_string()));
    assert_eq!(remote.execute(&Request::parse("SAVE out.wav").expect("a save request")), Err("no file is open".to_string()));
    drop(remote);
    assert_eq!(
        seen.recv().expect("the stand-in ends"),
        vec![format!("TOKEN {TOKEN}"), "SELECT 1 2".to_string(), "ACTION edit.delete".to_string(), "SAVE out.wav".to_string(),]
    );
}

#[test]
fn open_waits_for_busy_to_clear() {
    let (port, seen) = stand_in(vec![
        "OK",
        "OK frames=10 channels=2 rate=48000 selection=0-0 busy=1",
        "OK frames=10 channels=2 rate=48000 selection=0-0 busy=0",
        "OK -",
    ]);
    let mut remote = Remote::connect(port, TOKEN).expect("the token is accepted");
    assert_eq!(remote.execute(&Request::Open(PathBuf::from("a b.wav"))), Ok(String::new()));
    drop(remote);
    assert_eq!(
        seen.recv().expect("the stand-in ends"),
        vec![format!("TOKEN {TOKEN}"), "OPEN a b.wav".to_string(), "STATE".to_string(), "STATE".to_string(), "LASTERROR".to_string(),]
    );
}

#[test]
fn a_refused_token_is_an_error() {
    let (port, _seen) = stand_in(vec![]);
    let err = Remote::connect(port, "not-the-token").err().expect("refused");
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
}

#[test]
fn save_waits_for_busy_to_clear() {
    let (port, seen) = stand_in(vec![
        "OK",
        "OK frames=10 channels=2 rate=48000 selection=0-0 busy=1",
        "OK frames=10 channels=2 rate=48000 selection=0-0 busy=1",
        "OK frames=10 channels=2 rate=48000 selection=0-0 busy=0",
        "OK -",
    ]);
    let mut remote = Remote::connect(port, TOKEN).expect("the token is accepted");
    let save = Request::parse("SAVE out.wav").expect("a save request");
    assert_eq!(remote.execute(&save), Ok(String::new()));
    drop(remote);
    assert_eq!(
        seen.recv().expect("the stand-in ends"),
        vec![
            format!("TOKEN {TOKEN}"),
            "SAVE out.wav".to_string(),
            "STATE".to_string(),
            "STATE".to_string(),
            "STATE".to_string(),
            "LASTERROR".to_string(),
        ]
    );
}

#[test]
fn a_failed_save_is_an_error() {
    let (port, _seen) = stand_in(vec![
        "OK",
        "OK frames=10 channels=2 rate=48000 selection=0-0 busy=1",
        "OK frames=10 channels=2 rate=48000 selection=0-0 busy=1",
        "OK frames=10 channels=2 rate=48000 selection=0-0 busy=0",
        "OK Not saved: disk full. out.wav is unchanged.",
    ]);
    let mut remote = Remote::connect(port, TOKEN).expect("the token is accepted");
    let save = Request::parse("SAVE out.wav").expect("a save request");
    let err = remote.execute(&save).expect_err("the save failed in the app");
    assert!(err.contains("Not saved"), "{err}");
}

#[test]
fn a_failed_open_is_an_error() {
    let (port, _seen) = stand_in(vec!["OK", "OK frames=0 channels=0 rate=0 selection=0-0 busy=0", "OK could not open missing.wav"]);
    let mut remote = Remote::connect(port, TOKEN).expect("the token is accepted");
    let err = remote.execute(&Request::Open(PathBuf::from("missing.wav"))).expect_err("the open failed in the app");
    assert!(err.contains("could not open"), "{err}");
}
