//! XMP in the `_PMX` chunk: read, edit, save, read again (roadmap A268).

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::doc::history::History;
use wavcraft_core::io::{OpenOptions, open};
use wavcraft_core::save::{SampleType, save_wav};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-xmp-test-{}-{name}", std::process::id()));
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

const NS_DC: &str = "http://purl.org/dc/elements/1.1/";
const ACME: &str = "http://example.com/acme/1.0/";
const PACKET: &str = concat!(
    "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n",
    "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n",
    " <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n",
    "  <rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:acme=\"http://example.com/acme/1.0/\" acme:take=\"7\">\n",
    "   <dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">Old title</rdf:li></rdf:Alt></dc:title>\n",
    "   <acme:mood>calm</acme:mood>\n",
    "   <acme:shape rdf:parseType=\"Resource\"><acme:w>3</acme:w></acme:shape>\n",
    "  </rdf:Description>\n",
    " </rdf:RDF>\n",
    "</x:xmpmeta>\n",
    "<?xpacket end=\"w\"?>"
); // odd length: padded in the file
const AFTER: &[u8] = b"after the packet";

fn with_packet(packet: &[u8]) -> Vec<u8> {
    riff(&[fmt16(), chunk(b"_PMX", packet), pcm(5_000), chunk(b"tail", AFTER)])
}

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

fn edited(doc: &wavcraft_core::DocState, changes: &[(&str, &str)]) -> wavcraft_core::DocState {
    let mut meta = doc.source().meta.clone();
    for (k, v) in changes {
        meta.set(k, v).unwrap();
    }
    doc.with_metadata(meta)
}

#[test]
fn an_unedited_packet_is_copied_byte_for_byte() {
    let t = TempDir::new("copy");
    let f = t.0.join("in.wav");
    std::fs::write(&f, with_packet(PACKET.as_bytes())).unwrap();
    let doc = open_doc(&f, &t.0.join("s"));
    let x = doc.source().meta.xmp.as_ref().expect("the packet is read");
    assert_eq!(x.get(NS_DC, "title").as_deref(), Some("Old title"));
    let out = t.0.join("out.wav");
    save_wav(&doc, &out, SampleType::Int16, &mut |_| {}).unwrap();
    let b = std::fs::read(&out).unwrap();
    assert_eq!(body(&b, b"_PMX"), PACKET.as_bytes());
    assert_eq!(ids(&b), ["fmt ", "_PMX", "data", "tail"]);
    assert_eq!(body(&b, b"tail"), AFTER);
}

#[test]
fn an_edit_is_one_undo_step_and_changes_only_that_property_on_save() {
    let t = TempDir::new("edit");
    let f = t.0.join("in.wav");
    std::fs::write(&f, with_packet(PACKET.as_bytes())).unwrap();
    let mut hist = History::new(open_doc(&f, &t.0.join("s")), u64::MAX);
    let next = edited(hist.current(), &[("xmp_title", "New title"), ("xmp_rights", "(c) test"), ("xmp:acme:mood", "tense")]);
    hist.push_metadata("Edit Metadata", next);
    assert_eq!(hist.steps().len(), 2);
    let out = t.0.join("out.wav");
    save_wav(hist.current(), &out, SampleType::Int16, &mut |_| {}).unwrap();
    let b = std::fs::read(&out).unwrap();
    let text = String::from_utf8(body(&b, b"_PMX").to_vec()).unwrap();
    assert!(text.contains(">New title<") && text.contains(">tense<") && text.contains("(c) test"), "{text}");
    // The unknown properties and everything else around them are as they were.
    assert!(text.contains("acme:take=\"7\"") && text.contains("<acme:shape rdf:parseType=\"Resource\"><acme:w>3</acme:w></acme:shape>"));
    assert!(text.starts_with(&PACKET[..PACKET.find("<dc:title>").unwrap()]));
    assert!(text.ends_with("</rdf:Description>\n </rdf:RDF>\n</x:xmpmeta>\n<?xpacket end=\"w\"?>"));
    assert_eq!(ids(&b), ["fmt ", "_PMX", "data", "tail"]);
    assert_eq!(body(&b, b"tail"), AFTER);
    // Read again: the file's model has the edits, and a second save keeps them.
    let again = open_doc(&out, &t.0.join("s2"));
    let x = again.source().meta.xmp.as_ref().unwrap();
    assert_eq!(x.get(NS_DC, "title").as_deref(), Some("New title"));
    assert_eq!(x.get(NS_DC, "rights").as_deref(), Some("(c) test"));
    assert_eq!(x.get(ACME, "mood").as_deref(), Some("tense"));
    assert_eq!(x.get(ACME, "take").as_deref(), Some("7"));
    // Undo restores the old packet.
    hist.undo();
    assert_eq!(hist.current().source().meta.get("xmp_title").as_deref(), Some("Old title"));
    let out2 = t.0.join("out2.wav");
    save_wav(hist.current(), &out2, SampleType::Int16, &mut |_| {}).unwrap();
    assert_eq!(body(&std::fs::read(&out2).unwrap(), b"_PMX"), PACKET.as_bytes());
}

#[test]
fn saving_twice_after_an_edit_keeps_the_packet() {
    let t = TempDir::new("twice");
    let f = t.0.join("in.wav");
    std::fs::write(&f, with_packet(PACKET.as_bytes())).unwrap();
    let doc = edited(&open_doc(&f, &t.0.join("s")), &[("xmp_title", "Second")]);
    let out = t.0.join("out.wav");
    save_wav(&doc, &out, SampleType::Int16, &mut |_| {}).unwrap();
    save_wav(&doc, &out, SampleType::Int16, &mut |_| {}).unwrap();
    let b = std::fs::read(&out).unwrap();
    assert_eq!(ids(&b), ["fmt ", "_PMX", "data", "tail"]);
    assert!(String::from_utf8_lossy(body(&b, b"_PMX")).contains(">Second<"));
}

#[test]
fn a_file_without_xmp_gets_a_packet_when_a_field_is_set() {
    let t = TempDir::new("new");
    let f = t.0.join("plain.wav");
    std::fs::write(&f, riff(&[fmt16(), pcm(2_000)])).unwrap();
    let doc = open_doc(&f, &t.0.join("s"));
    assert!(doc.source().meta.xmp.is_none() && doc.source().meta.xmp_problem.is_none());
    let base = doc;
    let doc = edited(&base, &[("xmp_creator", "Ann; Bo"), ("xmp_artist", "The Band")]);
    let out = t.0.join("o.wav");
    save_wav(&doc, &out, SampleType::Int16, &mut |_| {}).unwrap();
    let b = std::fs::read(&out).unwrap();
    assert_eq!(ids(&b), ["fmt ", "data", "_PMX"]);
    let again = open_doc(&out, &t.0.join("s2"));
    assert_eq!(again.source().meta.get("xmp_creator").as_deref(), Some("Ann; Bo"));
    assert_eq!(again.source().meta.get("xmp_artist").as_deref(), Some("The Band"));
    // Saving the earlier step (undo) afterwards leaves no packet: the saved file's one is not copied.
    let out2 = t.0.join("o2.wav");
    save_wav(&base, &out2, SampleType::Int16, &mut |_| {}).unwrap();
    assert_eq!(ids(&std::fs::read(&out2).unwrap()), ["fmt ", "data"]);
}

#[test]
fn a_malformed_packet_is_reported_kept_verbatim_and_not_edited() {
    let t = TempDir::new("bad");
    let f = t.0.join("bad.wav");
    let bad: &[u8] = b"<?xpacket begin=''?><x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF><oops></x:xmpmeta>";
    std::fs::write(&f, with_packet(bad)).unwrap();
    let doc = open_doc(&f, &t.0.join("s"));
    assert!(doc.source().meta.xmp.is_none());
    assert!(doc.source().meta.xmp_problem.is_some());
    assert!(doc.source().warnings.iter().any(|w| w.contains("XMP")), "{:?}", doc.source().warnings);
    let mut meta = doc.source().meta.clone();
    assert!(meta.set("xmp_title", "x").is_err());
    assert!(!meta.xmp_edited);
    // Another edit still saves, and the packet comes through unchanged.
    let doc = edited(&doc, &[("description", "other field")]);
    let out = t.0.join("out.wav");
    save_wav(&doc, &out, SampleType::Int16, &mut |_| {}).unwrap();
    assert_eq!(body(&std::fs::read(&out).unwrap(), b"_PMX"), bad);
}

#[test]
fn hostile_packets_open_without_crashing() {
    let t = TempDir::new("hostile");
    let bombs: [&[u8]; 3] = [
        b"<?xml version='1.0'?><!DOCTYPE lolz [<!ENTITY lol 'lol'><!ENTITY lol2 '&lol;&lol;&lol;'>]><x:xmpmeta xmlns:x='adobe:ns:meta/'>&lol2;</x:xmpmeta>",
        &[0xff, 0xfe, b'<', 0, b'a', 0],
        b"",
    ];
    for (n, p) in bombs.iter().enumerate() {
        let f = t.0.join(format!("h{n}.wav"));
        std::fs::write(&f, with_packet(p)).unwrap();
        let doc = open_doc(&f, &t.0.join(format!("s{n}")));
        assert!(doc.source().meta.xmp.is_none());
    }
}
