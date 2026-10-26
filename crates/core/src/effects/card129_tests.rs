//! Shared fixtures for the locked F1/E6 tests only.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::rack::{RackConfig, RackSlot};
use super::{Effect, Run, RunContext};
use crate::doc::{Channel, DocState, Piece, SourceFormat};
use crate::store::{BLOCK_FRAMES, Store, StoreOptions};

pub struct Fixture {
    pub doc: DocState,
    dir: PathBuf,
}

impl Fixture {
    pub fn new(label: &str, data: &[Vec<f32>]) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("wavcraft-card129-{}-{label}-{unique}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let store = Store::create_with(&dir, StoreOptions::default()).unwrap();
        let channels = data
            .iter()
            .map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|chunk| Piece::whole(store.append(chunk).unwrap()).unwrap())))
            .collect();
        let doc = DocState::new(store, channels, 48_000, SourceFormat::synthetic(48_000, data.len() as u16)).unwrap();
        Self { doc, dir }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Store cleanup also owns the session; ignoring an already removed folder is intentional.
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub fn context(doc: &DocState, range: Range<u64>, channels: Vec<usize>) -> RunContext<'_> {
    RunContext { doc, range, channels, sample_rate: 48_000 }
}

pub fn slot(effect: &dyn Effect) -> RackSlot<'_> {
    RackSlot { effect, params: vec![], enabled: true }
}

pub fn config(slots: Vec<RackSlot<'_>>) -> RackConfig<'_> {
    RackConfig { slots, input_db: 0.0, output_db: 0.0, mix: 1.0 }
}

pub fn samples(doc: &DocState, channel: usize) -> Vec<f32> {
    let mut out = vec![0.0; doc.len_frames() as usize];
    assert_eq!(doc.read(channel, 0, &mut out).unwrap(), out.len());
    out
}

pub struct Bias;
pub struct Double;
pub struct Bomb;
pub struct Frames;
pub struct Accumulator;

impl Effect for Bias {
    fn id(&self) -> &'static str {
        "test_bias"
    }
    fn name(&self) -> &'static str {
        "Test Bias"
    }
    fn process(&self, _: &[f64], _: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        for (i, o) in input.iter().zip(output.iter_mut()) {
            for (a, b) in i.iter().zip(o.iter_mut()) {
                *b = *a + 1.0;
            }
        }
        Ok(())
    }
}

impl Effect for Double {
    fn id(&self) -> &'static str {
        "test_double"
    }
    fn name(&self) -> &'static str {
        "Test Double"
    }
    fn process(&self, _: &[f64], _: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        for (i, o) in input.iter().zip(output.iter_mut()) {
            for (a, b) in i.iter().zip(o.iter_mut()) {
                *b = *a * 2.0;
            }
        }
        Ok(())
    }
}

impl Effect for Bomb {
    fn id(&self) -> &'static str {
        "test_bomb"
    }
    fn name(&self) -> &'static str {
        "Test Bomb"
    }
    fn begin(&self, _: &[f64], _: &RunContext<'_>) -> io::Result<Option<Box<dyn Run>>> {
        panic!("bypassed begin called")
    }
    fn process(&self, _: &[f64], _: u64, _: &[&[f32]], _: &mut [&mut [f32]]) -> io::Result<()> {
        panic!("bypassed process called")
    }
}

impl Effect for Frames {
    fn id(&self) -> &'static str {
        "test_frames"
    }
    fn name(&self) -> &'static str {
        "Test Frames"
    }
    fn process(&self, _: &[f64], first_frame: u64, _: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        for o in output {
            for (i, b) in o.iter_mut().enumerate() {
                *b = (first_frame + i as u64) as f32;
            }
        }
        Ok(())
    }
}

struct Sums(Vec<f32>);

impl Run for Sums {
    fn process(&mut self, _: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        for ((i, o), sum) in input.iter().zip(output.iter_mut()).zip(&mut self.0) {
            for (a, b) in i.iter().zip(o.iter_mut()) {
                *sum += *a;
                *b = *sum;
            }
        }
        Ok(())
    }
}

impl Effect for Accumulator {
    fn id(&self) -> &'static str {
        "test_accumulator"
    }
    fn name(&self) -> &'static str {
        "Test Accumulator"
    }
    fn begin(&self, _: &[f64], ctx: &RunContext<'_>) -> io::Result<Option<Box<dyn Run>>> {
        assert_eq!(ctx.sample_rate, 48_000);
        assert!(ctx.range.start < ctx.range.end);
        Ok(Some(Box::new(Sums(vec![0.0; ctx.channels.len()]))))
    }
    fn process(&self, _: &[f64], _: u64, _: &[&[f32]], _: &mut [&mut [f32]]) -> io::Result<()> {
        panic!("stateful effect used stateless path")
    }
}
