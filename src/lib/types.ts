export interface RecordingInfo {
  id: string;
  date: string;
  duration_ms: number;
  frame_count: number;
  thumbnail_path: string | null;
  recording_dir: string;
  screen_width: number;
  screen_height: number;
}

export type RecordingState = "Idle" | "Recording" | "Paused" | "Processing";

export type ExportFormat = "Mp4" | "Gif" | "WebM";

export type QualityPreset = "Social" | "HighQuality" | "Lightweight";

export type ZoomIntensity = "Minimal" | "Balanced" | "Active";

export type AnimationSpeed = "Slow" | "Mellow" | "Quick" | "Rapid";

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

export type RecordingMode =
  | { type: "Display" }
  | { type: "Window"; hwnd: number; title: string; rect: number[] }
  | { type: "Area"; x: number; y: number; width: number; height: number };

export interface WindowInfo {
  hwnd: number;
  title: string;
  rect: number[];
}

export type TransitionType =
  | "SpringIn"
  | "SpringOut"
  | "Smooth";

export interface SpringHint {
  zoom_half_life: number;
  pan_half_life: number;
}

export interface ZoomKeyframe {
  time_ms: number;
  target_x: number;
  target_y: number;
  zoom_level: number;
  transition: TransitionType;
  spring_hint?: SpringHint;
}

export interface SceneRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface SceneInfo {
  id: number;
  start_ms: number;
  end_ms: number;
  bbox: SceneRect;
  center_x: number;
  center_y: number;
  zoom_level: number;
  window_rect: SceneRect | null;
  window_title: string | null;
  event_count: number;
}

export interface TimelineEvent {
  time_ms: number;
  event_type: "click" | "key" | "scroll" | "focus" | "window_focus";
  x: number | null;
  y: number | null;
  label: string | null;
}

export type SceneEditOp =
  | { type: "Merge"; scene_id: number }
  | { type: "Split"; scene_id: number; split_time_ms: number };

export interface AppSettings {
  recording: {
    hotkey: string;
    fps: number;
    capture_system_audio: boolean;
    capture_microphone: boolean;
    max_duration_seconds: number;
    recording_mode: RecordingMode;
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
    zoom_intensity: ZoomIntensity;
    animation_speed: AnimationSpeed;
    smart_zoom_enabled: boolean;
    motion_blur_enabled: boolean;
    frame_diff_enabled: boolean;
    idle_zoom_out_ms: number;
    idle_overview_ms: number;
    min_workarea_dwell_ms: number;
    min_window_dwell_ms: number;
    cluster_lifetime_ms: number;
    cluster_stability_ms: number;
  };
  output: {
    default_format: ExportFormat;
    default_quality: QualityPreset;
    save_directory: string;
  };
}
