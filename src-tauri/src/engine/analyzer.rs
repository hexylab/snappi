use crate::config::RecordingEvent;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SegmentType {
    Click,
    TextInput,
    Scroll,
    Idle,
    RapidAction,
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
}

const IDLE_THRESHOLD_MS: u64 = 500;
const RAPID_ACTION_WINDOW_MS: u64 = 200;
const RAPID_ACTION_MIN_CLICKS: usize = 3;

/// Analyze recorded events and split into segments
pub fn analyze_events(events: &[RecordingEvent]) -> Vec<Segment> {
    let mut segments = Vec::new();

    if events.is_empty() {
        return segments;
    }

    let mut i = 0;
    let mut last_event_time: u64 = 0;

    while i < events.len() {
        let event = &events[i];
        let event_time = event_timestamp(event);

        // Check for idle gap
        if event_time > last_event_time + IDLE_THRESHOLD_MS && last_event_time > 0 {
            segments.push(Segment {
                segment_type: SegmentType::Idle,
                start_ms: last_event_time,
                end_ms: event_time,
                focus_point: None,
            });
        }

        match event {
            RecordingEvent::Click { t, x, y, .. } => {
                // Check for rapid clicks
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
                    });
                    i = end_idx + 1;
                } else {
                    segments.push(Segment {
                        segment_type: SegmentType::Click,
                        start_ms: *t,
                        end_ms: *t + 100,
                        focus_point: Some(FocusPoint {
                            x: *x,
                            y: *y,
                            region: None,
                        }),
                    });
                    i += 1;
                }
                last_event_time = event_timestamp(&events[i.saturating_sub(1)]);
                continue;
            }
            RecordingEvent::Focus { t, rect, .. } => {
                // Check if followed by key events (text input)
                let mut has_keys = false;
                let mut end_time = *t;
                let mut j = i + 1;
                while j < events.len() {
                    match &events[j] {
                        RecordingEvent::Key { t: kt, .. } => {
                            if *kt - *t <= 500 || *kt - end_time <= 500 {
                                has_keys = true;
                                end_time = *kt;
                            } else {
                                break;
                            }
                        }
                        RecordingEvent::Focus { .. } | RecordingEvent::Click { .. } => break,
                        _ => {}
                    }
                    j += 1;
                }

                if has_keys {
                    segments.push(Segment {
                        segment_type: SegmentType::TextInput,
                        start_ms: *t,
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
                    });
                }
            }
            RecordingEvent::Scroll { t, x, y, .. } => {
                // Group consecutive scrolls
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
                });
                i = j;
                last_event_time = end_time;
                continue;
            }
            _ => {}
        }

        last_event_time = event_time;
        i += 1;
    }

    segments
}

fn event_timestamp(event: &RecordingEvent) -> u64 {
    match event {
        RecordingEvent::MouseMove { t, .. } => *t,
        RecordingEvent::Click { t, .. } => *t,
        RecordingEvent::Key { t, .. } => *t,
        RecordingEvent::Scroll { t, .. } => *t,
        RecordingEvent::Focus { t, .. } => *t,
    }
}
