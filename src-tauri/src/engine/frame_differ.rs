//! Frame differencing module for detecting visual change regions.
//!
//! Compares consecutive frames (sampled at intervals) to detect areas where
//! visual changes occur. These change regions are used to expand scene BBoxes
//! beyond just event coordinates, resulting in more appropriate zoom levels.
//!
//! Uses rayon for parallel frame loading and comparison.

use crate::engine::analyzer::Rect;
use anyhow::Result;
use image::GrayImage;
use rayon::prelude::*;
use std::path::Path;

/// Configuration for frame differencing.
pub struct DiffConfig {
    /// Sample every Nth frame for comparison.
    pub sample_interval: u64,
    /// Downsample factor (1/N resolution).
    pub downsample_factor: u32,
    /// Pixel difference threshold (0-255). Lower = more sensitive.
    pub pixel_threshold: u8,
    /// Cursor exclusion radius in original resolution pixels.
    pub cursor_exclude_radius: u32,
    /// Minimum change region dimension (original resolution pixels).
    pub min_region_size: u32,
    /// Maximum fraction of screen that can change before excluding the pair.
    pub max_change_fraction: f64,
}

impl Default for DiffConfig {
    fn default() -> Self {
        Self {
            sample_interval: 5,
            downsample_factor: 4,
            pixel_threshold: 10,
            cursor_exclude_radius: 50,
            min_region_size: 50,
            max_change_fraction: 0.5,
        }
    }
}

/// A detected change region between two frames.
#[derive(Debug, Clone)]
pub struct ChangeRegion {
    /// Time of the change (midpoint of the two compared frames).
    pub time_ms: u64,
    /// Bounding box of the change region (original resolution coordinates).
    pub bbox: Rect,
    /// Number of changed pixels (original resolution estimate).
    pub changed_pixel_count: u64,
}

/// Result of frame differencing analysis.
pub struct DiffResult {
    /// Detected change regions.
    pub regions: Vec<ChangeRegion>,
    /// Number of frame pairs analyzed.
    pub pairs_analyzed: usize,
    /// Number of pairs excluded (full-screen changes, etc.).
    pub pairs_excluded: usize,
}

/// Detect visual change regions across frames using parallel processing.
pub fn detect_frame_changes(
    frames_dir: &Path,
    frame_count: u64,
    duration_ms: u64,
    cursor_positions: &[(u64, f64, f64)],
    screen_w: u32,
    screen_h: u32,
    config: &DiffConfig,
) -> Result<DiffResult> {
    if frame_count < 2 {
        return Ok(DiffResult {
            regions: Vec::new(),
            pairs_analyzed: 0,
            pairs_excluded: 0,
        });
    }

    let frame_time_step_ms = if frame_count > 1 && duration_ms > 0 {
        duration_ms / frame_count
    } else {
        33
    };

    // Generate sampling pairs
    let pairs: Vec<(u64, u64)> = (0..frame_count)
        .step_by(config.sample_interval as usize)
        .zip(
            (0..frame_count)
                .step_by(config.sample_interval as usize)
                .skip(1),
        )
        .collect();

    if pairs.is_empty() {
        return Ok(DiffResult {
            regions: Vec::new(),
            pairs_analyzed: 0,
            pairs_excluded: 0,
        });
    }

    let ds = config.downsample_factor;

    // Process pairs in parallel with rayon
    let results: Vec<Option<ChangeRegion>> = pairs
        .par_iter()
        .map(|(idx_a, idx_b)| {
            let path_a = frames_dir.join(format!("frame_{:08}.png", idx_a));
            let path_b = frames_dir.join(format!("frame_{:08}.png", idx_b));

            let img_a = load_downsampled_gray(&path_a, ds).ok()?;
            let img_b = load_downsampled_gray(&path_b, ds).ok()?;

            let time_a = idx_a * frame_time_step_ms;
            let time_b = idx_b * frame_time_step_ms;
            let cursor_a = find_cursor_nearest(cursor_positions, time_a);
            let cursor_b = find_cursor_nearest(cursor_positions, time_b);

            let (bbox, count) = compute_pair_diff(
                &img_a,
                &img_b,
                cursor_a,
                cursor_b,
                config,
                screen_w,
                screen_h,
            )?;

            Some(ChangeRegion {
                time_ms: (time_a + time_b) / 2,
                bbox,
                changed_pixel_count: count,
            })
        })
        .collect();

    let total = results.len();
    let regions: Vec<ChangeRegion> = results.into_iter().flatten().collect();
    let pairs_excluded = total - regions.len();

    Ok(DiffResult {
        regions,
        pairs_analyzed: total,
        pairs_excluded,
    })
}

/// Expand a BBox by merging it with change region BBoxes.
pub fn expand_bbox_with_changes(
    event_bbox: &Rect,
    regions: &[&ChangeRegion],
    screen_w: f64,
    screen_h: f64,
) -> Rect {
    if regions.is_empty() {
        return event_bbox.clone();
    }

    let mut min_x = event_bbox.x;
    let mut min_y = event_bbox.y;
    let mut max_x = event_bbox.x + event_bbox.width;
    let mut max_y = event_bbox.y + event_bbox.height;

    for region in regions {
        min_x = min_x.min(region.bbox.x);
        min_y = min_y.min(region.bbox.y);
        max_x = max_x.max(region.bbox.x + region.bbox.width);
        max_y = max_y.max(region.bbox.y + region.bbox.height);
    }

    // Clamp to screen bounds
    min_x = min_x.max(0.0);
    min_y = min_y.max(0.0);
    max_x = max_x.min(screen_w);
    max_y = max_y.min(screen_h);

    Rect {
        x: min_x,
        y: min_y,
        width: (max_x - min_x).max(0.0),
        height: (max_y - min_y).max(0.0),
    }
}

// --- Internal functions ---

/// Load a frame as downsampled grayscale.
fn load_downsampled_gray(path: &Path, downsample_factor: u32) -> Result<GrayImage> {
    let img = image::open(path)?;
    let (w, h) = (img.width(), img.height());
    let new_w = (w / downsample_factor).max(1);
    let new_h = (h / downsample_factor).max(1);
    let resized = image::imageops::resize(
        &img.to_luma8(),
        new_w,
        new_h,
        image::imageops::FilterType::Nearest,
    );
    Ok(resized)
}

/// Compute difference between two grayscale frames.
/// Returns (bbox in original coords, changed_pixel_count) or None if filtered out.
fn compute_pair_diff(
    img_a: &GrayImage,
    img_b: &GrayImage,
    cursor_a: Option<(f64, f64)>,
    cursor_b: Option<(f64, f64)>,
    config: &DiffConfig,
    screen_w: u32,
    screen_h: u32,
) -> Option<(Rect, u64)> {
    let (w, h) = (img_a.width(), img_b.height());
    if w != img_b.width() || h != img_b.height() {
        return None;
    }
    if w == 0 || h == 0 {
        return None;
    }

    let ds = config.downsample_factor as f64;
    let cursor_radius_ds = config.cursor_exclude_radius as f64 / ds;
    let cursor_radius_sq = cursor_radius_ds * cursor_radius_ds;

    let total_pixels = (w * h) as u64;
    let mut changed_count: u64 = 0;
    let mut min_cx: u32 = w;
    let mut min_cy: u32 = h;
    let mut max_cx: u32 = 0;
    let mut max_cy: u32 = 0;

    for y in 0..h {
        for x in 0..w {
            let pa = img_a.get_pixel(x, y).0[0];
            let pb = img_b.get_pixel(x, y).0[0];
            let diff = (pa as i16 - pb as i16).unsigned_abs() as u8;

            if diff < config.pixel_threshold {
                continue;
            }

            // Check cursor exclusion for both frames
            let fx = x as f64;
            let fy = y as f64;
            let in_cursor = [cursor_a, cursor_b].iter().any(|cp| {
                if let Some((cx, cy)) = cp {
                    let dcx = cx / ds;
                    let dcy = cy / ds;
                    let dx = fx - dcx;
                    let dy = fy - dcy;
                    dx * dx + dy * dy < cursor_radius_sq
                } else {
                    false
                }
            });

            if in_cursor {
                continue;
            }

            changed_count += 1;
            min_cx = min_cx.min(x);
            min_cy = min_cy.min(y);
            max_cx = max_cx.max(x);
            max_cy = max_cy.max(y);
        }
    }

    if changed_count == 0 {
        return None;
    }

    // Filter: too much change (full-screen transition)
    let change_fraction = changed_count as f64 / total_pixels as f64;
    if change_fraction > config.max_change_fraction {
        return None;
    }

    // Convert back to original resolution
    let orig_min_x = (min_cx as f64 * ds).min(screen_w as f64);
    let orig_min_y = (min_cy as f64 * ds).min(screen_h as f64);
    let orig_max_x = ((max_cx + 1) as f64 * ds).min(screen_w as f64);
    let orig_max_y = ((max_cy + 1) as f64 * ds).min(screen_h as f64);

    let bbox_w = orig_max_x - orig_min_x;
    let bbox_h = orig_max_y - orig_min_y;

    // Filter: too small
    let min_size = config.min_region_size as f64;
    if bbox_w < min_size && bbox_h < min_size {
        return None;
    }

    let orig_changed = changed_count * (config.downsample_factor as u64).pow(2);

    Some((
        Rect {
            x: orig_min_x,
            y: orig_min_y,
            width: bbox_w,
            height: bbox_h,
        },
        orig_changed,
    ))
}

/// Find the nearest cursor position to a given time.
fn find_cursor_nearest(positions: &[(u64, f64, f64)], time_ms: u64) -> Option<(f64, f64)> {
    if positions.is_empty() {
        return None;
    }
    let idx = positions.partition_point(|&(t, _, _)| t < time_ms);
    if idx >= positions.len() {
        let last = positions.last().unwrap();
        Some((last.1, last.2))
    } else if idx == 0 {
        Some((positions[0].1, positions[0].2))
    } else {
        let (t_prev, x_prev, y_prev) = positions[idx - 1];
        let (t_next, x_next, y_next) = positions[idx];
        if time_ms - t_prev <= t_next - time_ms {
            Some((x_prev, y_prev))
        } else {
            Some((x_next, y_next))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GrayImage, Luma};

    fn make_config(min_region: u32, cursor_radius: u32) -> DiffConfig {
        DiffConfig {
            sample_interval: 1,
            downsample_factor: 1,
            pixel_threshold: 30,
            cursor_exclude_radius: cursor_radius,
            min_region_size: min_region,
            max_change_fraction: 0.5,
        }
    }

    #[test]
    fn test_identical_frames_no_change() {
        let img = GrayImage::from_pixel(100, 100, Luma([128]));
        let result = compute_pair_diff(&img, &img, None, None, &make_config(10, 0), 100, 100);
        assert!(result.is_none());
    }

    #[test]
    fn test_partial_change_detects_region() {
        let img_a = GrayImage::from_pixel(100, 100, Luma([0]));
        let mut img_b = img_a.clone();
        // Change region at (20,20)-(60,60)
        for y in 20..60 {
            for x in 20..60 {
                img_b.put_pixel(x, y, Luma([255]));
            }
        }
        let result = compute_pair_diff(&img_a, &img_b, None, None, &make_config(10, 0), 100, 100);
        assert!(result.is_some());
        let (bbox, count) = result.unwrap();
        assert!(bbox.x <= 20.0);
        assert!(bbox.y <= 20.0);
        assert!(bbox.x + bbox.width >= 60.0);
        assert!(bbox.y + bbox.height >= 60.0);
        assert!(count > 0);
    }

    #[test]
    fn test_full_screen_change_excluded() {
        let img_a = GrayImage::from_pixel(100, 100, Luma([0]));
        let img_b = GrayImage::from_pixel(100, 100, Luma([255]));
        let result = compute_pair_diff(&img_a, &img_b, None, None, &make_config(10, 0), 100, 100);
        assert!(result.is_none(), "Full-screen change should be excluded");
    }

    #[test]
    fn test_cursor_region_masked() {
        let img_a = GrayImage::from_pixel(100, 100, Luma([0]));
        let mut img_b = img_a.clone();
        // Only change near cursor at (50,50)
        for y in 45..55 {
            for x in 45..55 {
                img_b.put_pixel(x, y, Luma([255]));
            }
        }
        let result = compute_pair_diff(
            &img_a,
            &img_b,
            Some((50.0, 50.0)),
            Some((50.0, 50.0)),
            &make_config(5, 20),
            100,
            100,
        );
        assert!(result.is_none(), "Cursor-only changes should be masked");
    }

    #[test]
    fn test_tiny_change_filtered() {
        let img_a = GrayImage::from_pixel(200, 200, Luma([0]));
        let mut img_b = img_a.clone();
        // 5x5 change — smaller than min_region_size of 50
        for y in 50..55 {
            for x in 50..55 {
                img_b.put_pixel(x, y, Luma([255]));
            }
        }
        let result =
            compute_pair_diff(&img_a, &img_b, None, None, &make_config(50, 0), 200, 200);
        assert!(result.is_none(), "Tiny changes should be filtered");
    }

    #[test]
    fn test_change_outside_cursor_detected() {
        let img_a = GrayImage::from_pixel(200, 200, Luma([0]));
        let mut img_b = img_a.clone();
        // Change far from cursor at (10,10)
        for y in 150..200 {
            for x in 150..200 {
                img_b.put_pixel(x, y, Luma([255]));
            }
        }
        let result = compute_pair_diff(
            &img_a,
            &img_b,
            Some((10.0, 10.0)),
            Some((10.0, 10.0)),
            &make_config(10, 30),
            200,
            200,
        );
        assert!(
            result.is_some(),
            "Changes far from cursor should be detected"
        );
    }

    #[test]
    fn test_expand_bbox_no_regions() {
        let bbox = Rect {
            x: 400.0,
            y: 200.0,
            width: 360.0,
            height: 360.0,
        };
        let expanded = expand_bbox_with_changes(&bbox, &[], 1920.0, 1080.0);
        assert_eq!(expanded.x, bbox.x);
        assert_eq!(expanded.width, bbox.width);
    }

    #[test]
    fn test_expand_bbox_widens() {
        let bbox = Rect {
            x: 400.0,
            y: 200.0,
            width: 360.0,
            height: 360.0,
        };
        let region = ChangeRegion {
            time_ms: 1000,
            bbox: Rect {
                x: 300.0,
                y: 150.0,
                width: 600.0,
                height: 400.0,
            },
            changed_pixel_count: 5000,
        };
        let refs = vec![&region];
        let expanded = expand_bbox_with_changes(&bbox, &refs, 1920.0, 1080.0);
        assert_eq!(expanded.x, 300.0);
        assert_eq!(expanded.y, 150.0);
        assert!(expanded.width >= 600.0);
        assert!(expanded.height >= 400.0);
    }

    #[test]
    fn test_expand_bbox_clamps_to_screen() {
        let bbox = Rect {
            x: 400.0,
            y: 200.0,
            width: 360.0,
            height: 360.0,
        };
        let region = ChangeRegion {
            time_ms: 1000,
            bbox: Rect {
                x: -100.0,
                y: -50.0,
                width: 2200.0,
                height: 1200.0,
            },
            changed_pixel_count: 50000,
        };
        let refs = vec![&region];
        let expanded = expand_bbox_with_changes(&bbox, &refs, 1920.0, 1080.0);
        assert!(expanded.x >= 0.0);
        assert!(expanded.y >= 0.0);
        assert!(expanded.x + expanded.width <= 1920.0);
        assert!(expanded.y + expanded.height <= 1080.0);
    }

    #[test]
    fn test_find_cursor_nearest_empty() {
        assert!(find_cursor_nearest(&[], 1000).is_none());
    }

    #[test]
    fn test_find_cursor_nearest_exact() {
        let positions = vec![(100, 50.0, 60.0), (200, 70.0, 80.0)];
        let result = find_cursor_nearest(&positions, 100);
        assert_eq!(result, Some((50.0, 60.0)));
    }

    #[test]
    fn test_find_cursor_nearest_between() {
        let positions = vec![(100, 50.0, 60.0), (200, 70.0, 80.0)];
        // 160 is closer to 200
        let result = find_cursor_nearest(&positions, 160);
        assert_eq!(result, Some((70.0, 80.0)));
        // 130 is closer to 100
        let result = find_cursor_nearest(&positions, 130);
        assert_eq!(result, Some((50.0, 60.0)));
    }
}
