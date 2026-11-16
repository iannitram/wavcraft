//! The Noise Reduction dialog's extra section (G2.6): a graph of the noise print and its
//! threshold curve, and a Select Entire File button, drawn under the generic controls.

use egui::{Align2, FontId, Sense, Shape, Stroke, pos2, vec2};
use wavcraft_core::effects::threshold_curve_db;
use wavcraft_core::noise::NoisePrint;
use wavcraft_core::view::Selection;

use crate::theme;

/// The graph's size in points.
const GRAPH_W: f32 = 360.0;
const GRAPH_H: f32 = 140.0;
/// The graph's level range in dB, and its lowest frequency in Hz.
const TOP_DB: f64 = 0.0;
const BOTTOM_DB: f64 = -120.0;
const LOW_HZ: f64 = 20.0;

/// The two lines of the graph, one value per FFT bin each (bin 0 is DC).
#[derive(Debug, Clone, PartialEq)]
pub struct GraphData {
    pub sample_rate: u32,
    pub print_db: Vec<f64>,
    pub threshold_db: Vec<f64>,
}

/// The lines for `print` at the FFT size in `params`; None without a usable print.
pub fn graph_data(print: Option<&NoisePrint>, params: &[f64]) -> Option<GraphData> {
    let print = print?;
    // `magnitudes` reads the print's channel; an empty print has none to read.
    if print.density.first().is_none_or(|d| d.is_empty()) {
        return None;
    }
    let threshold_db = threshold_curve_db(print, 0, params);
    if threshold_db.is_empty() {
        return None;
    }
    let n = (threshold_db.len() - 1) * 2;
    let print_db = print.magnitudes(0, n).iter().map(|&m| 20.0 * f64::from(m).max(1e-12).log10()).collect();
    Some(GraphData { sample_rate: print.sample_rate, print_db, threshold_db })
}

/// The selection that covers a document of `total_frames`.
pub fn select_entire_file(total_frames: usize) -> Selection {
    Selection::all(total_frames)
}

/// Where a value falls on the graph, as points from its left edge, or None if it cannot be drawn.
fn x_of(hz: f64, nyquist: f64) -> Option<f32> {
    if !hz.is_finite() || !nyquist.is_finite() || nyquist <= LOW_HZ || hz <= 0.0 {
        return None;
    }
    let t = ((hz.max(LOW_HZ) / LOW_HZ).ln() / (nyquist / LOW_HZ).ln()).clamp(0.0, 1.0);
    Some(t as f32 * GRAPH_W)
}

fn y_of(db: f64) -> f32 {
    let db = if db.is_finite() { db.clamp(BOTTOM_DB, TOP_DB) } else { BOTTOM_DB };
    ((TOP_DB - db) / (TOP_DB - BOTTOM_DB)) as f32 * GRAPH_H
}

/// The points of one line, bin 0 skipped, non-finite values dropped.
fn line_points(db: &[f64], sample_rate: u32) -> Vec<(f32, f32)> {
    let bins = db.len().saturating_sub(1);
    if bins == 0 {
        return Vec::new();
    }
    let nyquist = f64::from(sample_rate) / 2.0;
    db.iter()
        .enumerate()
        .skip(1)
        .filter(|(_, v)| v.is_finite())
        .filter_map(|(k, &v)| Some((x_of(nyquist * k as f64 / bins as f64, nyquist)?, y_of(v))))
        .collect()
}

/// Draw the graph (or the no-print note) and the Select Entire File button; true when the
/// button was clicked this frame.
pub fn section(ui: &mut egui::Ui, params: &[f64], print: Option<&NoisePrint>) -> bool {
    ui.add_space(6.0);
    match graph_data(print, params) {
        Some(data) => draw_graph(ui, &data),
        None => {
            ui.label("No noise print yet. Select some noise and use Capture Noise Print (Shift+P).");
        }
    }
    ui.add_space(4.0);
    ui.button("Select Entire File").clicked()
}

fn draw_graph(ui: &mut egui::Ui, data: &GraphData) {
    let (rect, _) = ui.allocate_exact_size(vec2(GRAPH_W, GRAPH_H), Sense::hover());
    let painter = ui.painter_at(rect);
    let pal = theme::pal();
    painter.rect_filled(rect, 0.0, pal.well);
    let at = |x: f32, y: f32| pos2(rect.left() + x, rect.top() + y);
    let font = FontId::monospace(9.0);
    for db in [-20.0, -40.0, -60.0, -80.0, -100.0] {
        let y = y_of(db);
        painter.line_segment([at(0.0, y), at(GRAPH_W, y)], Stroke::new(1.0, pal.grid));
        painter.text(at(2.0, y), Align2::LEFT_BOTTOM, format!("{db:.0}"), font.clone(), pal.text_dim);
    }
    let nyquist = f64::from(data.sample_rate) / 2.0;
    for hz in [100.0, 1000.0, 10_000.0] {
        if let Some(x) = x_of(hz, nyquist).filter(|_| hz < nyquist) {
            painter.line_segment([at(x, 0.0), at(x, GRAPH_H)], Stroke::new(1.0, pal.grid));
        }
    }
    let lines = [(&data.print_db, pal.text_dim), (&data.threshold_db, pal.accent)];
    for (db, color) in lines {
        let pts = line_points(db, data.sample_rate).into_iter().map(|(x, y)| at(x, y)).collect();
        painter.add(Shape::line(pts, Stroke::new(1.5, color)));
    }
    painter.rect_stroke(rect, 0.0, Stroke::new(1.0, pal.divider), egui::StrokeKind::Inside);
    ui.horizontal(|ui| {
        ui.colored_label(pal.text_dim, "Print");
        ui.colored_label(pal.accent, "Threshold");
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use wavcraft_core::effects::{NoiseReduction, Params};

    #[test]
    fn g2_dialog_has_graph_and_select_entire_file() {
        let print = NoisePrint { sample_rate: 48_000, density: vec![vec![0.001_f32; 2049]], frames: 10 };
        let mut params = Params::defaults(&NoiseReduction);
        let data = graph_data(Some(&print), &params).expect("graph");
        assert_eq!(data.print_db.len(), 2049);
        assert_eq!(data.threshold_db.len(), 2049);
        assert_eq!(data.threshold_db, threshold_curve_db(&print, 0, &params));
        let margin = 20.0 * 3.5_f64.log10();
        for (t, p) in data.threshold_db.iter().zip(&data.print_db) {
            assert!((t - p - margin).abs() < 0.01);
        }
        assert!((data.print_db[10] - -28.1).abs() < 0.1);

        params[5] = 0.0;
        let small = graph_data(Some(&print), &params).expect("graph");
        assert_eq!((small.print_db.len(), small.threshold_db.len()), (257, 257));
        params[5] = 5.0;
        let big = graph_data(Some(&print), &params).expect("graph");
        assert_eq!((big.print_db.len(), big.threshold_db.len()), (8193, 8193));

        assert!(graph_data(None, &params).is_none());
        let empty = NoisePrint { sample_rate: 48_000, density: Vec::new(), frames: 0 };
        assert!(graph_data(Some(&empty), &params).is_none());

        let all = select_entire_file(48_000);
        assert_eq!((all.start(), all.end(), all.len()), (0, 48_000, 48_000));
        assert!(select_entire_file(0).is_empty());
    }
}
