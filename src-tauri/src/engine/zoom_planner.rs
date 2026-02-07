use super::analyzer::{IdleLevel, Segment, SegmentType};
use super::spring::SpringHalfLife;
use crate::config::RecordingMeta;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransitionType {
    SpringIn,
    SpringOut,
    Smooth,
    Cut,
    WindowZoom,
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

const MIN_ZOOM_INTERVAL_MS: u64 = 300;
const CUT_DISTANCE_THRESHOLD: f64 = 0.5; // 50% of screen

pub fn generate_zoom_plan(
    segments: &[Segment],
    meta: &RecordingMeta,
    default_zoom: f64,
    text_input_zoom: f64,
    max_zoom: f64,
) -> Vec<ZoomKeyframe> {
    let mut plan = Vec::new();
    let screen_w = meta.screen_width as f64;
    let screen_h = meta.screen_height as f64;

    for seg in segments {
        // Two-stage zoom: if window changed, first zoom to window, then to action point
        if seg.window_changed {
            if let Some(ref win_rect) = seg.window_rect {
                let win_cx = win_rect.center_x();
                let win_cy = win_rect.center_y();
                let win_zoom = calc_zoom_to_fit(win_rect, screen_w, screen_h, 0.1)
                    .min(max_zoom)
                    .max(1.2);

                plan.push(ZoomKeyframe {
                    time_ms: seg.start_ms.saturating_sub(400),
                    target_x: win_cx,
                    target_y: win_cy,
                    zoom_level: win_zoom,
                    transition: TransitionType::WindowZoom,
                    spring_hint: Some(SpringHint {
                        zoom_half_life: SpringHalfLife::WINDOW_ZOOM,
                        pan_half_life: SpringHalfLife::WINDOW_PAN,
                    }),
                });
            }
        }

        match seg.segment_type {
            SegmentType::Click => {
                if let Some(ref fp) = seg.focus_point {
                    plan.push(ZoomKeyframe {
                        time_ms: seg.start_ms,
                        target_x: fp.x,
                        target_y: fp.y,
                        zoom_level: default_zoom,
                        transition: TransitionType::SpringIn,
                        spring_hint: Some(SpringHint {
                            zoom_half_life: SpringHalfLife::ZOOM_IN_FAST,
                            pan_half_life: SpringHalfLife::VIEWPORT_PAN,
                        }),
                    });
                }
            }
            SegmentType::TextInput => {
                if let Some(ref fp) = seg.focus_point {
                    let zoom = if let Some(ref rect) = fp.region {
                        calc_zoom_to_fit(rect, screen_w, screen_h, 0.3).min(max_zoom)
                    } else {
                        text_input_zoom.min(max_zoom)
                    };

                    plan.push(ZoomKeyframe {
                        time_ms: seg.start_ms,
                        target_x: fp.x,
                        target_y: fp.y,
                        zoom_level: zoom,
                        transition: TransitionType::SpringIn,
                        spring_hint: Some(SpringHint {
                            zoom_half_life: SpringHalfLife::ZOOM_IN,
                            pan_half_life: SpringHalfLife::VIEWPORT_PAN,
                        }),
                    });
                }
            }
            SegmentType::Scroll => {
                plan.push(ZoomKeyframe {
                    time_ms: seg.start_ms,
                    target_x: screen_w / 2.0,
                    target_y: screen_h / 2.0,
                    zoom_level: 1.2,
                    transition: TransitionType::Smooth,
                    spring_hint: Some(SpringHint {
                        zoom_half_life: SpringHalfLife::ZOOM_IN_SLOW,
                        pan_half_life: SpringHalfLife::VIEWPORT_PAN,
                    }),
                });
            }
            SegmentType::Idle => {
                match seg.idle_level {
                    Some(IdleLevel::Medium) => {
                        plan.push(ZoomKeyframe {
                            time_ms: seg.start_ms + 300,
                            target_x: screen_w / 2.0,
                            target_y: screen_h / 2.0,
                            zoom_level: 1.2,
                            transition: TransitionType::SpringOut,
                            spring_hint: Some(SpringHint {
                                zoom_half_life: SpringHalfLife::ZOOM_OUT,
                                pan_half_life: SpringHalfLife::VIEWPORT_PAN,
                            }),
                        });
                    }
                    Some(IdleLevel::Long) => {
                        plan.push(ZoomKeyframe {
                            time_ms: seg.start_ms + 300,
                            target_x: screen_w / 2.0,
                            target_y: screen_h / 2.0,
                            zoom_level: 1.0,
                            transition: TransitionType::SpringOut,
                            spring_hint: Some(SpringHint {
                                zoom_half_life: SpringHalfLife::ZOOM_OUT_SLOW,
                                pan_half_life: SpringHalfLife::VIEWPORT_PAN,
                            }),
                        });
                    }
                    _ => {}
                }
            }
            SegmentType::RapidAction => {
                if let Some(ref fp) = seg.focus_point {
                    plan.push(ZoomKeyframe {
                        time_ms: seg.start_ms,
                        target_x: fp.x,
                        target_y: fp.y,
                        zoom_level: 1.8,
                        transition: TransitionType::SpringIn,
                        spring_hint: Some(SpringHint {
                            zoom_half_life: SpringHalfLife::ZOOM_IN_FAST,
                            pan_half_life: SpringHalfLife::VIEWPORT_PAN,
                        }),
                    });
                }
            }
        }
    }

    deduplicate_keyframes(&mut plan, MIN_ZOOM_INTERVAL_MS);
    detect_cuts(&mut plan, screen_w, screen_h, CUT_DISTANCE_THRESHOLD);

    plan
}

fn calc_zoom_to_fit(
    rect: &super::analyzer::Rect,
    screen_width: f64,
    screen_height: f64,
    padding: f64,
) -> f64 {
    let padded_w = rect.width * (1.0 + padding);
    let padded_h = rect.height * (1.0 + padding);

    let zoom_x = screen_width / padded_w;
    let zoom_y = screen_height / padded_h;

    zoom_x.min(zoom_y).max(1.0)
}

fn deduplicate_keyframes(plan: &mut Vec<ZoomKeyframe>, min_interval_ms: u64) {
    if plan.len() < 2 {
        return;
    }

    let mut i = 0;
    while i + 1 < plan.len() {
        let dt = plan[i + 1].time_ms.saturating_sub(plan[i].time_ms);

        // Remove keyframes that are too close together (keep the later one)
        if dt < min_interval_ms {
            plan.remove(i);
            continue;
        }

        // Remove consecutive same-zoom-level transitions
        if (plan[i].zoom_level - plan[i + 1].zoom_level).abs() < 0.01 {
            plan.remove(i + 1);
            continue;
        }

        // Remove zoom-in immediately followed by zoom-out (< 200ms)
        if dt < 200 && plan[i].zoom_level > 1.0 && plan[i + 1].zoom_level <= 1.0 {
            plan.remove(i);
            continue;
        }

        i += 1;
    }
}

fn detect_cuts(
    plan: &mut Vec<ZoomKeyframe>,
    screen_width: f64,
    screen_height: f64,
    threshold: f64,
) {
    for i in 1..plan.len() {
        let dx = (plan[i].target_x - plan[i - 1].target_x).abs() / screen_width;
        let dy = (plan[i].target_y - plan[i - 1].target_y).abs() / screen_height;
        let distance = (dx * dx + dy * dy).sqrt();

        if distance > threshold {
            plan[i].transition = TransitionType::Cut;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::analyzer::{FocusPoint, IdleLevel, Segment, SegmentType};

    fn test_meta() -> RecordingMeta {
        RecordingMeta {
            version: 1,
            id: "test".to_string(),
            screen_width: 1920,
            screen_height: 1080,
            fps: 30,
            start_time: "2024-01-01T00:00:00Z".to_string(),
            duration_ms: 10000,
            has_audio: false,
            monitor_scale: 1.0,
            recording_dir: "/tmp".to_string(),
        }
    }

    #[test]
    fn test_click_generates_zoom_in() {
        let segments = vec![Segment {
            segment_type: SegmentType::Click,
            start_ms: 1000,
            end_ms: 1100,
            focus_point: Some(FocusPoint {
                x: 500.0,
                y: 300.0,
                region: None,
            }),
            idle_level: None,
            window_rect: None,
            window_changed: false,
        }];
        let plan = generate_zoom_plan(&segments, &test_meta(), 2.0, 2.5, 3.0);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].zoom_level, 2.0);
        assert_eq!(plan[0].target_x, 500.0);
    }

    #[test]
    fn test_short_idle_no_keyframe() {
        let segments = vec![Segment {
            segment_type: SegmentType::Idle,
            start_ms: 0,
            end_ms: 1000,
            focus_point: None,
            idle_level: Some(IdleLevel::Short),
            window_rect: None,
            window_changed: false,
        }];
        let plan = generate_zoom_plan(&segments, &test_meta(), 2.0, 2.5, 3.0);
        assert!(plan.is_empty(), "Short idle should not generate a keyframe");
    }

    #[test]
    fn test_medium_idle_zoom_out_partial() {
        let segments = vec![Segment {
            segment_type: SegmentType::Idle,
            start_ms: 1000,
            end_ms: 4000,
            focus_point: None,
            idle_level: Some(IdleLevel::Medium),
            window_rect: None,
            window_changed: false,
        }];
        let plan = generate_zoom_plan(&segments, &test_meta(), 2.0, 2.5, 3.0);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].zoom_level, 1.2, "Medium idle should zoom to 1.2x");
        assert_eq!(plan[0].time_ms, 1300); // start_ms + 300
    }

    #[test]
    fn test_long_idle_zoom_out_full() {
        let segments = vec![Segment {
            segment_type: SegmentType::Idle,
            start_ms: 1000,
            end_ms: 7000,
            focus_point: None,
            idle_level: Some(IdleLevel::Long),
            window_rect: None,
            window_changed: false,
        }];
        let plan = generate_zoom_plan(&segments, &test_meta(), 2.0, 2.5, 3.0);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].zoom_level, 1.0, "Long idle should zoom to 1.0x");
    }

    #[test]
    fn test_text_input_zoom() {
        let segments = vec![Segment {
            segment_type: SegmentType::TextInput,
            start_ms: 0,
            end_ms: 2000,
            focus_point: Some(FocusPoint {
                x: 800.0,
                y: 400.0,
                region: None,
            }),
            idle_level: None,
            window_rect: None,
            window_changed: false,
        }];
        let plan = generate_zoom_plan(&segments, &test_meta(), 2.0, 2.5, 3.0);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].zoom_level, 2.5);
    }

    #[test]
    fn test_deduplication_removes_close_keyframes() {
        let segments = vec![
            Segment {
                segment_type: SegmentType::Click,
                start_ms: 0,
                end_ms: 100,
                focus_point: Some(FocusPoint { x: 100.0, y: 100.0, region: None }),
                idle_level: None,
                window_rect: None,
                window_changed: false,
            },
            Segment {
                segment_type: SegmentType::Click,
                start_ms: 100,
                end_ms: 200,
                focus_point: Some(FocusPoint { x: 110.0, y: 110.0, region: None }),
                idle_level: None,
                window_rect: None,
                window_changed: false,
            },
        ];
        let plan = generate_zoom_plan(&segments, &test_meta(), 2.0, 2.5, 3.0);
        // Two clicks within 300ms → deduplicated
        assert!(plan.len() <= 1, "Close keyframes should be deduplicated");
    }

    #[test]
    fn test_cut_transition_for_distant_targets() {
        // Use Click (2.0x) and TextInput (2.5x) to avoid same-zoom deduplication
        let segments = vec![
            Segment {
                segment_type: SegmentType::Click,
                start_ms: 0,
                end_ms: 100,
                focus_point: Some(FocusPoint { x: 100.0, y: 100.0, region: None }),
                idle_level: None,
                window_rect: None,
                window_changed: false,
            },
            Segment {
                segment_type: SegmentType::TextInput,
                start_ms: 2000,
                end_ms: 4000,
                focus_point: Some(FocusPoint { x: 1800.0, y: 900.0, region: None }),
                idle_level: None,
                window_rect: None,
                window_changed: false,
            },
        ];
        let plan = generate_zoom_plan(&segments, &test_meta(), 2.0, 2.5, 3.0);
        assert_eq!(plan.len(), 2);
        assert!(matches!(plan[1].transition, TransitionType::Cut),
            "Distant targets should use Cut transition");
    }

    #[test]
    fn test_window_change_produces_two_stage_zoom() {
        use super::super::analyzer::Rect;
        let segments = vec![Segment {
            segment_type: SegmentType::Click,
            start_ms: 1000,
            end_ms: 1100,
            focus_point: Some(FocusPoint { x: 300.0, y: 300.0, region: None }),
            idle_level: None,
            window_rect: Some(Rect { x: 100.0, y: 100.0, width: 800.0, height: 600.0 }),
            window_changed: true,
        }];
        let plan = generate_zoom_plan(&segments, &test_meta(), 2.0, 2.5, 3.0);
        assert!(plan.len() >= 2, "Window change should produce at least 2 keyframes, got {}", plan.len());
        assert!(matches!(plan[0].transition, TransitionType::WindowZoom));
        assert!(plan[0].time_ms < plan[1].time_ms);
    }

    #[test]
    fn test_no_window_events_backward_compatible() {
        let segments = vec![Segment {
            segment_type: SegmentType::Click,
            start_ms: 1000,
            end_ms: 1100,
            focus_point: Some(FocusPoint { x: 500.0, y: 300.0, region: None }),
            idle_level: None,
            window_rect: None,
            window_changed: false,
        }];
        let plan = generate_zoom_plan(&segments, &test_meta(), 2.0, 2.5, 3.0);
        assert_eq!(plan.len(), 1);
        assert!(matches!(plan[0].transition, TransitionType::SpringIn));
    }

    #[test]
    fn test_spring_hint_on_click() {
        let segments = vec![Segment {
            segment_type: SegmentType::Click,
            start_ms: 1000,
            end_ms: 1100,
            focus_point: Some(FocusPoint { x: 500.0, y: 300.0, region: None }),
            idle_level: None,
            window_rect: None,
            window_changed: false,
        }];
        let plan = generate_zoom_plan(&segments, &test_meta(), 2.0, 2.5, 3.0);
        assert_eq!(plan.len(), 1);
        let hint = plan[0].spring_hint.as_ref().expect("Click should have spring hint");
        assert_eq!(hint.zoom_half_life, SpringHalfLife::ZOOM_IN_FAST);
    }
}
