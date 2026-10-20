//! Recording into an open document (roadmap C026, C027, C030): the edit a finished take makes.
//! Synthetic data only; the take lives in its own store, as it does when opened from its file.

use std::path::{Path, PathBuf};

use wavcraft_core::EditError;
use wavcraft_core::doc::edit::{Change, ChannelMask, Crossfades};
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::rec_edit::{RecMode, RecPlan, RecTarget, apply_take, apply_take_with_offset, offset_frames, resolve_offset_ms};
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;
const NONE: Crossfades = Crossfades::NONE;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-recedit-test-{}-{name}", std::process::id()));
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

fn doc_in(dir: &Path, rate: u32, data: &[Vec<f32>]) -> DocState {
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES / 4).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    DocState::new(s.clone(), chans, rate, SourceFormat::synthetic(rate, data.len() as u16)).unwrap()
}

fn content(doc: &DocState) -> Vec<Vec<f32>> {
    (0..doc.channel_count())
        .map(|c| {
            let mut v = vec![f32::NAN; doc.len_frames() as usize];
            assert_eq!(doc.read(c, 0, &mut v).unwrap(), v.len());
            v
        })
        .collect()
}

/// Channel `c` of frame `i`: distinct values, exact in f32.
fn val(c: usize, i: usize) -> f32 {
    ((c + 1) * 1000 + i % 997) as f32 / 8192.0
}

fn music(channels: usize, frames: usize) -> Vec<Vec<f32>> {
    (0..channels).map(|c| (0..frames).map(|i| val(c, i)).collect()).collect()
}

/// A take: `frames` of a value no document frame has (negative).
fn take_data(channels: usize, frames: usize) -> Vec<Vec<f32>> {
    (0..channels).map(|c| (0..frames).map(|i| -val(c, i) - 0.01).collect()).collect()
}

fn plan(mode: RecMode, target: RecTarget, cursor: u64, selection: std::ops::Range<u64>) -> RecPlan {
    RecPlan { mode, target, cursor, selection }
}

struct Setup {
    _a: TempDir,
    _b: TempDir,
    doc: DocState,
    take: DocState,
    doc_data: Vec<Vec<f32>>,
    take_data: Vec<Vec<f32>>,
}

fn setup(name: &str, doc_ch: usize, doc_frames: usize, take_ch: usize, take_frames: usize) -> Setup {
    let (a, b) = (TempDir::new(&format!("{name}-doc")), TempDir::new(&format!("{name}-take")));
    let (dd, td) = (music(doc_ch, doc_frames), take_data(take_ch, take_frames));
    let doc = doc_in(&a.0, RATE, &dd);
    let take = doc_in(&b.0, RATE, &td);
    Setup { _a: a, _b: b, doc, take, doc_data: dd, take_data: td }
}

#[test]
fn insert_puts_the_take_at_the_cursor_in_both_channels() {
    let s = setup("insert", 2, 5000, 2, 700);
    let p = plan(RecMode::Insert, RecTarget::Both, 1200, 0..0);
    p.check(&s.doc).unwrap();
    let edit = apply_take(&s.doc, &s.take, &p, NONE).unwrap();
    assert_eq!(edit.state.len_frames(), 5700);
    assert_eq!(edit.change, Change::LengthFrom { frame: 1200 });
    let out = content(&edit.state);
    for (c, o) in out.iter().enumerate() {
        assert_eq!(o[..1200], s.doc_data[c][..1200], "before the cursor is untouched");
        assert_eq!(o[1200..1900], s.take_data[c][..], "the take");
        assert_eq!(o[1900..], s.doc_data[c][1200..], "after the cursor moves along unchanged");
    }
}

#[test]
fn insert_at_the_end_appends() {
    let s = setup("append", 1, 3000, 1, 500);
    let edit = apply_take(&s.doc, &s.take, &plan(RecMode::Insert, RecTarget::Both, 3000, 0..0), NONE).unwrap();
    let out = content(&edit.state);
    assert_eq!(out[0][..3000], s.doc_data[0][..]);
    assert_eq!(out[0][3000..], s.take_data[0][..]);
}

#[test]
fn replace_over_a_selection_swaps_it_for_the_take() {
    let s = setup("replace", 2, 5000, 2, 900);
    let p = plan(RecMode::Replace, RecTarget::Both, 1000, 1000..2000);
    let edit = apply_take(&s.doc, &s.take, &p, NONE).unwrap();
    assert_eq!(edit.state.len_frames(), 5000 - 1000 + 900);
    let out = content(&edit.state);
    for (c, o) in out.iter().enumerate() {
        assert_eq!(o[..1000], s.doc_data[c][..1000]);
        assert_eq!(o[1000..1900], s.take_data[c][..]);
        assert_eq!(o[1900..], s.doc_data[c][2000..]);
    }
}

#[test]
fn replace_at_a_cursor_overwrites_for_the_takes_length_and_keeps_the_length() {
    let s = setup("overwrite", 2, 5000, 2, 900);
    let edit = apply_take(&s.doc, &s.take, &plan(RecMode::Replace, RecTarget::Both, 4500, 4500..4500), NONE).unwrap();
    assert_eq!(edit.state.len_frames(), 5000, "the file never grows when recording over");
    let out = content(&edit.state);
    for (c, o) in out.iter().enumerate() {
        assert_eq!(o[..4500], s.doc_data[c][..4500]);
        assert_eq!(o[4500..], s.take_data[c][..500], "the take is cut where the file ends");
    }
}

#[test]
fn one_channel_leaves_the_other_sample_identical() {
    for (target, rec, other) in [(RecTarget::Left, 0usize, 1usize), (RecTarget::Right, 1, 0)] {
        let s = setup("onech", 2, 5000, 1, 600);
        let p = plan(RecMode::Replace, target, 2000, 2000..2000);
        p.check(&s.doc).unwrap();
        assert_eq!(p.take_channels(&s.doc), 1);
        let edit = apply_take(&s.doc, &s.take, &p, NONE).unwrap();
        assert_eq!(edit.state.len_frames(), 5000);
        let out = content(&edit.state);
        assert_eq!(out[other], s.doc_data[other], "the other channel is untouched");
        assert_eq!(out[rec][..2000], s.doc_data[rec][..2000]);
        assert_eq!(out[rec][2000..2600], s.take_data[0][..]);
        assert_eq!(out[rec][2600..], s.doc_data[rec][2600..]);
        assert!(std::sync::Arc::ptr_eq(edit.state.channel(other).unwrap(), s.doc.channel(other).unwrap()), "shared, not even copied");
    }
}

#[test]
fn punch_in_replaces_only_what_was_recorded_inside_the_selection() {
    // A take longer than the selection is cut at the selection's end.
    let s = setup("punch-long", 2, 6000, 2, 1500);
    let p = plan(RecMode::Punch, RecTarget::Both, 0, 2000..3000);
    assert_eq!(p.limit_frames(), Some(1000));
    let edit = apply_take(&s.doc, &s.take, &p, NONE).unwrap();
    assert_eq!(edit.state.len_frames(), 6000);
    let out = content(&edit.state);
    for (c, o) in out.iter().enumerate() {
        assert_eq!(o[..2000], s.doc_data[c][..2000]);
        assert_eq!(o[2000..3000], s.take_data[c][..1000]);
        assert_eq!(o[3000..], s.doc_data[c][3000..]);
    }
    // A take stopped early replaces only its own length.
    let s = setup("punch-short", 2, 6000, 2, 400);
    let edit = apply_take(&s.doc, &s.take, &plan(RecMode::Punch, RecTarget::Both, 0, 2000..3000), NONE).unwrap();
    let out = content(&edit.state);
    assert_eq!(out[0][2000..2400], s.take_data[0][..]);
    assert_eq!(out[0][2400..], s.doc_data[0][2400..]);
}

#[test]
fn punch_in_on_one_channel() {
    let s = setup("punch-one", 2, 6000, 1, 1000);
    let edit = apply_take(&s.doc, &s.take, &plan(RecMode::Punch, RecTarget::Right, 0, 2000..3000), NONE).unwrap();
    let out = content(&edit.state);
    assert_eq!(out[0], s.doc_data[0]);
    assert_eq!(out[1][2000..3000], s.take_data[0][..]);
    assert_eq!(out[1][..2000], s.doc_data[1][..2000]);
    assert_eq!(out[1][3000..], s.doc_data[1][3000..]);
}

#[test]
fn a_recording_is_one_history_step_and_undo_restores_every_sample() {
    let s = setup("history", 2, 5000, 2, 800);
    let before = content(&s.doc);
    let mut h = History::new(s.doc.clone(), u64::MAX);
    for p in [
        plan(RecMode::Insert, RecTarget::Both, 700, 0..0),
        plan(RecMode::Replace, RecTarget::Left, 100, 100..300),
        plan(RecMode::Punch, RecTarget::Both, 0, 1000..1500),
    ] {
        let steps = h.steps().len();
        let edit = apply_take(h.current(), &s.take, &p, Crossfades::default()).unwrap();
        h.push_edit(edit);
        assert_eq!(h.steps().len(), steps + 1, "{:?}: one step", p.mode);
    }
    assert!(h.steps().iter().skip(1).map(|st| st.label()).eq(["Record", "Record Over", "Punch In"]));
    while h.can_undo() {
        h.undo();
    }
    assert_eq!(content(h.current()), before);
}

#[test]
fn the_default_crossfades_leave_audio_away_from_the_edges_identical() {
    let s = setup("xf", 2, 20_000, 2, 3000);
    let p = plan(RecMode::Insert, RecTarget::Both, 8000, 0..0);
    let edit = apply_take(&s.doc, &s.take, &p, Crossfades::default()).unwrap();
    let out = content(&edit.state);
    let margin = 480; // 10 ms, twice the 5 ms edge fade
    for (c, o) in out.iter().enumerate() {
        assert_eq!(o[..8000 - margin], s.doc_data[c][..8000 - margin]);
        assert_eq!(o[8000 + 3000 + margin..], s.doc_data[c][8000 + margin..]);
        assert_eq!(o[8000 + margin..8000 + 3000 - margin], s.take_data[c][margin..3000 - margin]);
    }
}

#[test]
fn a_take_at_another_rate_is_refused_and_the_document_is_unchanged() {
    let (a, b) = (TempDir::new("rate-doc"), TempDir::new("rate-take"));
    let doc = doc_in(&a.0, RATE, &music(1, 2000));
    let take = doc_in(&b.0, 44_100, &take_data(1, 300));
    let err = apply_take(&doc, &take, &plan(RecMode::Insert, RecTarget::Both, 0, 0..0), NONE).unwrap_err();
    assert!(matches!(err, EditError::SampleRate { .. }), "{err}");
}

#[test]
fn plans_that_cannot_work_are_refused_before_recording() {
    let a = TempDir::new("check");
    let stereo = doc_in(&a.0, RATE, &music(2, 1000));
    let msg = |p: RecPlan, d: &DocState| p.check(d).unwrap_err();
    assert!(msg(plan(RecMode::Insert, RecTarget::Left, 0, 0..0), &stereo).contains("both channels"), "insert can't change one channel's length");
    assert!(msg(plan(RecMode::Punch, RecTarget::Both, 0, 5..5), &stereo).contains("selection"));
    assert!(plan(RecMode::Replace, RecTarget::Right, 10, 10..10).check(&stereo).is_ok());
    let b = TempDir::new("check-mono");
    let mono = doc_in(&b.0, RATE, &music(1, 1000));
    assert!(msg(plan(RecMode::Replace, RecTarget::Right, 0, 0..0), &mono).contains("no right channel"));
    assert!(plan(RecMode::Insert, RecTarget::Left, 0, 0..0).check(&mono).is_ok(), "the only channel is the whole file");
    assert_eq!(plan(RecMode::Insert, RecTarget::Both, 0, 0..0).take_channels(&mono), 1);
    assert_eq!(plan(RecMode::Insert, RecTarget::Both, 0, 0..0).take_channels(&stereo), 2);
    let c = TempDir::new("check-six");
    let six = doc_in(&c.0, RATE, &music(6, 1000));
    assert!(msg(plan(RecMode::Replace, RecTarget::Both, 0, 0..0), &six).contains("mono or stereo"));
    assert!(plan(RecMode::Replace, RecTarget::Left, 0, 0..0).check(&six).is_ok());
}

#[test]
fn an_empty_take_changes_nothing() {
    let (a, b) = (TempDir::new("empty-doc"), TempDir::new("empty-take"));
    let doc = doc_in(&a.0, RATE, &music(2, 1000));
    let take = doc_in(&b.0, RATE, &take_data(2, 0));
    let edit = apply_take(&doc, &take, &plan(RecMode::Insert, RecTarget::Both, 10, 0..0), NONE).unwrap();
    assert!(edit.is_noop());
    let _ = ChannelMask::ALL;
}

// C010, C050: latency compensation.

#[test]
fn an_offset_in_milliseconds_is_a_frame_count_at_each_rate() {
    assert_eq!(offset_frames(0, 48_000), 0);
    assert_eq!(offset_frames(10, 48_000), 480);
    assert_eq!(offset_frames(10, 44_100), 441);
    assert_eq!(offset_frames(10, 96_000), 960);
    assert_eq!(offset_frames(25, 22_050), 551, "rounded to the nearest frame (551.25)");
    assert_eq!(offset_frames(1, 8_000), 8);
    assert_eq!(offset_frames(2000, 192_000), 384_000);
}

#[test]
fn the_offset_is_the_preference_else_what_the_device_reported_else_nothing() {
    assert_eq!(resolve_offset_ms(Some(35), Some(12_000)), 35, "the preference wins");
    assert_eq!(resolve_offset_ms(Some(0), Some(12_000)), 0, "an explicit zero is a choice");
    assert_eq!(resolve_offset_ms(None, Some(12_400)), 12);
    assert_eq!(resolve_offset_ms(None, Some(12_500)), 13);
    assert_eq!(resolve_offset_ms(None, None), 0);
    assert_eq!(resolve_offset_ms(Some(99_999), None), 2000, "capped");
    assert_eq!(resolve_offset_ms(None, Some(u64::MAX / 2)), 2000, "capped");
}

#[test]
fn insert_with_an_offset_leaves_out_the_take_s_first_frames() {
    for (rate_ms, rate) in [(10u32, 48_000u32), (10, 44_100), (25, 96_000), (7, 22_050)] {
        let off = offset_frames(rate_ms, rate);
        let (a, b) = (TempDir::new(&format!("off-ins-{rate}-doc")), TempDir::new(&format!("off-ins-{rate}-take")));
        let (dd, td) = (music(2, 20_000), take_data(2, 5000));
        let (doc, take) = (doc_in(&a.0, rate, &dd), doc_in(&b.0, rate, &td));
        let p = plan(RecMode::Insert, RecTarget::Both, 3000, 0..0);
        let edit = apply_take_with_offset(&doc, &take, &p, NONE, off).unwrap();
        let n = 5000 - off as usize;
        assert_eq!(edit.state.len_frames(), (20_000 + n) as u64, "rate {rate}: the file grows by the take less the offset");
        assert_eq!(edit.change, Change::LengthFrom { frame: 3000 });
        let out = content(&edit.state);
        for (c, o) in out.iter().enumerate() {
            assert_eq!(o[..3000], dd[c][..3000]);
            assert_eq!(o[3000..3000 + n], td[c][off as usize..], "rate {rate}: the take starts {off} frames in");
            assert_eq!(o[3000 + n..], dd[c][3000..]);
        }
        // The take is not touched.
        assert_eq!(content(&take), td);
    }
}

#[test]
fn a_zero_offset_is_exactly_apply_take() {
    let s = setup("off-zero", 2, 5000, 2, 700);
    let p = plan(RecMode::Insert, RecTarget::Both, 1200, 0..0);
    let (x, y) = (apply_take(&s.doc, &s.take, &p, NONE).unwrap(), apply_take_with_offset(&s.doc, &s.take, &p, NONE, 0).unwrap());
    assert_eq!(content(&x.state), content(&y.state));
}

#[test]
fn punch_in_with_an_offset_still_fills_the_selection_to_its_end() {
    let s = setup("off-punch", 2, 6000, 2, 1480);
    let p = plan(RecMode::Punch, RecTarget::Both, 0, 2000..3000);
    assert_eq!(p.limit_frames_with_offset(480), Some(1480), "the take runs the offset longer");
    let edit = apply_take_with_offset(&s.doc, &s.take, &p, NONE, 480).unwrap();
    assert_eq!(edit.state.len_frames(), 6000);
    let out = content(&edit.state);
    for (c, o) in out.iter().enumerate() {
        assert_eq!(o[..2000], s.doc_data[c][..2000]);
        assert_eq!(o[2000..3000], s.take_data[c][480..1480], "the punch holds what was played, lined up");
        assert_eq!(o[3000..], s.doc_data[c][3000..]);
    }
}

#[test]
fn replace_over_a_cursor_with_an_offset_and_a_take_shorter_than_the_offset() {
    let s = setup("off-rep", 1, 4000, 1, 900);
    let p = plan(RecMode::Replace, RecTarget::Both, 1000, 1000..1000);
    let edit = apply_take_with_offset(&s.doc, &s.take, &p, NONE, 300).unwrap();
    let out = content(&edit.state);
    assert_eq!(edit.state.len_frames(), 4000);
    assert_eq!(out[0][1000..1600], s.take_data[0][300..]);
    assert_eq!(out[0][1600..], s.doc_data[0][1600..]);
    // A take that is all latency places nothing.
    let none = apply_take_with_offset(&s.doc, &s.take, &p, NONE, 5000).unwrap();
    assert!(none.is_noop());
}
