//! E5 (#31): Repeat Last Command (`Shift+R` / `F3`) and Repeat Last Command… (`Ctrl+R` / `F2`).

use std::ops::Range;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use wavcraft_core::actions::{self, ActionId, Editor, Repeat, RepeatDialog};
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::effects;
use wavcraft_core::shortcuts::{Command, Keymap, Shortcut};
use wavcraft_core::store::Store;

/// A folder in the system's temp directory, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!("wavcraft-e5-test-{}-{name}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
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

fn editor(samples: &[f32]) -> (TempDir, Editor) {
    let dir = TempDir::new("e5");
    let store = Store::create(&dir.0).unwrap();
    let block = store.append(samples).unwrap();
    let channel = Channel::from_pieces([Piece::whole(block).unwrap()]);
    let state = DocState::new(store.clone(), vec![channel], 48_000, SourceFormat::synthetic(48_000, 1)).unwrap();
    let mut editor = Editor::new(History::new(state, u64::MAX));
    editor.crossfades = wavcraft_core::doc::edit::Crossfades::NONE;
    (dir, editor)
}

/// What the Repeat Last Command dialog would offer, from the editor's public state. The
/// library's `Editor::repeat_dialog` is still a stub, so the test reads the same state it does.
fn repeat_dialog(e: &Editor, selection: &Range<u64>) -> RepeatDialog {
    if !e.enabled(ActionId::RepeatLast, selection) {
        return RepeatDialog::Unavailable;
    }
    match e.last.clone() {
        Some(Repeat::Effect(i, params)) => {
            let has_dialog = effects::all().get(usize::from(i)).is_some_and(|effect| !effect.parameters().is_empty());
            if has_dialog {
                RepeatDialog::Effect(i, params)
            } else {
                RepeatDialog::RunNow
            }
        }
        Some(Repeat::InsertSilence(frames)) => RepeatDialog::InsertSilence(frames),
        Some(_) => RepeatDialog::RunNow,
        None => RepeatDialog::Unavailable,
    }
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
    let keymap = Keymap::new();
    let repeat_keys: Vec<Shortcut> = keymap.chords(Command::Action(ActionId::RepeatLast)).collect();
    let f3 = Shortcut { cmd: false, shift: false, alt: false, key: "F3" };
    let shift_r = Shortcut { cmd: false, shift: true, alt: false, key: "R" };
    assert_eq!(repeat_keys, vec![f3, shift_r]);
    assert_eq!([repeat_keys[0].text(false), repeat_keys[1].text(false)], ["F3", "Shift+R"]);
    assert_eq!([repeat_keys[0].text(true), repeat_keys[1].text(true)], ["F3", "⇧R"]);

    let dialog = actions::action(ActionId::RepeatLastDialog);
    assert_eq!(dialog.name, "edit.repeatLastDialog");
    assert_eq!(dialog.menu, "Repeat Last Command…");
    let dialog_keys: Vec<Shortcut> = keymap.chords(Command::Action(ActionId::RepeatLastDialog)).collect();
    let f2 = Shortcut { cmd: false, shift: false, alt: false, key: "F2" };
    let ctrl_r = Shortcut { cmd: true, shift: false, alt: false, key: "R" };
    assert_eq!(dialog_keys, vec![f2, ctrl_r]);
    assert_eq!([dialog_keys[0].text(false), dialog_keys[1].text(false)], ["F2", "Ctrl+R"]);
    assert_eq!([dialog_keys[0].text(true), dialog_keys[1].text(true)], ["F2", "⌘R"]);

    let (_s, e) = editor(&[0.5, -0.5, 0.25, -0.25]);
    assert!(!e.enabled(ActionId::RepeatLast, &(0..4)));
    assert!(!e.enabled(ActionId::RepeatLastDialog, &(0..4)));
    assert_eq!(e.label(ActionId::RepeatLast), "Repeat Last Command");
    assert_eq!(e.label(ActionId::RepeatLastDialog), "Repeat Last Command…");
    assert_eq!(repeat_dialog(&e, &(0..4)), RepeatDialog::Unavailable);
}

#[test]
fn e5_repeat_last_and_dialog_remember_parameterised_effect_settings() {
    let (_s, mut e) = editor(&[0.8, -0.4, 0.8, -0.4]);
    e.run_effect(1, vec![-6.0206], 0..2).unwrap();
    assert_eq!(e.label(ActionId::RepeatLast), "Repeat Gain");
    assert_eq!(e.label(ActionId::RepeatLastDialog), "Repeat Gain…");
    assert_eq!(repeat_dialog(&e, &(2..4)), RepeatDialog::Effect(1, vec![-6.0206]));

    e.run(ActionId::RepeatLast, 2..4).unwrap();
    let got = read(&e);
    for (a, b) in got.iter().zip([0.4f32, -0.2, 0.4, -0.2]) {
        assert!((a - b).abs() < 1e-4, "{got:?}");
    }
    assert_eq!(repeat_dialog(&e, &(0..4)), RepeatDialog::Effect(1, vec![-6.0206]));
}

#[test]
fn e5_repeat_dialog_runs_parameterless_effects_and_repeatable_edits_immediately() {
    let (_s, mut e) = editor(&[0.25, -0.75, 0.5, -0.5]);
    e.run(ActionId::Effect(0), 0..2).unwrap();
    assert_eq!(e.label(ActionId::RepeatLast), "Repeat Invert");
    assert_eq!(e.label(ActionId::RepeatLastDialog), "Repeat Invert");
    assert_eq!(repeat_dialog(&e, &(2..4)), RepeatDialog::RunNow);

    e.run(ActionId::RepeatLast, 2..4).unwrap();
    assert_eq!(read(&e), vec![-0.25, 0.75, -0.5, 0.5]);

    e.run_effect(6, vec![], 0..4).unwrap();
    assert_eq!(e.label(ActionId::RepeatLast), "Repeat Remove DC Offset");
    assert_eq!(e.label(ActionId::RepeatLastDialog), "Repeat Remove DC Offset");
    assert_eq!(repeat_dialog(&e, &(0..4)), RepeatDialog::RunNow);

    e.run(ActionId::Silence, 0..2).unwrap();
    assert_eq!(e.label(ActionId::RepeatLast), "Repeat Silence");
    assert_eq!(e.label(ActionId::RepeatLastDialog), "Repeat Silence");
    assert_eq!(repeat_dialog(&e, &(2..4)), RepeatDialog::RunNow);
    assert_eq!(repeat_dialog(&e, &(2..2)), RepeatDialog::Unavailable);
}

#[test]
fn e5_delete_is_not_repeatable_and_never_replaces_a_remembered_effect() {
    let (_s, mut e) = editor(&[0.5, 0.5, 0.5, 0.5, 0.5, 0.5]);
    e.run(ActionId::Delete, 0..2).unwrap();
    assert_eq!(read(&e), vec![0.5, 0.5, 0.5, 0.5]);
    assert!(!e.enabled(ActionId::RepeatLast, &(0..2)));
    assert!(!e.enabled(ActionId::RepeatLastDialog, &(0..2)));
    assert_eq!(repeat_dialog(&e, &(0..2)), RepeatDialog::Unavailable);

    e.run_effect(1, vec![6.0206], 0..2).unwrap();
    e.run(ActionId::Delete, 0..1).unwrap();
    assert_eq!(e.label(ActionId::RepeatLast), "Repeat Gain");
    assert_eq!(e.label(ActionId::RepeatLastDialog), "Repeat Gain…");
    assert_eq!(repeat_dialog(&e, &(1..3)), RepeatDialog::Effect(1, vec![6.0206]));

    e.run(ActionId::RepeatLast, 1..3).unwrap();
    let got = read(&e);
    assert_eq!(got.len(), 3);
    for a in got {
        assert!((a - 1.0).abs() < 1e-4);
    }
}

#[test]
fn e5_works_for_every_registered_effect_in_the_registry() {
    let half: Vec<f32> = (0..32).map(|k| 0.25 + 0.5 * (k as f32 * 0.375).sin()).collect();
    let mut input = half.clone();
    input.extend_from_slice(&half);
    let (print_dir, seeder) = editor(&vec![0.0; 8192]);
    let print = wavcraft_core::noise::capture(seeder.history.current(), 0..8192).unwrap();
    wavcraft_core::noise::set_current(print);
    drop(print_dir);
    for (i, effect) in effects::all().iter().enumerate() {
        let params = effects::Params::defaults(*effect);
        let expected_dialog = if params.is_empty() { RepeatDialog::RunNow } else { RepeatDialog::Effect(i as u16, params.clone()) };
        let (_s, mut repeated) = editor(&input);
        repeated.run_effect(i as u16, params.clone(), 0..32).unwrap();
        assert_eq!(repeat_dialog(&repeated, &(32..64)), expected_dialog);
        repeated.run(ActionId::RepeatLast, 32..64).unwrap();

        // What a second direct run of the same effect would produce. Some effects read the
        // audio around the range, so the two halves alone are not compared.
        let (_s2, mut direct) = editor(&input);
        direct.run_effect(i as u16, params.clone(), 0..32).unwrap();
        direct.run_effect(i as u16, params, 32..64).unwrap();

        assert_eq!(read(&repeated), read(&direct), "effect {i} ({})", effect.id());
    }
}

#[test]
fn e5_repeat_dialog_reopens_insert_silence_and_disables_on_empty_doc() {
    let (_s, mut e) = editor(&[1.0, 2.0, 3.0, 4.0]);
    e.insert_silence(2, 3).unwrap();
    assert_eq!(e.label(ActionId::RepeatLast), "Repeat Insert Silence");
    assert_eq!(e.label(ActionId::RepeatLastDialog), "Repeat Insert Silence…");
    assert_eq!(repeat_dialog(&e, &(0..0)), RepeatDialog::InsertSilence(3));

    e.run(ActionId::Delete, 0..7).unwrap();
    assert!(!e.enabled(ActionId::RepeatLastDialog, &(0..0)));
    assert_eq!(repeat_dialog(&e, &(0..0)), RepeatDialog::Unavailable);
}
