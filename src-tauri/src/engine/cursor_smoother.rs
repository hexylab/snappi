use super::spring::{Spring, SpringHalfLife};

pub struct CursorSmoother {
    spring_x: Spring,
    spring_y: Spring,
}

impl CursorSmoother {
    pub fn new() -> Self {
        Self {
            spring_x: Spring::new(0.0),
            spring_y: Spring::new(0.0),
        }
    }

    /// Smooth raw mouse positions using critically damped spring physics.
    /// Uses actual timestamps for frame-rate independent smoothing.
    /// Input: Vec of (timestamp_ms, x, y)
    /// Output: Vec of (timestamp_ms, smoothed_x, smoothed_y)
    pub fn smooth(&mut self, raw_positions: &[(u64, f64, f64)]) -> Vec<(u64, f64, f64)> {
        if raw_positions.is_empty() {
            return Vec::new();
        }

        let mut result = Vec::with_capacity(raw_positions.len());
        let half_life = SpringHalfLife::CURSOR_SMOOTHING;

        // Initialize to first position
        self.spring_x.snap(raw_positions[0].1);
        self.spring_y.snap(raw_positions[0].2);

        for (i, &(t, raw_x, raw_y)) in raw_positions.iter().enumerate() {
            let dt = if i > 0 {
                (t.saturating_sub(raw_positions[i - 1].0)) as f64 / 1000.0
            } else {
                0.0
            };

            self.spring_x.set_target(raw_x);
            self.spring_y.set_target(raw_y);
            self.spring_x.update(half_life, dt);
            self.spring_y.update(half_life, dt);
            result.push((t, self.spring_x.position, self.spring_y.position));
        }

        result
    }
}

impl Default for CursorSmoother {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smooth_empty() {
        let mut smoother = CursorSmoother::new();
        assert!(smoother.smooth(&[]).is_empty());
    }

    #[test]
    fn test_smooth_single_point() {
        let mut smoother = CursorSmoother::new();
        let result = smoother.smooth(&[(0, 100.0, 200.0)]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], (0, 100.0, 200.0));
    }

    #[test]
    fn test_smooth_reduces_jitter() {
        let mut smoother = CursorSmoother::new();
        // Simulate jittery input
        let raw = vec![
            (0, 100.0, 100.0),
            (10, 102.0, 98.0),
            (20, 99.0, 101.0),
            (30, 101.0, 99.0),
            (40, 100.0, 100.0),
        ];
        let smoothed = smoother.smooth(&raw);
        assert_eq!(smoothed.len(), 5);

        // Smoothed positions should be closer to 100,100 center
        for &(_, x, y) in &smoothed[1..] {
            assert!((x - 100.0).abs() < 5.0);
            assert!((y - 100.0).abs() < 5.0);
        }
    }

    #[test]
    fn test_smooth_uses_real_timestamps() {
        let mut smoother1 = CursorSmoother::new();
        let mut smoother2 = CursorSmoother::new();

        // Same positions but different timestamps
        let fast = vec![(0, 0.0, 0.0), (10, 100.0, 0.0)];
        let slow = vec![(0, 0.0, 0.0), (100, 100.0, 0.0)];

        let result_fast = smoother1.smooth(&fast);
        let result_slow = smoother2.smooth(&slow);

        // Slow timing should result in smoother (closer to target) position
        // since dt is larger and the spring has more time to converge
        let fast_dist = (result_fast[1].1 - 100.0).abs();
        let slow_dist = (result_slow[1].1 - 100.0).abs();
        assert!(slow_dist < fast_dist,
            "Slow timing should converge more: fast_dist={}, slow_dist={}", fast_dist, slow_dist);
    }
}
