pub mod defaults;

pub use crate::camera::ZoomStrength;
use serde::{Deserialize, Serialize};

// ts-rs による TypeScript 型の自動生成
//
// `cargo test --features ts-export` を実行すると、`#[cfg_attr(feature = "ts-export", ts(export))]`
// が付いた構造体から TypeScript 定義が `../src/lib/generated/` に出力される。
// 通常ビルドではこの機能は無効化されるため、実行時コストはゼロ。
//
// フロントエンドと受け渡す型には下の各構造体と同じパターンで `#[cfg_attr(...)]` を付ける。

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]
pub struct AppSettings {
    pub recording: RecordingSettings,
    pub style: StyleSettings,
    pub effects: EffectsSettings,
    pub output: OutputSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]
pub struct RecordingSettings {
    pub fps: u32,
    pub capture_system_audio: bool,
    pub capture_microphone: bool,
    #[serde(default)]
    pub recording_mode: RecordingMode,
    /// キー入力のラベル（"a", "Return" など）を events.jsonl に平文で記録するか。
    /// false（既定）の場合はカテゴリのみ（"Character", "Modifier", "Navigation" など）を
    /// 記録し、パスワードやチャット内容などが録画ディレクトリに残らないようにする。
    #[serde(default)]
    pub record_key_labels: bool,
}

/// Recording mode: full display or specific window
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]
pub enum RecordingMode {
    Display,
    Window {
        hwnd: isize,
        title: String,
        rect: [f64; 4],
    },
}

impl Default for RecordingMode {
    fn default() -> Self {
        RecordingMode::Display
    }
}

/// Info about a visible window (for window selection UI)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]
pub struct WindowInfo {
    pub hwnd: isize,
    pub title: String,
    pub rect: [f64; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]
pub struct StyleSettings {
    pub background: BackgroundConfig,
    pub border_radius: u32,
    pub shadow_enabled: bool,
    pub shadow_blur: f64,
    pub shadow_offset_y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]
pub enum BackgroundConfig {
    Gradient {
        from: [u8; 3],
        to: [u8; 3],
        angle: f64,
    },
    Solid {
        color: [u8; 3],
    },
    Transparent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]
pub struct EffectsSettings {
    /// 自動ズームの強さ（Off で自動ズームしない）
    pub zoom_strength: ZoomStrength,
    pub click_ring_enabled: bool,
    pub key_badge_enabled: bool,
    pub cursor_smoothing: bool,
    pub motion_blur_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]
pub struct OutputSettings {
    pub default_format: ExportFormat,
    pub default_quality: QualityPreset,
    pub save_directory: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]
pub enum ExportFormat {
    Mp4,
    Gif,
    WebM,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]
pub enum QualityPreset {
    Social,
    HighQuality,
    Lightweight,
}

/// Metadata about a completed recording session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingMeta {
    pub version: u32,
    pub id: String,
    pub screen_width: u32,
    pub screen_height: u32,
    pub fps: u32,
    pub start_time: String,
    pub duration_ms: u64,
    pub has_audio: bool,
    pub recording_dir: String,
    pub recording_mode: Option<String>,
    pub window_title: Option<String>,
    pub window_initial_rect: Option<[f64; 4]>,
    /// 録画されたフレーム数
    pub frame_count: u32,
}

/// Lightweight event representation for Timeline UI visualization.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]
pub struct TimelineEvent {
    #[cfg_attr(feature = "ts-export", ts(type = "number"))]
    pub time_ms: u64,
    pub event_type: String,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub label: Option<String>,
}

/// Event types recorded during screen capture
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RecordingEvent {
    #[serde(rename = "mouse_move")]
    MouseMove { t: u64, x: f64, y: f64 },
    #[serde(rename = "click")]
    Click {
        t: u64,
        btn: String,
        x: f64,
        y: f64,
    },
    #[serde(rename = "click_release")]
    ClickRelease {
        t: u64,
        btn: String,
        x: f64,
        y: f64,
    },
    #[serde(rename = "key")]
    Key {
        t: u64,
        key: String,
        #[serde(default)]
        modifiers: Vec<String>,
    },
    #[serde(rename = "scroll")]
    Scroll {
        t: u64,
        x: f64,
        y: f64,
        dx: f64,
        dy: f64,
    },
    #[serde(rename = "focus")]
    Focus {
        t: u64,
        el: String,
        name: String,
        rect: [f64; 4],
    },
    #[serde(rename = "window_focus")]
    WindowFocus {
        t: u64,
        title: String,
        rect: [f64; 4],
    },
    #[serde(rename = "ui_focus")]
    UiFocus {
        t: u64,
        control: String,
        name: String,
        rect: [f64; 4],
        #[serde(default)]
        automation_id: String,
    },
    #[serde(rename = "ui_menu_open")]
    UiMenuOpen {
        t: u64,
        control: String,
        name: String,
        rect: [f64; 4],
    },
    #[serde(rename = "ui_menu_close")]
    UiMenuClose {
        t: u64,
        control: String,
        name: String,
    },
    #[serde(rename = "ui_dialog_open")]
    UiDialogOpen {
        t: u64,
        control: String,
        name: String,
        rect: [f64; 4],
    },
    #[serde(rename = "ui_dialog_close")]
    UiDialogClose {
        t: u64,
        control: String,
        name: String,
    },
    /// カーソル形状の変化（"arrow" / "ibeam" / "hand" / "wait" / "size_*" / "other"）
    #[serde(rename = "cursor_shape")]
    CursorShape { t: u64, shape: String },
}

impl RecordingEvent {
    /// イベントの録画時刻 (ms)
    pub fn timestamp(&self) -> u64 {
        match self {
            RecordingEvent::MouseMove { t, .. }
            | RecordingEvent::Click { t, .. }
            | RecordingEvent::ClickRelease { t, .. }
            | RecordingEvent::Key { t, .. }
            | RecordingEvent::Scroll { t, .. }
            | RecordingEvent::Focus { t, .. }
            | RecordingEvent::WindowFocus { t, .. }
            | RecordingEvent::UiFocus { t, .. }
            | RecordingEvent::UiMenuOpen { t, .. }
            | RecordingEvent::UiMenuClose { t, .. }
            | RecordingEvent::UiDialogOpen { t, .. }
            | RecordingEvent::UiDialogClose { t, .. }
            | RecordingEvent::CursorShape { t, .. } => *t,
        }
    }
}

/// Recording info for the frontend list
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]
pub struct RecordingInfo {
    pub id: String,
    pub date: String,
    #[cfg_attr(feature = "ts-export", ts(type = "number"))]
    pub duration_ms: u64,
    pub frame_count: u32,
    pub thumbnail_path: Option<String>,
    pub recording_dir: String,
    pub screen_width: u32,
    pub screen_height: u32,
}

/// Recording state
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]
pub enum RecordingState {
    Idle,
    Recording,
    Paused,
    Processing,
}

/// Export progress
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]
pub struct ExportProgress {
    pub stage: String,
    pub progress: f64,
    pub output_path: Option<String>,
}
