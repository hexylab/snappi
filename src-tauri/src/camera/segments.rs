//! ズーム区間の自動生成 (Issue #29)。仕様 `docs/camera-spec-v5.md` §4 の R1〜R5 と、
//! §5 F5 の「遠い移動ではいったん引いてから寄り直す」を区間の分割として実装する。

use super::config::CameraConfig;
use super::{extract_activities, spec, Activity};
use crate::recording_data::RecordingData;
use serde::{Deserialize, Serialize};

/// 寄っている間に映す場所の決め方
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]
pub enum FocusMode {
    /// 仕様 F1 の注目点（カーソル・入力位置）を追従する
    Auto,
    /// 固定位置（キャプチャ領域基準の px）を映す
    Fixed { x: f64, y: f64 },
}

/// 寄っている時間帯。`start_ms` で寄り始め、`end_ms` で引き始める。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]
pub struct ZoomSegment {
    /// 編集・保存用の安定 ID。自動生成した区間は `auto-<start_ms>`。
    pub id: String,
    #[cfg_attr(feature = "ts-export", ts(type = "number"))]
    pub start_ms: u64,
    #[cfg_attr(feature = "ts-export", ts(type = "number"))]
    pub end_ms: u64,
    /// None ならズーム強さの既定倍率
    pub zoom: Option<f64>,
    pub focus: FocusMode,
}

/// 録画のイベントからズーム区間を自動生成する。ズーム強さが Off なら空。
pub fn auto_segments(data: &RecordingData, config: &CameraConfig) -> Vec<ZoomSegment> {
    if !config.auto_segments {
        return Vec::new();
    }
    let duration_ms = data.meta.duration_ms;
    let activities = extract_activities(&data.events, duration_ms);
    let viewport_width = data.width() as f64 / config.zoom;
    plan_segments(&activities, duration_ms, viewport_width)
        .into_iter()
        .map(|(start_ms, end_ms)| ZoomSegment {
            id: format!("auto-{}", start_ms),
            start_ms,
            end_ms,
            zoom: None,
            focus: FocusMode::Auto,
        })
        .collect()
}

/// 区間を計画中のアクティビティの範囲
#[derive(Debug, Clone, Copy)]
struct Draft {
    start: u64,
    end: u64,
    /// 区間に含まれるアクティビティの添字 `[first, last]`
    first: usize,
    last: usize,
}

/// アクティビティ（時刻順・R5 のガード適用済み）から区間 `(start_ms, end_ms)` の列を求める。
/// `viewport_width` は寄っているときのビューポートの幅 (px)。F5 の距離判定に使う。
fn plan_segments(activities: &[Activity], duration_ms: u64, viewport_width: f64) -> Vec<(u64, u64)> {
    // R2: 間隔が MERGE_GAP 未満のアクティビティをまとめる
    let mut groups: Vec<(usize, usize)> = Vec::new();
    for (i, a) in activities.iter().enumerate() {
        match groups.last_mut() {
            Some((_, last)) if a.t - activities[*last].t < spec::MERGE_GAP_MS => *last = i,
            _ => groups.push((i, i)),
        }
    }

    // R3 / R4: 前後に余白を付け、最短長を確保し、隙間の小さい区間を結合する
    let mut drafts: Vec<Draft> = Vec::new();
    for (first, last) in groups {
        let (start, end) = pad(activities[first].t, activities[last].t, duration_ms);
        match drafts.last_mut() {
            Some(prev) if start < prev.end + spec::MIN_GAP_MS => {
                prev.end = prev.end.max(end);
                prev.last = last;
            }
            _ => drafts.push(Draft { start, end, first, last }),
        }
    }

    // F5: 区間内の遠い移動で、次のアクティビティまで時間があれば、いったん引いて寄り直す
    let mut out = Vec::new();
    for d in drafts {
        let mut start = d.start;
        let mut last_pos = None;
        for i in d.first..=d.last {
            let a = &activities[i];
            if let (Some(prev), Some(pos), true) = (last_pos, a.pos, i > d.first) {
                let gap = a.t - activities[i - 1].t;
                let far = dist(prev, pos) >= spec::FAR_JUMP_VIEWPORTS * viewport_width;
                if far && gap >= spec::REZOOM_MIN_MS {
                    // 前半は寄り直しの前に引き終わるように早めに終える
                    let prev_t = activities[i - 1].t;
                    let next_start = a.t.saturating_sub(spec::LEAD_MS);
                    let split_end = (prev_t + spec::HOLD_MS)
                        .min(next_start.saturating_sub(spec::ZOOM_OUT_MS))
                        .max(prev_t);
                    if split_end >= start + spec::MIN_SEGMENT_MS && d.end >= next_start + spec::MIN_SEGMENT_MS {
                        out.push((start, split_end));
                        start = next_start;
                    }
                }
            }
            if a.pos.is_some() {
                last_pos = a.pos;
            }
        }
        out.push((start, d.end));
    }
    out.retain(|&(s, e)| e > s);
    out
}

/// R3 / R4: アクティビティのまとまり `[t0, t1]` に LEAD / HOLD を付け、MIN_SEGMENT 以上にする。
/// 録画の長さを超えないようにし、足りない分は前に延ばす。
fn pad(t0: u64, t1: u64, duration_ms: u64) -> (u64, u64) {
    let start = t0.saturating_sub(spec::LEAD_MS);
    let end = (t1 + spec::HOLD_MS).max(start + spec::MIN_SEGMENT_MS);
    if end <= duration_ms {
        return (start, end);
    }
    let end = duration_ms;
    (start.min(end.saturating_sub(spec::MIN_SEGMENT_MS)), end)
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::ActivityKind;

    fn click(t: u64, x: f64, y: f64) -> Activity {
        Activity { t, kind: ActivityKind::Click, pos: Some((x, y)) }
    }

    fn key(t: u64) -> Activity {
        Activity { t, kind: ActivityKind::Key, pos: None }
    }

    /// 1920px 幅を 1.8 倍で見たときのビューポート幅
    const VPW: f64 = 1920.0 / 1.8;

    #[test]
    fn test_single_activity_gets_lead_hold_and_min_length() {
        // R3: 600ms 前に寄り始め、1500ms 後に引き始める → 2100ms（MIN_SEGMENT 以上）
        assert_eq!(plan_segments(&[click(5000, 0.0, 0.0)], 20_000, VPW), vec![(4400, 6500)]);
    }

    #[test]
    fn test_activities_closer_than_merge_gap_form_one_segment() {
        // R2: 2.9 秒間隔はまとめる / 3 秒以上空くと別のまとまり
        let acts = [key(2000), key(4900), key(7800)];
        assert_eq!(plan_segments(&acts, 20_000, VPW), vec![(1400, 9300)]);
    }

    #[test]
    fn test_small_gap_between_segments_is_merged() {
        // R4: まとまりは別（3.5 秒空く）でも、区間 [1400, 3500] と [4900, 7000] の隙間
        // 1400ms が MIN_GAP 未満なので結合する
        let acts = [key(2000), key(5500)];
        assert_eq!(plan_segments(&acts, 20_000, VPW), vec![(1400, 7000)]);
        // 隙間が十分あれば別の区間
        let acts = [key(2000), key(10_000)];
        assert_eq!(plan_segments(&acts, 20_000, VPW), vec![(1400, 3500), (9400, 11_500)]);
    }

    #[test]
    fn test_segment_does_not_exceed_recording_length() {
        // 録画末尾に近い場合は end を録画の長さで止め、最短長は前に延ばして確保する
        assert_eq!(plan_segments(&[key(3000)], 3800, VPW), vec![(1800, 3800)]);
    }

    #[test]
    fn test_far_jump_with_enough_time_rezooms() {
        // F5: 遠い移動で次のアクティビティまで 3.4 秒 → 前半は引き終えてから寄り直す
        let acts = [click(2000, 100.0, 100.0), click(3000, 120.0, 100.0), click(6400, 1800.0, 1000.0)];
        let segs = plan_segments(&acts, 20_000, VPW);
        // 前半: [1400, min(3000+1500, 5800-700)=4500]、後半: [5800, 7900]
        assert_eq!(segs, vec![(1400, 4500), (5800, 7900)]);
    }

    #[test]
    fn test_far_jump_without_enough_time_pans() {
        // 遠くても間隔が REZOOM_MIN 未満なら1つの区間のまま（パンで追う）
        let acts = [click(2000, 100.0, 100.0), click(4000, 1800.0, 1000.0), click(6000, 100.0, 100.0)];
        assert_eq!(plan_segments(&acts, 20_000, VPW), vec![(1400, 7500)]);
    }

    #[test]
    fn test_near_jump_does_not_split() {
        let acts = [click(2000, 100.0, 100.0), click(5400, 300.0, 100.0)];
        assert_eq!(plan_segments(&acts, 20_000, VPW), vec![(1400, 6900)]);
    }

    #[test]
    fn test_no_activities_no_segments() {
        assert!(plan_segments(&[], 10_000, VPW).is_empty());
    }
}
