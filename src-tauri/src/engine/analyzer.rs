use crate::config::RecordingEvent;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SegmentType {
    Click,
    TextInput,
    Scroll,
    Idle,
    RapidAction,
}

/// Sub-classification for idle duration
#[derive(Debug, Clone, PartialEq)]
pub enum IdleLevel {
    Short,  // 800-2000ms: maintain zoom
    Medium, // 2000-5000ms: slow zoom-out to 1.2x
    Long,   // 5000ms+: full zoom-out to 1.0x
}

#[derive(Debug, Clone)]
pub struct FocusPoint {
    pub x: f64,
    pub y: f64,
    pub region: Option<Rect>,
}

#[derive(Debug, Clone)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn center_x(&self) -> f64 {
        self.x + self.width / 2.0
    }
    pub fn center_y(&self) -> f64 {
        self.y + self.height / 2.0
    }
}

#[derive(Debug, Clone)]
pub struct Segment {
    pub segment_type: SegmentType,
    pub start_ms: u64,
    pub end_ms: u64,
    pub focus_point: Option<FocusPoint>,
    pub idle_level: Option<IdleLevel>,
    pub window_rect: Option<Rect>,
    pub window_changed: bool,
}

// Idle detection thresholds (using significant events only)
const IDLE_SHORT_MS: u64 = 800;
const IDLE_MEDIUM_MS: u64 = 2000;
const IDLE_LONG_MS: u64 = 5000;

const RAPID_ACTION_WINDOW_MS: u64 = 200;
const RAPID_ACTION_MIN_CLICKS: usize = 3;

// Click→Key text input detection window
const TEXT_INPUT_CLICK_KEY_GAP_MS: u64 = 500;
const TEXT_INPUT_KEY_CONTINUATION_MS: u64 = 300;

/// Window change threshold: if action occurs within this time after a window
/// change, the segment is marked as `window_changed = true`.
const WINDOW_CHANGE_THRESHOLD_MS: u64 = 500;

/// Analyze recorded events and split into semantic segments.
/// Only considers significant events (Click, Key, Scroll, ClickRelease) for
/// idle detection — mouse_move events are ignored for gap calculation.
pub fn analyze_events(events: &[RecordingEvent]) -> Vec<Segment> {
    let mut segments = Vec::new();

    if events.is_empty() {
        return segments;
    }

    // Phase 1: Collect significant event timestamps for idle detection
    let significant_events: Vec<(usize, u64)> = events
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let t = match e {
                RecordingEvent::Click { t, .. }
                | RecordingEvent::ClickRelease { t, .. }
                | RecordingEvent::Key { t, .. }
                | RecordingEvent::Scroll { t, .. }
                | RecordingEvent::WindowFocus { t, .. } => *t,
                _ => return None,
            };
            Some((i, t))
        })
        .collect();

    // Phase 2: Detect idle periods from significant event gaps
    for window in significant_events.windows(2) {
        let (_, t1) = window[0];
        let (_, t2) = window[1];
        let gap = t2.saturating_sub(t1);

        if gap >= IDLE_SHORT_MS {
            let idle_level = if gap >= IDLE_LONG_MS {
                IdleLevel::Long
            } else if gap >= IDLE_MEDIUM_MS {
                IdleLevel::Medium
            } else {
                IdleLevel::Short
            };

            segments.push(Segment {
                segment_type: SegmentType::Idle,
                start_ms: t1,
                end_ms: t2,
                focus_point: None,
                idle_level: Some(idle_level),
                window_rect: None,
                window_changed: false,
            });
        }
    }

    // Phase 3: Process non-idle segments with window context tracking
    let mut current_window: Option<Rect> = None;
    let mut last_window_change_ms: u64 = 0;
    let mut i = 0;
    while i < events.len() {
        let event = &events[i];

        match event {
            RecordingEvent::WindowFocus { t, rect, .. } => {
                current_window = Some(Rect {
                    x: rect[0],
                    y: rect[1],
                    width: rect[2] - rect[0],
                    height: rect[3] - rect[1],
                });
                last_window_change_ms = *t;
            }
            RecordingEvent::Click { t, x, y, .. } => {
                let win_changed = current_window.is_some()
                    && t.saturating_sub(last_window_change_ms) < WINDOW_CHANGE_THRESHOLD_MS;

                // Check for rapid clicks first
                let mut click_count = 1;
                let mut end_idx = i;
                let mut j = i + 1;
                while j < events.len() {
                    if let RecordingEvent::Click { t: t2, .. } = &events[j] {
                        if *t2 - t <= RAPID_ACTION_WINDOW_MS * click_count as u64 {
                            click_count += 1;
                            end_idx = j;
                        } else {
                            break;
                        }
                    }
                    j += 1;
                    if j - i > 10 {
                        break;
                    }
                }

                if click_count >= RAPID_ACTION_MIN_CLICKS {
                    segments.push(Segment {
                        segment_type: SegmentType::RapidAction,
                        start_ms: *t,
                        end_ms: event_timestamp(&events[end_idx]),
                        focus_point: Some(FocusPoint {
                            x: *x,
                            y: *y,
                            region: None,
                        }),
                        idle_level: None,
                        window_rect: current_window.clone(),
                        window_changed: win_changed,
                    });
                    i = end_idx + 1;
                    continue;
                }

                // Check for Click→Key text input pattern
                let text_input = detect_text_input_after_click(events, i, *t, *x, *y);
                if let Some(mut seg) = text_input {
                    seg.window_rect = current_window.clone();
                    seg.window_changed = win_changed;
                    segments.push(seg);
                    i += 1;
                    continue;
                }

                // Single click
                segments.push(Segment {
                    segment_type: SegmentType::Click,
                    start_ms: *t,
                    end_ms: *t + 100,
                    focus_point: Some(FocusPoint {
                        x: *x,
                        y: *y,
                        region: None,
                    }),
                    idle_level: None,
                    window_rect: current_window.clone(),
                    window_changed: win_changed,
                });
            }
            RecordingEvent::Focus { t, rect, .. } => {
                let text_input = detect_text_input_after_focus(events, i, *t, rect);
                if let Some(mut seg) = text_input {
                    let win_changed = current_window.is_some()
                        && t.saturating_sub(last_window_change_ms) < WINDOW_CHANGE_THRESHOLD_MS;
                    seg.window_rect = current_window.clone();
                    seg.window_changed = win_changed;
                    segments.push(seg);
                }
            }
            RecordingEvent::Scroll { t, x, y, .. } => {
                let win_changed = current_window.is_some()
                    && t.saturating_sub(last_window_change_ms) < WINDOW_CHANGE_THRESHOLD_MS;

                let mut end_time = *t;
                let mut j = i + 1;
                while j < events.len() {
                    if let RecordingEvent::Scroll { t: st, .. } = &events[j] {
                        if *st - end_time <= 300 {
                            end_time = *st;
                            j += 1;
                            continue;
                        }
                    }
                    break;
                }

                segments.push(Segment {
                    segment_type: SegmentType::Scroll,
                    start_ms: *t,
                    end_ms: end_time,
                    focus_point: Some(FocusPoint {
                        x: *x,
                        y: *y,
                        region: None,
                    }),
                    idle_level: None,
                    window_rect: current_window.clone(),
                    window_changed: win_changed,
                });
                i = j;
                continue;
            }
            _ => {}
        }

        i += 1;
    }

    // Sort segments by start time
    segments.sort_by_key(|s| s.start_ms);
    segments
}

/// Detect Click→Key text input pattern:
/// If Key events follow within TEXT_INPUT_CLICK_KEY_GAP_MS after a Click,
/// classify as TextInput segment centered on the click position.
fn detect_text_input_after_click(
    events: &[RecordingEvent],
    click_idx: usize,
    click_t: u64,
    click_x: f64,
    click_y: f64,
) -> Option<Segment> {
    let mut has_keys = false;
    let mut end_time = click_t;

    for j in (click_idx + 1)..events.len() {
        match &events[j] {
            RecordingEvent::Key { t: kt, .. } => {
                let gap = if !has_keys {
                    kt.saturating_sub(click_t)
                } else {
                    kt.saturating_sub(end_time)
                };
                if gap <= TEXT_INPUT_CLICK_KEY_GAP_MS
                    || (has_keys && gap <= TEXT_INPUT_KEY_CONTINUATION_MS)
                {
                    has_keys = true;
                    end_time = *kt;
                } else {
                    break;
                }
            }
            RecordingEvent::Click { .. } | RecordingEvent::Focus { .. } => break,
            _ => {}
        }
    }

    if has_keys {
        Some(Segment {
            segment_type: SegmentType::TextInput,
            start_ms: click_t,
            end_ms: end_time,
            focus_point: Some(FocusPoint {
                x: click_x,
                y: click_y,
                region: None,
            }),
            idle_level: None,
            window_rect: None,
            window_changed: false,
        })
    } else {
        None
    }
}

/// Detect text input after a Focus event (when focus events are available)
fn detect_text_input_after_focus(
    events: &[RecordingEvent],
    focus_idx: usize,
    focus_t: u64,
    rect: &[f64; 4],
) -> Option<Segment> {
    let mut has_keys = false;
    let mut end_time = focus_t;

    for j in (focus_idx + 1)..events.len() {
        match &events[j] {
            RecordingEvent::Key { t: kt, .. } => {
                if *kt - focus_t <= 500 || *kt - end_time <= 500 {
                    has_keys = true;
                    end_time = *kt;
                } else {
                    break;
                }
            }
            RecordingEvent::Focus { .. } | RecordingEvent::Click { .. } => break,
            _ => {}
        }
    }

    if has_keys {
        Some(Segment {
            segment_type: SegmentType::TextInput,
            start_ms: focus_t,
            end_ms: end_time,
            focus_point: Some(FocusPoint {
                x: (rect[0] + rect[2]) / 2.0,
                y: (rect[1] + rect[3]) / 2.0,
                region: Some(Rect {
                    x: rect[0],
                    y: rect[1],
                    width: rect[2] - rect[0],
                    height: rect[3] - rect[1],
                }),
            }),
            idle_level: None,
            window_rect: None,
            window_changed: false,
        })
    } else {
        None
    }
}

pub fn event_timestamp(event: &RecordingEvent) -> u64 {
    match event {
        RecordingEvent::MouseMove { t, .. } => *t,
        RecordingEvent::Click { t, .. } => *t,
        RecordingEvent::ClickRelease { t, .. } => *t,
        RecordingEvent::Key { t, .. } => *t,
        RecordingEvent::Scroll { t, .. } => *t,
        RecordingEvent::Focus { t, .. } => *t,
        RecordingEvent::WindowFocus { t, .. } => *t,
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

    fn mouse_move(t: u64, x: f64, y: f64) -> RecordingEvent {
        RecordingEvent::MouseMove { t, x, y }
    }

    fn scroll(t: u64) -> RecordingEvent {
        RecordingEvent::Scroll {
            t,
            x: 500.0,
            y: 500.0,
            dx: 0.0,
            dy: -3.0,
        }
    }

    #[test]
    fn test_idle_detection_ignores_mouse_move() {
        // Mouse moves every 10ms should NOT prevent idle detection
        let mut events = vec![click(0, 100.0, 100.0)];
        // 100 mouse moves at 10ms intervals
        for i in 1..=100 {
            events.push(mouse_move(i * 10, 100.0 + i as f64, 100.0));
        }
        events.push(click(2000, 200.0, 200.0));

        let segments = analyze_events(&events);
        let idle_segments: Vec<_> = segments
            .iter()
            .filter(|s| s.segment_type == SegmentType::Idle)
            .collect();

        assert!(!idle_segments.is_empty(),
            "Should detect idle despite mouse_move events");
        assert_eq!(idle_segments[0].idle_level, Some(IdleLevel::Medium));
    }

    #[test]
    fn test_staged_idle_levels() {
        // Short idle: 800-2000ms
        let events = vec![click(0, 100.0, 100.0), click(1000, 200.0, 200.0)];
        let segs = analyze_events(&events);
        let idle = segs.iter().find(|s| s.segment_type == SegmentType::Idle);
        assert!(idle.is_some());
        assert_eq!(idle.unwrap().idle_level, Some(IdleLevel::Short));

        // Medium idle: 2000-5000ms
        let events = vec![click(0, 100.0, 100.0), click(3000, 200.0, 200.0)];
        let segs = analyze_events(&events);
        let idle = segs.iter().find(|s| s.segment_type == SegmentType::Idle);
        assert!(idle.is_some());
        assert_eq!(idle.unwrap().idle_level, Some(IdleLevel::Medium));

        // Long idle: 5000ms+
        let events = vec![click(0, 100.0, 100.0), click(6000, 200.0, 200.0)];
        let segs = analyze_events(&events);
        let idle = segs.iter().find(|s| s.segment_type == SegmentType::Idle);
        assert!(idle.is_some());
        assert_eq!(idle.unwrap().idle_level, Some(IdleLevel::Long));
    }

    #[test]
    fn test_click_key_text_input() {
        // Click followed by key within 500ms → TextInput
        let events = vec![
            click(0, 100.0, 100.0),
            key(200),
            key(400),
            key(600),
        ];
        let segs = analyze_events(&events);
        let text_input = segs.iter().find(|s| s.segment_type == SegmentType::TextInput);
        assert!(text_input.is_some(), "Should detect TextInput from Click→Key pattern");
        assert_eq!(text_input.unwrap().end_ms, 600);
    }

    #[test]
    fn test_click_without_key_is_just_click() {
        let events = vec![click(0, 100.0, 100.0)];
        let segs = analyze_events(&events);
        assert!(segs.iter().any(|s| s.segment_type == SegmentType::Click));
        assert!(!segs.iter().any(|s| s.segment_type == SegmentType::TextInput));
    }

    #[test]
    fn test_rapid_action() {
        let events = vec![
            click(0, 100.0, 100.0),
            click(100, 100.0, 100.0),
            click(200, 100.0, 100.0),
        ];
        let segs = analyze_events(&events);
        assert!(segs.iter().any(|s| s.segment_type == SegmentType::RapidAction));
    }

    #[test]
    fn test_scroll_grouping() {
        let events = vec![
            scroll(0),
            scroll(100),
            scroll(200),
            scroll(800), // > 300ms gap → new segment
        ];
        let segs = analyze_events(&events);
        let scrolls: Vec<_> = segs.iter().filter(|s| s.segment_type == SegmentType::Scroll).collect();
        assert_eq!(scrolls.len(), 2);
    }

    #[test]
    fn test_no_idle_below_threshold() {
        // Gap of 500ms (below 800ms threshold) → no idle
        let events = vec![click(0, 100.0, 100.0), click(500, 200.0, 200.0)];
        let segs = analyze_events(&events);
        assert!(!segs.iter().any(|s| s.segment_type == SegmentType::Idle));
    }

    #[test]
    fn test_window_focus_attaches_to_click() {
        let events = vec![
            RecordingEvent::WindowFocus {
                t: 0,
                title: "Notepad".to_string(),
                rect: [100.0, 100.0, 600.0, 500.0],
            },
            click(200, 300.0, 300.0),
        ];
        let segs = analyze_events(&events);
        let click_seg = segs.iter().find(|s| s.segment_type == SegmentType::Click).unwrap();
        assert!(click_seg.window_rect.is_some(), "Click after WindowFocus should have window_rect");
        assert!(click_seg.window_changed, "Click within 500ms of WindowFocus should be window_changed");
    }

    #[test]
    fn test_window_focus_not_changed_after_threshold() {
        let events = vec![
            RecordingEvent::WindowFocus {
                t: 0,
                title: "Notepad".to_string(),
                rect: [100.0, 100.0, 600.0, 500.0],
            },
            click(1000, 300.0, 300.0),
        ];
        let segs = analyze_events(&events);
        let click_seg = segs.iter().find(|s| s.segment_type == SegmentType::Click).unwrap();
        assert!(click_seg.window_rect.is_some(), "Should still have window_rect");
        assert!(!click_seg.window_changed, "Click after 500ms should not be window_changed");
    }
}
