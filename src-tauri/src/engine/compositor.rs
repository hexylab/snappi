use super::effects::background::create_background_image;
use super::spring::AnimatedViewport;
use super::zoom_planner::{TransitionType, ZoomKeyframe};
use crate::config::defaults::OutputStyle;
use image::{Rgba, RgbaImage};

/// Dead Zone / Soft Zone / Push Zone boundaries (normalized 0-1)
const DEAD_ZONE_RADIUS: f64 = 0.3;
const SOFT_ZONE_RADIUS: f64 = 0.7;

/// Cursor sprite base size in pixels (before zoom scaling)
const CURSOR_BASE_SIZE: u32 = 32;

pub struct Compositor {
    style: OutputStyle,
    viewport: AnimatedViewport,
    screen_width: f64,
    screen_height: f64,
    /// Cached background image (same every frame)
    cached_background: Option<RgbaImage>,
    /// Pre-rendered cursor sprite at base size
    cursor_sprite: RgbaImage,
}

impl Compositor {
    pub fn new(style: OutputStyle, screen_width: u32, screen_height: u32) -> Self {
        let viewport = AnimatedViewport::new(
            screen_width as f64,
            screen_height as f64,
        );

        let cursor_sprite = create_cursor_sprite(CURSOR_BASE_SIZE);

        Self {
            style,
            viewport,
            screen_width: screen_width as f64,
            screen_height: screen_height as f64,
            cached_background: None,
            cursor_sprite,
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
        // (0) Dead Zone cursor tracking — update viewport target before spring update
        if let Some((cx, cy)) = cursor_pos {
            self.apply_cursor_follow(cx, cy);
        }

        // (1) Update spring animation
        self.viewport.update(dt);

        let vp = self.viewport.current_viewport(self.screen_width, self.screen_height);
        let zoom = vp.zoom;

        // (2) Crop and scale to output size (using Triangle filter for speed)
        let mut output = crop_and_scale(
            raw_frame,
            vp.x,
            vp.y,
            vp.width,
            vp.height,
            self.style.output_width,
            self.style.output_height,
        );

        // (3) Draw cursor — scale with zoom to maintain consistent visual size
        if let Some((cx, cy)) = cursor_pos {
            let (out_x, out_y) = self.viewport.to_output_coords(
                cx,
                cy,
                self.style.output_width as f64,
                self.style.output_height as f64,
                self.screen_width,
                self.screen_height,
            );
            let cursor_scale = self.style.cursor_size_multiplier * zoom;
            draw_cursor_sprite(
                &mut output,
                &self.cursor_sprite,
                out_x,
                out_y,
                cursor_scale,
            );
        }

        // (4) Click ring effects — scale with zoom
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
                    self.style.click_ring_max_radius * zoom,
                    &self.style.click_ring_color,
                    self.style.click_ring_stroke_width * zoom,
                );
            }
        }

        // (5) Key badge overlay
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

        // (6) Rounded corners with anti-aliasing
        if self.style.border_radius > 0 {
            apply_rounded_corners_aa(&mut output, self.style.border_radius);
        }

        // (7) Shadow + background composition (with caching)
        let canvas = self.get_or_create_background();
        let mut canvas = canvas.clone();

        let offset_x = (self.style.canvas_width - self.style.output_width) / 2;
        let offset_y = (self.style.canvas_height - self.style.output_height) / 2;

        // Draw rounded-rectangle shadow (matches border_radius)
        draw_drop_shadow(
            &mut canvas,
            offset_x,
            offset_y,
            self.style.output_width,
            self.style.output_height,
            self.style.shadow_blur,
            self.style.shadow_offset_y,
            &self.style.shadow_color,
            self.style.border_radius,
        );

        // Composite the output frame onto the canvas
        composite(&mut canvas, &output, offset_x, offset_y);

        canvas
    }

    /// Apply Dead Zone / Soft Zone / Push Zone cursor tracking.
    /// Adjusts viewport center target based on cursor distance from center.
    fn apply_cursor_follow(&mut self, cursor_x: f64, cursor_y: f64) {
        let zoom = self.viewport.zoom.position.max(1.0);
        if zoom <= 1.01 {
            return; // No tracking needed at 1x zoom
        }

        let vp_w = self.screen_width / zoom;
        let vp_h = self.screen_height / zoom;

        let cx = self.viewport.center_x.target;
        let cy = self.viewport.center_y.target;

        // Normalized cursor offset from viewport center
        let dx = (cursor_x - cx) / (vp_w / 2.0);
        let dy = (cursor_y - cy) / (vp_h / 2.0);
        let d = (dx * dx + dy * dy).sqrt();

        let strength = follow_strength(d, DEAD_ZONE_RADIUS, SOFT_ZONE_RADIUS);

        if strength > 0.0 {
            let shift_x = strength * (cursor_x - cx);
            let shift_y = strength * (cursor_y - cy);

            let new_x = cx + shift_x;
            let new_y = cy + shift_y;

            // Clamp to screen bounds
            let half_w = vp_w / 2.0;
            let half_h = vp_h / 2.0;
            let clamped_x = new_x.clamp(half_w, self.screen_width - half_w);
            let clamped_y = new_y.clamp(half_h, self.screen_height - half_h);

            self.viewport.center_x.set_target(clamped_x);
            self.viewport.center_y.set_target(clamped_y);
        }
    }

    fn get_or_create_background(&mut self) -> &RgbaImage {
        if self.cached_background.is_none() {
            self.cached_background = Some(create_background_image(
                self.style.canvas_width,
                self.style.canvas_height,
                &self.style.background,
            ));
        }
        self.cached_background.as_ref().unwrap()
    }
}

/// Smoothstep-based follow strength for the 3-zone model.
/// Returns 0.0 in dead zone, smoothstep 0→1 in soft zone, 1.0 in push zone.
fn follow_strength(d: f64, dead_zone: f64, soft_zone: f64) -> f64 {
    if d < dead_zone {
        0.0
    } else if d < soft_zone {
        let t = (d - dead_zone) / (soft_zone - dead_zone);
        t * t * (3.0 - 2.0 * t) // smoothstep (C^1 continuous)
    } else {
        1.0
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
        let linear = (time_ms - self.start_ms) as f64 / self.duration_ms as f64;
        // Apply ease-out cubic: 1 - (1 - t)^3
        ease_out_cubic(linear)
    }
}

/// Ease-out cubic easing function
fn ease_out_cubic(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
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
    // Use Triangle (bilinear) filter instead of Lanczos3 for 2-3x speedup
    image::imageops::resize(&cropped, out_w, out_h, image::imageops::FilterType::Triangle)
}

// --- High-quality cursor rendering ---

/// Create a pre-rendered cursor sprite using SDF-based anti-aliasing.
/// Generates a macOS-style arrow pointer with:
///   - White fill + black outline (2px) + drop shadow
///   - Sub-pixel anti-aliased edges via signed distance field
fn create_cursor_sprite(size: u32) -> RgbaImage {
    let pad = 6u32; // extra padding for shadow
    let total = size + pad * 2;
    let mut img = RgbaImage::new(total, total);
    let s = size as f64;

    // Arrow cursor vertices (normalized 0..1, origin at top-left)
    let vertices: [(f64, f64); 7] = [
        (0.0, 0.0),         // tip
        (0.0, 0.85),        // left edge bottom
        (0.22, 0.62),       // notch left
        (0.52, 0.95),       // tail bottom-right
        (0.68, 0.82),       // tail top-right
        (0.38, 0.52),       // notch right
        (0.58, 0.30),       // right edge
    ];

    // Scale vertices to pixel coordinates (with padding offset)
    let pts: Vec<(f64, f64)> = vertices
        .iter()
        .map(|(vx, vy)| (vx * s + pad as f64, vy * s + pad as f64))
        .collect();

    // For each pixel, compute signed distance to the polygon boundary
    for py in 0..total {
        for px in 0..total {
            let x = px as f64 + 0.5;
            let y = py as f64 + 0.5;

            let dist = signed_distance_to_polygon(&pts, x, y);

            // Shadow (offset +2px down, +1px right, blurred)
            let shadow_dist = signed_distance_to_polygon(&pts, x - 1.0, y - 2.0);
            let shadow_alpha = smoothstep(3.0, 0.0, shadow_dist) * 0.4;

            // Black outline: ~2px thick around the edge
            let outline_width = 1.8;
            let outline_alpha = smoothstep(0.5, -0.5, dist - outline_width);

            // White fill
            let fill_alpha = smoothstep(0.5, -0.5, dist);

            // Composite: shadow → outline → fill
            let mut r = 0.0f64;
            let mut g = 0.0f64;
            let mut b = 0.0f64;
            let mut a = 0.0f64;

            // Shadow layer
            if shadow_alpha > 0.0 {
                a = shadow_alpha;
                // shadow is black
            }

            // Outline layer (black)
            if outline_alpha > 0.0 {
                let sa = outline_alpha;
                r = r * (1.0 - sa);
                g = g * (1.0 - sa);
                b = b * (1.0 - sa);
                a = sa + a * (1.0 - sa);
            }

            // Fill layer (white)
            if fill_alpha > 0.0 {
                let sa = fill_alpha;
                let out_a = sa + a * (1.0 - sa);
                if out_a > 0.0 {
                    r = (255.0 * sa + r * a * (1.0 - sa)) / out_a;
                    g = (255.0 * sa + g * a * (1.0 - sa)) / out_a;
                    b = (255.0 * sa + b * a * (1.0 - sa)) / out_a;
                    a = out_a;
                }
            }

            if a > 0.001 {
                img.put_pixel(
                    px,
                    py,
                    Rgba([
                        r.clamp(0.0, 255.0) as u8,
                        g.clamp(0.0, 255.0) as u8,
                        b.clamp(0.0, 255.0) as u8,
                        (a * 255.0).clamp(0.0, 255.0) as u8,
                    ]),
                );
            }
        }
    }

    img
}

/// Signed distance from point to convex polygon.
/// Negative = inside, positive = outside.
fn signed_distance_to_polygon(pts: &[(f64, f64)], px: f64, py: f64) -> f64 {
    let n = pts.len();
    let mut min_dist_sq = f64::MAX;
    let mut sign = 1.0;

    for i in 0..n {
        let j = (i + 1) % n;
        let (ex, ey) = (pts[j].0 - pts[i].0, pts[j].1 - pts[i].1);
        let (wx, wy) = (px - pts[i].0, py - pts[i].1);

        let t = (wx * ex + wy * ey) / (ex * ex + ey * ey);
        let t = t.clamp(0.0, 1.0);
        let dx = wx - ex * t;
        let dy = wy - ey * t;
        let dist_sq = dx * dx + dy * dy;

        if dist_sq < min_dist_sq {
            min_dist_sq = dist_sq;
        }

        // Winding number test for inside/outside
        let c1 = pts[i].1 <= py;
        let c2 = pts[j].1 > py;
        let c3 = pts[j].1 <= py;
        let c4 = pts[i].1 > py;
        let cross = ex * wy - ey * wx;

        if (c1 && c2 && cross > 0.0) || (c3 && c4 && cross < 0.0) {
            sign = -sign;
        }
    }

    sign * min_dist_sq.sqrt()
}

/// Smooth interpolation for anti-aliasing
fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Draw cursor by scaling the pre-rendered sprite and alpha-compositing it.
fn draw_cursor_sprite(
    img: &mut RgbaImage,
    sprite: &RgbaImage,
    x: f64,
    y: f64,
    size_mult: f64,
) {
    let target_size = (CURSOR_BASE_SIZE as f64 * size_mult) as u32;
    if target_size == 0 {
        return;
    }

    // Scale sprite to target size (with padding)
    let pad_ratio = sprite.width() as f64 / CURSOR_BASE_SIZE as f64;
    let total_size = (target_size as f64 * pad_ratio) as u32;

    let scaled = image::imageops::resize(
        sprite,
        total_size.max(1),
        total_size.max(1),
        image::imageops::FilterType::Triangle,
    );

    // Position: cursor tip is at (x, y), offset by scaled padding
    let pad_px = (6.0 * size_mult) as i32;
    let start_x = x as i32 - pad_px;
    let start_y = y as i32 - pad_px;

    for sy in 0..scaled.height() {
        for sx in 0..scaled.width() {
            let px = start_x + sx as i32;
            let py = start_y + sy as i32;
            if px >= 0 && py >= 0 && (px as u32) < img.width() && (py as u32) < img.height() {
                let src = scaled.get_pixel(sx, sy);
                if src[3] > 0 {
                    let dst = img.get_pixel(px as u32, py as u32);
                    let blended = blend_pixel(*dst, *src);
                    img.put_pixel(px as u32, py as u32, blended);
                }
            }
        }
    }
}

// --- Click ring with fill ---

fn draw_click_ring(
    img: &mut RgbaImage,
    x: f64,
    y: f64,
    eased_progress: f64,
    max_radius: f64,
    color: &[u8; 4],
    stroke_width: f64,
) {
    let radius = max_radius * eased_progress;
    // Fade out alpha as the ring expands
    let base_alpha = (1.0 - eased_progress) * color[3] as f64;
    let ring_alpha = base_alpha as u8;
    let fill_alpha = (base_alpha * 0.15) as u8; // subtle inner fill
    let cx = x as i32;
    let cy = y as i32;
    let r = radius as i32;
    let sw = stroke_width.ceil() as i32;

    for dy in -r - sw..=r + sw {
        for dx in -r - sw..=r + sw {
            let dist = ((dx * dx + dy * dy) as f64).sqrt();
            let px = cx + dx;
            let py = cy + dy;
            if px < 0 || py < 0 || (px as u32) >= img.width() || (py as u32) >= img.height() {
                continue;
            }

            let ring_dist = (dist - radius).abs();

            if dist <= radius && fill_alpha > 0 {
                // Inner fill — subtle translucent disc
                let pixel = img.get_pixel(px as u32, py as u32);
                let fill_color = Rgba([color[0], color[1], color[2], fill_alpha]);
                let blended = blend_pixel(*pixel, fill_color);
                img.put_pixel(px as u32, py as u32, blended);
            }

            if ring_dist <= stroke_width {
                // Ring stroke with anti-aliased edges
                let edge_alpha = if ring_dist > stroke_width - 1.0 {
                    ((stroke_width - ring_dist).max(0.0) * ring_alpha as f64) as u8
                } else {
                    ring_alpha
                };
                let pixel = img.get_pixel(px as u32, py as u32);
                let ring_color = Rgba([color[0], color[1], color[2], edge_alpha]);
                let blended = blend_pixel(*pixel, ring_color);
                img.put_pixel(px as u32, py as u32, blended);
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

    // Badge background with rounded corners (8px)
    let badge_radius = 8u32;
    for y in y_start..y_start + badge_height {
        for x in x_start..x_start + badge_width {
            if x < img.width() && y < img.height() {
                // Check if we're in a corner that should be rounded
                let lx = x - x_start;
                let ly = y - y_start;
                let rx = badge_width - 1 - lx;
                let ry = badge_height - 1 - ly;

                let in_corner = |cx: u32, cy: u32| -> bool {
                    cx < badge_radius && cy < badge_radius
                        && {
                            let dx = badge_radius as f64 - cx as f64 - 0.5;
                            let dy = badge_radius as f64 - cy as f64 - 0.5;
                            (dx * dx + dy * dy).sqrt() > badge_radius as f64
                        }
                };

                if in_corner(lx, ly)
                    || in_corner(rx, ly)
                    || in_corner(lx, ry)
                    || in_corner(rx, ry)
                {
                    continue;
                }

                let pixel = img.get_pixel(x, y);
                let blended = blend_pixel(*pixel, Rgba([30, 30, 30, 200]));
                img.put_pixel(x, y, blended);
            }
        }
    }
}

/// Rounded corners with anti-aliasing.
/// Uses sub-pixel alpha calculation for smooth corner boundaries.
fn apply_rounded_corners_aa(img: &mut RgbaImage, radius: u32) {
    let w = img.width();
    let h = img.height();
    let r = radius as f64;

    // Only process corner regions for efficiency
    let corners: [(u32, u32); 4] = [
        (0, 0),           // top-left
        (w - radius, 0),  // top-right
        (0, h - radius),  // bottom-left
        (w - radius, h - radius), // bottom-right
    ];

    for &(corner_x, corner_y) in &corners {
        // Center of the rounding circle for this corner
        let center_x = if corner_x == 0 { r } else { (w as f64) - r };
        let center_y = if corner_y == 0 { r } else { (h as f64) - r };

        for y in corner_y..(corner_y + radius).min(h) {
            for x in corner_x..(corner_x + radius).min(w) {
                let dx = x as f64 + 0.5 - center_x;
                let dy = y as f64 + 0.5 - center_y;

                // Only process pixels in the corner quadrant
                let in_quadrant = (corner_x == 0 && dx < 0.0) || (corner_x > 0 && dx > 0.0);
                let in_quadrant_y = (corner_y == 0 && dy < 0.0) || (corner_y > 0 && dy > 0.0);

                if !in_quadrant || !in_quadrant_y {
                    continue;
                }

                let dist = (dx * dx + dy * dy).sqrt();

                if dist > r + 0.5 {
                    // Fully outside — transparent
                    img.put_pixel(x, y, Rgba([0, 0, 0, 0]));
                } else if dist > r - 0.5 {
                    // On the boundary — anti-alias
                    let alpha = ((r + 0.5 - dist).clamp(0.0, 1.0) * 255.0) as u8;
                    let pixel = img.get_pixel(x, y);
                    let new_alpha = ((pixel[3] as f64 * alpha as f64) / 255.0) as u8;
                    img.put_pixel(x, y, Rgba([pixel[0], pixel[1], pixel[2], new_alpha]));
                }
                // else: fully inside — keep as-is
            }
        }
    }
}

// --- Rounded-rectangle drop shadow ---

/// Draw a drop shadow shaped as a rounded rectangle.
/// Computes distance from each pixel to the nearest point on the rounded rect,
/// then applies a smooth falloff.
fn draw_drop_shadow(
    canvas: &mut RgbaImage,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    blur: f64,
    offset_y: f64,
    color: &[u8; 4],
    border_radius: u32,
) {
    if color[3] == 0 || blur <= 0.0 {
        return;
    }

    let r = border_radius as f64;
    let shadow_y = y as f64 + offset_y;
    let spread = blur.ceil() as i32;

    // Shadow rectangle bounds (shifted by offset_y)
    let rect_left = x as f64;
    let rect_right = (x + w) as f64;
    let rect_top = shadow_y;
    let rect_bottom = shadow_y + h as f64;

    for sy in (y as i32 - spread)..=(y as i32 + h as i32 + spread + offset_y.ceil() as i32) {
        for sx in (x as i32 - spread)..=(x as i32 + w as i32 + spread) {
            if sx < 0 || sy < 0 || sx as u32 >= canvas.width() || sy as u32 >= canvas.height() {
                continue;
            }

            let px = sx as f64 + 0.5;
            let py = sy as f64 + 0.5;

            // Distance from point to the rounded rectangle
            let dist = dist_to_rounded_rect(px, py, rect_left, rect_top, rect_right, rect_bottom, r);

            if dist > 0.0 && dist <= blur {
                // Smooth quadratic falloff (softer than linear)
                let t = 1.0 - dist / blur;
                let alpha = (t * t * color[3] as f64) as u8;
                let pixel = canvas.get_pixel(sx as u32, sy as u32);
                let blended = blend_pixel(*pixel, Rgba([color[0], color[1], color[2], alpha]));
                canvas.put_pixel(sx as u32, sy as u32, blended);
            }
        }
    }
}

/// Signed distance from a point to a rounded rectangle.
/// Returns 0 if inside, positive if outside.
fn dist_to_rounded_rect(
    px: f64,
    py: f64,
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
    radius: f64,
) -> f64 {
    // Clamp to the inner rectangle (inset by radius)
    let inner_left = left + radius;
    let inner_right = right - radius;
    let inner_top = top + radius;
    let inner_bottom = bottom - radius;

    // Distance to the axis-aligned inner rectangle
    let dx = if px < inner_left {
        inner_left - px
    } else if px > inner_right {
        px - inner_right
    } else {
        0.0
    };

    let dy = if py < inner_top {
        inner_top - py
    } else if py > inner_bottom {
        py - inner_bottom
    } else {
        0.0
    };

    if dx > 0.0 && dy > 0.0 {
        // In a corner region — distance to the corner circle
        let corner_dist = (dx * dx + dy * dy).sqrt();
        (corner_dist - radius).max(0.0)
    } else if dx > 0.0 {
        // Left or right of the inner rect
        (dx - radius).max(0.0)
    } else if dy > 0.0 {
        // Above or below the inner rect
        (dy - radius).max(0.0)
    } else {
        // Inside the rounded rectangle
        0.0
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_follow_strength_dead_zone() {
        assert_eq!(follow_strength(0.0, 0.3, 0.7), 0.0);
        assert_eq!(follow_strength(0.2, 0.3, 0.7), 0.0);
        assert_eq!(follow_strength(0.29, 0.3, 0.7), 0.0);
    }

    #[test]
    fn test_follow_strength_soft_zone() {
        let s = follow_strength(0.5, 0.3, 0.7);
        assert!(s > 0.0 && s < 1.0, "Soft zone should be partial: {}", s);

        // Smoothstep at midpoint (t=0.5) should be 0.5
        let mid = follow_strength(0.5, 0.3, 0.7);
        assert!((mid - 0.5).abs() < 0.01, "Midpoint should be ~0.5: {}", mid);
    }

    #[test]
    fn test_follow_strength_push_zone() {
        assert_eq!(follow_strength(0.7, 0.3, 0.7), 1.0);
        assert_eq!(follow_strength(0.9, 0.3, 0.7), 1.0);
        assert_eq!(follow_strength(1.5, 0.3, 0.7), 1.0);
    }

    #[test]
    fn test_follow_strength_monotonic() {
        let mut prev = 0.0;
        for i in 0..100 {
            let d = i as f64 / 100.0;
            let s = follow_strength(d, 0.3, 0.7);
            assert!(s >= prev, "follow_strength should be monotonically increasing");
            prev = s;
        }
    }

    #[test]
    fn test_ease_out_cubic() {
        assert_eq!(ease_out_cubic(0.0), 0.0);
        assert_eq!(ease_out_cubic(1.0), 1.0);
        // Ease-out: should be > linear at midpoint
        assert!(ease_out_cubic(0.5) > 0.5);
    }

    #[test]
    fn test_click_effect_eased_progress() {
        let effect = ClickEffect {
            x: 100.0,
            y: 100.0,
            start_ms: 0,
            duration_ms: 400,
        };

        // At 50% time, eased progress should be > 0.5 (ease-out)
        let p = effect.progress(200);
        assert!(p > 0.5, "Eased progress at 50% time should be > 0.5: {}", p);

        // At 0% and 100%
        assert_eq!(effect.progress(0), 0.0);
        let end_p = effect.progress(400);
        assert!((end_p - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_cursor_sprite_generation() {
        let sprite = create_cursor_sprite(32);
        assert_eq!(sprite.width(), 32 + 12); // 32 + 2*6 padding
        assert_eq!(sprite.height(), 32 + 12);

        // Tip of cursor (at padding offset) should have some opacity
        let tip = sprite.get_pixel(6, 6);
        assert!(tip[3] > 0, "Cursor tip should be visible");

        // Far corner should be transparent
        let corner = sprite.get_pixel(0, 0);
        assert!(corner[3] < 10, "Corner should be mostly transparent, got alpha {}", corner[3]);
    }

    #[test]
    fn test_dist_to_rounded_rect() {
        // Inside should be 0
        assert_eq!(dist_to_rounded_rect(50.0, 50.0, 0.0, 0.0, 100.0, 100.0, 10.0), 0.0);

        // Outside on edge (no corner) should be positive
        let d = dist_to_rounded_rect(110.0, 50.0, 0.0, 0.0, 100.0, 100.0, 10.0);
        assert!(d > 0.0, "Outside should be positive: {}", d);

        // Corner region — diagonal distance
        let d = dist_to_rounded_rect(105.0, 105.0, 0.0, 0.0, 100.0, 100.0, 10.0);
        assert!(d > 0.0, "Outside corner should be positive: {}", d);

        // Inside near corner should still be 0
        let d = dist_to_rounded_rect(93.0, 93.0, 0.0, 0.0, 100.0, 100.0, 10.0);
        assert_eq!(d, 0.0, "Inside near corner should be 0: {}", d);
    }

    #[test]
    fn test_signed_distance_polygon() {
        // Simple triangle
        let triangle = vec![(0.0, 0.0), (10.0, 0.0), (5.0, 10.0)];

        // Center should be inside (negative)
        let d = signed_distance_to_polygon(&triangle, 5.0, 3.0);
        assert!(d < 0.0, "Center should be inside: {}", d);

        // Far away should be outside (positive)
        let d = signed_distance_to_polygon(&triangle, 50.0, 50.0);
        assert!(d > 0.0, "Far away should be outside: {}", d);
    }
}
