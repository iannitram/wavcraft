use wavcraft_core::dsp::eq_graph::{GraphView, nearest_node, node_drag};

fn view() -> GraphView {
    GraphView { f_lo: 20.0, f_hi: 20000.0, db_range: 18.0, width_px: 800.0, height_px: 300.0 }
}

fn near(actual: f32, expected: f32, tol: f32) {
    assert!((actual - expected).abs() <= tol, "{actual} != {expected}");
}

// Pixel positions used below: (20 Hz, +18 dB) is (0, 0); (20 Hz, -18 dB) is (0, 300);
// (20000 Hz, 0 dB) is (800, 150).
const TOP_LEFT: (f32, f32) = (20.0, 18.0);
const BOTTOM_LEFT: (f32, f32) = (20.0, -18.0);
const RIGHT_MIDDLE: (f32, f32) = (20000.0, 0.0);

#[test]
fn axis_ends_map_to_the_view_edges() {
    let v = view();
    near(v.x_of_freq(20.0), 0.0, 1e-3);
    near(v.x_of_freq(20000.0), 800.0, 1e-3);
    near(v.freq_at_x(0.0), 20.0, 1e-3);
    near(v.freq_at_x(800.0), 20000.0, 1e-1);
}

#[test]
fn axis_centre_is_the_geometric_mean() {
    let v = view();
    near(v.freq_at_x(400.0), 632.4555, 0.07);
    near(v.x_of_freq(632.4555), 400.0, 1e-2);
}

#[test]
fn frequency_round_trip_stays_within_one_hundredth_percent() {
    let v = view();
    for freq in [20.0_f32, 55.0, 110.0, 440.0, 1000.0, 3520.0, 12000.0, 20000.0] {
        let back = v.freq_at_x(v.x_of_freq(freq));
        assert!((back - freq).abs() / freq <= 1e-4, "{freq} came back as {back}");
    }
}

#[test]
fn octaves_have_equal_pixel_width() {
    let v = view();
    near(v.x_of_freq(40.0), 80.27467, 1e-3);
    near(v.x_of_freq(80.0), 160.54933, 1e-3);
    near(v.x_of_freq(1000.0), 453.05867, 1e-3);
    near(v.freq_at_x(100.0), 47.427474, 1e-2);
    near(v.freq_at_x(200.0), 112.46827, 1e-2);
}

#[test]
fn frequency_outside_the_view_clamps_to_the_edges() {
    let v = view();
    near(v.x_of_freq(5.0), 0.0, 1e-3);
    near(v.x_of_freq(30000.0), 800.0, 1e-3);
    near(v.x_of_freq(f32::INFINITY), 800.0, 1e-3);
    near(v.freq_at_x(-10.0), 20.0, 1e-3);
    near(v.freq_at_x(900.0), 20000.0, 1e-1);
}

#[test]
fn gain_axis_maps_top_bottom_and_centre() {
    let v = view();
    near(v.y_of_gain(18.0), 0.0, 1e-3);
    near(v.y_of_gain(-18.0), 300.0, 1e-3);
    near(v.y_of_gain(0.0), 150.0, 1e-3);
    near(v.y_of_gain(4.5), 112.5, 1e-3);
    near(v.y_of_gain(-12.0), 250.0, 1e-3);
    near(v.gain_at_y(0.0), 18.0, 1e-3);
    near(v.gain_at_y(300.0), -18.0, 1e-3);
    near(v.gain_at_y(150.0), 0.0, 1e-3);
    near(v.gain_at_y(100.0), 6.0, 1e-3);
    near(v.gain_at_y(225.0), -9.0, 1e-3);
}

#[test]
fn gain_outside_the_range_clamps_to_the_edges() {
    let v = view();
    near(v.y_of_gain(30.0), 0.0, 1e-3);
    near(v.y_of_gain(-30.0), 300.0, 1e-3);
    near(v.gain_at_y(-5.0), 18.0, 1e-3);
    near(v.gain_at_y(400.0), -18.0, 1e-3);
}

#[test]
fn gain_round_trip_stays_within_a_tolerance() {
    let v = view();
    for gain in [-18.0_f32, -12.5, -3.25, 0.0, 4.75, 18.0] {
        near(v.gain_at_y(v.y_of_gain(gain)), gain, 1e-4);
    }
}

#[test]
fn nan_inputs_map_to_the_axis_start_and_the_centre() {
    let v = view();
    near(v.x_of_freq(f32::NAN), 0.0, 1e-3);
    near(v.freq_at_x(f32::NAN), 20.0, 1e-3);
    near(v.y_of_gain(f32::NAN), 150.0, 1e-3);
    near(v.gain_at_y(f32::NAN), 0.0, 1e-3);
}

#[test]
fn node_drag_follows_the_pixel_and_clamps_to_the_view() {
    let v = view();
    let (freq, gain) = node_drag(&v, 400.0, 75.0);
    near(freq, 632.4555, 0.07);
    near(gain, 9.0, 1e-3);
    let (freq, gain) = node_drag(&v, -50.0, -50.0);
    near(freq, 20.0, 1e-3);
    near(gain, 18.0, 1e-3);
    let (freq, gain) = node_drag(&v, 5000.0, 5000.0);
    near(freq, 20000.0, 1e-1);
    near(gain, -18.0, 1e-3);
}

#[test]
fn nearest_node_picks_the_closest_node() {
    let v = view();
    // Click (0, 200): 200 px from the top-left node, 100 px from the bottom-left node.
    assert_eq!(nearest_node(&v, &[TOP_LEFT, BOTTOM_LEFT, RIGHT_MIDDLE], 0.0, 200.0, 250.0), Some(1));
    assert_eq!(nearest_node(&v, &[BOTTOM_LEFT, TOP_LEFT, RIGHT_MIDDLE], 0.0, 200.0, 250.0), Some(0));
    // Click (790, 140): 14.1 px from the right-middle node.
    assert_eq!(nearest_node(&v, &[TOP_LEFT, BOTTOM_LEFT, RIGHT_MIDDLE], 790.0, 140.0, 20.0), Some(2));
}

#[test]
fn nearest_node_first_node_wins_a_tie() {
    let v = view();
    assert_eq!(nearest_node(&v, &[TOP_LEFT, TOP_LEFT, BOTTOM_LEFT], 0.0, 0.0, 10.0), Some(0));
}

#[test]
fn nearest_node_is_none_outside_the_radius() {
    let v = view();
    assert_eq!(nearest_node(&v, &[TOP_LEFT, BOTTOM_LEFT, RIGHT_MIDDLE], 400.0, 150.0, 100.0), None);
    assert_eq!(nearest_node(&v, &[], 0.0, 0.0, 100.0), None);
}

#[test]
fn nearest_node_radius_edge_is_inclusive() {
    let v = view();
    // Click (3, 4) is exactly 5 px from the top-left node at (0, 0).
    assert_eq!(nearest_node(&v, &[TOP_LEFT], 3.0, 4.0, 5.0), Some(0));
    assert_eq!(nearest_node(&v, &[TOP_LEFT], 3.0, 4.0, 4.9), None);
    assert_eq!(nearest_node(&v, &[TOP_LEFT], 0.0, 0.0, 0.0), Some(0));
}

#[test]
fn nearest_node_skips_non_finite_nodes() {
    let v = view();
    // A NaN frequency would land at x = 0; the click at (0, 150) must not pick it.
    let nodes = [(f32::NAN, 0.0), (20000.0, f32::NAN), RIGHT_MIDDLE];
    assert_eq!(nearest_node(&v, &nodes, 0.0, 150.0, 1000.0), Some(2));
}

#[test]
fn nearest_node_bad_click_or_radius_is_none() {
    let v = view();
    assert_eq!(nearest_node(&v, &[TOP_LEFT], f32::NAN, 0.0, 10.0), None);
    assert_eq!(nearest_node(&v, &[TOP_LEFT], 0.0, f32::INFINITY, 10.0), None);
    assert_eq!(nearest_node(&v, &[TOP_LEFT], 0.0, 0.0, f32::NAN), None);
    assert_eq!(nearest_node(&v, &[TOP_LEFT], 0.0, 0.0, -1.0), None);
}

#[test]
fn zero_size_view_is_inert() {
    let v = GraphView { f_lo: 20.0, f_hi: 20000.0, db_range: 18.0, width_px: 0.0, height_px: 0.0 };
    near(v.x_of_freq(1000.0), 0.0, 1e-3);
    near(v.freq_at_x(100.0), 0.0, 1e-3);
    near(v.y_of_gain(6.0), 0.0, 1e-3);
    near(v.gain_at_y(100.0), 0.0, 1e-3);
    assert_eq!(node_drag(&v, 10.0, 10.0), (0.0, 0.0));
    assert_eq!(nearest_node(&v, &[TOP_LEFT], 0.0, 0.0, 100.0), None);
}
