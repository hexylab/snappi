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
    #[serde(default)]
    pub recording_mode: RecordingMode,
}

/// Recording mode: full display or specific window
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum RecordingMode {
    Display,
    Window {
        hwnd: isize,
        title: String,
        rect: [f64; 4],
    },
    Area {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    },
}

impl Default for RecordingMode {
    fn default() -> Self {
        RecordingMode::Display
    }
}

/// Info about a visible window (for window selection UI)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub hwnd: isize,
    pub title: String,
    pub rect: [f64; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleSettings {
    pub background: BackgroundConfig,
    pub border_radius: u32,
    pub shadow_enabled: bool,
    pub shadow_blur: f64,
    pub shadow_offset_y: f64,
    /// Path to a custom cursor PNG image (with transparency).
    /// If set, this is used instead of the system cursor capture.
    #[serde(default)]
    pub cursor_image_path: Option<String>,
    /// Hotspot X coordinate within the cursor image (tip position).
    #[serde(default)]
    pub cursor_hotspot_x: u32,
    /// Hotspot Y coordinate within the cursor image (tip position).
    #[serde(default)]
    pub cursor_hotspot_y: u32,
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
    #[serde(default)]
    pub zoom_intensity: ZoomIntensity,
    #[serde(default)]
    pub animation_speed: AnimationSpeed,
    #[serde(default = "default_true")]
    pub smart_zoom_enabled: bool,
    #[serde(default)]
    pub motion_blur_enabled: bool,
    /// 画面差分(frame diff)を考慮してシーンBBoxを拡張するか
    #[serde(default = "default_true")]
    pub frame_diff_enabled: bool,
    /// WorkArea→Window のアイドルしきい値 (ms)
    #[serde(default = "default_idle_zoom_out_ms")]
    pub idle_zoom_out_ms: u64,
    /// Window→Overview のアイドルしきい値 (ms)
    #[serde(default = "default_idle_overview_ms")]
    pub idle_overview_ms: u64,
    /// WorkArea 最小滞在時間 (ms)
    #[serde(default = "default_min_workarea_dwell_ms")]
    pub min_workarea_dwell_ms: u64,
    /// Window 最小滞在時間 (ms)
    #[serde(default = "default_min_window_dwell_ms")]
    pub min_window_dwell_ms: u64,
    /// クラスタの有効期間 (ms)
    #[serde(default = "default_cluster_lifetime_ms")]
    pub cluster_lifetime_ms: u64,
    /// クラスタが安定するまでの時間 (ms)
    #[serde(default = "default_cluster_stability_ms")]
    pub cluster_stability_ms: u64,
}

fn default_true() -> bool { true }
fn default_idle_zoom_out_ms() -> u64 { 5000 }
fn default_idle_overview_ms() -> u64 { 8000 }
fn default_min_workarea_dwell_ms() -> u64 { 2000 }
fn default_min_window_dwell_ms() -> u64 { 1500 }
fn default_cluster_lifetime_ms() -> u64 { 5000 }
fn default_cluster_stability_ms() -> u64 { 1000 }

/// Controls how frequently auto-zoom triggers
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ZoomIntensity {
    Minimal,   // Only window changes and text input
    Balanced,  // Importance score >= 0.4 (default)
    Active,    // Importance score >= 0.25
}

impl Default for ZoomIntensity {
    fn default() -> Self {
        ZoomIntensity::Balanced
    }
}

impl ZoomIntensity {
    pub fn importance_threshold(&self) -> f64 {
        match self {
            ZoomIntensity::Minimal => 0.6,
            ZoomIntensity::Balanced => 0.4,
            ZoomIntensity::Active => 0.25,
        }
    }

    pub fn cooldown_ms(&self) -> u64 {
        match self {
            ZoomIntensity::Minimal => 3000,
            ZoomIntensity::Balanced => 1500,
            ZoomIntensity::Active => 800,
        }
    }

    /// v3: クラスタが安定したと判断するまでの時間
    pub fn cluster_stability_ms(&self) -> u64 {
        match self {
            ZoomIntensity::Minimal => 2000,
            ZoomIntensity::Balanced => 1000,
            ZoomIntensity::Active => 500,
        }
    }

    /// v3: WorkArea→Window へズームアウトするIdleしきい値
    pub fn idle_medium_ms_v3(&self) -> u64 {
        match self {
            ZoomIntensity::Minimal => 6000,
            ZoomIntensity::Balanced => 5000,
            ZoomIntensity::Active => 3000,
        }
    }

    /// v3: Window→Overview へズームアウトするIdleしきい値
    pub fn idle_long_ms_v3(&self) -> u64 {
        match self {
            ZoomIntensity::Minimal => 10000,
            ZoomIntensity::Balanced => 8000,
            ZoomIntensity::Active => 6000,
        }
    }
}

/// Controls animation speed for zoom/pan transitions
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AnimationSpeed {
    Slow,
    Mellow,  // default
    Quick,
    Rapid,
}

impl Default for AnimationSpeed {
    fn default() -> Self {
        AnimationSpeed::Mellow
    }
}

impl AnimationSpeed {
    pub fn zoom_in_half_life(&self) -> f64 {
        match self {
            AnimationSpeed::Slow => 0.30,
            AnimationSpeed::Mellow => 0.18,
            AnimationSpeed::Quick => 0.12,
            AnimationSpeed::Rapid => 0.08,
        }
    }

    pub fn zoom_out_half_life(&self) -> f64 {
        match self {
            AnimationSpeed::Slow => 0.50,
            AnimationSpeed::Mellow => 0.35,
            AnimationSpeed::Quick => 0.25,
            AnimationSpeed::Rapid => 0.18,
        }
    }

    pub fn pan_half_life(&self) -> f64 {
        match self {
            AnimationSpeed::Slow => 0.30,
            AnimationSpeed::Mellow => 0.22,
            AnimationSpeed::Quick => 0.15,
            AnimationSpeed::Rapid => 0.10,
        }
    }

    /// v3: half-lifeに掛けるスケール係数
    pub fn speed_scale(&self) -> f64 {
        match self {
            AnimationSpeed::Slow => 1.5,
            AnimationSpeed::Mellow => 1.0,
            AnimationSpeed::Quick => 0.7,
            AnimationSpeed::Rapid => 0.5,
        }
    }
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
    #[serde(default)]
    pub recording_mode: Option<String>,
    #[serde(default)]
    pub window_title: Option<String>,
    #[serde(default)]
    pub window_initial_rect: Option<[f64; 4]>,
}

/// Lightweight event representation for Timeline UI visualization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEvent {
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
}

/// Recording info for the frontend list
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingInfo {
    pub id: String,
    pub date: String,
    pub duration_ms: u64,
    pub frame_count: u32,
    pub thumbnail_path: Option<String>,
    pub recording_dir: String,
    pub screen_width: u32,
    pub screen_height: u32,
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
