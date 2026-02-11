//! Lookahead zoom planner with 2-state model (Overview ↔ WorkArea).
//!
//! Generates zoom keyframes by analyzing ALL scenes before making any decisions.
//! Uses a simple 2-state transition model:
//! - **Overview**: Full screen (display mode) or window view (window mode)
//! - **WorkArea**: Zoomed into the active focus area
//!
//! Idle detection considers both user input events AND frame changes:
//! zoom-out only occurs when there are no events AND no screen changes.

use crate::config::{EffectsSettings, RecordingMeta};
use crate::engine::frame_differ::ChangeRegion;
use crate::engine::scene_splitter::{calc_window_zoom, Scene};
use crate::engine::analyzer::Rect;
use serde::{Deserialize, Serialize};

// ------------------------------------------------------------------
// Data types (shared with compositor and frontend)
// ------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransitionType {
    SpringIn,
    SpringOut,
    Smooth,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpringHint {
    pub zoom_half_life: f64,
    pub pan_half_life: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoomKeyframe {
    pub time_ms: u64,
    pub target_x: f64,
    pub target_y: f64,
    pub zoom_level: f64,
    pub transition: TransitionType,
    #[serde(default)]
    pub spring_hint: Option<SpringHint>,
}

// ------------------------------------------------------------------
// Half-lives for different transitions (seconds)
// ------------------------------------------------------------------
mod half_lives {
    /// Zoom-in to first scene or after idle
    pub const ZOOM_IN: f64 = 0.20;
    pub const ZOOM_IN_PAN: f64 = 0.20;
    /// Scene-to-scene transition (smooth pan/zoom)
    pub const SCENE_TO_SCENE_ZOOM: f64 = 0.25;
    pub const SCENE_TO_SCENE_PAN: f64 = 0.25;
    /// Idle zoom-out to overview
    pub const ZOOMOUT_ZOOM: f64 = 0.35;
    pub const ZOOMOUT_PAN: f64 = 0.30;
}

/// Anticipation multiplier: how many half-lives before a scene to start moving.
/// At 4× half-life, the spring reaches ~93.75% of target.
const ANTICIPATION_HALF_LIVES: f64 = 4.0;
/// Minimum gap between keyframes to avoid jitter.
const MIN_KEYFRAME_INTERVAL_MS: u64 = 800;

// ------------------------------------------------------------------
// Public API
// ------------------------------------------------------------------

/// Generate zoom keyframes using the 2-state (Overview ↔ WorkArea) model.
///
/// Design principles:
/// 1. All scenes are known upfront (no incremental discovery)
/// 2. Camera moves BEFORE each scene starts (anticipation)
/// 3. First scene is targeted from t=0
/// 4. Idle gaps between scenes trigger zoom-out to overview
///    (only if there are no frame changes in the gap)
pub fn generate_zoom_plan(
    scenes: &[Scene],
    meta: &RecordingMeta,
    settings: &EffectsSettings,
    change_regions: &[ChangeRegion],
) -> Vec<ZoomKeyframe> {
    if scenes.is_empty() {
        return Vec::new();
    }

    let screen_w = meta.screen_width as f64;
    let screen_h = meta.screen_height as f64;
    let scale = settings.animation_speed.speed_scale();
    let idle_ms = settings.idle_zoom_out_ms;

    // Compute overview target based on recording mode
    let (overview_x, overview_y, overview_zoom) =
        compute_overview_target(meta, screen_w, screen_h, settings.max_zoom);

    let is_window_mode = meta.recording_mode.as_deref() == Some("window");

    let mut plan: Vec<ZoomKeyframe> = Vec::new();

    // 録画開始は必ずOverviewから（Windowモードの場合はWindow表示）
    plan.push(ZoomKeyframe {
        time_ms: 0,
        target_x: overview_x,
        target_y: overview_y,
        zoom_level: overview_zoom,
        transition: TransitionType::SpringOut,
        spring_hint: Some(SpringHint {
            zoom_half_life: half_lives::ZOOMOUT_ZOOM * scale,
            pan_half_life: half_lives::ZOOMOUT_PAN * scale,
        }),
    });

    for (i, scene) in scenes.iter().enumerate() {
        let is_first = i == 0;
        let prev_scene = if i > 0 { Some(&scenes[i - 1]) } else { None };
        let gap_before = if is_first {
            scene.start_ms // t=0からの距離
        } else {
            prev_scene
                .map(|ps| scene.start_ms.saturating_sub(ps.end_ms))
                .unwrap_or(0)
        };

        // --- Idle zoom-out between scenes ---
        // 最初のシーンはスキップ（t=0で既にOverview配置済み）
        if !is_first && gap_before >= idle_ms {
            if let Some(ps) = prev_scene {
                // Check if there are frame changes in the gap
                let gap_start = ps.end_ms;
                let gap_end = scene.start_ms;
                let has_screen_changes = change_regions
                    .iter()
                    .any(|cr| cr.time_ms > gap_start && cr.time_ms < gap_end);

                if !has_screen_changes {
                    let zoomout_time = ps.end_ms + idle_ms.min(gap_before / 3).min(2000);
                    if should_emit(&plan, zoomout_time) {
                        plan.push(ZoomKeyframe {
                            time_ms: zoomout_time,
                            target_x: overview_x,
                            target_y: overview_y,
                            zoom_level: overview_zoom,
                            transition: TransitionType::SpringOut,
                            spring_hint: Some(SpringHint {
                                zoom_half_life: half_lives::ZOOMOUT_ZOOM * scale,
                                pan_half_life: half_lives::ZOOMOUT_PAN * scale,
                            }),
                        });
                    }
                }
            }
        }

        // --- Anticipatory zoom-in to scene ---

        // Choose half-lives based on context
        let (zoom_hl, pan_hl) = if is_first || gap_before >= idle_ms {
            // Coming from overview → faster zoom-in
            (half_lives::ZOOM_IN, half_lives::ZOOM_IN_PAN)
        } else {
            // Scene-to-scene within same activity → smooth transition
            (
                half_lives::SCENE_TO_SCENE_ZOOM,
                half_lives::SCENE_TO_SCENE_PAN,
            )
        };

        let transition = if is_first || gap_before >= idle_ms {
            TransitionType::SpringIn
        } else {
            TransitionType::Smooth
        };

        // Calculate anticipation: start camera movement before scene begins
        let anticipation_ms = (pan_hl * scale * ANTICIPATION_HALF_LIVES * 1000.0) as u64;

        // 全シーン共通: 先読み配置（最初のシーンもOverview後に先読みズームイン）
        let kf_time = {
            let earliest = prev_scene.map_or(0, |ps| ps.end_ms);
            let anticipated = scene.start_ms.saturating_sub(anticipation_ms);
            let min_after_last = plan
                .last()
                .map_or(0, |kf| kf.time_ms + MIN_KEYFRAME_INTERVAL_MS);
            anticipated.max(earliest).max(min_after_last)
        };

        // Windowモード: WorkAreaのzoomがOverview以下になるようクランプ
        // （ウィンドウ全体表示より拡大しない。パンのみで追従）
        let clamped_zoom = if is_window_mode {
            scene.zoom_level.min(overview_zoom)
        } else {
            scene.zoom_level
        };

        plan.push(ZoomKeyframe {
            time_ms: kf_time,
            target_x: scene.center_x,
            target_y: scene.center_y,
            zoom_level: clamped_zoom,
            transition,
            spring_hint: Some(SpringHint {
                zoom_half_life: zoom_hl * scale,
                pan_half_life: pan_hl * scale,
            }),
        });
    }

    // Handle trailing idle (after last scene)
    if let Some(last) = scenes.last() {
        let remaining = meta.duration_ms.saturating_sub(last.end_ms);
        if remaining >= idle_ms {
            let trailing_start = last.end_ms;
            let trailing_end = meta.duration_ms;
            let has_screen_changes = change_regions
                .iter()
                .any(|cr| cr.time_ms > trailing_start && cr.time_ms < trailing_end);

            if !has_screen_changes {
                let zoomout_time = last.end_ms + idle_ms.min(remaining / 3);
                if should_emit(&plan, zoomout_time) {
                    plan.push(ZoomKeyframe {
                        time_ms: zoomout_time,
                        target_x: overview_x,
                        target_y: overview_y,
                        zoom_level: overview_zoom,
                        transition: TransitionType::SpringOut,
                        spring_hint: Some(SpringHint {
                            zoom_half_life: half_lives::ZOOMOUT_ZOOM * scale,
                            pan_half_life: half_lives::ZOOMOUT_PAN * scale,
                        }),
                    });
                }
            }
        }
    }

    // Final sort (should already be sorted)
    plan.sort_by_key(|kf| kf.time_ms);

    // Remove keyframes that are too close together
    deduplicate_keyframes(&mut plan, MIN_KEYFRAME_INTERVAL_MS);

    plan
}

/// Compute the overview (zoomed-out) target based on recording mode.
///
/// - Display mode: zoom 1.0 at screen center
/// - Window mode: zoom to fit window_initial_rect
fn compute_overview_target(
    meta: &RecordingMeta,
    screen_w: f64,
    screen_h: f64,
    max_zoom: f64,
) -> (f64, f64, f64) {
    let is_window_mode = meta.recording_mode.as_deref() == Some("window");

    if is_window_mode {
        if let Some(ref rect) = meta.window_initial_rect {
            let win_rect = Rect {
                x: rect[0],
                y: rect[1],
                width: rect[2] - rect[0],
                height: rect[3] - rect[1],
            };
            let zoom = calc_window_zoom(&win_rect, screen_w, screen_h, max_zoom);
            let cx = win_rect.x + win_rect.width / 2.0;
            let cy = win_rect.y + win_rect.height / 2.0;
            return (cx, cy, zoom);
        }
    }

    // Display mode (default): full screen overview
    (screen_w / 2.0, screen_h / 2.0, 1.0)
}

/// Check if we should emit a keyframe at this time (not too close to the last one).
fn should_emit(plan: &[ZoomKeyframe], time_ms: u64) -> bool {
    plan.last()
        .map_or(true, |kf| time_ms > kf.time_ms + MIN_KEYFRAME_INTERVAL_MS)
}

fn deduplicate_keyframes(plan: &mut Vec<ZoomKeyframe>, min_interval_ms: u64) {
    if plan.len() < 2 {
        return;
    }
    let mut i = 0;
    while i + 1 < plan.len() {
        let dt = plan[i + 1].time_ms.saturating_sub(plan[i].time_ms);
        if dt < min_interval_ms {
            // Keep the later keyframe (more recent decision)
            plan.remove(i);
            continue;
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AnimationSpeed, ZoomIntensity};

    fn test_meta() -> RecordingMeta {
        RecordingMeta {
            version: 2,
            id: "test".to_string(),
            screen_width: 1920,
            screen_height: 1080,
            fps: 30,
            start_time: "2024-01-01T00:00:00Z".to_string(),
            duration_ms: 30000,
            has_audio: false,
            monitor_scale: 1.0,
            recording_dir: "/tmp".to_string(),
            recording_mode: None, // Display mode
            window_title: None,
            window_initial_rect: None,
        }
    }

    fn test_settings() -> EffectsSettings {
        EffectsSettings {
            auto_zoom_enabled: true,
            default_zoom_level: 2.0,
            text_input_zoom_level: 2.5,
            max_zoom: 3.0,
            idle_timeout_ms: 3000,
            click_ring_enabled: true,
            key_badge_enabled: true,
            cursor_smoothing: true,
            zoom_intensity: ZoomIntensity::Balanced,
            animation_speed: AnimationSpeed::Mellow,
            smart_zoom_enabled: true,
            motion_blur_enabled: false,
            frame_diff_enabled: true,
            idle_zoom_out_ms: 5000,
            idle_overview_ms: 8000,
            min_workarea_dwell_ms: 2000,
            min_window_dwell_ms: 1500,
            cluster_lifetime_ms: 5000,
            cluster_stability_ms: 1000,
        }
    }

    #[test]
    fn test_empty_scenes_no_keyframes() {
        let plan = generate_zoom_plan(&[], &test_meta(), &test_settings(), &[]);
        assert!(plan.is_empty());
    }

    #[test]
    fn test_starts_with_overview() {
        let scenes = vec![Scene::for_test(0, 500, 3000, 500.0, 300.0, 2.0)];
        let plan = generate_zoom_plan(&scenes, &test_meta(), &test_settings(), &[]);
        assert!(plan.len() >= 2);
        // plan[0] = Overview at t=0
        assert_eq!(plan[0].time_ms, 0);
        assert!(
            (plan[0].zoom_level - 1.0).abs() < 0.01,
            "Should start with overview (zoom 1.0), got {:.2}",
            plan[0].zoom_level
        );
        assert!(matches!(plan[0].transition, TransitionType::SpringOut));
        // plan[1] = Scene's WorkArea (SpringIn with anticipation)
        assert!((plan[1].target_x - 500.0).abs() < 1.0);
        assert!(plan[1].zoom_level > 1.0);
        assert!(matches!(plan[1].transition, TransitionType::SpringIn));
        assert!(
            plan[1].time_ms > 0,
            "Scene keyframe should be after overview"
        );
    }

    #[test]
    fn test_two_scenes_with_anticipation() {
        let scenes = vec![
            Scene::for_test(0, 0, 2000, 500.0, 300.0, 2.0),
            Scene::for_test(1, 8000, 10000, 1500.0, 800.0, 2.0),
        ];
        let plan = generate_zoom_plan(&scenes, &test_meta(), &test_settings(), &[]);
        assert!(plan.len() >= 2);

        // Second scene's keyframe should be BEFORE it starts (anticipation)
        let scene2_kf = plan
            .iter()
            .find(|kf| (kf.target_x - 1500.0).abs() < 1.0)
            .unwrap();
        assert!(
            scene2_kf.time_ms < 8000,
            "Keyframe should anticipate scene start, got t={}",
            scene2_kf.time_ms
        );
        assert!(
            scene2_kf.time_ms >= 2000,
            "Keyframe should not overlap previous scene, got t={}",
            scene2_kf.time_ms
        );
    }

    #[test]
    fn test_idle_gap_generates_zoomout_display_mode() {
        let scenes = vec![
            Scene::for_test(0, 0, 2000, 500.0, 300.0, 2.0),
            // 8000ms gap → should trigger idle zoom-out to overview (1.0x)
            Scene::for_test(1, 10000, 12000, 1500.0, 800.0, 2.0),
        ];
        let plan = generate_zoom_plan(&scenes, &test_meta(), &test_settings(), &[]);
        // t>0のSpringOut = idle zoom-out（t=0はOverview初期配置）
        let zoomout = plan
            .iter()
            .find(|kf| kf.time_ms > 0 && matches!(kf.transition, TransitionType::SpringOut));
        assert!(zoomout.is_some(), "Should have idle zoom-out keyframe");
        assert!(
            (zoomout.unwrap().zoom_level - 1.0).abs() < 0.01,
            "Display mode should zoom out to 1.0, got {:.2}",
            zoomout.unwrap().zoom_level,
        );
    }

    #[test]
    fn test_idle_gap_with_screen_changes_no_zoomout() {
        let scenes = vec![
            Scene::for_test(0, 0, 2000, 500.0, 300.0, 2.0),
            Scene::for_test(1, 10000, 12000, 1500.0, 800.0, 2.0),
        ];
        // Frame changes exist in the idle gap
        let change_regions = vec![
            ChangeRegion {
                time_ms: 5000,
                bbox: Rect { x: 400.0, y: 200.0, width: 300.0, height: 200.0 },
                changed_pixel_count: 5000,
            },
        ];
        // Use short duration to avoid trailing idle zoom-out after last scene
        let mut meta = test_meta();
        meta.duration_ms = 13000;
        let plan = generate_zoom_plan(&scenes, &meta, &test_settings(), &change_regions);
        // t>0のSpringOutがないことを確認（t=0のOverviewは除外）
        let zoomout = plan
            .iter()
            .find(|kf| kf.time_ms > 0 && matches!(kf.transition, TransitionType::SpringOut));
        assert!(
            zoomout.is_none(),
            "Should NOT zoom out when screen changes exist in idle gap"
        );
    }

    #[test]
    fn test_window_mode_starts_with_window_overview() {
        let mut meta = test_meta();
        meta.recording_mode = Some("window".to_string());
        meta.window_initial_rect = Some([100.0, 100.0, 900.0, 700.0]); // 800x600 window

        let scenes = vec![
            Scene::for_test(0, 0, 2000, 300.0, 300.0, 2.0),
            Scene::for_test(1, 10000, 12000, 700.0, 500.0, 2.0),
        ];
        let plan = generate_zoom_plan(&scenes, &meta, &test_settings(), &[]);
        // t=0のOverviewがWindow表示であることを確認
        assert_eq!(plan[0].time_ms, 0);
        assert!(matches!(plan[0].transition, TransitionType::SpringOut));
        assert!(
            plan[0].zoom_level > 1.0,
            "Window mode should start at window zoom level, not full screen, got {:.2}",
            plan[0].zoom_level,
        );
        // Center should be near window center (500, 400)
        assert!(
            (plan[0].target_x - 500.0).abs() < 50.0,
            "Target X should be window center, got {}",
            plan[0].target_x,
        );
    }

    #[test]
    fn test_close_scenes_smooth_transition() {
        let scenes = vec![
            Scene::for_test(0, 0, 2000, 500.0, 300.0, 2.0),
            // 3000ms gap < idle_zoom_out_ms → smooth transition
            Scene::for_test(1, 5000, 7000, 800.0, 500.0, 2.0),
        ];
        let plan = generate_zoom_plan(&scenes, &test_meta(), &test_settings(), &[]);
        let scene2_kf = plan.iter().find(|kf| (kf.target_x - 800.0).abs() < 1.0);
        assert!(scene2_kf.is_some());
        assert!(matches!(
            scene2_kf.unwrap().transition,
            TransitionType::Smooth
        ));
    }

    #[test]
    fn test_no_cut_transitions() {
        let scenes = vec![
            Scene::for_test(0, 0, 2000, 100.0, 100.0, 2.0),
            Scene::for_test(1, 5000, 8000, 1800.0, 900.0, 2.0),
        ];
        let plan = generate_zoom_plan(&scenes, &test_meta(), &test_settings(), &[]);
        for kf in &plan {
            match kf.transition {
                TransitionType::SpringIn | TransitionType::SpringOut | TransitionType::Smooth => {}
            }
        }
    }

    #[test]
    fn test_trailing_idle_zoomout() {
        let meta = RecordingMeta {
            duration_ms: 30000,
            ..test_meta()
        };
        let scenes = vec![
            // Scene ends at 5000ms, recording is 30000ms → 25s remaining
            Scene::for_test(0, 0, 5000, 500.0, 300.0, 2.0),
        ];
        let plan = generate_zoom_plan(&scenes, &meta, &test_settings(), &[]);
        // t>0のSpringOut = trailing zoom-out
        let zoomout = plan
            .iter()
            .find(|kf| kf.time_ms > 0 && matches!(kf.transition, TransitionType::SpringOut));
        assert!(
            zoomout.is_some(),
            "Should have trailing zoom-out for long remaining time"
        );
    }

    #[test]
    fn test_trailing_idle_no_zoomout_with_screen_changes() {
        let meta = RecordingMeta {
            duration_ms: 30000,
            ..test_meta()
        };
        let scenes = vec![
            Scene::for_test(0, 0, 5000, 500.0, 300.0, 2.0),
        ];
        // Screen changes after last scene
        let change_regions = vec![
            ChangeRegion {
                time_ms: 10000,
                bbox: Rect { x: 400.0, y: 200.0, width: 300.0, height: 200.0 },
                changed_pixel_count: 5000,
            },
        ];
        let plan = generate_zoom_plan(&scenes, &meta, &test_settings(), &change_regions);
        // t>0のSpringOutがないことを確認（t=0のOverviewは除外）
        let zoomout = plan
            .iter()
            .find(|kf| kf.time_ms > 0 && matches!(kf.transition, TransitionType::SpringOut));
        assert!(
            zoomout.is_none(),
            "Should NOT have trailing zoom-out when screen changes exist"
        );
    }
}
