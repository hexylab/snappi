//! カメラの評価基盤。`snappi-eval` CLI と統合テストから使う。

pub mod debug_video;
pub mod expect;
pub mod metrics;

use crate::camera::{self, CameraConfig, CameraTrack};
use crate::recording_data::RecordingData;
use expect::{CheckResult, Expectation};
use metrics::{Criteria, MetricResult, Metrics};
use std::path::Path;

/// 評価に使う出力 fps（Social プリセットと同じ）
pub const EVAL_FPS: u32 = 30;

/// 既定設定（ズーム強さ「標準」）のカメラエンジンでカメラトラックを求める。
pub fn compute_track(data: &RecordingData, fps: u32) -> CameraTrack {
    let config = CameraConfig::default();
    camera::camera_track(data, &camera::auto_segments(data, &config), &config, fps)
}

/// 1つの録画の評価結果
pub struct Report {
    pub track: CameraTrack,
    pub metrics: Metrics,
    pub metric_results: Vec<MetricResult>,
    /// expect.json が無ければ空
    pub check_results: Vec<CheckResult>,
}

impl Report {
    pub fn passed(&self) -> bool {
        self.metric_results.iter().all(|r| r.passed) && self.check_results.iter().all(|r| r.passed)
    }
}

/// 録画ディレクトリを読み込み、カメラトラックを計算してメトリクスと expect.json を評価する。
pub fn evaluate_dir(dir: &Path) -> anyhow::Result<(RecordingData, Report)> {
    let data = RecordingData::load(dir)?;
    let track = compute_track(&data, EVAL_FPS);
    let metrics = Metrics::compute(&data, &track);
    let metric_results = metrics.evaluate(&Criteria::default());
    let check_results = Expectation::load(dir)?
        .map(|e| e.evaluate(&track, data.width() as f64, data.height() as f64))
        .unwrap_or_default();
    let report = Report { track, metrics, metric_results, check_results };
    Ok((data, report))
}
