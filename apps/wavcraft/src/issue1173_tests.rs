//! Tests for #1173: Edit › Snapping, the Edit menu's submenu of the snapping switches.
//!
//! The submenu calls `snap_items`, the same function View › Snapping uses, and a tick goes through
//! `toggle_snap_menu`. The first test checks the six labels in menu order. The second runs a real
//! window (as issue369_tests does) and checks that a tick of Snap to Zero Crossings is what View ›
//! Snapping and the ruler's menu read back.
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
        let dir = std::env::temp_dir().join(format!("wavcraft-issue1173-{}-{name}", std::process::id()));
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
    // The Edit menu's tick: the same call View › Snapping makes.
    app.toggle_snap_menu(ActionId::SnapZeroToggle);
    assert!(app.settings.snap.zero_crossings);
    assert_eq!(app.notice.as_deref(), Some("Snap to zero crossings on"));
    // View › Snapping and the ruler's menu read the same switch.
    assert_eq!(actions::snap_flag_value(&app.settings.snap, ActionId::SnapZeroToggle), Some(true));
}
