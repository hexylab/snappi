//! 評価シナリオごとの期待値 `expect.json` (Issue #28)。
//!
//! ```json
//! {
//!   "description": "操作 → 10秒放置 → 操作",
//!   "checks": [
//!     { "kind": "zoomed_out", "from_ms": 9000, "to_ms": 14000 },
//!     { "kind": "zoomed_in", "from_ms": 2500, "to_ms": 5000, "min_ratio": 0.9 },
//!     { "kind": "visible", "t_ms": 3000, "x": 400, "y": 300 },
//!     { "kind": "max_switches", "max": 4 }
//!   ]
//! }
//! ```

use crate::camera::{spec, CameraTrack};
use crate::eval::metrics::zoom_switches;
use serde::Deserialize;
use std::path::Path;

pub const EXPECT_FILE: &str = "expect.json";

#[derive(Debug, Clone, Deserialize)]
pub struct Expectation {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub checks: Vec<Check>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Check {
    /// `[from_ms, to_ms]` の間、全体表示（寄っていない）であること
    ZoomedOut {
        from_ms: u64,
        to_ms: u64,
        #[serde(default = "default_ratio")]
        min_ratio: f64,
    },
    /// `[from_ms, to_ms]` の間、寄っていること
    ZoomedIn {
        from_ms: u64,
        to_ms: u64,
        #[serde(default = "default_ratio")]
        min_ratio: f64,
    },
    /// 時刻 `t_ms` に点 `(x, y)`（キャプチャ領域基準）がビューポート内に写っていること
    Visible { t_ms: u64, x: f64, y: f64 },
    /// 録画全体での寄る/引くの切り替え回数の上限
    MaxSwitches { max: usize },
}

fn default_ratio() -> f64 {
    1.0
}

#[derive(Debug, Clone)]
pub struct CheckResult {
    pub description: String,
    pub passed: bool,
}

impl Expectation {
    /// 録画ディレクトリの expect.json を読む。無ければ None。
    pub fn load(dir: &Path) -> anyhow::Result<Option<Self>> {
        let path = dir.join(EXPECT_FILE);
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(serde_json::from_str(&std::fs::read_to_string(path)?)?))
    }

    pub fn evaluate(&self, track: &CameraTrack, width: f64, height: f64) -> Vec<CheckResult> {
        self.checks
            .iter()
            .map(|c| c.evaluate(track, width, height))
            .collect()
    }
}

impl Check {
    fn evaluate(&self, track: &CameraTrack, width: f64, height: f64) -> CheckResult {
        let (description, passed) = match *self {
            Check::ZoomedOut { from_ms, to_ms, min_ratio } => {
                let r = zoomed_ratio(track, from_ms, to_ms).map(|r| 1.0 - r);
                (
                    format!("{}〜{}ms 全体表示 {} (基準 >= {:.0}%)", from_ms, to_ms, fmt_ratio(r), min_ratio * 100.0),
                    r.is_some_and(|r| r >= min_ratio),
                )
            }
            Check::ZoomedIn { from_ms, to_ms, min_ratio } => {
                let r = zoomed_ratio(track, from_ms, to_ms);
                (
                    format!("{}〜{}ms 寄っている {} (基準 >= {:.0}%)", from_ms, to_ms, fmt_ratio(r), min_ratio * 100.0),
                    r.is_some_and(|r| r >= min_ratio),
                )
            }
            Check::Visible { t_ms, x, y } => {
                let ok = track
                    .state_at(t_ms)
                    .is_some_and(|s| s.viewport(width, height).contains(x, y));
                (format!("{}ms に ({:.0}, {:.0}) が写っている", t_ms, x, y), ok)
            }
            Check::MaxSwitches { max } => {
                let switches = zoom_switches(track);
                (format!("切り替え {} 回 (基準 <= {})", switches, max), switches <= max)
            }
        };
        CheckResult { description, passed }
    }
}

/// `[from_ms, to_ms]` のフレームのうち寄っている割合。該当フレームが無ければ None。
fn zoomed_ratio(track: &CameraTrack, from_ms: u64, to_ms: u64) -> Option<f64> {
    let zooms: Vec<bool> = (0..track.frames.len())
        .filter(|&k| (from_ms..=to_ms).contains(&track.time_ms(k)))
        .map(|k| track.frames[k].zoom >= spec::ZOOMED_THRESHOLD)
        .collect();
    (!zooms.is_empty()).then(|| zooms.iter().filter(|&&z| z).count() as f64 / zooms.len() as f64)
}

fn fmt_ratio(r: Option<f64>) -> String {
    r.map_or("-".to_string(), |r| format!("{:.1}%", r * 100.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::CameraState;

    #[test]
    fn test_checks() {
        // 10fps・3秒: 1.0〜2.0秒だけ寄る
        let frames = (0..30)
            .map(|k| {
                let zoom = if (10..20).contains(&k) { 2.0 } else { 1.0 };
                CameraState { center_x: 100.0, center_y: 50.0, zoom }
            })
            .collect();
        let track = CameraTrack { fps: 10, frames };
        let exp: Expectation = serde_json::from_str(
            r#"{"checks":[
                {"kind":"zoomed_out","from_ms":0,"to_ms":900},
                {"kind":"zoomed_in","from_ms":1000,"to_ms":1900},
                {"kind":"zoomed_in","from_ms":500,"to_ms":1900,"min_ratio":0.9},
                {"kind":"visible","t_ms":1500,"x":190,"y":50},
                {"kind":"visible","t_ms":500,"x":190,"y":50},
                {"kind":"max_switches","max":1}
            ]}"#,
        )
        .unwrap();
        let passed: Vec<bool> = exp.evaluate(&track, 200.0, 100.0).iter().map(|r| r.passed).collect();
        assert_eq!(passed, vec![true, true, false, false, true, false]);
    }
}
