//! Tests for #1173: Edit › Snapping, the Edit menu's submenu of the snapping switches.
//!
//! The submenu calls `snap_items`, the same function View › Snapping uses, and a tick goes through
//! `toggle_snap_menu`. The first test checks the six labels in menu order. The second renders the
//! real Edit menu in headless egui frames and clicks its submenu and checkbox before checking the
//! shared state used by View and the label used by the ruler.
use super::*;

/// The labels `snap_items` draws, in table order: the Edit menu's six tick items.
fn snap_menu_labels() -> Vec<&'static str> {
    TABLE.iter().filter(|a| a.name.starts_with("view.snap.")).map(|a| a.menu).collect()
}

#[test]
fn edit_snapping_lists_six_items_from_snapping_to_snap_to_markers() {
    assert_eq!(
        snap_menu_labels(),
        [
            "Snapping",
            "Snap to Zero Crossings",
            "Snap to Ruler (Coarse)",
            "Snap to Ruler (Fine)",
            "Snap to Selection Edges and Cursor",
            "Snap to Markers",
        ]
    );
}

/// A folder for one test, removed when it ends. Declare it before the window, so the window (and
/// its session store inside) is dropped first.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target").join(format!("wavcraft-issue1173-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch folder");
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The window as the program builds it, with no file open. Its settings file and recent-files list
/// are cut loose at once, so nothing a test does is written to the user's own.
fn a_window(scratch: &Scratch) -> WaveApp {
    let cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    let mut app = WaveApp::new(&cc, None, Some(scratch.0.join("sessions")), false);
    app.settings_path = None;
    app.recent_path = None;
    app
}

#[test]
fn ticking_snap_to_zero_crossings_in_edit_shows_in_view_and_the_ruler() {
    let scratch = Scratch::new("zero");
    let mut app = a_window(&scratch);
    app.settings.snap.zero_crossings = false;
    let state = newdoc::empty(&app.new_doc_options(), 44_100, 1, SampleFormat::Int16).expect("an empty document");
    let mut editor = Editor::new(History::new(state, u64::MAX));
    editor.snap = app.settings.snap;
    app.editor = Some(editor);
    let ctx = egui::Context::default();
    let mut frame = |events| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 1800.0))),
                events,
                ..Default::default()
            },
            |ui| app.edit_menu(ui),
        );
        output.textures_delta.clear();
        output
    };
    fn label_position(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
        fn find(shape: &egui::epaint::Shape, label: &str) -> Option<egui::Pos2> {
            match shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == label => Some(text.pos + text.galley.size() * 0.5),
                egui::epaint::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, label)),
                _ => None,
            }
        }
        output.shapes.iter().find_map(|shape| find(&shape.shape, label)).unwrap_or_else(|| panic!("missing rendered menu item: {label}"))
    }
    fn pointer(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
        ]
    }
    let output = frame(vec![]);
    let submenu = label_position(&output, "Snapping");
    frame(pointer(submenu, true));
    frame(pointer(submenu, false));
    let output = frame(vec![]);
    for label in snap_menu_labels() {
        label_position(&output, label);
    }
    let zero = label_position(&output, "Snap to Zero Crossings");
    frame(pointer(zero, true));
    frame(pointer(zero, false));
    assert!(app.settings.snap.zero_crossings);
    assert_eq!(app.notice.as_deref(), Some("Snap to zero crossings on"));
    // View uses snap_flag_value; the ruler uses Editor::label via menu_rows.
    // The ruler currently displays "(on)" rather than a checkbox tick.
    assert_eq!(actions::snap_flag_value(&app.settings.snap, ActionId::SnapZeroToggle), Some(true));
    let mut ruler = ctx.run_ui(egui::RawInput::default(), |ui| {
        app.menu_rows(ui, &Menu::Ruler.entries());
    });
    ruler.textures_delta.clear();
    label_position(&ruler, "Snap to Zero Crossings (on)");
}
