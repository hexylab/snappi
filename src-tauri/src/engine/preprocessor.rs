use crate::config::RecordingEvent;
use std::collections::HashSet;

/// Drag event detected by the preprocessor
#[derive(Debug, Clone)]
pub struct DragEvent {
    pub start_ms: u64,
    pub end_ms: u64,
    pub start_x: f64,
    pub start_y: f64,
    pub end_x: f64,
    pub end_y: f64,
}

/// Result of preprocessing: thinned events and detected drags
pub struct PreprocessedEvents {
    pub events: Vec<RecordingEvent>,
    pub drags: Vec<DragEvent>,
}

/// Run all preprocessing steps on raw events.
pub fn preprocess(events: &[RecordingEvent]) -> PreprocessedEvents {
    let thinned = thin_mouse_moves(events, 3.0);
    let drags = detect_drags(events);
    PreprocessedEvents {
        events: thinned,
        drags,
    }
}

/// Thin mouse_move events by removing sub-threshold movements,
/// while preserving positions near significant events.
pub fn thin_mouse_moves(
    events: &[RecordingEvent],
    distance_threshold: f64,
) -> Vec<RecordingEvent> {
    // Collect timestamps of significant events and create protection windows
    let mut protected_times = HashSet::new();
    for event in events {
        let t = match event {
            RecordingEvent::Click { t, .. }
            | RecordingEvent::ClickRelease { t, .. }
            | RecordingEvent::Key { t, .. }
            | RecordingEvent::Scroll { t, .. } => *t,
            _ => continue,
        };
        // Protect 100ms window around significant events
        for dt in 0..=100 {
            protected_times.insert(t.saturating_sub(dt));
            protected_times.insert(t + dt);
        }
    }

    let mut result = Vec::with_capacity(events.len());
    let mut last_x = f64::NAN;
    let mut last_y = f64::NAN;
    let mut last_mouse_t: u64 = 0;

    for event in events {
        match event {
            RecordingEvent::MouseMove { t, x, y } => {
                let dist = if last_x.is_nan() {
                    f64::MAX // Always keep first mouse event
                } else {
                    ((x - last_x).powi(2) + (y - last_y).powi(2)).sqrt()
                };

                let time_gap = t.saturating_sub(last_mouse_t);
                let is_protected = protected_times.contains(t);

                // Keep if: sufficient distance, or near significant event,
                // or 200ms+ gap (stop position)
                if dist >= distance_threshold || is_protected || time_gap >= 200 {
                    result.push(event.clone());
                    last_x = *x;
                    last_y = *y;
                    last_mouse_t = *t;
                }
            }
            _ => {
                result.push(event.clone());
            }
        }
    }
    result
}

/// Detect drag operations from Click → MouseMove(>20px) → ClickRelease patterns.
pub fn detect_drags(events: &[RecordingEvent]) -> Vec<DragEvent> {
    let mut drags = Vec::new();

    for (i, event) in events.iter().enumerate() {
        let (t, x, y, btn) = match event {
            RecordingEvent::Click { t, x, y, btn } => (*t, *x, *y, btn.as_str()),
            _ => continue,
        };

        if btn != "left" {
            continue;
        }

        // Look for ClickRelease or significant mouse movement
        let mut max_dist: f64 = 0.0;
        let mut end_time = t;
        let mut end_x = x;
        let mut end_y = y;
        let mut found_release = false;

        for j in (i + 1)..events.len() {
            match &events[j] {
                RecordingEvent::MouseMove {
                    t: mt,
                    x: mx,
                    y: my,
                } => {
                    let dist = ((mx - x).powi(2) + (my - y).powi(2)).sqrt();
                    if dist > max_dist {
                        max_dist = dist;
                        end_time = *mt;
                        end_x = *mx;
                        end_y = *my;
                    }
                }
                RecordingEvent::ClickRelease { t: rt, x: rx, y: ry, btn: rb } => {
                    if rb == "left" {
                        found_release = true;
                        end_time = *rt;
                        end_x = *rx;
                        end_y = *ry;
                        break;
                    }
                }
                RecordingEvent::Click { .. } => break, // Next click = end of drag window
                _ => {}
            }
        }

        // Classify as drag if total displacement > 20px
        let total_dist = ((end_x - x).powi(2) + (end_y - y).powi(2)).sqrt();
        if (found_release && total_dist > 20.0) || (!found_release && max_dist > 50.0) {
            drags.push(DragEvent {
                start_ms: t,
                end_ms: end_time,
                start_x: x,
                start_y: y,
                end_x,
                end_y,
            });
        }
    }

    drags
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mm(t: u64, x: f64, y: f64) -> RecordingEvent {
        RecordingEvent::MouseMove { t, x, y }
    }

    fn click(t: u64, x: f64, y: f64) -> RecordingEvent {
        RecordingEvent::Click {
            t,
            btn: "left".to_string(),
            x,
            y,
        }
    }

    fn click_release(t: u64, x: f64, y: f64) -> RecordingEvent {
        RecordingEvent::ClickRelease {
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
    fn test_thin_removes_small_movements() {
        let events = vec![
            mm(0, 100.0, 100.0),
            mm(10, 100.5, 100.5),  // < 3px, should be removed
            mm(20, 101.0, 101.0),  // < 3px, should be removed
            mm(30, 104.0, 100.0),  // > 3px, should be kept
        ];
        let result = thin_mouse_moves(&events, 3.0);
        assert_eq!(result.len(), 2); // first + 104.0
    }

    #[test]
    fn test_thin_preserves_near_click() {
        let events = vec![
            mm(0, 100.0, 100.0),
            mm(90, 100.5, 100.5),   // < 3px but within 100ms of click
            click(100, 100.0, 100.0),
            mm(110, 100.5, 100.5),  // < 3px but within 100ms of click
        ];
        let result = thin_mouse_moves(&events, 3.0);
        // All should be preserved due to protection window
        assert_eq!(result.len(), 4);
    }

    #[test]
    fn test_thin_preserves_stop_position() {
        let events = vec![
            mm(0, 100.0, 100.0),
            mm(300, 100.5, 100.5),  // > 200ms gap = stop position
        ];
        let result = thin_mouse_moves(&events, 3.0);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_detect_drags_with_release() {
        let events = vec![
            click(0, 100.0, 100.0),
            mm(10, 110.0, 100.0),
            mm(20, 120.0, 100.0),
            mm(30, 150.0, 100.0),  // 50px from start
            click_release(40, 150.0, 100.0),
        ];
        let drags = detect_drags(&events);
        assert_eq!(drags.len(), 1);
        assert_eq!(drags[0].start_ms, 0);
        assert_eq!(drags[0].end_ms, 40);
    }

    #[test]
    fn test_no_drag_for_small_movement() {
        let events = vec![
            click(0, 100.0, 100.0),
            mm(10, 105.0, 100.0),  // only 5px
            click_release(20, 105.0, 100.0),
        ];
        let drags = detect_drags(&events);
        assert_eq!(drags.len(), 0);
    }

    #[test]
    fn test_drag_without_release() {
        let events = vec![
            click(0, 100.0, 100.0),
            mm(10, 110.0, 100.0),
            mm(20, 120.0, 100.0),
            mm(30, 160.0, 100.0),  // 60px max distance > 50px threshold
            click(1000, 200.0, 200.0), // next click breaks window
        ];
        let drags = detect_drags(&events);
        assert_eq!(drags.len(), 1);
    }

    #[test]
    fn test_preprocess_returns_both() {
        let events = vec![
            mm(0, 100.0, 100.0),
            click(100, 100.0, 100.0),
            mm(110, 160.0, 100.0),
            click_release(200, 160.0, 100.0),
            key(300),
        ];
        let result = preprocess(&events);
        assert!(!result.events.is_empty());
        assert_eq!(result.drags.len(), 1);
    }
}
