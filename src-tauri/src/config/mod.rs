pub mod defaults;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub recording: RecordingSettings,
    pub style: StyleSettings,
    pub effects: EffectsSettings,
    pub output: OutputSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingSettings {
    pub hotkey: String,
    pub fps: u32,
    pub capture_system_audio: bool,
    pub capture_microphone: bool,
    pub max_duration_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleSettings {
    pub background: BackgroundConfig,
    pub border_radius: u32,
    pub shadow_enabled: bool,
    pub shadow_blur: f64,
    pub shadow_offset_y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
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
pub struct EffectsSettings {
    pub auto_zoom_enabled: bool,
    pub default_zoom_level: f64,
    pub text_input_zoom_level: f64,
    pub max_zoom: f64,
    pub idle_timeout_ms: u64,
    pub click_ring_enabled: bool,
    pub key_badge_enabled: bool,
    pub cursor_smoothing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputSettings {
    pub default_format: ExportFormat,
    pub default_quality: QualityPreset,
    pub save_directory: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ExportFormat {
    Mp4,
    Gif,
    WebM,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    pub monitor_scale: f64,
    pub recording_dir: String,
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
}

/// Recording info for the frontend list
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingInfo {
    pub id: String,
    pub date: String,
    pub duration_ms: u64,
    pub thumbnail_path: Option<String>,
    pub recording_dir: String,
}

/// Recording state
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RecordingState {
    Idle,
    Recording,
    Paused,
    Processing,
}

/// Export progress
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportProgress {
    pub stage: String,
    pub progress: f64,
    pub output_path: Option<String>,
}
