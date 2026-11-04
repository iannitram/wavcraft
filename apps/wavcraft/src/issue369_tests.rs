//! Tests for #369: Save and close must not close a document that changed while its save ran.
//!
//! The window's `finish_saving` closes the document after a Save-and-close only when the open step
//! is still the one that was saved (`closes_after_save`). Before, it closed unconditionally, so an
//! edit confirmed while the save was running (Insert Silence, say) was lost with the document.
use super::*;

fn a_doc() -> Arc<DocState> {
    Arc::new(newdoc::empty(&NewDocOptions::default(), 44_100, 1, SampleFormat::Int16).expect("an empty document"))
}

#[test]
fn a_save_that_finished_on_the_open_step_closes_the_document() {
    let saved = a_doc();
    let current = Arc::clone(&saved);
    assert!(closes_after_save(Some(&current), &saved));
}

/// The reported case: the file was written with the state at Save, then an edit (100 frames saved,
/// 580 frames current in the report) made a new step before the save finished. The document must
/// stay open.
#[test]
fn an_edit_made_while_the_save_ran_keeps_the_document_open() {
    let saved = a_doc();
    let edited = a_doc();
    assert!(!closes_after_save(Some(&edited), &saved));
}

#[test]
fn with_no_document_open_nothing_is_kept_open() {
    let saved = a_doc();
    assert!(closes_after_save(None, &saved));
}
