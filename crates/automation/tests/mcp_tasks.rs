//! Ten editing tasks through the MCP handler on the headless backend, with no window. Each task
//! is one `tools/call` line through `handle`, and each has its own check, read from the document
//! or from the file it saves.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use serde_json::{Value, json};
use wavcraft_automation::headless::Headless;
use wavcraft_automation::mcp::handle;
use wavcraft_core::DocState;
use wavcraft_core::doc::edit::Crossfades;
use wavcraft_core::io::{self, OpenOptions};

const RATE: u32 = 48_000;
const FRAMES: u32 = 20_000;
/// One 16-bit step: the save rounds each sample to the nearest 1/32768.
const LSB: f32 = 1.0 / 32768.0;

/// A 16-bit, 48000 Hz, stereo WAV, `FRAMES` long. Channel 0 at frame i is `100 * ((i % 100) + 1)`
/// (100 to 10000, never 0); channel 1 is its negative.
fn write_wav(path: &Path) {
    let data_len = FRAMES * 2 * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..FRAMES {
        let v = (100 * ((i % 100) + 1)) as i16;
        out.extend_from_slice(&v.to_le_bytes());
        out.extend_from_slice(&(-v).to_le_bytes());
    }
    std::fs::write(path, out).expect("write the test WAV");
}

/// Send one `tools/call` through `handle` on a headless backend and parse the reply.
fn call(head: &mut Headless, id: u64, name: &str, args: Value) -> Value {
    let line = json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": {"name": name, "arguments": args}})
    .to_string();
    let out = handle(&line, head).expect("a reply");
    serde_json::from_str(&out).expect("the reply is JSON")
}

/// The reply text of a call that must succeed.
fn text(res: &Value) -> String {
    assert_eq!(res["result"]["isError"], false, "a tool failed: {res}");
    res["result"]["content"][0]["text"].as_str().expect("a text reply").to_string()
}

/// Frames `start..start + n` of channel `ch` of the open document.
fn read(head: &Headless, ch: usize, start: u64, n: usize) -> Vec<f32> {
    let doc: &DocState = head.document().expect("a document is open");
    let mut out = vec![0.0f32; n];
    let got = doc.read(ch, start, &mut out).expect("read the document");
    assert_eq!(got, n, "the document ends before frame {}", start + n as u64);
    out
}

fn select(head: &mut Headless, id: u64, start: u64, end: u64) {
    let res = call(head, id, "select", json!({"start": start, "end": end}));
    assert_eq!(text(&res), format!("selection={start}-{end}"));
}

#[test]
fn ten_tasks_headless() {
    let dir: PathBuf = std::env::temp_dir().join(format!("wavcraft-mcp-tasks-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch folder");
    let wav = dir.join("tone.wav");
    write_wav(&wav);
    let mut head = Headless::with_crossfades(dir.join("sessions"), Crossfades::NONE);

    // 1. open
    let opened = call(&mut head, 1, "open", json!({"path": wav.to_string_lossy()}));
    assert_eq!(text(&opened), "ok", "task 1 open");
    let state = call(&mut head, 2, "state", json!({}));
    assert!(text(&state).contains("frames=20000 channels=2 rate=48000"), "task 1 open: {state}");

    // 2. select
    select(&mut head, 3, 1000, 2000);

    // 3. delete
    let res = call(&mut head, 4, "action", json!({"name": "edit.delete"}));
    text(&res);
    assert_eq!(head.document().expect("open").len_frames(), 19_000, "task 3 delete");

    // 4. silence
    select(&mut head, 5, 0, 500);
    let res = call(&mut head, 6, "action", json!({"name": "edit.silence"}));
    text(&res);
    for ch in 0..2 {
        for (i, s) in read(&head, ch, 0, 500).iter().enumerate() {
            assert_eq!(*s, 0.0, "task 4 silence: channel {ch} frame {i}");
        }
    }

    // 5. gain
    select(&mut head, 7, 500, 1000);
    let before = [read(&head, 0, 500, 500), read(&head, 1, 500, 500)];
    let res = call(&mut head, 8, "effect", json!({"name": "gain", "args": "gain_db=-6.0206"}));
    text(&res);
    for (ch, old_ch) in before.iter().enumerate() {
        for (i, (after, old)) in read(&head, ch, 500, 500).iter().zip(old_ch).enumerate() {
            assert!((after - old * 0.5).abs() <= LSB, "task 5 gain: channel {ch} frame {}: {after} vs {}", 500 + i, old * 0.5);
        }
    }

    // 6. normalize: an empty selection runs on the whole file
    select(&mut head, 9, 0, 0);
    let res = call(&mut head, 10, "effect", json!({"name": "normalize", "args": "unit=dB db=-2"}));
    text(&res);
    let n = head.document().expect("open").len_frames() as usize;
    let peak = (0..2).flat_map(|ch| read(&head, ch, 0, n)).fold(0.0f32, |m, s| m.max(s.abs()));
    let dbfs = 20.0 * f64::from(peak).log10();
    assert!((dbfs - (-2.0)).abs() < 0.01, "task 6 normalize: peak is {dbfs} dBFS");

    // 7. fade in
    select(&mut head, 11, 0, 1000);
    let old = [read(&head, 0, 500, 1)[0], read(&head, 1, 500, 1)[0]];
    let res = call(&mut head, 12, "action", json!({"name": "effect.fadeIn.linear"}));
    text(&res);
    for (ch, old_ch) in old.iter().enumerate() {
        let at_zero = read(&head, ch, 0, 1)[0];
        assert!(at_zero.abs() <= LSB, "task 7 fade in: channel {ch} frame 0 is {at_zero}");
        let at_half = read(&head, ch, 500, 1)[0];
        assert!((at_half - old_ch * 0.5).abs() <= LSB, "task 7 fade in: channel {ch} frame 500 is {at_half} vs {}", old_ch * 0.5);
    }

    // 8. invert
    select(&mut head, 13, 2000, 3000);
    let before = [read(&head, 0, 2000, 1000), read(&head, 1, 2000, 1000)];
    let res = call(&mut head, 14, "effect", json!({"name": "invert"}));
    text(&res);
    for (ch, old_ch) in before.iter().enumerate() {
        for (i, (after, old)) in read(&head, ch, 2000, 1000).iter().zip(old_ch).enumerate() {
            assert!((after + old).abs() <= LSB, "task 8 invert: channel {ch} frame {}: {after} vs {}", 2000 + i, -old);
        }
    }

    // 9. add marker
    select(&mut head, 15, 5000, 5000);
    let res = call(&mut head, 16, "action", json!({"name": "marker.add"}));
    text(&res);
    let doc = head.document().expect("open");
    let markers = doc.markers();
    assert_eq!(markers.len(), 1, "task 9 marker: {} markers", markers.len());
    let marker = markers.iter().next().expect("one marker");
    assert_eq!((marker.start, marker.end), (5000, 5000), "task 9 marker position");
    assert!(marker.is_point(), "task 9 marker is a point");

    // 10. save, then read the file back
    let out = dir.join("out.wav");
    let res = call(&mut head, 17, "save", json!({"path": out.to_string_lossy()}));
    assert_eq!(text(&res), "ok", "task 10 save");
    let check_root = dir.join("check");
    let opts = OpenOptions { sessions_root: Some(check_root), ..OpenOptions::default() };
    let saved = io::open(&out, &opts, &mut |_| {}, &AtomicBool::new(false)).expect("open the saved file");
    let doc = head.document().expect("open");
    assert_eq!(saved.len_frames(), doc.len_frames(), "task 10 save: frames");
    assert_eq!(saved.channel_count(), doc.channel_count(), "task 10 save: channels");
    assert_eq!(saved.sample_rate(), doc.sample_rate(), "task 10 save: rate");
    let chunk = 4096usize;
    let total = doc.len_frames();
    let mut start = 0u64;
    while start < total {
        let n = chunk.min((total - start) as usize);
        for ch in 0..2 {
            let mut a = vec![0.0f32; n];
            let mut b = vec![0.0f32; n];
            assert_eq!(doc.read(ch, start, &mut a).expect("read the document"), n);
            assert_eq!(saved.read(ch, start, &mut b).expect("read the saved file"), n);
            for (i, (x, y)) in a.iter().zip(&b).enumerate() {
                assert!((x - y).abs() <= LSB, "task 10 save: channel {ch} frame {}: {x} vs {y}", start + i as u64);
            }
        }
        start += n as u64;
    }

    let _ = std::fs::remove_dir_all(&dir);
}
