import { invoke } from "@tauri-apps/api/core";
import type {
  RecordingInfo,
  RecordingState,
  ExportFormat,
  QualityPreset,
  ExportProgress,
  AppSettings,
  WindowInfo,
  TimelineEvent,
} from "./types";
import type { ZoomSegment } from "./generated/ZoomSegment";
import type { CameraTrack } from "./generated/CameraTrack";
import type { PreviewData } from "./generated/PreviewData";
import type { Project } from "./generated/Project";

export async function startRecording(): Promise<void> {
  return invoke("start_recording");
}

export async function stopRecording(): Promise<string> {
  return invoke("stop_recording");
}

export async function pauseRecording(): Promise<void> {
  return invoke("pause_recording");
}

export async function resumeRecording(): Promise<void> {
  return invoke("resume_recording");
}

export async function getRecordingState(): Promise<RecordingState> {
  return invoke("get_recording_state");
}

export async function getRecordingsList(): Promise<RecordingInfo[]> {
  return invoke("get_recordings_list");
}

/** 録画を書き出す。ズーム区間とトリムは project.json（saveProject で保存した内容）を使う */
export async function exportRecording(
  recordingId: string,
  format: ExportFormat,
  quality: QualityPreset,
): Promise<void> {
  return invoke("export_recording", { recordingId, format, quality });
}

export async function getExportProgress(): Promise<ExportProgress | null> {
  return invoke("get_export_progress");
}

export async function getSettings(): Promise<AppSettings> {
  return invoke("get_settings");
}

export async function saveSettings(newSettings: AppSettings): Promise<void> {
  return invoke("save_settings", { newSettings });
}

export async function deleteRecording(recordingId: string): Promise<void> {
  return invoke("delete_recording", { recordingId });
}

export async function getRecordingThumbnail(
  recordingId: string
): Promise<string> {
  return invoke("get_recording_thumbnail", { recordingId });
}

export async function listWindows(): Promise<WindowInfo[]> {
  return invoke("list_windows");
}

/** ズーム区間からプレビュー用のカメラトラック（30fps）を求める */
export async function getCameraTrack(
  recordingId: string,
  segments: ZoomSegment[],
): Promise<CameraTrack> {
  return invoke("get_camera_track", { recordingId, segments });
}

export async function getRecordingEvents(
  recordingId: string
): Promise<TimelineEvent[]> {
  return invoke("get_recording_events", { recordingId });
}

/** 録画の編集内容（project.json）。無ければ自動生成した区間で作られる */
export async function loadProject(recordingId: string): Promise<Project> {
  return invoke("load_project", { recordingId });
}

export async function saveProject(recordingId: string, project: Project): Promise<void> {
  return invoke("save_project", { recordingId, project });
}

/** プレビューの合成に使うデータ（quality で書き出すときと同じ見た目） */
export async function getPreviewData(
  recordingId: string,
  quality: QualityPreset,
): Promise<PreviewData> {
  return invoke("get_preview_data", { recordingId, quality });
}
