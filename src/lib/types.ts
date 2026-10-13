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

export type { ZoomStrength } from "./generated/ZoomStrength";
import type { ZoomStrength } from "./generated/ZoomStrength";

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

export interface TimelineEvent {
  time_ms: number;
  event_type: "click" | "key" | "scroll" | "focus" | "window_focus";
  x: number | null;
  y: number | null;
  label: string | null;
}

export interface AppSettings {
  recording: {
    hotkey: string;
    fps: number;
    capture_system_audio: boolean;
    capture_microphone: boolean;
    max_duration_seconds: number;
    recording_mode: RecordingMode;
    /** キー入力ラベルを events.jsonl に平文で記録するか（既定: false） */
    record_key_labels?: boolean;
  };
  style: {
    background: BackgroundConfig;
    border_radius: number;
    shadow_enabled: boolean;
    shadow_blur: number;
    shadow_offset_y: number;
  };
  effects: {
    /** 自動ズームの強さ（Off で自動ズームしない） */
    zoom_strength: ZoomStrength;
    click_ring_enabled: boolean;
    key_badge_enabled: boolean;
    cursor_smoothing: boolean;
    motion_blur_enabled: boolean;
  };
  output: {
    default_format: ExportFormat;
    default_quality: QualityPreset;
    save_directory: string;
  };
}
