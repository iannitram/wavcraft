//! Bounded, read-only selection preview for an effect dialog (E6).
//!
//! A [`Preview`] renders a selected range through an effect, looping it, for a dialog's live
//! preview: it never writes to the [`Store`], makes no edit and touches no history, so the caller
//! can hear parameter changes on a worker without touching the device callback.

use std::io;
use std::ops::Range;

use super::{Effect, Run, RunContext, validate};
use crate::doc::DocState;
use crate::doc::edit::ChannelMask;
use crate::store::BLOCK_FRAMES;

pub struct Preview<'a> {
    effect: &'a dyn Effect,
    doc: &'a DocState,
    params: Vec<f64>,
    /// The selection to loop, normalized in [`Preview::new`] (never empty).
    range: Range<u64>,
    /// The document channels the effect sees, in the order of the [`RunContext`].
    active: Vec<usize>,
    /// The stateful run of the current loop pass, when the effect began one.
    run: Option<Box<dyn Run>>,
    /// The next frame to read, always inside `range`.
    cursor: u64,
}

impl<'a> Preview<'a> {
    pub fn new(effect: &'a dyn Effect, params: Vec<f64>, doc: &'a DocState, range: Range<u64>, channels: ChannelMask) -> io::Result<Self> {
        validate(effect, &params).map_err(|why| io::Error::new(io::ErrorKind::InvalidInput, why))?;
        let len = doc.len_frames();
        if len == 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "no source frames to preview"));
        }
        let range = if range.start == range.end { 0..len } else { range };
        if range.start > range.end || range.end > len {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "selection outside the document"));
        }
        let active = channels.active(doc.channel_count());
        let cursor = range.start;
        let mut preview = Preview { effect, doc, params, range, active, run: None, cursor };
        preview.begin_run()?;
        Ok(preview)
    }

    /// Fill at most BLOCK_FRAMES frames per channel, wrapping at the selection end.
    pub fn render(&mut self, output: &mut [&mut [f32]]) -> io::Result<()> {
        let channels = self.doc.channel_count();
        if output.len() != channels {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("{} buffers for {} channels", output.len(), channels)));
        }
        let frames = output.first().map_or(0, |s| s.len());
        if output.iter().any(|s| s.len() != frames) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "output buffers differ in length"));
        }
        if frames > BLOCK_FRAMES {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("{frames} frames: at most {BLOCK_FRAMES} per call")));
        }
        if frames == 0 {
            return Ok(());
        }
        let mut done = 0;
        while done < frames {
            let mut at = self.cursor;
            if at == self.range.end {
                at = self.range.start;
                self.cursor = at;
                self.begin_run()?;
            }
            let seg = (self.range.end - at).min((frames - done) as u64) as usize;
            let mut input = Vec::with_capacity(self.active.len());
            for &ch in &self.active {
                let mut buf = vec![0.0; seg];
                if self.doc.read(ch, at, &mut buf)? != seg {
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "document shorter than its length"));
                }
                input.push(buf);
            }
            let input_refs: Vec<&[f32]> = input.iter().map(Vec::as_slice).collect();
            let mut out_refs: Vec<&mut [f32]> = Vec::with_capacity(self.active.len());
            let mut rest: &mut [&mut [f32]] = output;
            for ch in 0..channels {
                let (head, tail) = rest.split_at_mut(1);
                rest = tail;
                if self.active.contains(&ch) {
                    out_refs.push(&mut head[0][done..done + seg]);
                } else {
                    let dst = &mut head[0][done..done + seg];
                    if self.doc.read(ch, at, dst)? != seg {
                        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "document shorter than its length"));
                    }
                }
            }
            match self.run.as_mut() {
                Some(run) => run.process(at, &input_refs, &mut out_refs)?,
                None => self.effect.process(&self.params, at, &input_refs, &mut out_refs)?,
            }
            self.cursor += seg as u64;
            done += seg;
        }
        Ok(())
    }

    /// Validate before replacing settings; the next render uses them at the current position.
    pub fn set_params(&mut self, params: Vec<f64>) -> io::Result<()> {
        validate(self.effect, &params).map_err(|why| io::Error::new(io::ErrorKind::InvalidInput, why))?;
        let run = self.effect.begin(&params, &self.context())?;
        self.params = params;
        self.run = run;
        Ok(())
    }

    /// The run context for a fresh loop pass.
    fn context(&self) -> RunContext<'_> {
        RunContext { doc: self.doc, range: self.range.clone(), channels: self.active.clone(), sample_rate: self.doc.sample_rate() }
    }

    /// Start the effect's run afresh for the next pass over the selection.
    fn begin_run(&mut self) -> io::Result<()> {
        self.run = self.effect.begin(&self.params, &self.context())?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod locked_129c {
    use super::*;
    use crate::effects::card129_tests::*;

    #[test]
    fn loops_selection_and_passes_inactive_channel_dry_without_writing() {
        let f = Fixture::new("preview-loop", &[vec![9.0, 1.0, 2.0, 3.0, 8.0], vec![7.0, 4.0, 5.0, 6.0, 7.0]]);
        assert_eq!(f.doc.store().bytes_on_disk(), 40);
        let mut p = Preview::new(&crate::effects::Invert, vec![], &f.doc, 1..4, ChannelMask::only(0)).unwrap();
        let (mut l, mut r) = ([0.0; 8], [0.0; 8]);
        p.render(&mut [&mut l, &mut r]).unwrap();
        assert_eq!(l, [-1.0, -2.0, -3.0, -1.0, -2.0, -3.0, -1.0, -2.0]);
        assert_eq!(r, [4.0, 5.0, 6.0, 4.0, 5.0, 6.0, 4.0, 5.0]);
        assert_eq!(samples(&f.doc, 0), vec![9.0, 1.0, 2.0, 3.0, 8.0]);
        assert_eq!(samples(&f.doc, 1), vec![7.0, 4.0, 5.0, 6.0, 7.0]);
        assert_eq!(f.doc.store().bytes_on_disk(), 40);
    }

    #[test]
    fn new_settings_take_effect_on_next_render_without_rewinding() {
        let f = Fixture::new("preview-settings", &[vec![1.0, 2.0, 3.0]]);
        let mut p = Preview::new(&crate::effects::Gain, vec![0.0], &f.doc, 0..3, ChannelMask::ALL).unwrap();
        let mut out = [0.0; 1];
        p.render(&mut [&mut out]).unwrap();
        assert_eq!(out, [1.0]);
        p.set_params(vec![-6.020599913279624]).unwrap();
        p.render(&mut [&mut out]).unwrap();
        assert!((out[0] - 1.0).abs() < 0.000001);
        p.render(&mut [&mut out]).unwrap();
        assert!((out[0] - 1.5).abs() < 0.000001);
        assert_eq!(p.set_params(vec![999.0]).unwrap_err().kind(), io::ErrorKind::InvalidInput);
        p.render(&mut [&mut out]).unwrap();
        assert!((out[0] - 0.5).abs() < 0.000001);
    }

    #[test]
    fn state_carries_between_calls_and_restarts_on_loop() {
        let f = Fixture::new("preview-state", &[vec![9.0, 1.0, 2.0, 3.0, 8.0]]);
        let mut p = Preview::new(&Accumulator, vec![], &f.doc, 1..4, ChannelMask::ALL).unwrap();
        let mut out = [0.0; 2];
        p.render(&mut [&mut out]).unwrap();
        assert_eq!(out, [1.0, 3.0]);
        p.render(&mut [&mut out]).unwrap();
        assert_eq!(out, [6.0, 1.0]);
        p.render(&mut [&mut out]).unwrap();
        assert_eq!(out, [3.0, 6.0]);
    }

    #[test]
    fn empty_selection_uses_whole_file_and_frames_are_absolute() {
        let f = Fixture::new("preview-frames", &[vec![9.0; 5]]);
        let mut p = Preview::new(&Frames, vec![], &f.doc, 1..4, ChannelMask::ALL).unwrap();
        let mut out = [0.0; 4];
        p.render(&mut [&mut out]).unwrap();
        assert_eq!(out, [1.0, 2.0, 3.0, 1.0]);
        let mut p = Preview::new(&Frames, vec![], &f.doc, 2..2, ChannelMask::ALL).unwrap();
        let mut out = [0.0; 6];
        p.render(&mut [&mut out]).unwrap();
        assert_eq!(out, [0.0, 1.0, 2.0, 3.0, 4.0, 0.0]);
    }

    #[test]
    fn rejects_empty_source_bad_settings_range_and_output_shape() {
        let empty = Fixture::new("preview-empty", &[vec![]]);
        assert_eq!(Preview::new(&Frames, vec![], &empty.doc, 0..0, ChannelMask::ALL).err().unwrap().kind(), io::ErrorKind::InvalidInput);
        let f = Fixture::new("preview-invalid", &[vec![1.0, 2.0]]);
        assert!(Preview::new(&Frames, vec![], &f.doc, 1..3, ChannelMask::ALL).is_err());
        assert!(Preview::new(&crate::effects::Gain, vec![999.0], &f.doc, 0..2, ChannelMask::ALL).is_err());
        let mut p = Preview::new(&Frames, vec![], &f.doc, 0..2, ChannelMask::ALL).unwrap();
        assert_eq!(p.render(&mut []).unwrap_err().kind(), io::ErrorKind::InvalidInput);
        let mut too_long = vec![9.0; 65_537];
        assert_eq!(p.render(&mut [&mut too_long]).unwrap_err().kind(), io::ErrorKind::InvalidInput);
        let mut out = [0.0; 1];
        p.render(&mut [&mut out]).unwrap();
        assert_eq!(out, [0.0]);
    }
}
