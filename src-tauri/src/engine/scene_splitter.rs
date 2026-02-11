//! Scene splitting module for the global lookahead zoom system.
//!
//! Splits recording events into scenes (periods of continuous user activity)
//! separated by idle gaps. Each scene has a bounding box and optimal zoom
//! level computed from ALL events within it.
//!
//! This module does NOT use window focus tracking for scene splitting.
//! Zoom transitions are controlled by a 2-state model (Overview ↔ WorkArea)
//! in the zoom_planner module.

use crate::config::RecordingEvent;
use crate::engine::analyzer::Rect;
use serde::{Deserialize, Serialize};

/// Minimum idle gap to split scenes (ms)
const SCENE_GAP_MS: u64 = 1500;
/// Spatial jump threshold for sub-scene splitting (px)
const SUB_SCENE_SPATIAL_JUMP_PX: f64 = 250.0;
/// Minimum time gap for sub-scene splitting within a scene (ms).
/// Must be shorter than SCENE_GAP_MS to find split points within large scenes.
const SUB_SCENE_TIME_GAP_MS: u64 = 800;
/// Maximum center distance to merge consecutive scenes (px)
const MERGE_CENTER_DISTANCE_PX: f64 = 150.0;
/// Padding around scene bounding box (px)
const BBOX_PADDING: f64 = 80.0;
/// Minimum bbox dimension (width or height)
const MIN_BBOX_SIZE: f64 = 200.0;
/// Maximum fraction of screen area a single scene bbox can cover before splitting
const MAX_BBOX_SCREEN_FRACTION: f64 = 0.25;
/// Time window for "recent click" when positioning Key events (ms)
const RECENT_CLICK_WINDOW_MS: u64 = 2000;

/// A scene represents a period of continuous user activity with a defined
/// spatial focus area.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scene {
    pub id: u32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub bbox: Rect,
    pub center_x: f64,
    pub center_y: f64,
    pub zoom_level: f64,
    pub event_count: usize,
}

/// A manual scene editing operation from the Timeline UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SceneEditOp {
    /// Merge scene `scene_id` with the next scene (scene_id + 1).
    Merge { scene_id: u32 },
    /// Split scene `scene_id` at the given time.
    Split { scene_id: u32, split_time_ms: u64 },
}

/// Activity point for scene construction.
pub(crate) struct ActivityPoint {
    pub time_ms: u64,
    pub x: f64,
    pub y: f64,
}

/// Extract activity points from recording events.
///
/// Key events use the last click/focus position (within 2s) for text input
/// patterns. If there's no recent click or focus, key events are skipped
/// (no reliable position to place them).
pub(crate) fn extract_activity_points(events: &[RecordingEvent]) -> Vec<ActivityPoint> {
    let mut points = Vec::new();
    let mut last_click_pos: Option<(f64, f64, u64)> = None; // (x, y, time)

    for event in events {
        match event {
            RecordingEvent::Click { t, x, y, .. } => {
                points.push(ActivityPoint {
                    time_ms: *t,
                    x: *x,
                    y: *y,
                });
                last_click_pos = Some((*x, *y, *t));
            }
            RecordingEvent::ClickRelease { t, x, y, .. } => {
                points.push(ActivityPoint {
                    time_ms: *t,
                    x: *x,
                    y: *y,
                });
            }
            RecordingEvent::Key { t, .. } => {
                // Use recent click/focus position (within 2s) for text input patterns
                let pos = last_click_pos
                    .filter(|(_, _, ct)| t.saturating_sub(*ct) < RECENT_CLICK_WINDOW_MS)
                    .map(|(cx, cy, _)| (cx, cy));
                if let Some((x, y)) = pos {
                    points.push(ActivityPoint {
                        time_ms: *t,
                        x,
                        y,
                    });
                }
            }
            RecordingEvent::Scroll { t, x, y, .. } => {
                points.push(ActivityPoint {
                    time_ms: *t,
                    x: *x,
                    y: *y,
                });
            }
            RecordingEvent::Focus { t, rect, .. } => {
                let cx = (rect[0] + rect[2]) / 2.0;
                let cy = (rect[1] + rect[3]) / 2.0;
                points.push(ActivityPoint {
                    time_ms: *t,
                    x: cx,
                    y: cy,
                });
                last_click_pos = Some((cx, cy, *t));
            }
            _ => {}
        }
    }

    points.sort_by_key(|p| p.time_ms);
    points
}

/// Compute a padded bounding box from activity points.
pub(crate) fn compute_bbox(points: &[&ActivityPoint]) -> Rect {
    if points.is_empty() {
        return Rect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        };
    }
    let min_x = points.iter().map(|p| p.x).fold(f64::MAX, f64::min);
    let max_x = points.iter().map(|p| p.x).fold(f64::MIN, f64::max);
    let min_y = points.iter().map(|p| p.y).fold(f64::MAX, f64::min);
    let max_y = points.iter().map(|p| p.y).fold(f64::MIN, f64::max);

    let raw_w = max_x - min_x;
    let raw_h = max_y - min_y;
    let w = raw_w.max(MIN_BBOX_SIZE);
    let h = raw_h.max(MIN_BBOX_SIZE);
    let cx = min_x + raw_w / 2.0;
    let cy = min_y + raw_h / 2.0;

    Rect {
        x: cx - w / 2.0 - BBOX_PADDING,
        y: cy - h / 2.0 - BBOX_PADDING,
        width: w + BBOX_PADDING * 2.0,
        height: h + BBOX_PADDING * 2.0,
    }
}

/// Create a Scene from a group of activity points.
pub(crate) fn make_scene(
    points: &[&ActivityPoint],
    screen_w: f64,
    screen_h: f64,
    max_zoom: f64,
    id: u32,
) -> Scene {
    let bbox = compute_bbox(points);
    let zoom_level = calc_scene_zoom(&bbox, screen_w, screen_h, max_zoom);
    let center_x = bbox.x + bbox.width / 2.0;
    let center_y = bbox.y + bbox.height / 2.0;

    Scene {
        id,
        start_ms: points.first().map(|p| p.time_ms).unwrap_or(0),
        end_ms: points.last().map(|p| p.time_ms).unwrap_or(0),
        bbox,
        center_x,
        center_y,
        zoom_level,
        event_count: points.len(),
    }
}

/// Calculate optimal zoom level to fit a scene's bbox within the screen.
pub(crate) fn calc_scene_zoom(bbox: &Rect, screen_w: f64, screen_h: f64, max_zoom: f64) -> f64 {
    let zoom_w = screen_w / bbox.width.max(1.0);
    let zoom_h = screen_h / bbox.height.max(1.0);
    let fit_zoom = zoom_w.min(zoom_h);
    fit_zoom.min(max_zoom).max(1.0)
}

/// Calculate zoom level to fit a window within the screen (with padding).
pub fn calc_window_zoom(
    window_rect: &Rect,
    screen_w: f64,
    screen_h: f64,
    max_zoom: f64,
) -> f64 {
    let padding = 0.05;
    let padded_w = window_rect.width * (1.0 + padding * 2.0);
    let padded_h = window_rect.height * (1.0 + padding * 2.0);
    let zoom_w = screen_w / padded_w.max(1.0);
    let zoom_h = screen_h / padded_h.max(1.0);
    zoom_w.min(zoom_h).min(max_zoom).max(1.0)
}

/// Split a large scene into sub-scenes based on spatial jumps and time gaps.
fn split_scene_if_needed(
    points: &[&ActivityPoint],
    screen_w: f64,
    screen_h: f64,
    max_zoom: f64,
    next_id: &mut u32,
) -> Vec<Scene> {
    let overall_bbox = compute_bbox(points);
    let screen_area = screen_w * screen_h;
    let bbox_area = overall_bbox.width * overall_bbox.height;

    if bbox_area <= screen_area * MAX_BBOX_SCREEN_FRACTION || points.len() < 3 {
        let scene = make_scene(points, screen_w, screen_h, max_zoom, *next_id);
        *next_id += 1;
        return vec![scene];
    }

    // Try splitting at spatial jumps + time gaps
    let mut sub_groups: Vec<Vec<&ActivityPoint>> = Vec::new();
    let mut current: Vec<&ActivityPoint> = vec![points[0]];

    for i in 1..points.len() {
        let prev = points[i - 1];
        let curr = points[i];
        let time_gap = curr.time_ms.saturating_sub(prev.time_ms);
        let spatial_dist =
            ((curr.x - prev.x).powi(2) + (curr.y - prev.y).powi(2)).sqrt();

        let should_split =
            time_gap >= SUB_SCENE_TIME_GAP_MS && spatial_dist >= SUB_SCENE_SPATIAL_JUMP_PX;

        if should_split {
            sub_groups.push(current);
            current = Vec::new();
        }
        current.push(curr);
    }
    if !current.is_empty() {
        sub_groups.push(current);
    }

    // If splitting didn't help (still one group), keep as single scene
    if sub_groups.len() <= 1 {
        let scene = make_scene(points, screen_w, screen_h, max_zoom, *next_id);
        *next_id += 1;
        return vec![scene];
    }

    sub_groups
        .iter()
        .map(|group| {
            let scene = make_scene(group, screen_w, screen_h, max_zoom, *next_id);
            *next_id += 1;
            scene
        })
        .collect()
}

/// Split recording events into scenes.
///
/// Scenes are separated by idle gaps >= SCENE_GAP_MS.
/// Large scenes are further split spatially into sub-scenes.
pub fn split_into_scenes(
    events: &[RecordingEvent],
    screen_w: f64,
    screen_h: f64,
    max_zoom: f64,
) -> Vec<Scene> {
    let points = extract_activity_points(events);
    if points.is_empty() {
        return Vec::new();
    }

    // Phase 1: Group points by time gaps
    let mut raw_groups: Vec<Vec<usize>> = Vec::new();
    let mut current_group: Vec<usize> = vec![0];

    for i in 1..points.len() {
        let time_gap = points[i].time_ms.saturating_sub(points[i - 1].time_ms);

        if time_gap >= SCENE_GAP_MS {
            raw_groups.push(current_group);
            current_group = Vec::new();
        }
        current_group.push(i);
    }
    if !current_group.is_empty() {
        raw_groups.push(current_group);
    }

    // Phase 2: Convert groups to scenes (splitting large ones)
    let mut scenes = Vec::new();
    let mut next_id = 0u32;

    for group_indices in &raw_groups {
        let group_points: Vec<&ActivityPoint> =
            group_indices.iter().map(|&i| &points[i]).collect();
        let sub_scenes =
            split_scene_if_needed(&group_points, screen_w, screen_h, max_zoom, &mut next_id);
        scenes.extend(sub_scenes);
    }

    // Phase 3: Merge consecutive scenes with nearby centers
    merge_nearby_scenes(&mut scenes, screen_w, screen_h, max_zoom);

    scenes
}

/// Merge consecutive scenes whose centers are close together.
/// This reduces unnecessary camera movement when activity stays in a similar area.
fn merge_nearby_scenes(scenes: &mut Vec<Scene>, screen_w: f64, screen_h: f64, max_zoom: f64) {
    if scenes.len() < 2 {
        return;
    }

    let mut merged = Vec::with_capacity(scenes.len());
    let mut current = scenes[0].clone();

    for i in 1..scenes.len() {
        let next = &scenes[i];
        let dist = ((current.center_x - next.center_x).powi(2)
            + (current.center_y - next.center_y).powi(2))
        .sqrt();

        if dist <= MERGE_CENTER_DISTANCE_PX {
            // Merge: expand bbox to cover both, keep earlier start and later end
            let min_x = current.bbox.x.min(next.bbox.x);
            let min_y = current.bbox.y.min(next.bbox.y);
            let max_x = (current.bbox.x + current.bbox.width)
                .max(next.bbox.x + next.bbox.width);
            let max_y = (current.bbox.y + current.bbox.height)
                .max(next.bbox.y + next.bbox.height);
            current.bbox = Rect {
                x: min_x,
                y: min_y,
                width: max_x - min_x,
                height: max_y - min_y,
            };
            current.end_ms = next.end_ms;
            current.center_x = current.bbox.x + current.bbox.width / 2.0;
            current.center_y = current.bbox.y + current.bbox.height / 2.0;
            current.zoom_level =
                calc_scene_zoom(&current.bbox, screen_w, screen_h, max_zoom);
            current.event_count += next.event_count;
        } else {
            merged.push(current);
            current = next.clone();
        }
    }
    merged.push(current);

    // Reassign IDs
    for (i, scene) in merged.iter_mut().enumerate() {
        scene.id = i as u32;
    }

    *scenes = merged;
}

/// Expand scene BBoxes using detected frame change regions.
///
/// For each scene, finds change regions within its time range and expands
/// the BBox to include them. This makes zoom levels more appropriate by
/// covering actual visual activity, not just event coordinates.
pub fn expand_scenes_with_change_regions(
    scenes: &mut [Scene],
    change_regions: &[crate::engine::frame_differ::ChangeRegion],
    screen_w: f64,
    screen_h: f64,
    max_zoom: f64,
) {
    for scene in scenes.iter_mut() {
        let relevant: Vec<&crate::engine::frame_differ::ChangeRegion> = change_regions
            .iter()
            .filter(|cr| cr.time_ms >= scene.start_ms && cr.time_ms <= scene.end_ms)
            .collect();

        if relevant.is_empty() {
            continue;
        }

        let expanded = crate::engine::frame_differ::expand_bbox_with_changes(
            &scene.bbox,
            &relevant,
            screen_w,
            screen_h,
        );

        scene.bbox = expanded;
        scene.center_x = scene.bbox.x + scene.bbox.width / 2.0;
        scene.center_y = scene.bbox.y + scene.bbox.height / 2.0;
        scene.zoom_level = calc_scene_zoom(&scene.bbox, screen_w, screen_h, max_zoom);
    }
}

/// Apply a sequence of manual edit operations to a scene list.
///
/// Each operation is applied in order, modifying the scene list.
/// After all edits, scene IDs are reassigned sequentially.
/// Events are needed to recalculate BBox from raw activity points.
pub fn apply_scene_edits(
    scenes: &[Scene],
    edits: &[SceneEditOp],
    events: &[RecordingEvent],
    screen_w: f64,
    screen_h: f64,
    max_zoom: f64,
) -> Vec<Scene> {
    let all_points = extract_activity_points(events);
    let mut result = scenes.to_vec();

    for edit in edits {
        match edit {
            SceneEditOp::Merge { scene_id } => {
                let idx = result.iter().position(|s| s.id == *scene_id);
                if let Some(i) = idx {
                    if i + 1 < result.len() {
                        let merged_start = result[i].start_ms.min(result[i + 1].start_ms);
                        let merged_end = result[i].end_ms.max(result[i + 1].end_ms);
                        let relevant: Vec<&ActivityPoint> = all_points
                            .iter()
                            .filter(|p| p.time_ms >= merged_start && p.time_ms <= merged_end)
                            .collect();
                        if relevant.is_empty() {
                            // Fallback: bbox union (like merge_nearby_scenes)
                            let a = &result[i];
                            let b = &result[i + 1];
                            let min_x = a.bbox.x.min(b.bbox.x);
                            let min_y = a.bbox.y.min(b.bbox.y);
                            let max_x = (a.bbox.x + a.bbox.width).max(b.bbox.x + b.bbox.width);
                            let max_y =
                                (a.bbox.y + a.bbox.height).max(b.bbox.y + b.bbox.height);
                            let bbox = Rect {
                                x: min_x,
                                y: min_y,
                                width: max_x - min_x,
                                height: max_y - min_y,
                            };
                            let merged = Scene {
                                id: 0,
                                start_ms: merged_start,
                                end_ms: merged_end,
                                center_x: bbox.x + bbox.width / 2.0,
                                center_y: bbox.y + bbox.height / 2.0,
                                zoom_level: calc_scene_zoom(&bbox, screen_w, screen_h, max_zoom),
                                event_count: a.event_count + b.event_count,
                                bbox,
                            };
                            result.splice(i..=i + 1, std::iter::once(merged));
                        } else {
                            let merged =
                                make_scene(&relevant, screen_w, screen_h, max_zoom, 0);
                            result.splice(i..=i + 1, std::iter::once(merged));
                        }
                    }
                }
            }
            SceneEditOp::Split { scene_id, split_time_ms } => {
                let idx = result.iter().position(|s| s.id == *scene_id);
                if let Some(i) = idx {
                    let scene = &result[i];
                    let left_points: Vec<&ActivityPoint> = all_points
                        .iter()
                        .filter(|p| p.time_ms >= scene.start_ms && p.time_ms < *split_time_ms)
                        .collect();
                    let right_points: Vec<&ActivityPoint> = all_points
                        .iter()
                        .filter(|p| p.time_ms >= *split_time_ms && p.time_ms <= scene.end_ms)
                        .collect();
                    let mut replacements = Vec::new();
                    if !left_points.is_empty() {
                        replacements.push(make_scene(
                            &left_points, screen_w, screen_h, max_zoom, 0,
                        ));
                    }
                    if !right_points.is_empty() {
                        replacements.push(make_scene(
                            &right_points, screen_w, screen_h, max_zoom, 0,
                        ));
                    }
                    if !replacements.is_empty() {
                        result.splice(i..=i, replacements);
                    }
                }
            }
        }
        // Reassign IDs after each edit
        for (j, scene) in result.iter_mut().enumerate() {
            scene.id = j as u32;
        }
    }

    result
}

/// Compute activity center and zoom for a given time range.
/// Used by frontend when merging/adding zoom segments to get correct
/// center coordinates that cover all user activity in the range.
pub fn compute_activity_center(
    events: &[RecordingEvent],
    start_ms: u64,
    end_ms: u64,
    screen_w: f64,
    screen_h: f64,
    max_zoom: f64,
) -> (f64, f64, f64) {
    let all_points = extract_activity_points(events);
    let relevant: Vec<&ActivityPoint> = all_points
        .iter()
        .filter(|p| p.time_ms >= start_ms && p.time_ms <= end_ms)
        .collect();
    if relevant.is_empty() {
        return (screen_w / 2.0, screen_h / 2.0, 1.0);
    }
    let bbox = compute_bbox(&relevant);
    let zoom = calc_scene_zoom(&bbox, screen_w, screen_h, max_zoom);
    let cx = bbox.x + bbox.width / 2.0;
    let cy = bbox.y + bbox.height / 2.0;
    (cx, cy, zoom)
}

#[cfg(test)]
impl Scene {
    /// Create a Scene for testing purposes.
    pub fn for_test(
        id: u32,
        start_ms: u64,
        end_ms: u64,
        cx: f64,
        cy: f64,
        zoom_level: f64,
    ) -> Self {
        let bbox = Rect {
            x: cx - 100.0,
            y: cy - 50.0,
            width: 200.0,
            height: 100.0,
        };
        Self {
            id,
            start_ms,
            end_ms,
            bbox,
            center_x: cx,
            center_y: cy,
            zoom_level,
            event_count: 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(t: u64, x: f64, y: f64) -> RecordingEvent {
        RecordingEvent::Click {
            t,
            btn: "left".to_string(),
            x,
            y,
        }
    }

    fn key(t: u64) -> RecordingEvent {
        RecordingEvent::Key {
            t,
            key: "KeyA".to_string(),
            modifiers: vec![],
        }
    }

    #[test]
    fn test_empty_events() {
        let scenes = split_into_scenes(&[], 1920.0, 1080.0, 3.0);
        assert!(scenes.is_empty());
    }

    #[test]
    fn test_single_click_one_scene() {
        let events = vec![click(0, 500.0, 300.0)];
        let scenes = split_into_scenes(&events, 1920.0, 1080.0, 3.0);
        assert_eq!(scenes.len(), 1);
        assert_eq!(scenes[0].event_count, 1);
    }

    #[test]
    fn test_nearby_clicks_one_scene() {
        let events = vec![
            click(0, 500.0, 300.0),
            click(500, 520.0, 310.0),
            click(1000, 490.0, 290.0),
        ];
        let scenes = split_into_scenes(&events, 1920.0, 1080.0, 3.0);
        assert_eq!(scenes.len(), 1);
        assert_eq!(scenes[0].event_count, 3);
    }

    #[test]
    fn test_idle_gap_splits_scenes() {
        let events = vec![
            click(0, 500.0, 300.0),
            click(500, 520.0, 310.0),
            // 6000ms gap > SCENE_GAP_MS (1500ms)
            click(6500, 800.0, 600.0),
            click(7000, 810.0, 610.0),
        ];
        let scenes = split_into_scenes(&events, 1920.0, 1080.0, 3.0);
        assert_eq!(scenes.len(), 2);
    }

    #[test]
    fn test_key_events_use_click_position() {
        let events = vec![click(0, 500.0, 300.0), key(200), key(400), key(600)];
        let scenes = split_into_scenes(&events, 1920.0, 1080.0, 3.0);
        assert_eq!(scenes.len(), 1);
        assert_eq!(scenes[0].event_count, 4);
        // Center should be near the click position
        assert!(
            (scenes[0].center_x - 500.0).abs() < 200.0,
            "Scene center should be near click pos, got {}",
            scenes[0].center_x
        );
    }

    #[test]
    fn test_key_events_skipped_without_recent_click() {
        // Keys without any recent click should be skipped (no position)
        let events = vec![
            key(3000),
            key(3200),
            key(3400),
        ];
        let scenes = split_into_scenes(&events, 1920.0, 1080.0, 3.0);
        assert!(scenes.is_empty(), "Keys without position should produce no scenes");
    }

    #[test]
    fn test_key_events_use_recent_click() {
        let events = vec![
            click(500, 200.0, 200.0),
            // Keys at 1000ms — click at 500ms is < 2000ms old
            key(1000),
            key(1200),
        ];
        let scenes = split_into_scenes(&events, 1920.0, 1080.0, 3.0);
        assert_eq!(scenes.len(), 1);
        // Should be near click position (200, 200)
        assert!(
            (scenes[0].center_x - 200.0).abs() < 200.0,
            "Scene center should be near click position, got {}",
            scenes[0].center_x
        );
    }

    #[test]
    fn test_zoom_level_in_range() {
        let events = vec![click(0, 500.0, 300.0), click(500, 700.0, 500.0)];
        let scenes = split_into_scenes(&events, 1920.0, 1080.0, 3.0);
        assert_eq!(scenes.len(), 1);
        assert!(scenes[0].zoom_level >= 1.0);
        assert!(scenes[0].zoom_level <= 3.0);
    }

    #[test]
    fn test_calc_window_zoom_range() {
        let rect = Rect {
            x: 100.0,
            y: 100.0,
            width: 800.0,
            height: 600.0,
        };
        let zoom = calc_window_zoom(&rect, 1920.0, 1080.0, 3.0);
        assert!(zoom >= 1.0);
        assert!(zoom <= 3.0);
    }

    #[test]
    fn test_large_scene_splits_spatially() {
        // Events spanning most of the screen with time gaps > SCENE_GAP_MS
        let events = vec![
            click(0, 100.0, 100.0),
            click(200, 120.0, 110.0),
            // 2500ms gap + 1400px jump → sub-scene split
            click(2700, 1800.0, 900.0),
            click(2900, 1820.0, 910.0),
        ];
        let scenes = split_into_scenes(&events, 1920.0, 1080.0, 3.0);
        // Should split because the bbox would span most of the screen
        assert!(
            scenes.len() >= 2,
            "Large spatial span should cause split, got {} scenes",
            scenes.len()
        );
    }

    #[test]
    fn test_rapid_clicks_not_split_despite_distance() {
        // Two clicks 800ms apart and 800px apart
        // Should NOT be split (only 2 points, below min 3 for sub-scene splitting)
        let events = vec![
            click(5700, 400.0, 200.0),
            click(6500, 1200.0, 800.0),
        ];
        let scenes = split_into_scenes(&events, 1920.0, 1080.0, 3.0);
        assert_eq!(
            scenes.len(), 1,
            "Rapid clicks (800ms gap) should be in one scene, got {}",
            scenes.len()
        );
    }

    // --- expand_scenes_with_change_regions tests ---

    #[test]
    fn test_expand_with_no_regions() {
        let mut scenes = vec![Scene::for_test(0, 0, 2000, 500.0, 300.0, 2.5)];
        let original_width = scenes[0].bbox.width;
        expand_scenes_with_change_regions(&mut scenes, &[], 1920.0, 1080.0, 3.0);
        assert_eq!(scenes[0].bbox.width, original_width);
    }

    #[test]
    fn test_expand_widens_bbox() {
        let mut scenes = vec![Scene::for_test(0, 0, 2000, 500.0, 300.0, 3.0)];
        let original_width = scenes[0].bbox.width;
        let regions = vec![crate::engine::frame_differ::ChangeRegion {
            time_ms: 1000,
            bbox: Rect {
                x: 200.0,
                y: 100.0,
                width: 600.0,
                height: 400.0,
            },
            changed_pixel_count: 5000,
        }];
        expand_scenes_with_change_regions(&mut scenes, &regions, 1920.0, 1080.0, 3.0);
        assert!(
            scenes[0].bbox.width > original_width,
            "BBox should be wider after expansion"
        );
        assert!(
            scenes[0].zoom_level < 3.0,
            "Zoom should decrease with wider BBox"
        );
    }

    #[test]
    fn test_expand_ignores_out_of_range_regions() {
        let mut scenes = vec![Scene::for_test(0, 1000, 2000, 500.0, 300.0, 3.0)];
        let original_zoom = scenes[0].zoom_level;
        let regions = vec![crate::engine::frame_differ::ChangeRegion {
            time_ms: 5000, // outside scene range
            bbox: Rect {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            },
            changed_pixel_count: 100000,
        }];
        expand_scenes_with_change_regions(&mut scenes, &regions, 1920.0, 1080.0, 3.0);
        assert_eq!(scenes[0].zoom_level, original_zoom);
    }

    // --- merge_nearby_scenes tests ---

    #[test]
    fn test_merge_nearby_scenes() {
        // Two scenes with centers 100px apart → should merge
        let mut scenes = vec![
            Scene::for_test(0, 0, 2000, 500.0, 300.0, 2.0),
            Scene::for_test(1, 2500, 4000, 550.0, 340.0, 2.0),
        ];
        merge_nearby_scenes(&mut scenes, 1920.0, 1080.0, 2.0);
        assert_eq!(scenes.len(), 1, "Nearby scenes should merge");
        assert_eq!(scenes[0].start_ms, 0);
        assert_eq!(scenes[0].end_ms, 4000);
        assert_eq!(scenes[0].event_count, 6); // 3 + 3
    }

    #[test]
    fn test_no_merge_distant_scenes() {
        // Two scenes with centers 800px apart → should not merge
        let mut scenes = vec![
            Scene::for_test(0, 0, 2000, 200.0, 200.0, 2.0),
            Scene::for_test(1, 2500, 4000, 1000.0, 800.0, 2.0),
        ];
        merge_nearby_scenes(&mut scenes, 1920.0, 1080.0, 2.0);
        assert_eq!(scenes.len(), 2, "Distant scenes should not merge");
    }

    #[test]
    fn test_merge_chain() {
        // Three scenes each 100px apart → all should merge into one
        let mut scenes = vec![
            Scene::for_test(0, 0, 1000, 500.0, 300.0, 2.0),
            Scene::for_test(1, 1500, 2500, 550.0, 330.0, 2.0),
            Scene::for_test(2, 3000, 4000, 600.0, 360.0, 2.0),
        ];
        merge_nearby_scenes(&mut scenes, 1920.0, 1080.0, 2.0);
        assert_eq!(scenes.len(), 1, "Chain of nearby scenes should merge");
        assert_eq!(scenes[0].event_count, 9);
        assert_eq!(scenes[0].id, 0);
    }

    #[test]
    fn test_expand_clamps_to_screen() {
        let mut scenes = vec![Scene::for_test(0, 0, 2000, 500.0, 300.0, 3.0)];
        let regions = vec![crate::engine::frame_differ::ChangeRegion {
            time_ms: 1000,
            bbox: Rect {
                x: -100.0,
                y: -50.0,
                width: 2200.0,
                height: 1200.0,
            },
            changed_pixel_count: 50000,
        }];
        expand_scenes_with_change_regions(&mut scenes, &regions, 1920.0, 1080.0, 3.0);
        assert!(scenes[0].bbox.x >= 0.0);
        assert!(scenes[0].bbox.y >= 0.0);
        assert!(scenes[0].bbox.x + scenes[0].bbox.width <= 1920.0);
        assert!(scenes[0].bbox.y + scenes[0].bbox.height <= 1080.0);
    }

    // --- apply_scene_edits tests ---

    #[test]
    fn test_merge_two_scenes() {
        let events = vec![
            click(0, 200.0, 200.0),
            click(500, 210.0, 210.0),
            // gap > 1500ms
            click(3000, 800.0, 600.0),
            click(3500, 810.0, 610.0),
        ];
        let scenes = split_into_scenes(&events, 1920.0, 1080.0, 3.0);
        assert_eq!(scenes.len(), 2);

        let edits = vec![SceneEditOp::Merge { scene_id: 0 }];
        let result = apply_scene_edits(&scenes, &edits, &events, 1920.0, 1080.0, 3.0);
        assert_eq!(result.len(), 1, "Merge should combine into one scene");
        assert_eq!(result[0].id, 0);
        assert_eq!(result[0].event_count, 4);
    }

    #[test]
    fn test_split_scene() {
        // All events within 1400ms gap (< SCENE_GAP_MS=1500) so they form one scene
        let events = vec![
            click(0, 200.0, 200.0),
            click(500, 210.0, 210.0),
            click(1400, 220.0, 220.0),
            click(1900, 230.0, 230.0),
        ];
        let scenes = split_into_scenes(&events, 1920.0, 1080.0, 3.0);
        assert_eq!(scenes.len(), 1);

        let edits = vec![SceneEditOp::Split {
            scene_id: 0,
            split_time_ms: 1000,
        }];
        let result = apply_scene_edits(&scenes, &edits, &events, 1920.0, 1080.0, 3.0);
        assert_eq!(result.len(), 2, "Split should create two scenes");
        assert_eq!(result[0].id, 0);
        assert_eq!(result[1].id, 1);
        assert_eq!(result[0].event_count, 2);
        assert_eq!(result[1].event_count, 2);
    }

    #[test]
    fn test_merge_then_split() {
        let events = vec![
            click(0, 200.0, 200.0),
            click(500, 210.0, 210.0),
            click(3000, 800.0, 600.0),
            click(3500, 810.0, 610.0),
        ];
        let scenes = split_into_scenes(&events, 1920.0, 1080.0, 3.0);
        assert_eq!(scenes.len(), 2);

        // Merge S0+S1, then split the merged scene
        let edits = vec![
            SceneEditOp::Merge { scene_id: 0 },
            SceneEditOp::Split { scene_id: 0, split_time_ms: 1500 },
        ];
        let result = apply_scene_edits(&scenes, &edits, &events, 1920.0, 1080.0, 3.0);
        assert_eq!(result.len(), 2, "Merge then split should give 2 scenes");
    }

    #[test]
    fn test_merge_invalid_id() {
        let events = vec![click(0, 200.0, 200.0)];
        let scenes = split_into_scenes(&events, 1920.0, 1080.0, 3.0);
        let edits = vec![SceneEditOp::Merge { scene_id: 99 }];
        let result = apply_scene_edits(&scenes, &edits, &events, 1920.0, 1080.0, 3.0);
        assert_eq!(result.len(), scenes.len(), "Invalid merge should be no-op");
    }
}
