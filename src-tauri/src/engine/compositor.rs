use super::spring::AnimatedViewport;
use super::zoom_planner::{TransitionType, ZoomKeyframe};
use crate::config::defaults::OutputStyle;
use image::{Rgba, RgbaImage};

pub struct Compositor {
    style: OutputStyle,
    viewport: AnimatedViewport,
    screen_width: f64,
    screen_height: f64,
}

impl Compositor {
    pub fn new(style: OutputStyle, screen_width: u32, screen_height: u32) -> Self {
        let viewport = AnimatedViewport::new(
            screen_width as f64,
            screen_height as f64,
            style.zoom_spring_tension,
            style.zoom_spring_friction,
        );

        Self {
            style,
            viewport,
            screen_width: screen_width as f64,
            screen_height: screen_height as f64,
        }
    }

    pub fn apply_keyframe(&mut self, kf: &ZoomKeyframe) {
        match kf.transition {
            TransitionType::Cut => {
                self.viewport.snap_to(kf.target_x, kf.target_y, kf.zoom_level);
            }
            _ => {
                self.viewport.set_target(kf.target_x, kf.target_y, kf.zoom_level);
            }
        }
    }

    pub fn compose_frame(
        &mut self,
        raw_frame: &RgbaImage,
        frame_time_ms: u64,
        cursor_pos: Option<(f64, f64)>,
        click_effects: &[ClickEffect],
        key_overlay: Option<&KeyOverlay>,
        dt: f64,
    ) -> RgbaImage {
        // Update spring animation
        self.viewport.update(dt);

        let vp = self.viewport.current_viewport(self.screen_width, self.screen_height);

        // (a) Crop and scale to output size
        let mut output = crop_and_scale(
            raw_frame,
            vp.x,
            vp.y,
            vp.width,
            vp.height,
            self.style.output_width,
            self.style.output_height,
        );

        // (b) Draw cursor
        if let Some((cx, cy)) = cursor_pos {
            let (out_x, out_y) = self.viewport.to_output_coords(
                cx,
                cy,
                self.style.output_width as f64,
                self.style.output_height as f64,
                self.screen_width,
                self.screen_height,
            );
            draw_cursor(&mut output, out_x, out_y, self.style.cursor_size_multiplier);
        }

        // (c) Click ring effects
        for effect in click_effects {
            if effect.is_active(frame_time_ms) {
                let (out_x, out_y) = self.viewport.to_output_coords(
                    effect.x,
                    effect.y,
                    self.style.output_width as f64,
                    self.style.output_height as f64,
                    self.screen_width,
                    self.screen_height,
                );
                let progress = effect.progress(frame_time_ms);
                draw_click_ring(
                    &mut output,
                    out_x,
                    out_y,
                    progress,
                    self.style.click_ring_max_radius,
                    &self.style.click_ring_color,
                    self.style.click_ring_stroke_width,
                );
            }
        }

        // (d) Key badge overlay
        if let Some(overlay) = key_overlay {
            if overlay.is_visible(frame_time_ms) {
                draw_key_badge(
                    &mut output,
                    &overlay.keys,
                    self.style.output_width,
                    self.style.output_height,
                );
            }
        }

        // (e) Rounded corners
        if self.style.border_radius > 0 {
            apply_rounded_corners(&mut output, self.style.border_radius);
        }

        // (f) Shadow + background composition
        let mut canvas = create_background(
            self.style.canvas_width,
            self.style.canvas_height,
            &self.style,
        );

        let offset_x = (self.style.canvas_width - self.style.output_width) / 2;
        let offset_y = (self.style.canvas_height - self.style.output_height) / 2;

        // Draw shadow
        draw_drop_shadow(
            &mut canvas,
            offset_x,
            offset_y,
            self.style.output_width,
            self.style.output_height,
            self.style.shadow_blur,
            self.style.shadow_offset_y,
            &self.style.shadow_color,
        );

        // Composite the output frame onto the canvas
        composite(&mut canvas, &output, offset_x, offset_y);

        canvas
    }
}

#[derive(Debug, Clone)]
pub struct ClickEffect {
    pub x: f64,
    pub y: f64,
    pub start_ms: u64,
    pub duration_ms: u64,
}

impl ClickEffect {
    pub fn is_active(&self, time_ms: u64) -> bool {
        time_ms >= self.start_ms && time_ms <= self.start_ms + self.duration_ms
    }

    pub fn progress(&self, time_ms: u64) -> f64 {
        if !self.is_active(time_ms) {
            return 0.0;
        }
        (time_ms - self.start_ms) as f64 / self.duration_ms as f64
    }
}

#[derive(Debug, Clone)]
pub struct KeyOverlay {
    pub keys: String,
    pub start_ms: u64,
    pub duration_ms: u64,
}

impl KeyOverlay {
    pub fn is_visible(&self, time_ms: u64) -> bool {
        time_ms >= self.start_ms && time_ms <= self.start_ms + self.duration_ms
    }
}

fn crop_and_scale(
    src: &RgbaImage,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    out_w: u32,
    out_h: u32,
) -> RgbaImage {
    let src_x = x.max(0.0) as u32;
    let src_y = y.max(0.0) as u32;
    let src_w = (w as u32).min(src.width().saturating_sub(src_x));
    let src_h = (h as u32).min(src.height().saturating_sub(src_y));

    if src_w == 0 || src_h == 0 {
        return RgbaImage::new(out_w, out_h);
    }

    let cropped = image::imageops::crop_imm(src, src_x, src_y, src_w, src_h).to_image();
    image::imageops::resize(&cropped, out_w, out_h, image::imageops::FilterType::Lanczos3)
}

fn draw_cursor(img: &mut RgbaImage, x: f64, y: f64, size_mult: f64) {
    let size = (12.0 * size_mult) as i32;
    let cx = x as i32;
    let cy = y as i32;

    // Draw a simple arrow cursor
    for dy in 0..size {
        let width = (dy as f64 * 0.6) as i32;
        for dx in 0..=width {
            let px = cx + dx;
            let py = cy + dy;
            if px >= 0 && py >= 0 && (px as u32) < img.width() && (py as u32) < img.height() {
                if dx == 0 || dx == width || dy == size - 1 {
                    img.put_pixel(px as u32, py as u32, Rgba([0, 0, 0, 255]));
                } else {
                    img.put_pixel(px as u32, py as u32, Rgba([255, 255, 255, 255]));
                }
            }
        }
    }
}

fn draw_click_ring(
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
    let sw = stroke_width as i32;

    for dy in -r - sw..=r + sw {
        for dx in -r - sw..=r + sw {
            let dist = ((dx * dx + dy * dy) as f64).sqrt();
            if (dist - radius).abs() <= stroke_width {
                let px = cx + dx;
                let py = cy + dy;
                if px >= 0 && py >= 0 && (px as u32) < img.width() && (py as u32) < img.height() {
                    img.put_pixel(px as u32, py as u32, Rgba([color[0], color[1], color[2], alpha]));
                }
            }
        }
    }
}

fn draw_key_badge(img: &mut RgbaImage, keys: &str, output_width: u32, output_height: u32) {
    // Draw a simple key badge at bottom center
    let badge_height = 32u32;
    let badge_width = (keys.len() as u32 * 10 + 20).min(output_width);
    let x_start = (output_width - badge_width) / 2;
    let y_start = output_height - badge_height - 20;

    // Badge background
    for y in y_start..y_start + badge_height {
        for x in x_start..x_start + badge_width {
            if x < img.width() && y < img.height() {
                img.put_pixel(x, y, Rgba([30, 30, 30, 200]));
            }
        }
    }
}

fn apply_rounded_corners(img: &mut RgbaImage, radius: u32) {
    let w = img.width();
    let h = img.height();
    let r = radius as f64;

    for y in 0..h {
        for x in 0..w {
            let corners = [
                (0.0, 0.0),
                (w as f64, 0.0),
                (0.0, h as f64),
                (w as f64, h as f64),
            ];

            for &(cx, cy) in &corners {
                let dx = if x as f64 <= r && cx == 0.0 {
                    r - x as f64
                } else if x as f64 >= w as f64 - r && cx == w as f64 {
                    x as f64 - (w as f64 - r)
                } else {
                    continue;
                };

                let dy = if y as f64 <= r && cy == 0.0 {
                    r - y as f64
                } else if y as f64 >= h as f64 - r && cy == h as f64 {
                    y as f64 - (h as f64 - r)
                } else {
                    continue;
                };

                if dx > 0.0 && dy > 0.0 {
                    let dist = (dx * dx + dy * dy).sqrt();
                    if dist > r {
                        img.put_pixel(x, y, Rgba([0, 0, 0, 0]));
                    }
                }
            }
        }
    }
}

fn create_background(width: u32, height: u32, _style: &OutputStyle) -> RgbaImage {
    let mut canvas = RgbaImage::new(width, height);

    // Default gradient background (purple to blue, 135 degrees)
    let from = [139u8, 92, 246];
    let to = [59u8, 130, 246];

    for y in 0..height {
        for x in 0..width {
            let t = ((x as f64 / width as f64) + (y as f64 / height as f64)) / 2.0;
            let r = (from[0] as f64 * (1.0 - t) + to[0] as f64 * t) as u8;
            let g = (from[1] as f64 * (1.0 - t) + to[1] as f64 * t) as u8;
            let b = (from[2] as f64 * (1.0 - t) + to[2] as f64 * t) as u8;
            canvas.put_pixel(x, y, Rgba([r, g, b, 255]));
        }
    }

    canvas
}

fn draw_drop_shadow(
    canvas: &mut RgbaImage,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    blur: f64,
    offset_y: f64,
    color: &[u8; 4],
) {
    let shadow_y = y as f64 + offset_y;
    let spread = blur as i32;

    for sy in (y as i32 - spread)..=(y as i32 + h as i32 + spread) {
        for sx in (x as i32 - spread)..=(x as i32 + w as i32 + spread) {
            if sx < 0 || sy < 0 || sx as u32 >= canvas.width() || sy as u32 >= canvas.height() {
                continue;
            }

            // Distance from content rectangle
            let dx = if sx < x as i32 {
                (x as i32 - sx) as f64
            } else if sx >= (x + w) as i32 {
                (sx - (x + w) as i32 + 1) as f64
            } else {
                0.0
            };

            let dy = if (sy as f64) < shadow_y {
                shadow_y - sy as f64
            } else if sy as f64 >= shadow_y + h as f64 {
                sy as f64 - shadow_y - h as f64 + 1.0
            } else {
                0.0
            };

            let dist = (dx * dx + dy * dy).sqrt();
            if dist > 0.0 && dist <= blur {
                let alpha = ((1.0 - dist / blur) * color[3] as f64) as u8;
                let pixel = canvas.get_pixel(sx as u32, sy as u32);
                let blended = blend_pixel(*pixel, Rgba([color[0], color[1], color[2], alpha]));
                canvas.put_pixel(sx as u32, sy as u32, blended);
            }
        }
    }
}

fn composite(canvas: &mut RgbaImage, overlay: &RgbaImage, offset_x: u32, offset_y: u32) {
    for y in 0..overlay.height() {
        for x in 0..overlay.width() {
            let cx = x + offset_x;
            let cy = y + offset_y;
            if cx < canvas.width() && cy < canvas.height() {
                let src = overlay.get_pixel(x, y);
                if src[3] > 0 {
                    let dst = canvas.get_pixel(cx, cy);
                    let blended = blend_pixel(*dst, *src);
                    canvas.put_pixel(cx, cy, blended);
                }
            }
        }
    }
}

fn blend_pixel(dst: Rgba<u8>, src: Rgba<u8>) -> Rgba<u8> {
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
