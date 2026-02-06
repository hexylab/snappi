import { invoke } from "@tauri-apps/api/core";
import type {
  RecordingInfo,
  RecordingState,
  ExportFormat,
  QualityPreset,
  ExportProgress,
  AppSettings,
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
): Promise<string> {
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
