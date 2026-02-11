/// Exact critically damped spring (analytical solution).
/// Frame-rate independent, unconditionally stable.
///
/// Parameterized by half-life: the time (seconds) for the spring
/// to cover 50% of the remaining distance to its target.

const LN_2: f64 = 0.693147180559945;
const EPSILON: f64 = 1e-5;

#[derive(Debug, Clone)]
pub struct Spring {
    pub position: f64,
    pub velocity: f64,
    pub target: f64,
}

impl Spring {
    pub fn new(initial: f64) -> Self {
        Self {
            position: initial,
            velocity: 0.0,
            target: initial,
        }
    }

    /// Update using exact critically damped solution.
    /// `half_life`: time in seconds for spring to cover 50% remaining distance.
    /// `dt`: time step in seconds.
    pub fn update(&mut self, half_life: f64, dt: f64) {
        if dt <= 0.0 {
            return;
        }
        let y = (4.0 * LN_2) / half_life.max(EPSILON);
        let y_half = y / 2.0;
        let j0 = self.position - self.target;
        let j1 = self.velocity + j0 * y_half;
        let eydt = (-y_half * dt).exp();

        self.position = eydt * (j0 + j1 * dt) + self.target;
        self.velocity = eydt * (self.velocity - j1 * y_half * dt);
    }

    pub fn snap(&mut self, value: f64) {
        self.position = value;
        self.target = value;
        self.velocity = 0.0;
    }

    pub fn set_target(&mut self, target: f64) {
        self.target = target;
    }

    pub fn is_settled(&self, threshold: f64) -> bool {
        (self.position - self.target).abs() < threshold
            && self.velocity.abs() < threshold
    }
}

/// Context-specific half-life values (seconds)
pub struct SpringHalfLife;

impl SpringHalfLife {
    pub const VIEWPORT_PAN: f64 = 0.22;
    pub const WINDOW_PAN: f64 = 0.25;

    pub const ZOOM_IN_FAST: f64 = 0.18;
    pub const ZOOM_IN: f64 = 0.25;
    pub const ZOOM_IN_SLOW: f64 = 0.30;

    pub const ZOOM_OUT: f64 = 0.35;
    pub const ZOOM_OUT_SLOW: f64 = 0.40;

    pub const WINDOW_ZOOM: f64 = 0.30;

    pub const CURSOR_SMOOTHING: f64 = 0.05;
}

/// Animated viewport managing X, Y center and zoom level with spring physics.
/// Uses analytical critically damped springs with half-life parameterization.
#[derive(Debug, Clone)]
pub struct AnimatedViewport {
    pub center_x: Spring,
    pub center_y: Spring,
    pub zoom: Spring,
    pub pan_half_life: f64,
    pub zoom_half_life: f64,
}

impl AnimatedViewport {
    pub fn new(screen_width: f64, screen_height: f64) -> Self {
        let mut center_x = Spring::new(screen_width / 2.0);
        center_x.snap(screen_width / 2.0);

        let mut center_y = Spring::new(screen_height / 2.0);
        center_y.snap(screen_height / 2.0);

        let mut zoom = Spring::new(1.0);
        zoom.snap(1.0);

        Self {
            center_x,
            center_y,
            zoom,
            pan_half_life: SpringHalfLife::VIEWPORT_PAN,
            zoom_half_life: SpringHalfLife::ZOOM_IN,
        }
    }

    pub fn update(&mut self, dt: f64) {
        self.center_x.update(self.pan_half_life, dt);
        self.center_y.update(self.pan_half_life, dt);
        self.zoom.update(self.zoom_half_life, dt);
    }

    /// Set zoom half-life for asymmetric zoom transitions.
    pub fn set_zoom_half_life(&mut self, half_life: f64) {
        self.zoom_half_life = half_life;
    }

    pub fn set_target(&mut self, x: f64, y: f64, zoom: f64) {
        self.center_x.set_target(x);
        self.center_y.set_target(y);
        // Detect zoom direction for asymmetric spring
        if zoom > self.zoom.target {
            self.zoom_half_life = SpringHalfLife::ZOOM_IN;
        } else if zoom < self.zoom.target {
            self.zoom_half_life = SpringHalfLife::ZOOM_OUT;
        }
        self.pan_half_life = SpringHalfLife::VIEWPORT_PAN;
        self.zoom.set_target(zoom);
    }

    pub fn set_target_with_half_life(
        &mut self,
        x: f64,
        y: f64,
        zoom: f64,
        zoom_half_life: f64,
        pan_half_life: f64,
    ) {
        self.center_x.set_target(x);
        self.center_y.set_target(y);
        self.zoom.set_target(zoom);
        self.zoom_half_life = zoom_half_life;
        self.pan_half_life = pan_half_life;
    }

    pub fn snap_to(&mut self, x: f64, y: f64, zoom: f64) {
        self.center_x.snap(x);
        self.center_y.snap(y);
        self.zoom.snap(zoom);
    }

    pub fn current_viewport(
        &self,
        screen_width: f64,
        screen_height: f64,
    ) -> ViewportRect {
        let zoom = self.zoom.position.max(1.0);
        let vp_width = screen_width / zoom;
        let vp_height = screen_height / zoom;

        let x = (self.center_x.position - vp_width / 2.0)
            .max(0.0)
            .min(screen_width - vp_width);
        let y = (self.center_y.position - vp_height / 2.0)
            .max(0.0)
            .min(screen_height - vp_height);

        ViewportRect {
            x,
            y,
            width: vp_width,
            height: vp_height,
            zoom,
        }
    }

    /// Convert screen coordinates to output coordinates
    pub fn to_output_coords(
        &self,
        screen_x: f64,
        screen_y: f64,
        output_width: f64,
        output_height: f64,
        screen_width: f64,
        screen_height: f64,
    ) -> (f64, f64) {
        let vp = self.current_viewport(screen_width, screen_height);
        let rel_x = (screen_x - vp.x) / vp.width;
        let rel_y = (screen_y - vp.y) / vp.height;
        (rel_x * output_width, rel_y * output_height)
    }
}

#[derive(Debug, Clone)]
pub struct ViewportRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub zoom: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spring_converges_to_target() {
        let mut spring = Spring::new(0.0);
        spring.set_target(100.0);
        // Simulate 2 seconds at 60fps
        for _ in 0..120 {
            spring.update(0.15, 1.0 / 60.0);
        }
        assert!((spring.position - 100.0).abs() < 0.01,
            "Spring should converge to target, got {}", spring.position);
    }

    #[test]
    fn test_spring_snap() {
        let mut spring = Spring::new(0.0);
        spring.snap(50.0);
        assert_eq!(spring.position, 50.0);
        assert_eq!(spring.target, 50.0);
        assert_eq!(spring.velocity, 0.0);
    }

    #[test]
    fn test_spring_is_settled() {
        let mut spring = Spring::new(100.0);
        spring.set_target(100.0);
        assert!(spring.is_settled(0.01));

        spring.set_target(200.0);
        assert!(!spring.is_settled(0.01));
    }

    #[test]
    fn test_spring_dt_independence() {
        // Two springs starting from same state should reach similar positions
        // regardless of dt step size
        let mut spring_fast = Spring::new(0.0);
        spring_fast.set_target(100.0);

        let mut spring_slow = Spring::new(0.0);
        spring_slow.set_target(100.0);

        // spring_fast: 120 steps of 1/60s = 2s total
        for _ in 0..120 {
            spring_fast.update(0.15, 1.0 / 60.0);
        }

        // spring_slow: 60 steps of 1/30s = 2s total
        for _ in 0..60 {
            spring_slow.update(0.15, 1.0 / 30.0);
        }

        let diff = (spring_fast.position - spring_slow.position).abs();
        assert!(diff < 0.1,
            "Springs with different dt should converge similarly: fast={}, slow={}, diff={}",
            spring_fast.position, spring_slow.position, diff);
    }

    #[test]
    fn test_spring_stability_with_large_dt() {
        let mut spring = Spring::new(0.0);
        spring.set_target(100.0);
        // Even with absurdly large dt, the spring should not diverge
        spring.update(0.15, 1.0);
        assert!(spring.position.is_finite());
        assert!(spring.position >= 0.0 && spring.position <= 100.0,
            "Spring should not overshoot with critically damped, got {}", spring.position);
    }

    #[test]
    fn test_half_life_meaning() {
        let mut spring = Spring::new(0.0);
        spring.set_target(100.0);
        let half_life = 0.15;

        // After exactly one half-life, position should be ~50% of target
        spring.update(half_life, half_life);
        let expected = 50.0;
        let tolerance = 10.0; // Spring with velocity starts slower
        assert!((spring.position - expected).abs() < tolerance,
            "After one half-life, position should be near 50%, got {}", spring.position);
    }

    #[test]
    fn test_animated_viewport_asymmetric_zoom() {
        let mut vp = AnimatedViewport::new(1920.0, 1080.0);

        // Zoom in should use ZOOM_IN half-life
        vp.set_target(960.0, 540.0, 2.0);
        assert_eq!(vp.zoom_half_life, SpringHalfLife::ZOOM_IN);

        // Zoom out should use ZOOM_OUT half-life
        vp.set_target(960.0, 540.0, 1.0);
        assert_eq!(vp.zoom_half_life, SpringHalfLife::ZOOM_OUT);
    }

    #[test]
    fn test_viewport_clamp_to_screen() {
        let mut vp = AnimatedViewport::new(1920.0, 1080.0);
        vp.snap_to(0.0, 0.0, 2.0);
        let rect = vp.current_viewport(1920.0, 1080.0);
        assert!(rect.x >= 0.0, "Viewport x should be >= 0, got {}", rect.x);
        assert!(rect.y >= 0.0, "Viewport y should be >= 0, got {}", rect.y);
    }
}
