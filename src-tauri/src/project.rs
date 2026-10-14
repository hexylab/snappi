//! 録画ごとの編集内容 `project.json`。
//!
//! プレビューで編集したズーム区間・トリムを録画ディレクトリに保存し、書き出しもこの内容を使う。
//! ファイルが無い録画は、自動生成した区間と録画全体のトリムで作る。

use crate::camera::{self, spec, CameraConfig, ZoomSegment, ZoomStrength};
use crate::recording_data::RecordingData;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const PROJECT_FILE: &str = "project.json";
pub const PROJECT_VERSION: u32 = 1;

/// 書き出す範囲（録画時刻 ms）。`[start_ms, end_ms)`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]
pub struct Trim {
    #[cfg_attr(feature = "ts-export", ts(type = "number"))]
    pub start_ms: u64,
    #[cfg_attr(feature = "ts-export", ts(type = "number"))]
    pub end_ms: u64,
}

impl Trim {
    pub fn full(duration_ms: u64) -> Self {
        Self { start_ms: 0, end_ms: duration_ms }
    }

    /// 録画の長さに収める。範囲が空になる（壊れた値）なら録画全体にする。
    pub fn clamped(self, duration_ms: u64) -> Self {
        let end_ms = self.end_ms.min(duration_ms);
        let start_ms = self.start_ms.min(end_ms);
        if start_ms >= end_ms {
            Self::full(duration_ms)
        } else {
            Self { start_ms, end_ms }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]
pub struct Project {
    pub version: u32,
    pub segments: Vec<ZoomSegment>,
    pub trim: Trim,
    /// この録画だけのズーム強さ。None なら設定の値を使う
    pub zoom_strength: Option<ZoomStrength>,
}

impl Project {
    pub fn new(segments: Vec<ZoomSegment>, duration_ms: u64) -> Self {
        Self {
            version: PROJECT_VERSION,
            segments,
            trim: Trim::full(duration_ms),
            zoom_strength: None,
        }
    }

    /// 自動生成した区間と自動トリムで作る（`project.json` が無い録画の初期値）
    pub fn auto(data: &RecordingData, strength: ZoomStrength) -> Self {
        let segments = camera::auto_segments(data, &CameraConfig::from_strength(strength));
        let mut project = Self::new(segments, data.meta.duration_ms);
        project.trim = auto_trim(data);
        project
    }

    pub fn path(recording_dir: &Path) -> PathBuf {
        recording_dir.join(PROJECT_FILE)
    }

    /// `project.json` を読む。ファイルが無ければ None。
    pub fn load(recording_dir: &Path) -> Result<Option<Self>> {
        let path = Self::path(recording_dir);
        if !path.exists() {
            return Ok(None);
        }
        let project: Self = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
        if project.version > PROJECT_VERSION {
            anyhow::bail!(
                "project.json のバージョン {} には対応していません（対応: {}）",
                project.version,
                PROJECT_VERSION
            );
        }
        Ok(Some(project))
    }

    /// 書き込み途中で壊れないよう、一時ファイルに書いてから置き換える。
    pub fn save(&self, recording_dir: &Path) -> Result<()> {
        let path = Self::path(recording_dir);
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// この録画のカメラ設定。ズーム強さはプロジェクトの指定、無ければ `default_strength`。
    pub fn camera_config(&self, default_strength: ZoomStrength) -> CameraConfig {
        CameraConfig::from_strength(self.zoom_strength.unwrap_or(default_strength))
    }
}

/// T1: 録画の開始・停止の操作（Snappi 自身のウィンドウやホットキー）が写らないように切るトリム。
///
/// - 開始: 開始ガード（R5）と、録画冒頭 `AUTO_TRIM_START_SETTLE_MS` 以内の大きな画面の変化
///   （Snappi のウィンドウが閉じる等）が写ったフレームの遅い方。最初の操作より後にはしない
/// - 終了: 終了ガード（R5）の手前
/// - 短い録画（`AUTO_TRIM_MIN_DURATION_MS` 未満）は切らない
pub fn auto_trim(data: &RecordingData) -> Trim {
    let duration = data.meta.duration_ms;
    if duration < spec::AUTO_TRIM_MIN_DURATION_MS {
        return Trim::full(duration);
    }
    let settled = data
        .changes
        .iter()
        .filter(|c| c.t <= spec::AUTO_TRIM_START_SETTLE_MS)
        .filter(|c| c.rects.iter().any(|r| r.cells >= spec::BIG_CHANGE_CELLS))
        .map(|c| c.t)
        .max()
        .unwrap_or(0);
    let first_activity = camera::extract_activities(&data.events, duration).first().map_or(u64::MAX, |a| a.t);
    let start = spec::START_GUARD_MS.max(settled).min(first_activity);
    let end = duration.saturating_sub(spec::END_GUARD_MS).max(start + 1);
    Trim { start_ms: start, end_ms: end }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::FocusMode;
    use crate::changes::{ChangeRect, FrameChange, Rect};
    use crate::config::{RecordingEvent, RecordingMeta};

    fn data(duration_ms: u64, events: Vec<RecordingEvent>, changes: Vec<FrameChange>) -> RecordingData {
        RecordingData {
            dir: Default::default(),
            meta: RecordingMeta {
                version: 2,
                id: "t".into(),
                screen_width: 1920,
                screen_height: 1080,
                fps: 60,
                start_time: String::new(),
                duration_ms,
                has_audio: false,
                recording_dir: String::new(),
                recording_mode: None,
                window_title: None,
                window_initial_rect: None,
                frame_count: 0,
            },
            events,
            frame_times: vec![],
            changes,
        }
    }

    fn big_change(t: u64) -> FrameChange {
        FrameChange { t, rects: vec![ChangeRect { rect: Rect::new(100.0, 200.0, 1000.0, 900.0), cells: 1400 }] }
    }

    #[test]
    fn test_auto_trim_cuts_start_and_stop_operations() {
        // Snappi のウィンドウが 1124ms のフレームで消える
        let d = data(17_000, vec![], vec![big_change(1124), big_change(6000)]);
        assert_eq!(auto_trim(&d), Trim { start_ms: 1124, end_ms: 15_500 });
        // 大きな変化が無ければ開始ガード分
        let d = data(17_000, vec![], vec![]);
        assert_eq!(auto_trim(&d), Trim { start_ms: 1000, end_ms: 15_500 });
    }

    #[test]
    fn test_auto_trim_keeps_first_activity_and_short_recordings() {
        let click = RecordingEvent::Click { t: 1100, btn: "left".into(), x: 1.0, y: 1.0 };
        let d = data(17_000, vec![click], vec![big_change(2000)]);
        assert_eq!(auto_trim(&d).start_ms, 1100);
        assert_eq!(auto_trim(&data(4000, vec![], vec![])), Trim::full(4000));
    }

    fn segment(start_ms: u64, end_ms: u64) -> ZoomSegment {
        ZoomSegment { id: format!("auto-{}", start_ms), start_ms, end_ms, zoom: None, focus: FocusMode::Auto }
    }

    #[test]
    fn test_trim_clamped() {
        assert_eq!(Trim { start_ms: 100, end_ms: 900 }.clamped(1000), Trim { start_ms: 100, end_ms: 900 });
        assert_eq!(Trim { start_ms: 100, end_ms: 5000 }.clamped(1000), Trim { start_ms: 100, end_ms: 1000 });
        // 空・逆転は全体に戻す
        assert_eq!(Trim { start_ms: 800, end_ms: 800 }.clamped(1000), Trim::full(1000));
        assert_eq!(Trim { start_ms: 2000, end_ms: 3000 }.clamped(1000), Trim::full(1000));
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let dir = tempfile::TempDir::new().unwrap();
        assert_eq!(Project::load(dir.path()).unwrap(), None);

        let mut project = Project::new(vec![segment(1000, 3000)], 10_000);
        project.segments.push(ZoomSegment {
            id: "user-1".into(),
            start_ms: 5000,
            end_ms: 6000,
            zoom: Some(2.2),
            focus: FocusMode::Fixed { x: 10.0, y: 20.0 },
        });
        project.trim = Trim { start_ms: 500, end_ms: 9000 };
        project.save(dir.path()).unwrap();

        assert_eq!(Project::load(dir.path()).unwrap(), Some(project));
        assert!(!dir.path().join("project.json.tmp").exists());
    }

    #[test]
    fn test_json_format() {
        let json = serde_json::to_value(Project::new(vec![segment(1000, 3000)], 12_345)).unwrap();
        assert_eq!(json["version"], 1);
        assert_eq!(json["trim"], serde_json::json!({ "start_ms": 0, "end_ms": 12345 }));
        assert_eq!(json["zoom_strength"], serde_json::Value::Null);
        assert_eq!(json["segments"][0]["focus"], serde_json::json!({ "type": "Auto" }));
    }

    #[test]
    fn test_rejects_newer_version() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut project = Project::new(vec![], 1000);
        project.version = PROJECT_VERSION + 1;
        project.save(dir.path()).unwrap();
        assert!(Project::load(dir.path()).is_err());
    }

    #[test]
    fn test_camera_config_prefers_project_strength() {
        let mut project = Project::new(vec![], 1000);
        assert_eq!(project.camera_config(ZoomStrength::Strong), CameraConfig::from_strength(ZoomStrength::Strong));
        project.zoom_strength = Some(ZoomStrength::Subtle);
        assert_eq!(project.camera_config(ZoomStrength::Strong), CameraConfig::from_strength(ZoomStrength::Subtle));
    }
}
