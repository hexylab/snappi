import { invoke } from "@tauri-apps/api/core";
import type {
  RecordingInfo,
  RecordingState,
  ExportFormat,
  QualityPreset,
  ExportProgress,
  AppSettings,
  WindowInfo,
  ZoomKeyframe,
  SceneInfo,
  TimelineEvent,
  SceneEditOp,
} from "./types";

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

export async function exportRecording(
  recordingId: string,
  format: ExportFormat,
  quality: QualityPreset
): Promise<void> {
  return invoke("export_recording", {
    recordingId,
    format,
    quality,
  });
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

export async function getZoomKeyframes(
  recordingId: string
): Promise<ZoomKeyframe[]> {
  return invoke("get_zoom_keyframes", { recordingId });
}

export async function getRecordingScenes(
  recordingId: string
): Promise<SceneInfo[]> {
  return invoke("get_recording_scenes", { recordingId });
}

export async function getRecordingEvents(
  recordingId: string
): Promise<TimelineEvent[]> {
  return invoke("get_recording_events", { recordingId });
}

export async function exportWithKeyframes(
  recordingId: string,
  keyframes: ZoomKeyframe[],
  format: ExportFormat,
  quality: QualityPreset
): Promise<void> {
  return invoke("export_with_keyframes", {
    recordingId,
    keyframes,
    format,
    quality,
  });
}

export async function applySceneEdits(
  recordingId: string,
  edits: SceneEditOp[]
): Promise<{ scenes: SceneInfo[]; keyframes: ZoomKeyframe[] }> {
  return invoke("apply_scene_edits", { recordingId, edits });
}
