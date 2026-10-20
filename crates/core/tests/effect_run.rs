//! The effect seam's chunk state: `Effect::begin` gives a `Run` that sees every chunk of a range
//! in order, so state carries across `replace_range` chunks.

use std::io;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use wavcraft_core::doc::edit::{ChannelMask, Crossfades};
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::effects::{self, Effect, ParamDesc, ParamKind, Run, RunContext};
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 44_100;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-run-test-{}-{name}", std::process::id()));
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

fn doc(dir: &std::path::Path, data: &[Vec<f32>]) -> DocState {
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    DocState::new(s, chans, RATE, SourceFormat::synthetic(RATE, data.len() as u16)).unwrap()
}

/// Writes, per channel, the number of frames that channel has seen so far in the run (counting
/// the current frame), scaled by 1e-6. Only a run that persists across chunks gets this right.
struct CountFrames;

struct CountRun {
    seen: Vec<u64>,
    rate: u32,
}

impl Effect for CountFrames {
    fn id(&self) -> &'static str {
        "count_frames"
    }
    fn name(&self) -> &'static str {
        "Count Frames"
    }
    fn begin(&self, _params: &[f64], ctx: &RunContext) -> io::Result<Option<Box<dyn Run>>> {
        Ok(Some(Box::new(CountRun { seen: vec![0; ctx.channels.len()], rate: ctx.sample_rate })))
    }
    fn process(&self, _p: &[f64], _f: u64, _i: &[&[f32]], _o: &mut [&mut [f32]]) -> io::Result<()> {
        Err(io::Error::other("the stateless path must not run when begin gives a run"))
    }
}

impl Run for CountRun {
    fn process(&mut self, _first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        assert_eq!(self.rate, RATE);
        for ((i, o), seen) in input.iter().zip(output.iter_mut()).zip(self.seen.iter_mut()) {
            for (_, b) in i.iter().zip(o.iter_mut()) {
                *seen += 1;
                *b = *seen as f32 * 1e-6;
            }
        }
        Ok(())
    }
}

#[test]
fn a_run_keeps_its_state_across_chunks() {
    let tmp = TempDir::new("state");
    let frames = BLOCK_FRAMES * 2 + 500;
    let data = vec![vec![0.0f32; frames], vec![0.0f32; frames]];
    let d = doc(&tmp.0, &data);
    let range = 100..(frames as u64 - 50);
    let cancel = AtomicBool::new(false);
    let edit = effects::apply(&CountFrames, &[], &d, range.clone(), ChannelMask::ALL, Crossfades::NONE, &cancel).unwrap();
    for c in 0..2 {
        let mut got = vec![f32::NAN; frames];
        assert_eq!(edit.state.read(c, 0, &mut got).unwrap(), frames);
        assert_eq!(got[99], 0.0, "before the range is untouched");
        for k in [0usize, 1, BLOCK_FRAMES - 1, BLOCK_FRAMES, 2 * BLOCK_FRAMES - 1] {
            let want = (k + 1) as f32 * 1e-6;
            assert_eq!(got[100 + k], want, "channel {c}, frame {k} of the range");
        }
        let last = (range.end - range.start) as f32 * 1e-6;
        assert_eq!(got[range.end as usize - 1], last);
        assert_eq!(got[range.end as usize], 0.0, "after the range is untouched");
    }
}

#[test]
fn the_new_param_kinds_describe_themselves() {
    let f = ParamDesc::frequency("freq", "Frequency", 20.0, 20_000.0, 1000.0);
    assert_eq!((f.kind, f.unit), (ParamKind::Frequency, "Hz"));
    let q = ParamDesc::number("q", "Q", 0.1, 20.0, 0.7);
    assert_eq!((q.kind, q.unit), (ParamKind::Number, ""));
}
