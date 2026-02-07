use crate::engine::spring::AnimatedViewport;

/// Manages the viewport (visible region) of the recording
/// Handles zoom, pan, and viewport clamping
pub struct ViewportManager {
    pub viewport: AnimatedViewport,
    pub screen_width: f64,
    pub screen_height: f64,
}

impl ViewportManager {
    pub fn new(
        screen_width: f64,
        screen_height: f64,
    ) -> Self {
        Self {
            viewport: AnimatedViewport::new(
                screen_width,
                screen_height,
            ),
            screen_width,
            screen_height,
        }
    }

    pub fn update(&mut self, dt: f64) {
        self.viewport.update(dt);
    }

    pub fn zoom_to(&mut self, x: f64, y: f64, zoom: f64) {
        self.viewport.set_target(x, y, zoom);
    }

    pub fn zoom_out(&mut self) {
        self.viewport.set_target(
            self.screen_width / 2.0,
            self.screen_height / 2.0,
            1.0,
        );
    }

    pub fn snap_to(&mut self, x: f64, y: f64, zoom: f64) {
        self.viewport.snap_to(x, y, zoom);
    }
}
