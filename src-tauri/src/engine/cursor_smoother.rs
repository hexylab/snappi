use super::spring::SpringAnimation;

pub struct CursorSmoother {
    spring_x: SpringAnimation,
    spring_y: SpringAnimation,
}

impl CursorSmoother {
    pub fn new() -> Self {
        Self {
            spring_x: SpringAnimation::new(300.0, 30.0, 1.0),
            spring_y: SpringAnimation::new(300.0, 30.0, 1.0),
        }
    }

    /// Smooth raw mouse positions using spring physics
    /// Input: Vec of (timestamp_ms, x, y)
    /// Output: Vec of (timestamp_ms, smoothed_x, smoothed_y)
    pub fn smooth(&mut self, raw_positions: &[(u64, f64, f64)]) -> Vec<(u64, f64, f64)> {
        if raw_positions.is_empty() {
            return Vec::new();
        }

        let mut result = Vec::with_capacity(raw_positions.len());
        let dt = 1.0 / 60.0;

        // Initialize to first position
        self.spring_x.snap_to(raw_positions[0].1);
        self.spring_y.snap_to(raw_positions[0].2);

        for &(t, raw_x, raw_y) in raw_positions {
            self.spring_x.target = raw_x;
            self.spring_y.target = raw_y;
            let smooth_x = self.spring_x.update(dt);
            let smooth_y = self.spring_y.update(dt);
            result.push((t, smooth_x, smooth_y));
        }

        result
    }
}

impl Default for CursorSmoother {
    fn default() -> Self {
        Self::new()
    }
}
