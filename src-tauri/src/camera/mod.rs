//! カメラ（ズーム・パン）エンジン。仕様は `docs/camera-spec.md`。
//!
//! データの流れ: イベント・画面の変化領域 → `subject`（操作ごとの主役）→ `auto_segments`（ショット =
//! ZoomSegment 列）→ `camera_track`（各出力フレームの `CameraState`）→ 合成。評価 (`crate::eval`) は `CameraTrack` を検証する。
//! 座標はすべてキャプチャ領域基準（フレーム左上が原点）の px。

pub mod config;
pub mod segments;
pub mod spec;
pub mod subject;
pub mod track;

pub use config::{CameraConfig, ZoomStrength};
pub use segments::{auto_segments, plan, FocusMode, Shot, ZoomSegment};
pub use track::camera_track;

use crate::config::RecordingEvent;
use crate::recording_data::output_frame_time;
use serde::{Deserialize, Serialize};

/// ある時刻のカメラの状態。ビューポートは `(center_x, center_y)` を中心とする
/// 「フレームの 1/zoom の大きさ」の矩形。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]
pub struct CameraState {
    pub center_x: f64,
    pub center_y: f64,
    pub zoom: f64,
}

impl CameraState {
    /// 全体表示（zoom 1.0）
    pub fn overview(width: f64, height: f64) -> Self {
        Self {
            center_x: width / 2.0,
            center_y: height / 2.0,
            zoom: 1.0,
        }
    }

    /// フレームサイズ `width` x `height` に対するビューポート矩形。フレーム内への
    /// クランプはしない（はみ出しの検出に使うため）。
    pub fn viewport(&self, width: f64, height: f64) -> Viewport {
        let w = width / self.zoom;
        let h = height / self.zoom;
        Viewport {
            x: self.center_x - w / 2.0,
            y: self.center_y - h / 2.0,
            width: w,
            height: h,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Viewport {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }

    /// 中心を保ったまま幅・高さを `ratio` 倍した矩形（デッドゾーン等）
    pub fn scaled(&self, ratio: f64) -> Viewport {
        let w = self.width * ratio;
        let h = self.height * ratio;
        Viewport {
            x: self.x + (self.width - w) / 2.0,
            y: self.y + (self.height - h) / 2.0,
            width: w,
            height: h,
        }
    }

    /// フレーム `[0, width] x [0, height]` に収まっているか（`tolerance` px の誤差は許す）
    pub fn is_within(&self, width: f64, height: f64, tolerance: f64) -> bool {
        self.x >= -tolerance
            && self.y >= -tolerance
            && self.x + self.width <= width + tolerance
            && self.y + self.height <= height + tolerance
    }
}

/// カメラエンジンの出力。出力フレーム k（時刻 `output_frame_time(k, fps)`）ごとのカメラ状態。
/// エクスポートの出力フレームと 1 対 1 に対応する。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]
pub struct CameraTrack {
    pub fps: u32,
    pub frames: Vec<CameraState>,
}

impl CameraTrack {
    /// フレーム `k` の時刻 (ms)
    pub fn time_ms(&self, k: usize) -> u64 {
        output_frame_time(k as u64, self.fps)
    }

    /// 時刻 `t_ms` 以前で最も新しいフレームの番号。トラックが空なら None。
    pub fn index_at(&self, t_ms: u64) -> Option<usize> {
        if self.frames.is_empty() {
            return None;
        }
        // 時刻はほぼ k * 1000 / fps なので、概算してから丸め誤差分を補正する
        let last = self.frames.len() - 1;
        let mut k = ((t_ms * self.fps as u64 / 1000) as usize).min(last);
        while k < last && self.time_ms(k + 1) <= t_ms {
            k += 1;
        }
        while k > 0 && self.time_ms(k) > t_ms {
            k -= 1;
        }
        Some(k)
    }

    pub fn state_at(&self, t_ms: u64) -> Option<CameraState> {
        self.index_at(t_ms).map(|k| self.frames[k])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityKind {
    Click,
    Key,
    Scroll,
    /// ドラッグの終了（ドロップ）。開始はクリックとして別に数える。
    Drag,
}

/// 仕様 R1 の「アクティビティ」。キー入力は位置を持たない。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Activity {
    pub t: u64,
    pub kind: ActivityKind,
    pub pos: Option<(f64, f64)>,
}

/// イベント列（時刻順）から仕様 R1 のアクティビティを取り出す。
/// R5 に従い、録画開始直後・終了直前（Snappi 自身の操作）は除く。
pub fn extract_activities(events: &[RecordingEvent], duration_ms: u64) -> Vec<Activity> {
    let end_guard_start = duration_ms.saturating_sub(spec::END_GUARD_MS);
    let mut press: Option<(String, f64, f64)> = None;
    let mut activities = Vec::new();
    for event in events {
        let activity = match event {
            RecordingEvent::Click { t, btn, x, y } => {
                press = Some((btn.clone(), *x, *y));
                Some(Activity { t: *t, kind: ActivityKind::Click, pos: Some((*x, *y)) })
            }
            RecordingEvent::ClickRelease { t, btn, x, y } => match press.take() {
                Some((pb, px, py))
                    if pb == *btn && (x - px).hypot(y - py) >= spec::DRAG_MIN_DISTANCE =>
                {
                    Some(Activity { t: *t, kind: ActivityKind::Drag, pos: Some((*x, *y)) })
                }
                _ => None,
            },
            RecordingEvent::Key { t, .. } => {
                Some(Activity { t: *t, kind: ActivityKind::Key, pos: None })
            }
            RecordingEvent::Scroll { t, x, y, .. } => {
                Some(Activity { t: *t, kind: ActivityKind::Scroll, pos: Some((*x, *y)) })
            }
            _ => None,
        };
        if let Some(a) = activity {
            if a.t >= spec::START_GUARD_MS && a.t < end_guard_start {
                activities.push(a);
            }
        }
    }
    activities
}

/// 間隔が `gap_ms` 未満の操作をまとめ、各まとまりの
/// `(最初の時刻, 最後の時刻)` を返す。`activities` は時刻順であること。
pub fn group_activities(activities: &[Activity], gap_ms: u64) -> Vec<(u64, u64)> {
    let mut groups: Vec<(u64, u64)> = Vec::new();
    for a in activities {
        match groups.last_mut() {
            Some((_, end)) if a.t.saturating_sub(*end) < gap_ms => *end = a.t,
            _ => groups.push((a.t, a.t)),
        }
    }
    groups
}

/// 時刻 `t_ms` 以前で最新のカーソル位置（マウス移動・クリック・スクロールの位置）
pub fn cursor_at(events: &[RecordingEvent], t_ms: u64) -> Option<(f64, f64)> {
    let end = events.partition_point(|e| e.timestamp() <= t_ms);
    events[..end].iter().rev().find_map(|e| match e {
        RecordingEvent::MouseMove { x, y, .. }
        | RecordingEvent::Click { x, y, .. }
        | RecordingEvent::ClickRelease { x, y, .. }
        | RecordingEvent::Scroll { x, y, .. } => Some((*x, *y)),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(t: u64, x: f64, y: f64) -> RecordingEvent {
        RecordingEvent::Click { t, btn: "left".into(), x, y }
    }

    fn release(t: u64, x: f64, y: f64) -> RecordingEvent {
        RecordingEvent::ClickRelease { t, btn: "left".into(), x, y }
    }

    fn key(t: u64) -> RecordingEvent {
        RecordingEvent::Key { t, key: "Character".into(), modifiers: vec![] }
    }

    #[test]
    fn test_viewport_geometry() {
        let vp = CameraState { center_x: 960.0, center_y: 540.0, zoom: 2.0 }.viewport(1920.0, 1080.0);
        assert_eq!(vp, Viewport { x: 480.0, y: 270.0, width: 960.0, height: 540.0 });
        assert!(vp.contains(480.0, 810.0));
        assert!(!vp.contains(479.0, 540.0));
        assert!(vp.is_within(1920.0, 1080.0, 0.0));
        let dz = vp.scaled(0.5);
        assert_eq!(dz, Viewport { x: 720.0, y: 405.0, width: 480.0, height: 270.0 });

        let outside = CameraState { center_x: 100.0, center_y: 540.0, zoom: 2.0 }.viewport(1920.0, 1080.0);
        assert!(!outside.is_within(1920.0, 1080.0, 0.5));
    }

    #[test]
    fn test_track_index_at() {
        let track = CameraTrack { fps: 30, frames: vec![CameraState::overview(10.0, 10.0); 4] };
        assert_eq!(track.time_ms(1), 33);
        assert_eq!(track.index_at(0), Some(0));
        assert_eq!(track.index_at(32), Some(0));
        assert_eq!(track.index_at(33), Some(1));
        assert_eq!(track.index_at(10_000), Some(3));
        let empty = CameraTrack { fps: 30, frames: vec![] };
        assert_eq!(empty.index_at(0), None);
    }

    #[test]
    fn test_extract_activities_detects_drag_and_applies_guards() {
        let events = vec![
            click(500, 0.0, 0.0), // 開始ガード内
            release(550, 0.0, 0.0),
            click(2000, 100.0, 100.0),
            release(2100, 102.0, 101.0), // 動きが小さいのでドラッグではない
            click(3000, 100.0, 100.0),
            RecordingEvent::MouseMove { t: 3100, x: 300.0, y: 100.0 },
            release(3200, 300.0, 100.0), // ドラッグ
            key(4000),
            RecordingEvent::Scroll { t: 5000, x: 10.0, y: 20.0, dx: 0.0, dy: -1.0 },
            click(9000, 5.0, 5.0), // 終了ガード内（duration 10000）
        ];
        let acts = extract_activities(&events, 10_000);
        let kinds: Vec<_> = acts.iter().map(|a| (a.t, a.kind)).collect();
        assert_eq!(
            kinds,
            vec![
                (2000, ActivityKind::Click),
                (3000, ActivityKind::Click),
                (3200, ActivityKind::Drag),
                (4000, ActivityKind::Key),
                (5000, ActivityKind::Scroll),
            ]
        );
    }

    #[test]
    fn test_group_activities_by_gap() {
        let acts: Vec<Activity> = [1000, 2000, 4999, 8000, 8500]
            .iter()
            .map(|&t| Activity { t, kind: ActivityKind::Key, pos: None })
            .collect();
        assert_eq!(group_activities(&acts, 3000), vec![(1000, 4999), (8000, 8500)]);
    }

    #[test]
    fn test_cursor_at_uses_latest_position() {
        let events = vec![
            RecordingEvent::MouseMove { t: 10, x: 1.0, y: 2.0 },
            key(20),
            click(30, 5.0, 6.0),
            key(40),
        ];
        assert_eq!(cursor_at(&events, 5), None);
        assert_eq!(cursor_at(&events, 25), Some((1.0, 2.0)));
        assert_eq!(cursor_at(&events, 100), Some((5.0, 6.0)));
    }
}
