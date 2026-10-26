//! E5 (#31): Repeat Last Command (`Shift+R` / `F3`) and Repeat Last Command… (`Ctrl+R` / `F2`).

use std::sync::Arc;
use wavcraft_core::actions::{self, ActionId, Editor, RepeatDialog, Shortcut};
use wavcraft_core::doc::{BlockRef, DocState, History, Piece, SourceFormat, Store};
use wavcraft_core::effects;

fn editor(samples: &[f32]) -> (Store, Editor) {
    let store = Store::new_temp("e5-test").unwrap();
    let mut w = store.begin_source(0, 1).unwrap();
    w.push(samples).unwrap();
    let len = samples.len() as u64;
    w.finish().unwrap();
    let state = DocState::from_channel_pieces(48_000, SourceFormat::WavPcm16, vec![vec![Piece { block: BlockRef::Source(0), offset: 0, len }]]);
    (store, Editor::new(History::new(Arc::new(store_clone_ref(&store)), state)))
}

fn store_clone_ref(s: &Store) -> Store {
    Store::reopen(s.dir().to_path_buf()).unwrap()
}

fn read(e: &Editor) -> Vec<f32> {
    let doc = e.history.current();
    let mut out = vec![0.0; doc.len_frames() as usize];
    doc.read(0, 0, &mut out).unwrap();
    out
}

#[test]
fn e5_shortcuts_bind_f3_shift_r_and_f2_ctrl_r() {
    let repeat = actions::action(ActionId::RepeatLast);
    assert_eq!(repeat.name, "edit.repeatLast");
    assert_eq!(repeat.menu, "Repeat Last Command");
    let f3 = Shortcut { cmd: false, shift: false, alt: false, key: "F3" };
    let shift_r = Shortcut { cmd: false, shift: true, alt: false, key: "R" };
    assert_eq!(repeat.keys, &[f3, shift_r]);
    assert_eq!([repeat.keys[0].text(false), repeat.keys[1].text(false)], ["F3", "Shift+R"]);
    assert_eq!([repeat.keys[0].text(true), repeat.keys[1].text(true)], ["F3", "⇧R"]);

    let dialog = actions::action(ActionId::RepeatLastDialog);
    assert_eq!(dialog.name, "edit.repeatLastDialog");
    assert_eq!(dialog.menu, "Repeat Last Command…");
    let f2 = Shortcut { cmd: false, shift: false, alt: false, key: "F2" };
    let ctrl_r = Shortcut { cmd: true, shift: false, alt: false, key: "R" };
    assert_eq!(dialog.keys, &[f2, ctrl_r]);
    assert_eq!([dialog.keys[0].text(false), dialog.keys[1].text(false)], ["F2", "Ctrl+R"]);
    assert_eq!([dialog.keys[0].text(true), dialog.keys[1].text(true)], ["F2", "⌘R"]);

    let (_s, e) = editor(&[0.5, -0.5, 0.25, -0.25]);
    assert!(!e.enabled(ActionId::RepeatLast, &(0..4)));
    assert!(!e.enabled(ActionId::RepeatLastDialog, &(0..4)));
    assert_eq!(e.label(ActionId::RepeatLast), "Repeat Last Command");
    assert_eq!(e.label(ActionId::RepeatLastDialog), "Repeat Last Command…");
    assert_eq!(e.repeat_dialog(&(0..4)), RepeatDialog::Unavailable);
}

#[test]
fn e5_repeat_last_and_dialog_remember_parameterised_effect_settings() {
    let (_s, mut e) = editor(&[0.8, -0.4, 0.8, -0.4]);
    e.run_effect(1, vec![-6.0206], 0..2).unwrap();
    assert_eq!(e.label(ActionId::RepeatLast), "Repeat Gain");
    assert_eq!(e.label(ActionId::RepeatLastDialog), "Repeat Gain…");
    assert_eq!(e.repeat_dialog(&(2..4)), RepeatDialog::Effect(1, vec![-6.0206]));

    e.run(ActionId::RepeatLast, 2..4).unwrap();
    let got = read(&e);
    for (a, b) in got.iter().zip([0.4f32, -0.2, 0.4, -0.2]) {
        assert!((a - b).abs() < 1e-4, "{got:?}");
    }
    assert_eq!(e.repeat_dialog(&(0..4)), RepeatDialog::Effect(1, vec![-6.0206]));
}

#[test]
fn e5_repeat_dialog_runs_parameterless_effects_and_repeatable_edits_immediately() {
    let (_s, mut e) = editor(&[0.25, -0.75, 0.5, -0.5]);
    e.run(ActionId::Effect(0), 0..2).unwrap();
    assert_eq!(e.label(ActionId::RepeatLast), "Repeat Invert");
    assert_eq!(e.label(ActionId::RepeatLastDialog), "Repeat Invert");
    assert_eq!(e.repeat_dialog(&(2..4)), RepeatDialog::RunNow);

    e.run(ActionId::RepeatLast, 2..4).unwrap();
    assert_eq!(read(&e), vec![-0.25, 0.75, -0.5, 0.5]);

    e.run_effect(6, vec![], 0..4).unwrap();
    assert_eq!(e.label(ActionId::RepeatLast), "Repeat DC Offset");
    assert_eq!(e.label(ActionId::RepeatLastDialog), "Repeat DC Offset");
    assert_eq!(e.repeat_dialog(&(0..4)), RepeatDialog::RunNow);

    e.run(ActionId::Silence, 0..2).unwrap();
    assert_eq!(e.label(ActionId::RepeatLast), "Repeat Silence");
    assert_eq!(e.label(ActionId::RepeatLastDialog), "Repeat Silence");
    assert_eq!(e.repeat_dialog(&(2..4)), RepeatDialog::RunNow);
    assert_eq!(e.repeat_dialog(&(2..2)), RepeatDialog::Unavailable);
}

#[test]
fn e5_delete_is_not_repeatable_and_never_replaces_a_remembered_effect() {
    let (_s, mut e) = editor(&[0.5, 0.5, 0.5, 0.5, 0.5, 0.5]);
    e.run(ActionId::Delete, 0..2).unwrap();
    assert_eq!(read(&e), vec![0.5, 0.5, 0.5, 0.5]);
    assert!(!e.enabled(ActionId::RepeatLast, &(0..2)));
    assert!(!e.enabled(ActionId::RepeatLastDialog, &(0..2)));
    assert_eq!(e.repeat_dialog(&(0..2)), RepeatDialog::Unavailable);

    e.run_effect(1, vec![6.0206], 0..2).unwrap();
    e.run(ActionId::Delete, 0..1).unwrap();
    assert_eq!(e.label(ActionId::RepeatLast), "Repeat Gain");
    assert_eq!(e.label(ActionId::RepeatLastDialog), "Repeat Gain…");
    assert_eq!(e.repeat_dialog(&(1..3)), RepeatDialog::Effect(1, vec![6.0206]));

    e.run(ActionId::RepeatLast, 1..3).unwrap();
    let got = read(&e);
    assert_eq!(got.len(), 3);
    for a in got {
        assert!((a - 1.0).abs() < 1e-4);
    }
}

#[test]
fn e5_works_for_every_registered_effect_in_the_registry() {
    let eq = vec![1.0, 120.0, 3.0, 1.0, 1200.0, -3.0, 1.5, 1.0, 6000.0, 2.0, 0.8];
    let cases: [(u16, Vec<f64>, RepeatDialog); 7] = [
        (0, vec![], RepeatDialog::RunNow),
        (1, vec![-3.0], RepeatDialog::Effect(1, vec![-3.0])),
        (2, eq.clone(), RepeatDialog::Effect(2, eq)),
        (3, vec![250.0, 2.0], RepeatDialog::Effect(3, vec![250.0, 2.0])),
        (4, vec![2500.0, 2.0], RepeatDialog::Effect(4, vec![2500.0, 2.0])),
        (5, vec![1000.0, 4.0], RepeatDialog::Effect(5, vec![1000.0, 4.0])),
        (6, vec![], RepeatDialog::RunNow),
    ];
    assert_eq!(effects::all().len(), cases.len());
    let half: Vec<f32> = (0..32).map(|k| 0.25 + 0.5 * (k as f32 * 0.375).sin()).collect();
    let mut input = half.clone();
    input.extend_from_slice(&half);
    for (i, params, expected_dialog) in cases {
        let (_s, mut e) = editor(&input);
        e.run_effect(i, params, 0..32).unwrap();
        assert_eq!(e.repeat_dialog(&(32..64)), expected_dialog);
        e.run(ActionId::RepeatLast, 32..64).unwrap();
        let got = read(&e);
        assert_eq!(&got[0..32], &got[32..64]);
    }
}

#[test]
fn e5_repeat_dialog_reopens_insert_silence_and_disables_on_empty_doc() {
    let (_s, mut e) = editor(&[1.0, 2.0, 3.0, 4.0]);
    e.insert_silence(2, 3).unwrap();
    assert_eq!(e.label(ActionId::RepeatLast), "Repeat Insert Silence");
    assert_eq!(e.label(ActionId::RepeatLastDialog), "Repeat Silence…");
    assert_eq!(e.repeat_dialog(&(0..0)), RepeatDialog::InsertSilence(3));

    e.run(ActionId::Delete, 0..7).unwrap();
    assert!(!e.enabled(ActionId::RepeatLastDialog, &(0..0)));
    assert_eq!(e.repeat_dialog(&(0..0)), RepeatDialog::Unavailable);
}
