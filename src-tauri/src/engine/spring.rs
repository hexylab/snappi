/// Spring physics animation engine
/// Used for smooth transitions of zoom, cursor, and viewport
#[derive(Debug, Clone)]
pub struct SpringAnimation {
    pub position: f64,
    pub velocity: f64,
    pub target: f64,
    pub tension: f64,
    pub friction: f64,
    pub mass: f64,
}

impl SpringAnimation {
    pub fn new(tension: f64, friction: f64, mass: f64) -> Self {
        Self {
            position: 0.0,
            velocity: 0.0,
            target: 0.0,
            tension,
            friction,
            mass,
        }
    }

    pub fn update(&mut self, dt: f64) -> f64 {
        let displacement = self.position - self.target;
        let spring_force = -self.tension * displacement;
        let damping_force = -self.friction * self.velocity;
        let acceleration = (spring_force + damping_force) / self.mass;

        self.velocity += acceleration * dt;
        self.position += self.velocity * dt;
        self.position
    }

    pub fn is_settled(&self) -> bool {
        (self.position - self.target).abs() < 0.5 && self.velocity.abs() < 0.1
    }

    pub fn set_target(&mut self, target: f64) {
        self.target = target;
    }

    pub fn snap_to(&mut self, value: f64) {
        self.position = value;
        self.target = value;
        self.velocity = 0.0;
    }
}

/// Animated viewport managing X, Y center and zoom level with spring physics
#[derive(Debug, Clone)]
pub struct AnimatedViewport {
    pub center_x: SpringAnimation,
    pub center_y: SpringAnimation,
    pub zoom: SpringAnimation,
}

impl AnimatedViewport {
    pub fn new(
        screen_width: f64,
        screen_height: f64,
        zoom_tension: f64,
        zoom_friction: f64,
    ) -> Self {
        let mut center_x = SpringAnimation::new(zoom_tension, zoom_friction, 1.0);
        center_x.snap_to(screen_width / 2.0);

        let mut center_y = SpringAnimation::new(zoom_tension, zoom_friction, 1.0);
        center_y.snap_to(screen_height / 2.0);

        let mut zoom = SpringAnimation::new(zoom_tension, zoom_friction, 1.0);
        zoom.snap_to(1.0);

        Self {
            center_x,
            center_y,
            zoom,
        }
    }

    pub fn update(&mut self, dt: f64) {
        self.center_x.update(dt);
        self.center_y.update(dt);
        self.zoom.update(dt);
    }

    pub fn set_target(&mut self, x: f64, y: f64, zoom: f64) {
        self.center_x.set_target(x);
        self.center_y.set_target(y);
        self.zoom.set_target(zoom);
    }

    pub fn snap_to(&mut self, x: f64, y: f64, zoom: f64) {
        self.center_x.snap_to(x);
        self.center_y.snap_to(y);
        self.zoom.snap_to(zoom);
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
