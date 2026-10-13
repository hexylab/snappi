//! カメラエンジンの設定 (Issue #29)。ユーザー設定「ズーム強さ」から内部の値を決める。

use super::spec;
use serde::{Deserialize, Serialize};

/// ユーザー設定「ズーム強さ」（仕様 §2.1）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]
pub enum ZoomStrength {
    /// 自動ズームしない
    Off,
    Subtle,
    #[default]
    Standard,
    Strong,
}

impl ZoomStrength {
    /// 寄るときの倍率 Z。Off は None。
    pub fn zoom(self) -> Option<f64> {
        match self {
            ZoomStrength::Off => None,
            ZoomStrength::Subtle => Some(spec::ZOOM_SUBTLE),
            ZoomStrength::Standard => Some(spec::ZOOM_STANDARD),
            ZoomStrength::Strong => Some(spec::ZOOM_STRONG),
        }
    }
}

/// カメラエンジンの設定
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraConfig {
    /// 区間を自動生成するか（ズーム強さ Off なら false）
    pub auto_segments: bool,
    /// 区間の zoom が None のときに使う倍率 Z
    pub zoom: f64,
}

impl CameraConfig {
    pub fn from_strength(strength: ZoomStrength) -> Self {
        Self {
            auto_segments: strength != ZoomStrength::Off,
            // Off でも手動で追加した区間は既定の倍率で寄る
            zoom: strength.zoom().unwrap_or(spec::ZOOM_STANDARD),
        }
    }
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self::from_strength(ZoomStrength::default())
    }
}
