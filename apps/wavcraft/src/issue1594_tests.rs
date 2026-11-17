//! Locked tests for #1594:
//! The right dock is clipped at the default 1280 x 720 window.
//!
//! Done when: at 1280 x 720 (and at the 1280 x 800 handheld size) with the default layout,
//! every right-dock tab label and button is fully visible, either because the zone takes the
//! width its content needs or because the tabs scroll or wrap; a test lays out the default
//! workspace at 1280 x 720 and checks that no right-zone tab or button rect is clipped by the
//! panel's left edge.

use super::*;
use std::path::PathBuf;
use wavcraft_core::panels::{DockSide, EDITOR_MIN};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target").join(format!("wavcraft-issue1594-{}-{name}", std::process::id()));
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

fn a_window(scratch: &Scratch) -> WaveApp {
    let cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    let mut app = WaveApp::new(&cc, None, Some(scratch.0.join("sessions")), false);
    app.settings_path = None;
    app.recent_path = None;
    app
}

fn find_text_shapes<'a>(shape: &'a egui::epaint::Shape, out: &mut Vec<(&'a str, Rect)>) {
    match shape {
        egui::epaint::Shape::Text(text) => {
            let r = Rect::from_min_size(text.pos, text.galley.size());
            out.push((text.galley.text(), r));
        }
        egui::epaint::Shape::Vec(shapes) => {
            for s in shapes {
                find_text_shapes(s, out);
            }
        }
        _ => {}
    }
}

/// Named rects: (tab id or text, rect).
type Named = Vec<(String, Rect)>;

fn layout_frame(app: &mut WaveApp, width: f32, height: f32) -> (Rect, Named, Named) {
    let ctx = app.ctx.clone();
    let mut frame = eframe::Frame::_new_kittest();
    <WaveApp as eframe::App>::logic(app, &ctx, &mut frame);
    let mut output = ctx
        .run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, height))), ..Default::default() }, |ui| {
            <WaveApp as eframe::App>::ui(app, ui, &mut frame)
        });
    output.textures_delta.clear();

    let right_zone = app.zone_rects.iter().find(|(z, _)| *z == DockSide::Right).map(|(_, r)| *r).expect("right dock zone rect must exist");

    let mut text_shapes = Vec::new();
    for shape in &output.shapes {
        find_text_shapes(&shape.shape, &mut text_shapes);
    }
    let shapes = text_shapes.into_iter().map(|(t, r)| (t.to_string(), r)).collect();

    // tab_rects holds every zone's tabs; keep the right zone's own.
    let right_ids = app.layout.visible(DockSide::Right, &app.panels);
    let tabs = app.tab_rects.iter().filter(|(id, _)| right_ids.contains(&id.as_str())).cloned().collect();
    (right_zone, tabs, shapes)
}

#[test]
fn right_dock_not_clipped_at_default_1280x720_window() {
    let scratch = Scratch::new("720p");
    let mut app = a_window(&scratch);

    let (right_zone, tabs, shapes) = layout_frame(&mut app, 1280.0, 720.0);

    // The editor retains at least EDITOR_MIN width.
    let left_zone_width = app.zone_rects.iter().find(|(z, _)| *z == DockSide::Left).map(|(_, r)| r.width()).unwrap_or(0.0);
    let editor_width = 1280.0 - left_zone_width - right_zone.width();
    assert!(editor_width >= EDITOR_MIN.0, "editor width {editor_width} should be at least EDITOR_MIN {}", EDITOR_MIN.0);

    // Right dock took the width it needed (larger than the legacy 240.0 default).
    assert!(right_zone.width() > 240.0, "right zone width should expand to fit content, got {}", right_zone.width());

    let left_edge = right_zone.left();
    let right_edge = right_zone.right();

    // Check all registered tab rects: none clipped by panel's left edge or past window/panel right.
    assert!(!tabs.is_empty(), "expected right zone tabs");
    for (id, rect) in &tabs {
        assert!(rect.left() >= left_edge - 0.5, "tab {id} rect {:?} clipped by right panel left edge {left_edge}", rect);
        assert!(rect.right() <= right_edge + 0.5, "tab {id} rect {:?} past right edge {right_edge}", rect);
    }

    // Check rendered tab label text shapes.
    let tab_labels = ["Effects Rack", "History", "Playlist", "Match Loudness", "Episode Check", "Transcript"];
    for label in tab_labels {
        let matching: Vec<Rect> = shapes.iter().filter(|(t, _)| t == label).map(|(_, r)| *r).collect();
        assert!(!matching.is_empty(), "tab label {label} should be rendered");
        for r in matching {
            assert!(r.left() >= left_edge - 0.5, "tab label '{label}' rect {:?} clipped by panel left edge {left_edge}", r);
            assert!(r.right() <= right_edge + 0.5, "tab label '{label}' rect {:?} past panel right edge {right_edge}", r);
        }
    }

    // Check Effects Rack buttons (the ones reported clipped: Apply, Save rack, etc.).
    let buttons = ["Power", "▶ Preview", "Apply", "Save rack", "Load rack", "Delete rack"];
    for btn in buttons {
        let matching: Vec<Rect> = shapes.iter().filter(|(t, _)| t == btn).map(|(_, r)| *r).collect();
        assert!(!matching.is_empty(), "button {btn} should be rendered in right dock");
        for r in matching {
            assert!(r.left() >= left_edge - 0.5, "button '{btn}' rect {:?} clipped by panel left edge {left_edge}", r);
            assert!(r.right() <= right_edge + 0.5, "button '{btn}' rect {:?} past panel right edge {right_edge}", r);
        }
    }
}

#[test]
fn right_dock_not_clipped_at_1280x800_handheld_window() {
    let scratch = Scratch::new("800p");
    let mut app = a_window(&scratch);

    let (right_zone, tabs, shapes) = layout_frame(&mut app, 1280.0, 800.0);

    let left_edge = right_zone.left();
    let right_edge = right_zone.right();

    assert!(right_zone.width() > 240.0, "right zone width should expand to fit content, got {}", right_zone.width());

    for (id, rect) in &tabs {
        assert!(rect.left() >= left_edge - 0.5, "tab {id} rect {:?} clipped by right panel left edge {left_edge}", rect);
        assert!(rect.right() <= right_edge + 0.5, "tab {id} rect {:?} past right edge {right_edge}", rect);
    }

    let buttons = ["Power", "▶ Preview", "Apply", "Save rack", "Load rack", "Delete rack"];
    for btn in buttons {
        let matching: Vec<Rect> = shapes.iter().filter(|(t, _)| t == btn).map(|(_, r)| *r).collect();
        assert!(!matching.is_empty(), "button {btn} should be rendered in right dock");
        for r in matching {
            assert!(r.left() >= left_edge - 0.5, "button '{btn}' rect {:?} clipped by panel left edge {left_edge}", r);
            assert!(r.right() <= right_edge + 0.5, "button '{btn}' rect {:?} past panel right edge {right_edge}", r);
        }
    }
}

#[test]
fn right_dock_not_clipped_with_document_open() {
    let scratch = Scratch::new("doc-open");
    let mut app = a_window(&scratch);

    let ctx = app.ctx.clone();
    let state = newdoc::empty(&app.new_doc_options(), 44_100, 1, SampleFormat::Int16).expect("an empty document");
    app.add_document(state, &ctx);

    let (right_zone, tabs, shapes) = layout_frame(&mut app, 1280.0, 720.0);

    let left_edge = right_zone.left();
    let right_edge = right_zone.right();

    for (id, rect) in &tabs {
        assert!(rect.left() >= left_edge - 0.5, "tab {id} rect {:?} clipped by right panel left edge {left_edge}", rect);
        assert!(rect.right() <= right_edge + 0.5, "tab {id} rect {:?} past right edge {right_edge}", rect);
    }

    let buttons = ["Power", "▶ Preview", "Apply", "Save rack", "Load rack", "Delete rack"];
    for btn in buttons {
        let matching: Vec<Rect> = shapes.iter().filter(|(t, _)| t == btn).map(|(_, r)| *r).collect();
        assert!(!matching.is_empty(), "button {btn} should be rendered in right dock");
        for r in matching {
            assert!(r.left() >= left_edge - 0.5, "button '{btn}' rect {:?} clipped by panel left edge {left_edge}", r);
        }
    }
}
