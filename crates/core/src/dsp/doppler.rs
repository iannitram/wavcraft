//! Doppler shifter, sub-job a (roadmap B089): the path model. A source moves in a straight line
//! past a listener. This module turns that move into per-sample lists that a delay line can use:
//! the distance, the propagation delay and the distance gain. No audio is touched here.
//!
//! Coordinates are metres on a flat plane as `(x, y)`. Every calculation runs in f64 and is cast
//! to f32 once, at the end.

/// Speed of sound in air, metres per second.
pub const SPEED_OF_SOUND: f32 = 343.0;

/// A straight move of a source past a fixed listener.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Path {
    /// Where the source is at sample 0.
    pub start: (f32, f32),
    /// Where the source stops; it stays there once reached.
    pub end: (f32, f32),
    /// Where the listener stands.
    pub listener: (f32, f32),
    /// How fast the source moves from `start` towards `end`, metres per second.
    pub speed_m_s: f32,
}

/// The distance in metres from the listener to the source at each of `frames` samples.
///
/// Sample `i` is at `i / rate` seconds. The source does not move when the speed is not above 0
/// or not finite, when `rate` is not finite or not above 0, or when `start == end`. When a
/// coordinate is not finite the result is `frames` zeros.
pub fn distances(path: &Path, frames: usize, rate: f32) -> Vec<f32> {
    let coords = [path.start.0, path.start.1, path.end.0, path.end.1, path.listener.0, path.listener.1];
    if coords.iter().any(|c| !c.is_finite()) {
        return vec![0.0; frames];
    }
    let (sx, sy) = (f64::from(path.start.0), f64::from(path.start.1));
    let (ex, ey) = (f64::from(path.end.0), f64::from(path.end.1));
    let (lx, ly) = (f64::from(path.listener.0), f64::from(path.listener.1));
    let (dx, dy) = (ex - sx, ey - sy);
    let length = dx.hypot(dy);
    let moving = length > 0.0 && path.speed_m_s.is_finite() && path.speed_m_s > 0.0 && rate.is_finite() && rate > 0.0;
    (0..frames)
        .map(|index| {
            let fraction = if moving { (f64::from(path.speed_m_s) * index as f64 / f64::from(rate) / length).min(1.0) } else { 0.0 };
            (sx + dx * fraction - lx).hypot(sy + dy * fraction - ly) as f32
        })
        .collect()
}

/// The propagation delay in samples for each distance: `distance / c * rate`.
///
/// An element is 0.0 when it is not finite or not above 0. Every element is 0.0 when `rate` or
/// `c` is not finite or not above 0.
pub fn delays_samples(distances: &[f32], rate: f32, c: f32) -> Vec<f32> {
    let valid = rate.is_finite() && rate > 0.0 && c.is_finite() && c > 0.0;
    distances.iter().map(|&d| if valid && d.is_finite() && d > 0.0 { (f64::from(d) / f64::from(c) * f64::from(rate)) as f32 } else { 0.0 }).collect()
}

/// The gain for each distance: 1.0 up to `ref_m`, then `ref_m / distance` (1/r).
///
/// Every element is 1.0 when `ref_m` is not finite or not above 0. An element that is not finite
/// is 1.0.
pub fn gains(distances: &[f32], ref_m: f32) -> Vec<f32> {
    let valid = ref_m.is_finite() && ref_m > 0.0;
    distances.iter().map(|&d| if valid && d.is_finite() && d > ref_m { (f64::from(ref_m) / f64::from(d)) as f32 } else { 1.0 }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) {
        assert!((a - b).abs() <= 1e-4, "{a} != {b}");
    }

    fn fly_by() -> Path {
        Path { start: (-10.0, 3.0), end: (10.0, 3.0), listener: (1.0, -1.0), speed_m_s: 10.0 }
    }

    #[test]
    fn fly_by_closest_approach_is_the_minimum() {
        let d = distances(&fly_by(), 201, 100.0);
        assert_eq!(d.len(), 201);
        approx(d[0], 11.7047);
        approx(d[50], 7.2111);
        approx(d[110], 4.0);
        approx(d[150], 5.65685);
        approx(d[200], 9.84886);
        approx(d.iter().copied().fold(f32::INFINITY, f32::min), 4.0);
    }

    #[test]
    fn path_stops_at_the_end() {
        let d = distances(&fly_by(), 301, 100.0);
        approx(d[200], 9.84886);
        approx(d[250], 9.84886);
        approx(d[300], 9.84886);
    }

    #[test]
    fn zero_negative_or_infinite_speed_and_a_zero_length_path_give_a_constant() {
        let still = Path { speed_m_s: 0.0, ..fly_by() };
        let backwards = Path { speed_m_s: -3.0, ..fly_by() };
        let infinite = Path { speed_m_s: f32::INFINITY, ..fly_by() };
        let nan = Path { speed_m_s: f32::NAN, ..fly_by() };
        let no_move = Path { end: (-10.0, 3.0), ..fly_by() };
        for path in [still, backwards, infinite, nan, no_move] {
            for value in distances(&path, 5, 100.0) {
                approx(value, 11.7047);
            }
        }
    }

    #[test]
    fn a_bad_rate_gives_a_constant() {
        for rate in [0.0, -100.0, f32::NAN, f32::INFINITY] {
            for value in distances(&fly_by(), 5, rate) {
                approx(value, 11.7047);
            }
        }
    }

    #[test]
    fn non_finite_coordinates_give_zero_distances() {
        let bad = Path { listener: (f32::NAN, 0.0), ..fly_by() };
        assert_eq!(distances(&bad, 3, 100.0), vec![0.0, 0.0, 0.0]);
        let bad = Path { start: (0.0, f32::INFINITY), ..fly_by() };
        assert_eq!(distances(&bad, 2, 100.0), vec![0.0, 0.0]);
    }

    #[test]
    fn zero_frames_give_empty_lists() {
        assert!(distances(&fly_by(), 0, 100.0).is_empty());
        assert!(delays_samples(&[], 48_000.0, SPEED_OF_SOUND).is_empty());
        assert!(gains(&[], 1.0).is_empty());
    }

    #[test]
    fn delay_is_distance_over_c_times_rate() {
        let delays = delays_samples(&[343.0, 0.0, 171.5], 48_000.0, SPEED_OF_SOUND);
        assert_eq!(delays.len(), 3);
        approx(delays[0], 48_000.0);
        approx(delays[1], 0.0);
        approx(delays[2], 24_000.0);
    }

    #[test]
    fn delays_with_bad_inputs_are_zero() {
        for rate in [0.0, f32::NAN] {
            assert_eq!(delays_samples(&[343.0], rate, SPEED_OF_SOUND), vec![0.0]);
        }
        for c in [0.0, f32::NAN] {
            assert_eq!(delays_samples(&[343.0], 48_000.0, c), vec![0.0]);
        }
        assert_eq!(delays_samples(&[f32::NAN, -5.0, f32::INFINITY], 48_000.0, SPEED_OF_SOUND), vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn gain_is_one_at_the_reference_and_half_at_twice_the_distance() {
        let g = gains(&[1.0, 2.0, 4.0, 8.0], 2.0);
        assert_eq!(g.len(), 4);
        approx(g[0], 1.0);
        approx(g[1], 1.0);
        approx(g[2], 0.5);
        approx(g[3], 0.25);
    }

    #[test]
    fn gain_inside_the_reference_is_capped_at_one() {
        for value in gains(&[0.5, 0.0, 1.9], 2.0) {
            approx(value, 1.0);
        }
    }

    #[test]
    fn gains_with_bad_inputs_are_one() {
        for ref_m in [0.0, -1.0, f32::NAN] {
            assert_eq!(gains(&[10.0], ref_m), vec![1.0]);
        }
        assert_eq!(gains(&[f32::NAN, f32::INFINITY], 2.0), vec![1.0, 1.0]);
    }
}
