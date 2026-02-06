use super::*;
use crate::export::presets::EncodingParams;

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            recording: RecordingSettings::default(),
            style: StyleSettings::default(),
            effects: EffectsSettings::default(),
            output: OutputSettings::default(),
        }
    }
}

impl Default for RecordingSettings {
    fn default() -> Self {
        Self {
            hotkey: "Ctrl+Shift+R".to_string(),
            fps: 60,
            capture_system_audio: true,
            capture_microphone: false,
            max_duration_seconds: 300, // 5 minutes
        }
    }
}

impl Default for StyleSettings {
    fn default() -> Self {
        Self {
            background: BackgroundConfig::Gradient {
                from: [139, 92, 246],  // purple
                to: [59, 130, 246],    // blue
                angle: 135.0,
            },
            border_radius: 12,
            shadow_enabled: true,
            shadow_blur: 40.0,
            shadow_offset_y: 10.0,
        }
    }
}

impl Default for EffectsSettings {
    fn default() -> Self {
        Self {
            auto_zoom_enabled: true,
            default_zoom_level: 2.0,
            text_input_zoom_level: 2.5,
            max_zoom: 3.0,
            idle_timeout_ms: 1500,
            click_ring_enabled: true,
            key_badge_enabled: true,
            cursor_smoothing: true,
        }
    }
}

impl Default for OutputSettings {
    fn default() -> Self {
        let save_dir = dirs::video_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("Snappi");
        Self {
            default_format: ExportFormat::Mp4,
            default_quality: QualityPreset::Social,
            save_directory: save_dir.to_string_lossy().to_string(),
        }
    }
}

/// Output style used by the effects engine
pub struct OutputStyle {
    pub output_width: u32,
    pub output_height: u32,
    pub canvas_width: u32,
    pub canvas_height: u32,
    pub border_radius: u32,
    pub shadow_blur: f64,
    pub shadow_offset_y: f64,
    pub shadow_color: [u8; 4],
    pub cursor_size_multiplier: f64,
    pub click_ring_max_radius: f64,
    pub click_ring_duration_ms: u64,
    pub click_ring_color: [u8; 4],
    pub click_ring_stroke_width: f64,
    pub key_badge_duration_ms: u64,
    pub zoom_spring_tension: f64,
    pub zoom_spring_friction: f64,
}

impl Default for OutputStyle {
    fn default() -> Self {
        Self {
            output_width: 1920,
            output_height: 1080,
            canvas_width: 2048,
            canvas_height: 1208,
            border_radius: 12,
            shadow_blur: 40.0,
            shadow_offset_y: 10.0,
            shadow_color: [0, 0, 0, 80],
            cursor_size_multiplier: 1.2,
            click_ring_max_radius: 30.0,
            click_ring_duration_ms: 400,
            click_ring_color: [59, 130, 246, 180],
            click_ring_stroke_width: 2.5,
            key_badge_duration_ms: 1500,
            zoom_spring_tension: 170.0,
            zoom_spring_friction: 26.0,
        }
    }
}

impl OutputStyle {
    pub fn from_settings(params: &EncodingParams, settings: &AppSettings) -> Self {
        let shadow_enabled = settings.style.shadow_enabled;
        Self {
            output_width: params.width.unwrap_or(1920),
            output_height: params.height.unwrap_or(1080),
            canvas_width: params.canvas_width,
            canvas_height: params.canvas_height,
            border_radius: settings.style.border_radius,
            shadow_blur: if shadow_enabled { settings.style.shadow_blur } else { 0.0 },
            shadow_offset_y: if shadow_enabled { settings.style.shadow_offset_y } else { 0.0 },
            shadow_color: if shadow_enabled { [0, 0, 0, 80] } else { [0, 0, 0, 0] },
            cursor_size_multiplier: 1.2,
            click_ring_max_radius: 30.0,
            click_ring_duration_ms: 400,
            click_ring_color: [59, 130, 246, 180],
            click_ring_stroke_width: 2.5,
            key_badge_duration_ms: 1500,
            zoom_spring_tension: 170.0,
            zoom_spring_friction: 26.0,
        }
    }
}
