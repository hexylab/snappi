use image::{Rgba, RgbaImage};

pub struct KeyBadgeRenderer {
    pub show_only_with_modifiers: bool,
    pub duration_ms: u64,
}

impl Default for KeyBadgeRenderer {
    fn default() -> Self {
        Self {
            show_only_with_modifiers: true,
            duration_ms: 1500,
        }
    }
}

impl KeyBadgeRenderer {
    pub fn should_display(&self, key: &str, modifiers: &[String]) -> bool {
        if self.show_only_with_modifiers {
            !modifiers.is_empty()
                || key == "Control"
                || key == "Shift"
                || key == "Alt"
                || key == "Meta"
        } else {
            true
        }
    }

    pub fn draw_badge(
        &self,
        img: &mut RgbaImage,
        text: &str,
        output_width: u32,
        output_height: u32,
    ) {
        let char_width = 8u32;
        let padding = 12u32;
        let badge_height = 28u32;
        let badge_width = text.len() as u32 * char_width + padding * 2;
        let x_start = (output_width - badge_width) / 2;
        let y_start = output_height - badge_height - 24;
        let radius = 6u32;

        // Draw rounded rectangle background
        for y in y_start..y_start + badge_height {
            for x in x_start..x_start + badge_width {
                if x < img.width() && y < img.height() {
                    // Check if inside rounded rect
                    let in_corner = is_in_rounded_rect(
                        x - x_start,
                        y - y_start,
                        badge_width,
                        badge_height,
                        radius,
                    );
                    if in_corner {
                        img.put_pixel(x, y, Rgba([20, 20, 20, 220]));
                    }
                }
            }
        }
    }
}

fn is_in_rounded_rect(x: u32, y: u32, w: u32, h: u32, r: u32) -> bool {
    if x < r && y < r {
        let dx = r - x;
        let dy = r - y;
        return dx * dx + dy * dy <= r * r;
    }
    if x >= w - r && y < r {
        let dx = x - (w - r);
        let dy = r - y;
        return dx * dx + dy * dy <= r * r;
    }
    if x < r && y >= h - r {
        let dx = r - x;
        let dy = y - (h - r);
        return dx * dx + dy * dy <= r * r;
    }
    if x >= w - r && y >= h - r {
        let dx = x - (w - r);
        let dy = y - (h - r);
        return dx * dx + dy * dy <= r * r;
    }
    true
}
