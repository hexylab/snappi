use super::effects::background::create_background_image;
use super::spring::AnimatedViewport;
use super::zoom_planner::ZoomKeyframe;
use crate::config::defaults::OutputStyle;
use image::{Rgba, RgbaImage};

/// Cursor sprite base size in pixels (before zoom scaling)
const CURSOR_BASE_SIZE: u32 = 32;

/// Embedded custom cursor PNG (icon/カーソル.png) and its hotspot
const EMBEDDED_CURSOR_PNG: &[u8] = include_bytes!("../../../icon/カーソル.png");
const EMBEDDED_CURSOR_HOTSPOT: (u32, u32) = (35, 22);

pub struct Compositor {
    style: OutputStyle,
    viewport: AnimatedViewport,
    screen_width: f64,
    screen_height: f64,
    /// Cached background image (same every frame)
    cached_background: Option<RgbaImage>,
    /// Pre-rendered cursor sprite at base size
    cursor_sprite: RgbaImage,
    /// Cursor hotspot offset within sprite (tip position)
    cursor_hotspot: (u32, u32),
    /// Previous composed frame for motion blur
    prev_output: Option<RgbaImage>,
    /// Previous viewport state for motion amount calculation
    prev_vp_center: Option<(f64, f64, f64)>, // (cx, cy, zoom)
    /// Whether motion blur is enabled
    motion_blur_enabled: bool,
}

impl Compositor {
    pub fn new(style: OutputStyle, screen_width: u32, screen_height: u32) -> Self {
        let viewport = AnimatedViewport::new(
            screen_width as f64,
            screen_height as f64,
        );

        // Load embedded cursor PNG, fallback to system capture, then to SDF sprite
        let (cursor_sprite, cursor_hotspot) = load_embedded_cursor()
            .or_else(|| {
                log::info!("Embedded cursor decode failed, trying system capture");
                capture_system_cursor_sprite()
                    .map(|(img, hx, hy)| {
                        log::info!("System cursor captured: {}x{}", img.width(), img.height());
                        (img, (hx, hy))
                    })
            })
            .unwrap_or_else(|| {
                log::warn!("All cursor sources failed, using fallback SDF sprite");
                (create_cursor_sprite(CURSOR_BASE_SIZE), (6, 6))
            });

        Self {
            style,
            viewport,
            screen_width: screen_width as f64,
            screen_height: screen_height as f64,
            cached_background: None,
            cursor_sprite,
            cursor_hotspot,
            prev_output: None,
            prev_vp_center: None,
            motion_blur_enabled: false,
        }
    }

    /// Load a custom cursor from a PNG file path.
    /// Returns true if loaded successfully.
    pub fn set_cursor_from_path(&mut self, path: &str, hotspot_x: u32, hotspot_y: u32) -> bool {
        match image::open(path) {
            Ok(img) => {
                let rgba = img.to_rgba8();
                log::info!("Custom cursor loaded: {}x{} from '{}' hotspot=({},{})",
                    rgba.width(), rgba.height(), path, hotspot_x, hotspot_y);
                self.cursor_sprite = rgba;
                self.cursor_hotspot = (hotspot_x, hotspot_y);
                true
            }
            Err(e) => {
                log::warn!("Failed to load custom cursor from '{}': {}", path, e);
                false
            }
        }
    }

    /// Try to capture the Windows system cursor.
    /// Returns true if captured successfully.
    pub fn set_cursor_from_system(&mut self) -> bool {
        match capture_system_cursor_sprite() {
            Some((img, hx, hy)) => {
                log::info!("System cursor captured: {}x{} hotspot=({},{})",
                    img.width(), img.height(), hx, hy);
                self.cursor_sprite = img;
                self.cursor_hotspot = (hx, hy);
                true
            }
            None => {
                log::warn!("System cursor capture failed, keeping current sprite");
                false
            }
        }
    }

    pub fn set_motion_blur(&mut self, enabled: bool) {
        self.motion_blur_enabled = enabled;
    }

    pub fn apply_keyframe(&mut self, kf: &ZoomKeyframe) {
        if let Some(ref hint) = kf.spring_hint {
            self.viewport.set_target_with_half_life(
                kf.target_x,
                kf.target_y,
                kf.zoom_level,
                hint.zoom_half_life,
                hint.pan_half_life,
            );
        } else {
            self.viewport.set_target(kf.target_x, kf.target_y, kf.zoom_level);
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
                self.cursor_hotspot,
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

        // Motion blur: blend with previous frame when viewport is moving fast
        if self.motion_blur_enabled {
            let current_vp = (
                self.viewport.center_x.position,
                self.viewport.center_y.position,
                self.viewport.zoom.position,
            );

            if let (Some(ref prev_frame), Some(prev_vp)) = (&self.prev_output, self.prev_vp_center) {
                if prev_frame.dimensions() == canvas.dimensions() {
                    let dx = (current_vp.0 - prev_vp.0) / self.screen_width;
                    let dy = (current_vp.1 - prev_vp.1) / self.screen_height;
                    let dz = (current_vp.2 - prev_vp.2).abs();
                    let motion = (dx * dx + dy * dy).sqrt() + dz;

                    // Only apply blur when motion exceeds threshold
                    if motion > 0.005 {
                        let blend_amount = (motion * 3.0).min(0.35);
                        motion_blur_blend(&mut canvas, prev_frame, blend_amount);
                    }
                }
            }

            self.prev_vp_center = Some(current_vp);
            self.prev_output = Some(canvas.clone());
        }

        canvas
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
    hotspot: (u32, u32),
) {
    let scale = size_mult * (CURSOR_BASE_SIZE as f64) / (sprite.width().max(1) as f64);
    let target_w = ((sprite.width() as f64) * scale) as u32;
    let target_h = ((sprite.height() as f64) * scale) as u32;
    if target_w == 0 || target_h == 0 {
        return;
    }

    // Use CatmullRom for high-quality cursor scaling (better than Triangle for sharp edges)
    let scaled = image::imageops::resize(
        sprite,
        target_w.max(1),
        target_h.max(1),
        image::imageops::FilterType::CatmullRom,
    );

    let hotspot_x = (hotspot.0 as f64 * scale) as i32;
    let hotspot_y = (hotspot.1 as f64 * scale) as i32;
    let start_x = x as i32 - hotspot_x;
    let start_y = y as i32 - hotspot_y;

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

/// Blend current frame with previous frame for motion blur effect.
/// `amount` controls blur intensity (0.0 = no blur, 0.35 = max blur).
fn motion_blur_blend(current: &mut RgbaImage, prev: &RgbaImage, amount: f64) {
    let amount = amount.clamp(0.0, 0.5);
    let curr_weight = 1.0 - amount;
    let prev_weight = amount;

    for (c_pixel, p_pixel) in current.pixels_mut().zip(prev.pixels()) {
        c_pixel[0] = (c_pixel[0] as f64 * curr_weight + p_pixel[0] as f64 * prev_weight) as u8;
        c_pixel[1] = (c_pixel[1] as f64 * curr_weight + p_pixel[1] as f64 * prev_weight) as u8;
        c_pixel[2] = (c_pixel[2] as f64 * curr_weight + p_pixel[2] as f64 * prev_weight) as u8;
        c_pixel[3] = (c_pixel[3] as f64 * curr_weight + p_pixel[3] as f64 * prev_weight) as u8;
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

/// Load the embedded cursor PNG (compiled into the binary).
fn load_embedded_cursor() -> Option<(RgbaImage, (u32, u32))> {
    let cursor = image::load_from_memory_with_format(
        EMBEDDED_CURSOR_PNG,
        image::ImageFormat::Png,
    ).ok()?;
    let rgba = cursor.to_rgba8();
    log::info!("Embedded cursor loaded: {}x{} hotspot=({},{})",
        rgba.width(), rgba.height(),
        EMBEDDED_CURSOR_HOTSPOT.0, EMBEDDED_CURSOR_HOTSPOT.1);
    Some((rgba, EMBEDDED_CURSOR_HOTSPOT))
}

/// Capture the system cursor bitmap via Windows API.
/// Returns (cursor_image, hotspot_x, hotspot_y) or None on failure.
///
/// Uses GetIconInfo to extract the color and mask bitmaps, then combines them
/// for proper alpha transparency. Falls back to mask-based reconstruction
/// if the color bitmap has no alpha channel.
#[cfg(windows)]
fn capture_system_cursor_sprite() -> Option<(RgbaImage, u32, u32)> {
    use windows::Win32::UI::WindowsAndMessaging::{
        CopyIcon, GetIconInfo, LoadCursorW, IDC_ARROW, ICONINFO,
    };
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits,
        BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS,
    };

    unsafe {
        let cursor = LoadCursorW(None, IDC_ARROW).ok()?;
        let icon = CopyIcon(cursor).ok()?;

        let mut icon_info = ICONINFO::default();
        GetIconInfo(icon, &mut icon_info).ok()?;

        let hbm_color = icon_info.hbmColor;
        let hbm_mask = icon_info.hbmMask;
        let hotspot_x = icon_info.xHotspot;
        let hotspot_y = icon_info.yHotspot;

        if hbm_color.is_invalid() {
            if !hbm_mask.is_invalid() {
                let _ = DeleteObject(hbm_mask);
            }
            return None;
        }

        let hdc = CreateCompatibleDC(None);

        // Query dimensions of color bitmap
        let mut bmp_info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biPlanes: 1,
                biBitCount: 32,
                ..Default::default()
            },
            ..Default::default()
        };
        GetDIBits(hdc, hbm_color, 0, 0, None, &mut bmp_info, DIB_RGB_COLORS);

        let width = bmp_info.bmiHeader.biWidth as u32;
        let height = bmp_info.bmiHeader.biHeight.unsigned_abs();

        if width == 0 || height == 0 {
            let _ = DeleteDC(hdc);
            let _ = DeleteObject(hbm_color);
            if !hbm_mask.is_invalid() {
                let _ = DeleteObject(hbm_mask);
            }
            return None;
        }

        // Read color pixel data top-down
        bmp_info.bmiHeader.biHeight = -(height as i32);
        bmp_info.bmiHeader.biWidth = width as i32;
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        GetDIBits(
            hdc,
            hbm_color,
            0,
            height,
            Some(pixels.as_mut_ptr() as *mut _),
            &mut bmp_info,
            DIB_RGB_COLORS,
        );

        // BGRA → RGBA
        for chunk in pixels.chunks_exact_mut(4) {
            chunk.swap(0, 2);
        }

        // Check if color bitmap has valid alpha channel
        let has_alpha = pixels.chunks_exact(4).any(|c| c[3] != 0);

        if !has_alpha && !hbm_mask.is_invalid() {
            // Color bitmap has no alpha — use the mask bitmap to generate alpha.
            // The AND mask: 1 = transparent, 0 = opaque
            let mut mask_info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width as i32,
                    biHeight: -(height as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut mask_pixels = vec![0u8; (width * height * 4) as usize];
            GetDIBits(
                hdc,
                hbm_mask,
                0,
                height,
                Some(mask_pixels.as_mut_ptr() as *mut _),
                &mut mask_info,
                DIB_RGB_COLORS,
            );

            // Apply mask: where mask pixel is black (0), cursor is opaque
            for (color, mask) in pixels.chunks_exact_mut(4).zip(mask_pixels.chunks_exact(4)) {
                // mask is BGRA — if R/G/B are all 0, cursor pixel is opaque
                if mask[0] == 0 && mask[1] == 0 && mask[2] == 0 {
                    color[3] = 255; // opaque
                } else {
                    color[3] = 0; // transparent
                }
            }
        } else if !has_alpha {
            // No mask bitmap — set alpha based on pixel content
            for chunk in pixels.chunks_exact_mut(4) {
                if chunk[0] > 0 || chunk[1] > 0 || chunk[2] > 0 {
                    chunk[3] = 255;
                }
            }
        }

        // Cleanup
        let _ = DeleteDC(hdc);
        let _ = DeleteObject(hbm_color);
        if !hbm_mask.is_invalid() {
            let _ = DeleteObject(hbm_mask);
        }

        let img = RgbaImage::from_raw(width, height, pixels)?;
        Some((img, hotspot_x, hotspot_y))
    }
}

#[cfg(not(windows))]
fn capture_system_cursor_sprite() -> Option<(RgbaImage, u32, u32)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

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
