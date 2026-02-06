export interface RecordingInfo {
  id: string;
  date: string;
  duration_ms: number;
  thumbnail_path: string | null;
  recording_dir: string;
}

export type RecordingState = "Idle" | "Recording" | "Paused" | "Processing";

export type ExportFormat = "Mp4" | "Gif" | "WebM";

export type QualityPreset = "Social" | "HighQuality" | "Lightweight";

export interface ExportProgress {
  stage: string;
  progress: number;
  output_path: string | null;
}

export interface BackgroundConfig {
  type: "Gradient" | "Solid" | "Transparent";
  from?: number[];
  to?: number[];
  angle?: number;
  color?: number[];
}

export interface AppSettings {
  recording: {
    hotkey: string;
    fps: number;
    capture_system_audio: boolean;
    capture_microphone: boolean;
    max_duration_seconds: number;
  };
  style: {
    background: BackgroundConfig;
    border_radius: number;
    shadow_enabled: boolean;
    shadow_blur: number;
    shadow_offset_y: number;
  };
  effects: {
    auto_zoom_enabled: boolean;
    default_zoom_level: number;
    text_input_zoom_level: number;
    max_zoom: number;
    idle_timeout_ms: number;
    click_ring_enabled: boolean;
    key_badge_enabled: boolean;
    cursor_smoothing: boolean;
  };
  output: {
    default_format: ExportFormat;
    default_quality: QualityPreset;
    save_directory: string;
  };
}
