//! Bounded, read-only selection preview for an effect dialog (E6).

use std::io;
use std::ops::Range;

use super::Effect;
use crate::doc::DocState;
use crate::doc::edit::ChannelMask;

pub struct Preview<'a> {
    _marker: std::marker::PhantomData<&'a ()>,
}

#[allow(unused_variables, clippy::todo)]
impl<'a> Preview<'a> {
    pub fn new(effect: &'a dyn Effect, params: Vec<f64>, doc: &'a DocState, range: Range<u64>, channels: ChannelMask) -> io::Result<Self> {
        todo!()
    }

    /// Fill at most BLOCK_FRAMES frames per channel, wrapping at the selection end.
    pub fn render(&mut self, output: &mut [&mut [f32]]) -> io::Result<()> {
        todo!()
    }

    /// Validate before replacing settings; the next render uses them at the current position.
    pub fn set_params(&mut self, params: Vec<f64>) -> io::Result<()> {
        todo!()
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
