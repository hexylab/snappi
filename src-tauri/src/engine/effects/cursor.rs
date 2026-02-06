use image::{Rgba, RgbaImage};

/// Draw a cursor at the given position
pub fn draw_system_cursor(img: &mut RgbaImage, x: f64, y: f64, size_mult: f64) {
    let size = (16.0 * size_mult) as i32;
    let cx = x as i32;
    let cy = y as i32;

    // Draw arrow cursor shape
    let cursor_points = generate_arrow_cursor(size);

    for (dx, dy, is_border) in cursor_points {
        let px = cx + dx;
        let py = cy + dy;
        if px >= 0 && py >= 0 && (px as u32) < img.width() && (py as u32) < img.height() {
            let color = if is_border {
                Rgba([0, 0, 0, 255])
            } else {
                Rgba([255, 255, 255, 255])
            };
            img.put_pixel(px as u32, py as u32, color);
        }
    }
}

fn generate_arrow_cursor(size: i32) -> Vec<(i32, i32, bool)> {
    let mut points = Vec::new();

    for y in 0..size {
        let max_x = (y as f64 * 0.65) as i32;
        for x in 0..=max_x {
            let is_border = x == 0 || x == max_x || y == size - 1;
            points.push((x, y, is_border));
        }
    }

    points
}
