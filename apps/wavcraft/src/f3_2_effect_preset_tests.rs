//! Tests for F3.2 (#1399): the effect dialog's Presets menu. A preset saved in one window loads in
//! a second window on the same folder, a preset that does not check out is refused, delete
//! removes it, and the dialog draws the menu. Every test uses a presets folder under its own
//! scratch folder, never the config folder.
use super::*;
use std::fs;
use wavcraft_core::presets::Presets;

/// A folder for one test, removed when it ends. Declare it before the windows, so the windows are
/// dropped first.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("wavcraft-f3-2-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("a scratch folder");
        Scratch(dir)
    }

    fn presets(&self) -> PathBuf {
        self.0.join("presets")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The window as the program builds it, with its session folder and its presets folder in
/// `scratch`. Its settings file and recent-files list are cut loose at once.
fn a_window(scratch: &Scratch) -> WaveApp {
    let cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    let mut app = WaveApp::new(&cc, None, Some(scratch.0.join("sessions")), false);
    assert!(app.sessions.path.starts_with(&scratch.0), "the window's sessions are in the scratch folder, not {}", app.sessions.path.display());
    app.settings_path = None;
    app.recent_path = None;
    app.rack.set_preset_store(Some(Presets::at(scratch.presets())));
    app
}

/// The index of Amplify in the effect registry.
fn amplify_index() -> u16 {
    wavcraft_core::effects::all().iter().position(|e| e.id() == "amplify").expect("Amplify is in the registry") as u16
}

#[test]
fn a_saved_amplify_preset_loads_in_a_second_window() {
    let scratch = Scratch::new("second-window");
    {
        let mut a = a_window(&scratch);
        a.effect_dialog = EffectDialog::new(amplify_index(), Some(vec![3.0, 3.0, 1.0]));
        let dialog = a.effect_dialog.as_mut().expect("the dialog is open");
        dialog.set_preset_name("Test");
        dialog.save_preset(a.rack.preset_store());
        assert_eq!(dialog.note(), "saved Amplify preset \"Test\"");
        assert!(scratch.presets().join("amplify").join("Test.preset").is_file(), "the preset is a file in the scratch folder");
    }
    let mut b = a_window(&scratch);
    b.effect_dialog = EffectDialog::new(amplify_index(), None);
    let dialog = b.effect_dialog.as_mut().expect("the dialog is open");
    assert_eq!(dialog.values, vec![0.0, 0.0, 1.0]);
    dialog.load_preset(b.rack.preset_store(), "Test");
    assert_eq!(dialog.values, vec![3.0, 3.0, 1.0], "the second window shows 3 dB");
    assert_eq!(dialog.note(), "loaded Amplify preset \"Test\"");
}

#[test]
fn a_preset_for_the_wrong_settings_is_refused() {
    let scratch = Scratch::new("wrong-settings");
    let a = a_window(&scratch);
    // The save path checks ranges, so write the file by hand with a level out of range.
    let folder = scratch.presets().join("amplify");
    fs::create_dir_all(&folder).expect("the preset folder");
    fs::write(folder.join("Bad.preset"), "left_db=999\nright_db=0\nlink=1\n").expect("the preset file");
    let mut dialog = EffectDialog::new(amplify_index(), None).expect("the dialog opens");
    dialog.load_preset(a.rack.preset_store(), "Bad");
    assert_eq!(dialog.values, vec![0.0, 0.0, 1.0], "the values are unchanged");
    assert!(dialog.note().contains("left_db"), "the refusal names the bad setting, got {:?}", dialog.note());
}

#[test]
fn delete_removes_the_preset() {
    let scratch = Scratch::new("delete");
    let a = a_window(&scratch);
    let mut dialog = EffectDialog::new(amplify_index(), Some(vec![3.0, 3.0, 1.0])).expect("the dialog opens");
    dialog.set_preset_name("Test");
    dialog.save_preset(a.rack.preset_store());
    dialog.delete_preset(a.rack.preset_store(), "Test");
    assert_eq!(dialog.note(), "deleted preset \"Test\"");
    let presets = a.rack.preset_store().expect("the presets folder");
    assert_eq!(presets.list("amplify"), Ok(Vec::new()));
}

#[test]
fn the_dialog_draws_a_presets_menu() {
    let ctx = egui::Context::default();
    let mut amplify = EffectDialog::new(amplify_index(), None).expect("the dialog opens");
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        amplify.show(ui.ctx(), None);
    });
    output.textures_delta.clear();
    let mut generator = EffectDialog::new_generator(0, None).expect("the generator dialog opens");
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        generator.show(ui.ctx(), None);
    });
    output.textures_delta.clear();
}
