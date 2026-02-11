use super::analyzer::{Rect, ScoredSegment, SegmentType};

/// Spatial proximity threshold (pixels) for clustering clicks.
const CLUSTER_SPATIAL_EPS: f64 = 200.0;
/// Temporal proximity threshold (ms) for clustering clicks.
const CLUSTER_TEMPORAL_EPS: u64 = 3000;

/// A cluster of spatially and temporally close clicks.
#[derive(Debug, Clone)]
pub struct ClickCluster {
    pub center_x: f64,
    pub center_y: f64,
    pub count: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    pub bounding_rect: Rect,
}

impl ClickCluster {
    /// Recommended zoom level based on cluster size.
    /// More clicks in a cluster → wider view needed.
    pub fn recommended_zoom(&self, default_zoom: f64) -> f64 {
        match self.count {
            1 => default_zoom.min(1.8),
            2..=3 => default_zoom.min(1.6),
            _ => default_zoom.min(1.4),
        }
    }
}

/// Cluster click segments that are spatially and temporally close.
/// Uses a simple single-pass greedy algorithm (similar to DBSCAN's core idea):
/// each new click either joins an existing cluster or starts a new one.
pub fn cluster_clicks(scored_segments: &[ScoredSegment]) -> Vec<ClickCluster> {
    let click_segments: Vec<&ScoredSegment> = scored_segments
        .iter()
        .filter(|s| s.segment.segment_type == SegmentType::Click)
        .collect();

    if click_segments.is_empty() {
        return Vec::new();
    }

    let mut clusters: Vec<ClickCluster> = Vec::new();

    for seg in &click_segments {
        let fp = match &seg.segment.focus_point {
            Some(fp) => fp,
            None => continue,
        };

        // Try to find an existing cluster this click belongs to
        let mut merged = false;
        for cluster in clusters.iter_mut() {
            let dx = fp.x - cluster.center_x;
            let dy = fp.y - cluster.center_y;
            let spatial_dist = (dx * dx + dy * dy).sqrt();
            let temporal_dist = seg.segment.start_ms.saturating_sub(cluster.end_ms);

            if spatial_dist <= CLUSTER_SPATIAL_EPS && temporal_dist <= CLUSTER_TEMPORAL_EPS {
                // Merge into this cluster
                let n = cluster.count as f64;
                cluster.center_x = (cluster.center_x * n + fp.x) / (n + 1.0);
                cluster.center_y = (cluster.center_y * n + fp.y) / (n + 1.0);
                cluster.count += 1;
                cluster.end_ms = seg.segment.start_ms;
                // Update bounding rect
                cluster.bounding_rect.x = cluster.bounding_rect.x.min(fp.x);
                cluster.bounding_rect.y = cluster.bounding_rect.y.min(fp.y);
                let max_x = (cluster.bounding_rect.x + cluster.bounding_rect.width).max(fp.x);
                let max_y = (cluster.bounding_rect.y + cluster.bounding_rect.height).max(fp.y);
                cluster.bounding_rect.width = max_x - cluster.bounding_rect.x;
                cluster.bounding_rect.height = max_y - cluster.bounding_rect.y;
                merged = true;
                break;
            }
        }

        if !merged {
            clusters.push(ClickCluster {
                center_x: fp.x,
                center_y: fp.y,
                count: 1,
                start_ms: seg.segment.start_ms,
                end_ms: seg.segment.start_ms,
                bounding_rect: Rect {
                    x: fp.x,
                    y: fp.y,
                    width: 0.0,
                    height: 0.0,
                },
            });
        }
    }

    clusters
}

/// Look up the cluster that a click at the given time belongs to.
/// Returns the recommended zoom level for that cluster, or None if not in any cluster.
pub fn cluster_zoom_for_click(
    clusters: &[ClickCluster],
    click_ms: u64,
    click_x: f64,
    click_y: f64,
    default_zoom: f64,
) -> Option<f64> {
    for cluster in clusters {
        if cluster.count < 2 {
            continue; // Only adjust zoom for multi-click clusters
        }
        if click_ms >= cluster.start_ms && click_ms <= cluster.end_ms + CLUSTER_TEMPORAL_EPS {
            let dx = click_x - cluster.center_x;
            let dy = click_y - cluster.center_y;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist <= CLUSTER_SPATIAL_EPS {
                return Some(cluster.recommended_zoom(default_zoom));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::analyzer::{FocusPoint, Segment};

    fn click_scored(t: u64, x: f64, y: f64, importance: f64) -> ScoredSegment {
        ScoredSegment {
            segment: Segment {
                segment_type: SegmentType::Click,
                start_ms: t,
                end_ms: t + 100,
                focus_point: Some(FocusPoint { x, y, region: None }),
                idle_level: None,
                window_rect: None,
                window_changed: false,
            },
            importance,
        }
    }

    #[test]
    fn test_single_click_one_cluster() {
        let segs = vec![click_scored(1000, 500.0, 300.0, 0.5)];
        let clusters = cluster_clicks(&segs);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].count, 1);
    }

    #[test]
    fn test_close_clicks_merge() {
        let segs = vec![
            click_scored(1000, 500.0, 300.0, 0.5),
            click_scored(1500, 550.0, 320.0, 0.5), // 50px away, 500ms later
            click_scored(2000, 520.0, 310.0, 0.5), // 20px away, 500ms later
        ];
        let clusters = cluster_clicks(&segs);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].count, 3);
    }

    #[test]
    fn test_distant_clicks_separate() {
        let segs = vec![
            click_scored(1000, 100.0, 100.0, 0.5),
            click_scored(1500, 1500.0, 900.0, 0.5), // far away
        ];
        let clusters = cluster_clicks(&segs);
        assert_eq!(clusters.len(), 2);
    }

    #[test]
    fn test_temporally_distant_clicks_separate() {
        let segs = vec![
            click_scored(1000, 500.0, 300.0, 0.5),
            click_scored(5000, 510.0, 310.0, 0.5), // 4s later, beyond temporal eps
        ];
        let clusters = cluster_clicks(&segs);
        assert_eq!(clusters.len(), 2);
    }

    #[test]
    fn test_cluster_zoom_levels() {
        let segs = vec![
            click_scored(1000, 500.0, 300.0, 0.5),
            click_scored(1500, 520.0, 310.0, 0.5),
            click_scored(2000, 540.0, 320.0, 0.5),
        ];
        let clusters = cluster_clicks(&segs);
        assert_eq!(clusters[0].count, 3);
        // 2-3 clicks → 1.6x max
        assert_eq!(clusters[0].recommended_zoom(2.0), 1.6);
    }

    #[test]
    fn test_cluster_zoom_for_click_lookup() {
        let segs = vec![
            click_scored(1000, 500.0, 300.0, 0.5),
            click_scored(1500, 520.0, 310.0, 0.5),
        ];
        let clusters = cluster_clicks(&segs);
        // Click in the cluster area
        let zoom = cluster_zoom_for_click(&clusters, 1200, 510.0, 305.0, 2.0);
        assert!(zoom.is_some());
        assert_eq!(zoom.unwrap(), 1.6);
        // Click far away
        let zoom = cluster_zoom_for_click(&clusters, 1200, 1500.0, 900.0, 2.0);
        assert!(zoom.is_none());
    }
}
