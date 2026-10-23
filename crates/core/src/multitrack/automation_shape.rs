//! Segment shapes for automation (roadmap C179): how the value moves between two neighbouring
//! points of a lane. SoundCraft's `AutomationPoint` has no shape field (the model is pinned and
//! interpolates linearly, only mute lanes step), so the shapes live beside the lane in
//! [`SegmentShapes`], one per segment, and a segment that was never set is `Linear`. Nothing here
//! changes `automation_value_at` (playback still ramps linearly): a later pass reads these.

use soundcraft_model::AutomationLane;

use super::{MtError, err};

/// How the value moves from a point to the next one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SegmentShape {
    /// A straight ramp from the first point's value to the next one's.
    #[default]
    Linear,
    /// The first point's value for the first half of the segment, then the next point's value.
    Step,
    /// The first point's value until the next point is reached, then the next point's value.
    Hold,
}

/// The shape of each segment of one lane. Segment `i` starts at point `i`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SegmentShapes {
    shapes: Vec<SegmentShape>,
}

impl SegmentShapes {
    /// No shapes set: every segment is linear.
    pub fn new() -> Self {
        Self::default()
    }

    /// The shape of segment `index`; `Linear` when it was never set (or is past the end).
    pub fn get(&self, index: usize) -> SegmentShape {
        self.shapes.get(index).copied().unwrap_or(SegmentShape::Linear)
    }
}

/// The value at time `t` on the segment from (`a_at`, `a_value`) to (`b_at`, `b_value`), times in
/// any one unit. `t` is clamped into the segment: before it gives `a_value`, after it `b_value`.
/// A segment of zero length gives `a_value` (as the model's lane does). A NaN or infinite time or
/// value, or a segment that ends before it starts, is refused.
pub fn shape_value(shape: SegmentShape, a_at: f64, a_value: f32, b_at: f64, b_value: f32, t: f64) -> Result<f32, MtError> {
    if !a_at.is_finite() || !b_at.is_finite() || !t.is_finite() {
        return err("a time must be a number");
    }
    if !a_value.is_finite() || !b_value.is_finite() {
        return err("a value must be a number");
    }
    if b_at < a_at {
        return err("a segment must not end before it starts");
    }
    if b_at == a_at {
        return Ok(a_value);
    }
    let frac = (t.clamp(a_at, b_at) - a_at) / (b_at - a_at);
    match shape {
        SegmentShape::Linear => {
            let (a, b) = (f64::from(a_value), f64::from(b_value));
            Ok((a + (b - a) * frac) as f32)
        }
        SegmentShape::Step => Ok(if frac < 0.5 { a_value } else { b_value }),
        SegmentShape::Hold => Ok(if frac < 1.0 { a_value } else { b_value }),
    }
}

/// Set the shape of the segment that starts at point `index` of `lane`. The lane must have a next
/// point: with `n` points the segments are `0` to `n - 2`, anything else is refused and nothing
/// changes.
pub fn set_segment_shape(lane: &AutomationLane, shapes: &mut SegmentShapes, index: usize, shape: SegmentShape) -> Result<(), MtError> {
    let n = lane.points.len();
    if n < 2 {
        return err(format!("a lane needs 2 points before a segment has a shape: it has {n}"));
    }
    if index >= n - 1 {
        return err(format!("no segment {index}: this lane's segments are 0 to {}", n - 2));
    }
    if shapes.shapes.len() <= index {
        shapes.shapes.resize(index + 1, SegmentShape::Linear);
    }
    if let Some(slot) = shapes.shapes.get_mut(index) {
        *slot = shape;
    }
    Ok(())
}
