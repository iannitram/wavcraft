//! Chunks and Broadcast Wave metadata on save (roadmap A272, A265, A256, A036).

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::actions::{ActionId, Editor};
use wavcraft_core::doc::history::History;
use wavcraft_core::io::{OpenOptions, open};
use wavcraft_core::save::{SampleType, save_wav};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-chunks-test-{}-{name}", std::process::id()));
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

fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut v = id.to_vec();
    v.extend_from_slice(&(body.len() as u32).to_le_bytes());
    v.extend_from_slice(body);
    if body.len() & 1 == 1 {
        v.push(0);
    }
    v
}

fn riff(chunks: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = chunks.concat();
    let mut v = b"RIFF".to_vec();
    v.extend_from_slice(&(4 + body.len() as u32).to_le_bytes());
    v.extend_from_slice(b"WAVE");
    v.extend_from_slice(&body);
    v
}

fn fmt16() -> Vec<u8> {
    let mut b = Vec::new();
    for x in [1u16, 2] {
        b.extend_from_slice(&x.to_le_bytes());
    }
    b.extend_from_slice(&48_000u32.to_le_bytes());
    b.extend_from_slice(&(48_000u32 * 4).to_le_bytes());
    b.extend_from_slice(&4u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    chunk(b"fmt ", &b)
}

fn pcm(frames: usize) -> Vec<u8> {
    let mut b = Vec::new();
    for i in 0..frames {
        b.extend_from_slice(&(((i * 37) % 20_000) as i16 - 10_000).to_le_bytes());
        b.extend_from_slice(&(((i * 91) % 20_000) as i16 - 10_000).to_le_bytes());
    }
    chunk(b"data", &b)
}

fn bext_body(history: &str) -> Vec<u8> {
    let mut b = vec![0u8; 602];
    b[..9].copy_from_slice(b"scene one");
    b[256..260].copy_from_slice(b"mic1");
    b[288..292].copy_from_slice(b"ref7");
    b[320..330].copy_from_slice(b"2026-10-20");
    b[330..338].copy_from_slice(b"12:30:00");
    b[338..346].copy_from_slice(&5_000_000u64.to_le_bytes());
    b[346..348].copy_from_slice(&1u16.to_le_bytes());
    b[348] = 0xAB; // UMID byte the app must keep
    b.extend_from_slice(history.as_bytes());
    b
}

const IXML: &[u8] = b"<BWFXML><PROJECT>demo</PROJECT></BWFXML>"; // even length
const ODD: &[u8] = b"odd-sized unknown body"; // 22
const AFTER: &[u8] = b"tail chunk after the audio!"; // odd length: padded

fn rich_file(frames: usize) -> Vec<u8> {
    let mut info = b"INFO".to_vec();
    info.extend_from_slice(&chunk(b"INAM", b"Old title\0"));
    info.extend_from_slice(&chunk(b"ISFT", b"RecorderX\0"));
    riff(&[
        fmt16(),
        chunk(b"bext", &bext_body("A=PCM,F=48000,W=16,M=stereo,T=Recorder\r\n")),
        chunk(b"iXML", IXML),
        chunk(b"junk", ODD),
        chunk(b"LIST", &info),
        pcm(frames),
        chunk(b"tail", AFTER),
    ])
}

/// A small RIFF reader: (id, offset of the body, size) for each chunk.
fn read_chunks(bytes: &[u8]) -> Vec<([u8; 4], usize, usize)> {
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize, bytes.len() - 8, "RIFF size");
    let (mut pos, mut out) = (12, Vec::new());
    while pos + 8 <= bytes.len() {
        let id: [u8; 4] = bytes[pos..pos + 4].try_into().unwrap();
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        out.push((id, pos + 8, size));
        pos += 8 + size + (size & 1);
    }
    assert_eq!(pos, bytes.len(), "chunks end exactly at the end of the file");
    out
}

fn body<'a>(bytes: &'a [u8], id: &[u8; 4]) -> &'a [u8] {
    let (_, at, size) = read_chunks(bytes).into_iter().find(|c| &c.0 == id).unwrap_or_else(|| panic!("no {} chunk", String::from_utf8_lossy(id)));
    &bytes[at..at + size]
}

fn ids(bytes: &[u8]) -> Vec<String> {
    read_chunks(bytes).iter().map(|c| String::from_utf8_lossy(&c.0).into_owned()).collect()
}

fn open_doc(path: &Path, sessions: &Path) -> wavcraft_core::DocState {
    let opts = OpenOptions { sessions_root: Some(sessions.to_path_buf()), ..OpenOptions::default() };
    open(path, &opts, &mut |_| {}, &AtomicBool::new(false)).unwrap()
}

#[test]
fn unknown_chunks_and_bext_survive_a_save_in_order() {
    let t = TempDir::new("keep");
    let src = t.0.join("in.wav");
    std::fs::write(&src, rich_file(70_000)).unwrap();
    let doc = open_doc(&src, &t.0.join("s"));
    assert_eq!(doc.source().meta.bext.as_ref().unwrap().description, "scene one");
    assert_eq!(doc.source().meta.info.0.len(), 2);
    let out = t.0.join("out.wav");
    save_wav(&doc, &out, SampleType::Int16, &mut |_| {}).unwrap();
    let (a, b) = (std::fs::read(&src).unwrap(), std::fs::read(&out).unwrap());
    assert_eq!(ids(&b), ["fmt ", "bext", "iXML", "junk", "LIST", "data", "tail"], "original order");
    assert_eq!(body(&b, b"iXML"), IXML);
    assert_eq!(body(&b, b"junk"), ODD);
    assert_eq!(body(&b, b"tail"), AFTER);
    assert_eq!(body(&b, b"LIST"), body(&a, b"LIST"), "INFO untouched until edited, ISFT included");
    assert_eq!(body(&b, b"data"), body(&a, b"data"), "audio bit for bit");
    // bext: every byte the same except the coding history gained a line.
    let (ba, bb) = (body(&a, b"bext"), body(&b, b"bext"));
    assert_eq!(&ba[..602], &bb[..602], "fields, time reference and the UMID byte unchanged");
    let hist = String::from_utf8_lossy(&bb[602..]).into_owned();
    assert!(hist.starts_with("A=PCM,F=48000,W=16,M=stereo,T=Recorder\r\n"), "{hist:?}");
    assert!(hist.ends_with("A=PCM,F=48000,W=16,M=stereo,T=WavCraft\r\n"), "{hist:?}");
}

#[test]
fn a_file_without_them_saves_as_before() {
    let t = TempDir::new("plain");
    let src = t.0.join("plain.wav");
    std::fs::write(&src, riff(&[fmt16(), pcm(1000)])).unwrap();
    let doc = open_doc(&src, &t.0.join("s"));
    assert!(doc.source().meta.bext.is_none());
    let out = t.0.join("out.wav");
    save_wav(&doc, &out, SampleType::Int16, &mut |_| {}).unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), std::fs::read(&src).unwrap(), "byte for byte the same file");
}

#[test]
fn a_second_save_still_keeps_the_chunks_and_adds_one_history_line_only() {
    let t = TempDir::new("twice");
    let f = t.0.join("a.wav");
    std::fs::write(&f, rich_file(5_000)).unwrap();
    let doc = open_doc(&f, &t.0.join("s"));
    save_wav(&doc, &f, SampleType::Int16, &mut |_| {}).unwrap();
    let first = std::fs::read(&f).unwrap();
    save_wav(&doc, &f, SampleType::Int16, &mut |_| {}).unwrap();
    let second = std::fs::read(&f).unwrap();
    assert_eq!(first, second, "saving the same state twice gives the same file");
    assert_eq!(body(&second, b"iXML"), IXML);
    assert_eq!(body(&second, b"tail"), AFTER);
}

#[test]
fn editing_metadata_is_one_undoable_step_and_reaches_the_file() {
    let t = TempDir::new("edit");
    let f = t.0.join("a.wav");
    std::fs::write(&f, rich_file(5_000)).unwrap();
    let doc = open_doc(&f, &t.0.join("s"));
    let mut hist = History::new(doc, u64::MAX);
    let mut meta = hist.current().source().meta.clone();
    meta.set("description", "new description").unwrap();
    meta.set("title", "New title").unwrap();
    meta.set("artist", "Someone").unwrap();
    let next = hist.current().with_metadata(meta);
    hist.push_metadata("Edit Metadata", next);
    assert_eq!(hist.steps().len(), 2);
    assert_eq!(hist.undo_label(), Some("Edit Metadata"));
    let out = t.0.join("out.wav");
    save_wav(hist.current(), &out, SampleType::Int16, &mut |_| {}).unwrap();
    let b = std::fs::read(&out).unwrap();
    assert_eq!(&body(&b, b"bext")[..13], b"new descripti");
    let list = body(&b, b"LIST");
    let text = String::from_utf8_lossy(list).into_owned();
    assert!(text.contains("New title") && text.contains("Someone") && text.contains("ISFT"), "{text:?}");
    assert_eq!(body(&b, b"iXML"), IXML);
    // Undo: the metadata is the original again.
    hist.undo();
    assert_eq!(hist.current().source().meta.get("description").unwrap(), "scene one");
    assert_eq!(hist.current().source().meta.get("title").unwrap(), "Old title");
}

#[test]
fn deleting_from_the_start_moves_the_time_reference_and_other_edits_do_not() {
    let t = TempDir::new("timeref");
    let f = t.0.join("a.wav");
    std::fs::write(&f, rich_file(100_000)).unwrap();
    let doc = open_doc(&f, &t.0.join("s"));
    let mut ed = Editor::new(History::new(doc, u64::MAX));
    // An edit in the middle leaves it alone.
    ed.run(ActionId::Delete, 50_000..60_000).unwrap();
    let mid = t.0.join("mid.wav");
    save_wav(ed.history.current(), &mid, SampleType::Int16, &mut |_| {}).unwrap();
    let tr = |p: &Path| u64::from_le_bytes(body(&std::fs::read(p).unwrap(), b"bext")[338..346].try_into().unwrap());
    assert_eq!(tr(&mid), 5_000_000);
    // Deleting the first 1000 frames moves the start by 1000 samples.
    ed.run(ActionId::Delete, 0..1000).unwrap();
    let cut = t.0.join("cut.wav");
    save_wav(ed.history.current(), &cut, SampleType::Int16, &mut |_| {}).unwrap();
    assert_eq!(tr(&cut), 5_001_000);
}

#[test]
fn setting_a_bext_field_on_a_plain_file_gives_it_a_bext() {
    let t = TempDir::new("create");
    let f = t.0.join("plain.wav");
    std::fs::write(&f, riff(&[fmt16(), pcm(2000)])).unwrap();
    let doc = open_doc(&f, &t.0.join("s"));
    let mut meta = doc.source().meta.clone();
    meta.set("originator", "WavCraft test").unwrap();
    let doc = doc.with_metadata(meta);
    let out = t.0.join("o.wav");
    save_wav(&doc, &out, SampleType::Int16, &mut |_| {}).unwrap();
    let b = std::fs::read(&out).unwrap();
    assert_eq!(ids(&b), ["fmt ", "bext", "data"]);
    assert_eq!(&body(&b, b"bext")[256..269], b"WavCraft test");
}

#[test]
fn a_source_that_changed_loses_chunks_with_a_warning_but_the_save_succeeds() {
    let t = TempDir::new("changed");
    let f = t.0.join("a.wav");
    std::fs::write(&f, rich_file(2_000)).unwrap();
    let doc = open_doc(&f, &t.0.join("s"));
    std::fs::write(&f, riff(&[fmt16(), pcm(10)])).unwrap(); // another program rewrote it
    let out = t.0.join("o.wav");
    save_wav(&doc, &out, SampleType::Int16, &mut |_| {}).unwrap();
    let b = std::fs::read(&out).unwrap();
    assert!(ids(&b).contains(&"data".to_string()));
    assert!(read_chunks(&b).len() >= 2);
}

const IXML_TRACKS: &[u8] = b"<BWFXML>\n<PROJECT>demo &#39;x&#39;</PROJECT>\n<TRACK_LIST><TRACK_COUNT>2</TRACK_COUNT>\n<TRACK><CHANNEL_INDEX>1</CHANNEL_INDEX><INTERLEAVE_INDEX>1</INTERLEAVE_INDEX><NAME>Boom</NAME></TRACK>\n<TRACK><CHANNEL_INDEX>2</CHANNEL_INDEX><INTERLEAVE_INDEX>2</INTERLEAVE_INDEX><NAME>Lav &amp; Co</NAME></TRACK>\n</TRACK_LIST>\n<UNKNOWN_TAG>kept  as is</UNKNOWN_TAG>\n</BWFXML>\n\0\0";

#[test]
fn channel_names_come_from_ixml_are_edited_and_rewritten_keeping_the_rest() {
    let t = TempDir::new("chnames");
    let f = t.0.join("a.wav");
    std::fs::write(&f, riff(&[fmt16(), chunk(b"iXML", IXML_TRACKS), chunk(b"junk", ODD), pcm(3_000)])).unwrap();
    let doc = open_doc(&f, &t.0.join("s"));
    assert_eq!(doc.source().meta.chnames, ["Boom", "Lav & Co"]);
    // Unedited: the chunk is copied through exactly.
    let same = t.0.join("same.wav");
    save_wav(&doc, &same, SampleType::Int16, &mut |_| {}).unwrap();
    assert_eq!(body(&std::fs::read(&same).unwrap(), b"iXML"), IXML_TRACKS);
    // One undoable edit.
    let mut hist = History::new(doc, u64::MAX);
    let mut meta = hist.current().source().meta.clone();
    meta.set("chname.2", "Plant <mic>").unwrap();
    assert!(meta.set("chname.3", "x").is_err());
    let next = hist.current().with_metadata(meta);
    hist.push_metadata("Edit Metadata", next);
    let out = t.0.join("out.wav");
    save_wav(hist.current(), &out, SampleType::Int16, &mut |_| {}).unwrap();
    let b = std::fs::read(&out).unwrap();
    assert_eq!(ids(&b), ["fmt ", "iXML", "junk", "data"]);
    let want = String::from_utf8_lossy(IXML_TRACKS).replace("Lav &amp; Co", "Plant &lt;mic&gt;");
    assert_eq!(String::from_utf8(body(&b, b"iXML").to_vec()).unwrap(), want, "only the NAME text changed");
    assert_eq!(body(&b, b"junk"), ODD);
    let reopened = open_doc(&out, &t.0.join("s2"));
    assert_eq!(reopened.source().meta.chnames, ["Boom", "Plant <mic>"]);
    hist.undo();
    assert_eq!(hist.current().source().meta.chnames, ["Boom", "Lav & Co"]);
}

#[test]
fn channel_names_fall_back_to_the_channel_mask_and_a_plain_file_gets_an_ixml() {
    let t = TempDir::new("chmask");
    let f = t.0.join("ext.wav");
    // WAVE_FORMAT_EXTENSIBLE stereo, mask front left + front right.
    let mut b = Vec::new();
    b.extend_from_slice(&0xFFFEu16.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&48_000u32.to_le_bytes());
    b.extend_from_slice(&(48_000u32 * 4).to_le_bytes());
    b.extend_from_slice(&4u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(&22u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(&3u32.to_le_bytes());
    b.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xaa, 0, 0x38, 0x9b, 0x71]);
    std::fs::write(&f, riff(&[chunk(b"fmt ", &b), pcm(500)])).unwrap();
    let doc = open_doc(&f, &t.0.join("s"));
    assert_eq!(doc.source().meta.chnames, ["L", "R"]);
    let mut meta = doc.source().meta.clone();
    meta.set("chname.1", "Dialog").unwrap();
    let doc = doc.with_metadata(meta);
    let out = t.0.join("o.wav");
    save_wav(&doc, &out, SampleType::Int16, &mut |_| {}).unwrap();
    let bytes = std::fs::read(&out).unwrap();
    assert!(ids(&bytes).contains(&"iXML".to_string()));
    let re = open_doc(&out, &t.0.join("s2"));
    assert_eq!(re.source().meta.chnames, ["Dialog", "R"]);
}

#[test]
fn info_tags_edit_as_one_step_and_round_trip_with_unknown_tags() {
    let t = TempDir::new("info");
    let f = t.0.join("a.wav");
    std::fs::write(&f, rich_file(2_000)).unwrap();
    let doc = open_doc(&f, &t.0.join("s"));
    let mut hist = History::new(doc, u64::MAX);
    let mut meta = hist.current().source().meta.clone();
    for (k, v) in [
        ("INFO.INAM", "A title"),
        ("INFO.IART", "An artist"),
        ("INFO.ICMT", "a note"),
        ("INFO.ICRD", "2026-10-20"),
        ("INFO.IGNR", "Ambient"),
        ("INFO.ICOP", "(c) test"),
        ("INFO.ISFT", "WavCraft"),
        ("INFO.IKEY", "kept"),
    ] {
        meta.set(k, v).unwrap();
    }
    let next = hist.current().with_metadata(meta);
    hist.push_metadata("Edit Metadata", next);
    assert_eq!(hist.steps().len(), 2, "one undo step");
    let out = t.0.join("o.wav");
    save_wav(hist.current(), &out, SampleType::Int16, &mut |_| {}).unwrap();
    let re = open_doc(&out, &t.0.join("s2"));
    let m = &re.source().meta;
    for (k, v) in [
        ("title", "A title"),
        ("artist", "An artist"),
        ("comment", "a note"),
        ("date", "2026-10-20"),
        ("genre", "Ambient"),
        ("copyright", "(c) test"),
        ("software", "WavCraft"),
        ("INFO.IKEY", "kept"),
    ] {
        assert_eq!(m.get(k).as_deref(), Some(v), "{k}");
    }
    assert_eq!(body(&std::fs::read(&out).unwrap(), b"iXML"), IXML, "an INFO edit leaves the iXML alone");
    hist.undo();
    assert_eq!(hist.current().source().meta.get("title").unwrap(), "Old title");
}
