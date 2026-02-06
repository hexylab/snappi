use crate::config::BackgroundConfig;
use image::{Rgba, RgbaImage};

pub fn create_background_image(
    width: u32,
    height: u32,
    config: &BackgroundConfig,
) -> RgbaImage {
    match config {
        BackgroundConfig::Gradient { from, to, angle } => {
            create_gradient(width, height, from, to, *angle)
        }
        BackgroundConfig::Solid { color } => {
            RgbaImage::from_pixel(width, height, Rgba([color[0], color[1], color[2], 255]))
        }
        BackgroundConfig::Transparent => {
            RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 0]))
        }
    }
}

fn create_gradient(
    width: u32,
    height: u32,
    from: &[u8; 3],
    to: &[u8; 3],
    angle: f64,
) -> RgbaImage {
    let mut img = RgbaImage::new(width, height);
    let rad = angle.to_radians();
    let cos_a = rad.cos();
    let sin_a = rad.sin();

    let max_dist = (width as f64 * cos_a.abs() + height as f64 * sin_a.abs()) / 2.0;

    for y in 0..height {
        for x in 0..width {
            let nx = x as f64 - width as f64 / 2.0;
            let ny = y as f64 - height as f64 / 2.0;
            let dist = nx * cos_a + ny * sin_a;
            let t = ((dist / max_dist + 1.0) / 2.0).clamp(0.0, 1.0);

            let r = (from[0] as f64 * (1.0 - t) + to[0] as f64 * t) as u8;
            let g = (from[1] as f64 * (1.0 - t) + to[1] as f64 * t) as u8;
            let b = (from[2] as f64 * (1.0 - t) + to[2] as f64 * t) as u8;
            img.put_pixel(x, y, Rgba([r, g, b, 255]));
        }
    }

    img
}
