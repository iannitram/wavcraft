//! Piecewise-linear fade envelope implementation.

pub fn piecewise_linear(points: &[(f32, f32)], times: &[f32]) -> Vec<f32> {
    if points.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::with_capacity(times.len());

    for &t in times {
        if t <= points[0].0 {
            // Clamp low
            result.push(points[0].1);
        } else if t >= points[points.len() - 1].0 {
            // Clamp high
            result.push(points[points.len() - 1].1);
        } else {
            // Find the segment and interpolate
            let mut found = false;
            for i in 0..points.len() - 1 {
                let (t0, g0) = points[i];
                let (t1, g1) = points[i + 1];

                if t0 <= t && t <= t1 {
                    // Handle the case where t1 == t0 (avoid division by zero)
                    if t1 == t0 {
                        result.push(g1);
                    } else {
                        let gain = g0 + (t - t0) / (t1 - t0) * (g1 - g0);
                        result.push(gain);
                    }
                    found = true;
                    break;
                }
            }

            // This shouldn't happen due to the earlier clamping, but just in case
            if !found {
                result.push(points[points.len() - 1].1);
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn piecewise_linear_ramp_is_exact() {
        let points = [(0.0, 0.0), (1.0, 1.0)];
        let times = [0.0, 0.25, 0.5, 0.75, 1.0];
        let expected = [0.0, 0.25, 0.5, 0.75, 1.0];
        let result = piecewise_linear(&points, &times);
        assert_eq!(result, expected);
    }

    #[test]
    fn piecewise_linear_multi_segment_shape() {
        let points = [(0.0, 0.0), (0.5, 1.0), (1.0, 0.2)];
        let times = [-0.1, 0.25, 0.5, 0.75, 1.0, 1.5];
        let expected = [0.0, 0.5, 1.0, 0.6, 0.2, 0.2];
        let result = piecewise_linear(&points, &times);
        assert_eq!(result.len(), expected.len());
        for (a, b) in result.iter().zip(expected.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn piecewise_linear_mid_ramp_scaled() {
        let points = [(0.0, 0.0), (2.0, 0.5)];
        let times = [0.5, 1.0, 3.0];
        let expected = [0.125, 0.25, 0.5];
        let result = piecewise_linear(&points, &times);
        assert_eq!(result, expected);
    }

    #[test]
    fn piecewise_linear_empty_points_returns_empty() {
        let points: [(f32, f32); 0] = [];
        let times = [0.0, 0.5];
        let result = piecewise_linear(&points, &times);
        assert!(result.is_empty());
    }
}
