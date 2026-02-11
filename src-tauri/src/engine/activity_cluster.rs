//! Activity clustering module for the 3-tier zoom model (v3).
//!
//! Clusters user activity events (clicks, text input, scrolls, drags)
//! into spatial-temporal groups that define WorkArea bounding boxes.

use crate::config::RecordingEvent;
use crate::engine::analyzer::Rect;

/// Default sliding window for temporal clustering (overridable via settings)
const DEFAULT_TIME_WINDOW_MS: u64 = 5000;
/// Spatial distance threshold for same cluster
const SPATIAL_RADIUS: f64 = 300.0;
/// Minimum events to form a valid cluster
const MIN_EVENTS: usize = 2;
/// Padding added to bounding box edges
const BBOX_PADDING: f64 = 50.0;
/// Minimum bbox dimension (width or height)
const MIN_BBOX_SIZE: f64 = 200.0;

/// An activity cluster representing a WorkArea.
#[derive(Debug, Clone)]
pub struct ActivityCluster {
    pub id: u32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub bbox: Rect,
    pub center_x: f64,
    pub center_y: f64,
    pub zoom_level: f64,
    pub event_count: usize,
    pub window_rect: Option<Rect>,
    /// Time at which the cluster becomes stable (enough events + stability time elapsed)
    pub stable_from_ms: u64,
    // Internal
    points: Vec<(f64, f64)>,
    raw_bbox: Rect,
}

impl ActivityCluster {
    fn new(id: u32, x: f64, y: f64, time_ms: u64, window_rect: Option<Rect>) -> Self {
        let raw_bbox = Rect { x, y, width: 0.0, height: 0.0 };
        Self {
            id,
            start_ms: time_ms,
            end_ms: time_ms,
            bbox: padded_bbox(&raw_bbox),
            center_x: x,
            center_y: y,
            zoom_level: 1.0,
            event_count: 1,
            window_rect,
            stable_from_ms: u64::MAX,
            points: vec![(x, y)],
            raw_bbox,
        }
    }

    fn add_point(&mut self, x: f64, y: f64, time_ms: u64, stability_time: u64) {
        self.points.push((x, y));
        self.end_ms = time_ms;
        self.event_count = self.points.len();
        self.recalculate_bbox();

        // Check stability
        if self.event_count >= MIN_EVENTS
            && self.stable_from_ms == u64::MAX
            && time_ms >= self.start_ms + stability_time
        {
            self.stable_from_ms = time_ms;
        }
    }

    fn contains_or_near(&self, x: f64, y: f64) -> bool {
        x >= self.bbox.x - SPATIAL_RADIUS
            && x <= self.bbox.x + self.bbox.width + SPATIAL_RADIUS
            && y >= self.bbox.y - SPATIAL_RADIUS
            && y <= self.bbox.y + self.bbox.height + SPATIAL_RADIUS
    }

    fn would_expand_too_much(&self, x: f64, y: f64) -> bool {
        if self.points.len() < 2 {
            return false;
        }
        let new_min_x = self.raw_bbox.x.min(x);
        let new_max_x = (self.raw_bbox.x + self.raw_bbox.width).max(x);
        let new_min_y = self.raw_bbox.y.min(y);
        let new_max_y = (self.raw_bbox.y + self.raw_bbox.height).max(y);
        let new_area = (new_max_x - new_min_x).max(1.0) * (new_max_y - new_min_y).max(1.0);
        let current_area = self.raw_bbox.width.max(MIN_BBOX_SIZE) * self.raw_bbox.height.max(MIN_BBOX_SIZE);
        current_area > 0.0 && new_area > current_area * 2.0
    }

    fn recalculate_bbox(&mut self) {
        if self.points.is_empty() {
            return;
        }
        let min_x = self.points.iter().map(|p| p.0).fold(f64::MAX, f64::min);
        let max_x = self.points.iter().map(|p| p.0).fold(f64::MIN, f64::max);
        let min_y = self.points.iter().map(|p| p.1).fold(f64::MAX, f64::min);
        let max_y = self.points.iter().map(|p| p.1).fold(f64::MIN, f64::max);

        self.raw_bbox = Rect {
            x: min_x,
            y: min_y,
            width: max_x - min_x,
            height: max_y - min_y,
        };
        self.bbox = padded_bbox(&self.raw_bbox);
        self.center_x = self.bbox.x + self.bbox.width / 2.0;
        self.center_y = self.bbox.y + self.bbox.height / 2.0;
    }

    /// Create an ActivityCluster for testing purposes.
    #[cfg(test)]
    pub fn for_test(
        id: u32, start_ms: u64, end_ms: u64, stable_from_ms: u64,
        cx: f64, cy: f64, zoom_level: f64,
    ) -> Self {
        let raw_bbox = Rect { x: cx - 50.0, y: cy - 25.0, width: 100.0, height: 50.0 };
        Self {
            id,
            start_ms,
            end_ms,
            bbox: padded_bbox(&raw_bbox),
            center_x: cx,
            center_y: cy,
            zoom_level,
            event_count: 5,
            window_rect: None,
            stable_from_ms,
            points: vec![],
            raw_bbox,
        }
    }

    fn calc_zoom(&mut self, screen_w: f64, screen_h: f64, max_zoom: f64) {
        let zoom_w = screen_w / self.bbox.width.max(1.0);
        let zoom_h = screen_h / self.bbox.height.max(1.0);
        let fit_zoom = zoom_w.min(zoom_h);
        self.zoom_level = fit_zoom.min(max_zoom).max(1.2);
    }
}

fn padded_bbox(raw: &Rect) -> Rect {
    let w = raw.width.max(MIN_BBOX_SIZE);
    let h = raw.height.max(MIN_BBOX_SIZE);
    let cx = raw.x + raw.width / 2.0;
    let cy = raw.y + raw.height / 2.0;
    Rect {
        x: cx - w / 2.0 - BBOX_PADDING,
        y: cy - h / 2.0 - BBOX_PADDING,
        width: w + BBOX_PADDING * 2.0,
        height: h + BBOX_PADDING * 2.0,
    }
}

struct ActivityPoint {
    time_ms: u64,
    x: f64,
    y: f64,
    weight: f64,
    window_rect: Option<Rect>,
}

/// Extract activity points from recording events with weights.
fn extract_activity_points(events: &[RecordingEvent]) -> Vec<ActivityPoint> {
    let mut points = Vec::new();
    let mut last_move_ms: u64 = 0;
    let mut last_pos: Option<(f64, f64)> = None;
    let mut current_window: Option<Rect> = None;

    for event in events {
        match event {
            RecordingEvent::WindowFocus { rect, .. } => {
                current_window = Some(Rect {
                    x: rect[0],
                    y: rect[1],
                    width: rect[2] - rect[0],
                    height: rect[3] - rect[1],
                });
            }
            RecordingEvent::Click { t, x, y, .. } => {
                points.push(ActivityPoint {
                    time_ms: *t, x: *x, y: *y, weight: 1.0,
                    window_rect: current_window.clone(),
                });
                last_pos = Some((*x, *y));
            }
            RecordingEvent::Key { t, .. } => {
                // Key events extend clusters at the last known position
                // (important for text input — user clicks then types)
                if let Some((lx, ly)) = last_pos {
                    points.push(ActivityPoint {
                        time_ms: *t, x: lx, y: ly, weight: 0.3,
                        window_rect: current_window.clone(),
                    });
                }
            }
            RecordingEvent::Scroll { t, x, y, .. } => {
                points.push(ActivityPoint {
                    time_ms: *t, x: *x, y: *y, weight: 0.5,
                    window_rect: current_window.clone(),
                });
                last_pos = Some((*x, *y));
            }
            RecordingEvent::ClickRelease { t, x, y, .. } => {
                points.push(ActivityPoint {
                    time_ms: *t, x: *x, y: *y, weight: 0.8,
                    window_rect: current_window.clone(),
                });
                last_pos = Some((*x, *y));
            }
            RecordingEvent::MouseMove { t, x, y } => {
                // Thin to every 100ms for range extension only
                if *t >= last_move_ms + 100 {
                    points.push(ActivityPoint {
                        time_ms: *t, x: *x, y: *y, weight: 0.1,
                        window_rect: current_window.clone(),
                    });
                    last_move_ms = *t;
                }
                last_pos = Some((*x, *y));
            }
            _ => {}
        }
    }

    points.sort_by_key(|p| p.time_ms);
    points
}

/// Cluster activity events into spatial-temporal groups.
///
/// Returns a sorted list of ActivityClusters with computed bounding boxes and zoom levels.
pub fn cluster_activities(
    events: &[RecordingEvent],
    screen_w: f64,
    screen_h: f64,
    max_zoom: f64,
    stability_time: u64,
    cluster_lifetime_ms: u64,
) -> Vec<ActivityCluster> {
    let time_window = if cluster_lifetime_ms > 0 { cluster_lifetime_ms } else { DEFAULT_TIME_WINDOW_MS };
    let points = extract_activity_points(events);
    if points.is_empty() {
        return Vec::new();
    }

    let mut active_clusters: Vec<ActivityCluster> = Vec::new();
    let mut finalized: Vec<ActivityCluster> = Vec::new();
    let mut next_id: u32 = 0;

    for point in &points {
        let can_form_cluster = point.weight >= 0.5;

        // Expire old clusters (no activity within TIME_WINDOW)
        let mut i = 0;
        while i < active_clusters.len() {
            if point.time_ms > active_clusters[i].end_ms + time_window {
                let c = active_clusters.remove(i);
                if c.event_count >= MIN_EVENTS {
                    finalized.push(c);
                }
            } else {
                i += 1;
            }
        }

        // Try to find a matching cluster
        let mut matched_idx: Option<usize> = None;
        for (i, cluster) in active_clusters.iter().enumerate() {
            if cluster.contains_or_near(point.x, point.y)
                && !cluster.would_expand_too_much(point.x, point.y)
            {
                matched_idx = Some(i);
                break;
            }
        }

        if let Some(idx) = matched_idx {
            active_clusters[idx].add_point(point.x, point.y, point.time_ms, stability_time);
        } else if can_form_cluster {
            let cluster = ActivityCluster::new(
                next_id, point.x, point.y, point.time_ms, point.window_rect.clone(),
            );
            next_id += 1;
            active_clusters.push(cluster);
        }
    }

    // Finalize remaining active clusters
    for c in active_clusters {
        if c.event_count >= MIN_EVENTS {
            finalized.push(c);
        }
    }

    // Calculate zoom levels
    for cluster in &mut finalized {
        cluster.calc_zoom(screen_w, screen_h, max_zoom);
    }

    // Sort by start time
    finalized.sort_by_key(|c| c.start_ms);
    finalized
}

/// Calculate zoom level to fit a window within the screen.
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
    let fit_zoom = zoom_w.min(zoom_h);
    fit_zoom.min(max_zoom).max(1.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(t: u64, x: f64, y: f64) -> RecordingEvent {
        RecordingEvent::Click { t, btn: "left".to_string(), x, y }
    }

    fn key(t: u64) -> RecordingEvent {
        RecordingEvent::Key { t, key: "KeyA".to_string(), modifiers: vec![] }
    }

    #[test]
    fn test_single_cluster_from_nearby_clicks() {
        let events = vec![
            click(0, 500.0, 300.0),
            click(500, 520.0, 310.0),
            click(1000, 490.0, 290.0),
        ];
        let clusters = cluster_activities(&events, 1920.0, 1080.0, 3.0, 1000, 5000);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].event_count, 3);
    }

    #[test]
    fn test_two_clusters_from_distant_clicks() {
        let events = vec![
            click(0, 100.0, 100.0),
            click(500, 110.0, 110.0),
            click(1000, 1800.0, 900.0),
            click(1500, 1810.0, 910.0),
        ];
        let clusters = cluster_activities(&events, 1920.0, 1080.0, 3.0, 1000, 5000);
        assert_eq!(clusters.len(), 2);
    }

    #[test]
    fn test_cluster_stability() {
        let events = vec![
            click(0, 500.0, 300.0),
            click(500, 520.0, 310.0),
            click(1500, 490.0, 290.0), // stability_time=1000, start=0, this is at 1500 > 0+1000
        ];
        let clusters = cluster_activities(&events, 1920.0, 1080.0, 3.0, 1000, 5000);
        assert_eq!(clusters.len(), 1);
        assert!(clusters[0].stable_from_ms <= 1500);
    }

    #[test]
    fn test_key_events_extend_cluster() {
        let events = vec![
            click(0, 500.0, 300.0),
            click(200, 510.0, 300.0),
            key(500),
            key(800),
            key(1100),
            key(2500), // extends cluster past stability time
        ];
        let clusters = cluster_activities(&events, 1920.0, 1080.0, 3.0, 1000, 5000);
        assert_eq!(clusters.len(), 1);
        assert!(clusters[0].end_ms >= 2500);
    }

    #[test]
    fn test_empty_events() {
        let clusters = cluster_activities(&[], 1920.0, 1080.0, 3.0, 1000, 5000);
        assert!(clusters.is_empty());
    }

    #[test]
    fn test_zoom_level_calculation() {
        let events = vec![
            click(0, 500.0, 300.0),
            click(500, 700.0, 500.0),
        ];
        let clusters = cluster_activities(&events, 1920.0, 1080.0, 3.0, 1000, 5000);
        assert_eq!(clusters.len(), 1);
        assert!(clusters[0].zoom_level >= 1.2);
        assert!(clusters[0].zoom_level <= 3.0);
    }

    #[test]
    fn test_window_zoom_calculation() {
        let rect = Rect { x: 100.0, y: 100.0, width: 800.0, height: 600.0 };
        let zoom = calc_window_zoom(&rect, 1920.0, 1080.0, 3.0);
        assert!(zoom >= 1.1);
        assert!(zoom <= 3.0);
    }
}
