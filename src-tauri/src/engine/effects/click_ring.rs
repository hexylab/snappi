use image::{Rgba, RgbaImage};

pub fn draw_click_ring_effect(
    img: &mut RgbaImage,
    x: f64,
    y: f64,
    progress: f64,
    max_radius: f64,
    color: &[u8; 4],
    stroke_width: f64,
) {
    let radius = max_radius * progress;
    let alpha = ((1.0 - progress) * color[3] as f64) as u8;
    let cx = x as i32;
    let cy = y as i32;
    let r = radius as i32;
    let sw = (stroke_width * 0.5) as i32 + 1;

    for dy in -r - sw..=r + sw {
        for dx in -r - sw..=r + sw {
            let dist = ((dx * dx + dy * dy) as f64).sqrt();
            if (dist - radius).abs() <= stroke_width * 0.5 {
                let px = cx + dx;
                let py = cy + dy;
                if px >= 0 && py >= 0 && (px as u32) < img.width() && (py as u32) < img.height() {
                    // Anti-alias the edge
                    let edge_dist = (dist - radius).abs();
                    let edge_alpha = if edge_dist > stroke_width * 0.5 - 1.0 {
                        let t = (stroke_width * 0.5 - edge_dist).max(0.0);
                        (t * alpha as f64) as u8
                    } else {
                        alpha
                    };

                    let src = Rgba([color[0], color[1], color[2], edge_alpha]);
                    let dst = *img.get_pixel(px as u32, py as u32);
                    img.put_pixel(px as u32, py as u32, blend(dst, src));
                }
            }
        }
    }
}

fn blend(dst: Rgba<u8>, src: Rgba<u8>) -> Rgba<u8> {
    let sa = src[3] as f64 / 255.0;
    let da = dst[3] as f64 / 255.0;
    let out_a = sa + da * (1.0 - sa);
    if out_a == 0.0 {
        return Rgba([0, 0, 0, 0]);
    }
    let r = (src[0] as f64 * sa + dst[0] as f64 * da * (1.0 - sa)) / out_a;
    let g = (src[1] as f64 * sa + dst[1] as f64 * da * (1.0 - sa)) / out_a;
    let b = (src[2] as f64 * sa + dst[2] as f64 * da * (1.0 - sa)) / out_a;
    Rgba([r as u8, g as u8, b as u8, (out_a * 255.0) as u8])
}
