use crate::config::{
    AppSettings, ExportFormat, ExportProgress, QualityPreset, RecordingInfo, RecordingState,
};
use std::sync::Mutex;
use tauri::State;

pub struct AppState {
    pub recording_state: Mutex<RecordingState>,
    pub settings: Mutex<AppSettings>,
    pub export_progress: Mutex<Option<ExportProgress>>,
    pub current_session: Mutex<Option<crate::recording::session::RecordingSession>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            recording_state: Mutex::new(RecordingState::Idle),
            settings: Mutex::new(AppSettings::default()),
            export_progress: Mutex::new(None),
            current_session: Mutex::new(None),
        }
    }
}

#[tauri::command]
pub fn start_recording(state: State<'_, AppState>) -> Result<(), String> {
    let mut rec_state = state.recording_state.lock().map_err(|e| e.to_string())?;
    if *rec_state != RecordingState::Idle {
        return Err("Already recording".to_string());
    }

    let settings = state.settings.lock().map_err(|e| e.to_string())?;
    let session = crate::recording::session::RecordingSession::new(&settings)
        .map_err(|e| e.to_string())?;

    let mut current = state.current_session.lock().map_err(|e| e.to_string())?;
    *current = Some(session);

    if let Some(ref session) = *current {
        session.start().map_err(|e| e.to_string())?;
    }

    *rec_state = RecordingState::Recording;
    Ok(())
}

#[tauri::command]
pub fn stop_recording(state: State<'_, AppState>) -> Result<String, String> {
    let mut rec_state = state.recording_state.lock().map_err(|e| e.to_string())?;
    if *rec_state != RecordingState::Recording && *rec_state != RecordingState::Paused {
        return Err("Not recording".to_string());
    }

    *rec_state = RecordingState::Processing;

    let mut current = state.current_session.lock().map_err(|e| e.to_string())?;
    if let Some(session) = current.take() {
        let recording_id = session.id().to_string();
        session.stop().map_err(|e| e.to_string())?;

        *rec_state = RecordingState::Idle;
        Ok(recording_id)
    } else {
        *rec_state = RecordingState::Idle;
        Err("No active session".to_string())
    }
}

#[tauri::command]
pub fn pause_recording(state: State<'_, AppState>) -> Result<(), String> {
    let mut rec_state = state.recording_state.lock().map_err(|e| e.to_string())?;
    if *rec_state != RecordingState::Recording {
        return Err("Not recording".to_string());
    }
    let current = state.current_session.lock().map_err(|e| e.to_string())?;
    if let Some(ref session) = *current {
        session.pause().map_err(|e| e.to_string())?;
    }
    *rec_state = RecordingState::Paused;
    Ok(())
}

#[tauri::command]
pub fn resume_recording(state: State<'_, AppState>) -> Result<(), String> {
    let mut rec_state = state.recording_state.lock().map_err(|e| e.to_string())?;
    if *rec_state != RecordingState::Paused {
        return Err("Not paused".to_string());
    }
    let current = state.current_session.lock().map_err(|e| e.to_string())?;
    if let Some(ref session) = *current {
        session.resume().map_err(|e| e.to_string())?;
    }
    *rec_state = RecordingState::Recording;
    Ok(())
}

#[tauri::command]
pub fn get_recording_state(state: State<'_, AppState>) -> Result<RecordingState, String> {
    let rec_state = state.recording_state.lock().map_err(|e| e.to_string())?;
    Ok(rec_state.clone())
}

#[tauri::command]
pub fn get_recordings_list() -> Result<Vec<RecordingInfo>, String> {
    crate::recording::session::list_recordings().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn export_recording(
    recording_id: String,
    format: ExportFormat,
    quality: QualityPreset,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?;
    crate::export::encoder::export(
        &recording_id,
        &format,
        &quality,
        &settings,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_export_progress(state: State<'_, AppState>) -> Result<Option<ExportProgress>, String> {
    let progress = state.export_progress.lock().map_err(|e| e.to_string())?;
    Ok(progress.clone())
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?;
    Ok(settings.clone())
}

#[tauri::command]
pub fn save_settings(
    new_settings: AppSettings,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut settings = state.settings.lock().map_err(|e| e.to_string())?;
    *settings = new_settings;
    Ok(())
}

#[tauri::command]
pub fn delete_recording(recording_id: String) -> Result<(), String> {
    crate::recording::session::delete_recording(&recording_id).map_err(|e| e.to_string())
}
