//! Stream an Effects Rack into one undoable edit (F1).

use std::ops::Range;
use std::sync::atomic::AtomicBool;

use super::rack::RackConfig;
use crate::doc::DocState;
use crate::doc::edit::{ChannelMask, Crossfades, Edit, EditError};

#[allow(unused_variables, clippy::todo)]
pub fn apply(
    config: &RackConfig<'_>,
    doc: &DocState,
    range: Range<u64>,
    channels: ChannelMask,
    xf: Crossfades,
    cancel: &AtomicBool,
) -> Result<Edit, EditError> {
    todo!()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod locked_129b {
    use super::*;
    use crate::doc::edit::Change;
    use crate::doc::history::History;
    use crate::effects::card129_tests::*;

    #[test]
    fn selection_is_one_named_undo_step_and_mask_is_preserved() {
        let f = Fixture::new("apply-selection", &[vec![1.0, 2.0, 3.0, 4.0, 5.0], vec![6.0, 7.0, 8.0, 9.0, 10.0]]);
        let c = config(vec![slot(&Bias), slot(&Double), slot(&crate::effects::Invert)]);
        let edit = apply(&c, &f.doc, 1..4, ChannelMask::only(0), Crossfades::NONE, &AtomicBool::new(false)).unwrap();
        assert_eq!(edit.label, "Effects Rack");
        assert_eq!(edit.change, Change::SameLength { range: 1..4, channels: ChannelMask::only(0) });
        assert_eq!(edit.state.len_frames(), 5);
        assert_eq!(samples(&edit.state, 0), vec![1.0, -6.0, -8.0, -10.0, 5.0]);
        assert_eq!(samples(&edit.state, 1), vec![6.0, 7.0, 8.0, 9.0, 10.0]);
        let mut h = History::new(f.doc.clone(), u64::MAX);
        h.push_edit(edit);
        assert_eq!(h.undo_label(), Some("Effects Rack"));
        assert_eq!(samples(&h.undo().unwrap(), 0), vec![1.0, 2.0, 3.0, 4.0, 5.0]);
        assert!(h.undo().is_none());
        assert_eq!(samples(&h.redo().unwrap(), 0), vec![1.0, -6.0, -8.0, -10.0, 5.0]);
    }

    #[test]
    fn empty_selection_means_whole_file() {
        let f = Fixture::new("apply-whole", &[vec![1.0, -2.0, 3.0]]);
        let c = config(vec![slot(&crate::effects::Invert)]);
        let edit = apply(&c, &f.doc, 2..2, ChannelMask::ALL, Crossfades::NONE, &AtomicBool::new(false)).unwrap();
        assert_eq!(samples(&edit.state, 0), vec![-1.0, 2.0, -3.0]);
        assert_eq!(samples(&f.doc, 0), vec![1.0, -2.0, 3.0]);
    }

    #[test]
    fn absolute_frame_offset_is_not_added_twice_across_chunks() {
        let f = Fixture::new("apply-offset", &[vec![0.0; 65_542]]);
        let c = config(vec![slot(&Frames)]);
        let edit = apply(&c, &f.doc, 3..65_542, ChannelMask::ALL, Crossfades::NONE, &AtomicBool::new(false)).unwrap();
        let out = samples(&edit.state, 0);
        assert_eq!(&out[..5], &[0.0, 0.0, 0.0, 3.0, 4.0]);
        assert_eq!(&out[65_537..], &[65_537.0, 65_538.0, 65_539.0, 65_540.0, 65_541.0]);
    }

    #[test]
    fn stateful_slot_survives_store_chunk_boundary() {
        let f = Fixture::new("apply-state", &[vec![1.0; 65_540]]);
        let c = config(vec![slot(&Accumulator)]);
        let edit = apply(&c, &f.doc, 2..65_540, ChannelMask::ALL, Crossfades::NONE, &AtomicBool::new(false)).unwrap();
        let out = samples(&edit.state, 0);
        assert_eq!(&out[..5], &[1.0, 1.0, 1.0, 2.0, 3.0]);
        assert_eq!(&out[65_537..], &[65_536.0, 65_537.0, 65_538.0]);
    }

    #[test]
    fn cancellation_and_bad_range_never_publish_an_edit() {
        let f = Fixture::new("apply-errors", &[vec![1.0, 2.0, 3.0]]);
        let c = config(vec![slot(&Double)]);
        assert!(matches!(apply(&c, &f.doc, 0..3, ChannelMask::ALL, Crossfades::NONE, &AtomicBool::new(true)), Err(EditError::Cancelled)));
        assert!(apply(&c, &f.doc, 1..4, ChannelMask::ALL, Crossfades::NONE, &AtomicBool::new(false)).is_err());
        assert_eq!(samples(&f.doc, 0), vec![1.0, 2.0, 3.0]);
    }
}
