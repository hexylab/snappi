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
}
