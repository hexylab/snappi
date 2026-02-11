use super::analyzer::{Rect, ScoredSegment};
use crate::config::RecordingEvent;

/// A scored segment enriched with UI context information.
#[derive(Debug, Clone)]
pub struct UiEnrichedSegment {
    pub scored: ScoredSegment,
    /// UI element rectangle (from UI Automation) that corresponds to this segment
    pub ui_rect: Option<Rect>,
    /// Additional importance boost from UI event correlation
    pub ui_importance_boost: f64,
}

/// Time window (ms) for matching UI events to scored segments
const UI_MATCH_WINDOW_MS: u64 = 200;

/// Enrich scored segments with UI context from UI Automation events.
/// For each segment, find temporally close UI events and:
/// 1. Boost importance score based on UI event type
/// 2. Attach UI element rectangle for precision zoom targeting
pub fn enrich_with_ui_context(
    scored_segments: &[ScoredSegment],
    ui_events: &[RecordingEvent],
) -> Vec<UiEnrichedSegment> {
    scored_segments
        .iter()
        .map(|scored| {
            let seg_time = scored.segment.start_ms;
            let mut best_boost: f64 = 0.0;
            let mut best_rect: Option<Rect> = None;

            for event in ui_events {
                let (event_time, boost, rect) = match event {
                    RecordingEvent::UiFocus { t, rect, .. } => {
                        (*t, 0.3, Some(rect_from_array(rect)))
                    }
                    RecordingEvent::UiMenuOpen { t, rect, .. } => {
                        (*t, 0.5, Some(rect_from_array(rect)))
                    }
                    RecordingEvent::UiDialogOpen { t, rect, .. } => {
                        (*t, 0.6, Some(rect_from_array(rect)))
                    }
                    _ => continue,
                };

                let time_diff = if event_time > seg_time {
                    event_time - seg_time
                } else {
                    seg_time - event_time
                };

                if time_diff <= UI_MATCH_WINDOW_MS && boost > best_boost {
                    best_boost = boost;
                    best_rect = rect;
                }
            }

            UiEnrichedSegment {
                scored: ScoredSegment {
                    segment: scored.segment.clone(),
                    importance: (scored.importance + best_boost).min(1.0),
                },
                ui_rect: best_rect,
                ui_importance_boost: best_boost,
            }
        })
        .collect()
}

fn rect_from_array(arr: &[f64; 4]) -> Rect {
    Rect {
        x: arr[0],
        y: arr[1],
        width: arr[2] - arr[0],
        height: arr[3] - arr[1],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::analyzer::{FocusPoint, Segment, SegmentType};

    #[test]
    fn test_enrich_boosts_importance() {
        let segs = vec![ScoredSegment {
            segment: Segment {
                segment_type: SegmentType::Click,
                start_ms: 1000,
                end_ms: 1100,
                focus_point: Some(FocusPoint { x: 500.0, y: 300.0, region: None }),
                idle_level: None,
                window_rect: None,
                window_changed: false,
            },
            importance: 0.5,
        }];

        let ui_events = vec![RecordingEvent::UiFocus {
            t: 1050,
            control: "Button".to_string(),
            name: "OK".to_string(),
            rect: [480.0, 280.0, 560.0, 320.0],
            automation_id: "btnOk".to_string(),
        }];

        let enriched = enrich_with_ui_context(&segs, &ui_events);
        assert_eq!(enriched.len(), 1);
        assert!((enriched[0].scored.importance - 0.8).abs() < 0.01);
        assert!(enriched[0].ui_rect.is_some());
    }

    #[test]
    fn test_enrich_no_match_no_boost() {
        let segs = vec![ScoredSegment {
            segment: Segment {
                segment_type: SegmentType::Click,
                start_ms: 1000,
                end_ms: 1100,
                focus_point: Some(FocusPoint { x: 500.0, y: 300.0, region: None }),
                idle_level: None,
                window_rect: None,
                window_changed: false,
            },
            importance: 0.5,
        }];

        // UI event far away in time
        let ui_events = vec![RecordingEvent::UiFocus {
            t: 5000,
            control: "Button".to_string(),
            name: "OK".to_string(),
            rect: [480.0, 280.0, 560.0, 320.0],
            automation_id: "btnOk".to_string(),
        }];

        let enriched = enrich_with_ui_context(&segs, &ui_events);
        assert_eq!(enriched.len(), 1);
        assert!((enriched[0].scored.importance - 0.5).abs() < 0.01);
        assert!(enriched[0].ui_rect.is_none());
    }

    #[test]
    fn test_dialog_highest_boost() {
        let segs = vec![ScoredSegment {
            segment: Segment {
                segment_type: SegmentType::Click,
                start_ms: 1000,
                end_ms: 1100,
                focus_point: Some(FocusPoint { x: 500.0, y: 300.0, region: None }),
                idle_level: None,
                window_rect: None,
                window_changed: false,
            },
            importance: 0.3,
        }];

        let ui_events = vec![
            RecordingEvent::UiFocus {
                t: 1050,
                control: "Edit".to_string(),
                name: "".to_string(),
                rect: [480.0, 280.0, 560.0, 320.0],
                automation_id: "".to_string(),
            },
            RecordingEvent::UiDialogOpen {
                t: 1080,
                control: "Dialog".to_string(),
                name: "Save".to_string(),
                rect: [300.0, 200.0, 700.0, 500.0],
            },
        ];

        let enriched = enrich_with_ui_context(&segs, &ui_events);
        assert!((enriched[0].scored.importance - 0.9).abs() < 0.01, "Dialog should give highest boost");
        // Dialog rect should be chosen (higher boost)
        let r = enriched[0].ui_rect.as_ref().unwrap();
        assert!((r.x - 300.0).abs() < 0.01);
    }
}
