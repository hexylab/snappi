use crate::config::{
    AppSettings, ExportFormat, ExportProgress, QualityPreset, RecordingInfo, RecordingState,
    WindowInfo,
};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};

pub struct AppState {
    pub recording_state: Mutex<RecordingState>,
    pub settings: Mutex<AppSettings>,
    pub export_progress: Arc<Mutex<Option<ExportProgress>>>,
    pub current_session: Mutex<Option<crate::recording::session::RecordingSession>>,
}

/// Settings file path: %APPDATA%\Snappi\settings.json
fn settings_file_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Snappi")
        .join("settings.json")
}

/// Load settings from disk, falling back to defaults if file missing or invalid.
fn load_settings_from_disk() -> AppSettings {
    let path = settings_file_path();
    if path.exists() {
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                match serde_json::from_str::<AppSettings>(&content) {
                    Ok(settings) => {
                        log::info!("Settings loaded from {}", path.display());
                        return settings;
                    }
                    Err(e) => {
                        log::warn!("Failed to parse settings file, using defaults: {}", e);
                    }
                }
            }
            Err(e) => {
                log::warn!("Failed to read settings file, using defaults: {}", e);
            }
        }
    }
    AppSettings::default()
}

/// Save settings to disk.
fn save_settings_to_disk(settings: &AppSettings) -> Result<(), String> {
    let path = settings_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create settings dir: {}", e))?;
    }
    let json = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;
    std::fs::write(&path, json)
        .map_err(|e| format!("Failed to write settings file: {}", e))?;
    log::info!("Settings saved to {}", path.display());
    Ok(())
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            recording_state: Mutex::new(RecordingState::Idle),
            settings: Mutex::new(load_settings_from_disk()),
            export_progress: Arc::new(Mutex::new(None)),
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
    // Check state and take session while holding locks briefly
    let session = {
        let mut rec_state = state.recording_state.lock().map_err(|e| e.to_string())?;
        if *rec_state != RecordingState::Recording && *rec_state != RecordingState::Paused {
            return Err("Not recording".to_string());
        }
        *rec_state = RecordingState::Processing;

        let mut current = state.current_session.lock().map_err(|e| e.to_string())?;
        current.take()
    }; // Both locks released here

    if let Some(session) = session {
        let recording_id = session.id().to_string();

        // stop() sleeps 500ms and writes meta.json - do this without holding locks
        if let Err(e) = session.stop() {
            log::error!("Error during session stop: {}", e);
        }

        let mut rec_state = state.recording_state.lock().map_err(|e| e.to_string())?;
        *rec_state = RecordingState::Idle;
        Ok(recording_id)
    } else {
        let mut rec_state = state.recording_state.lock().map_err(|e| e.to_string())?;
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
pub async fn export_recording(
    recording_id: String,
    format: ExportFormat,
    quality: QualityPreset,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    // Extract all data from State before any async work (State is not Send)
    let settings = {
        let prog = state.export_progress.lock().map_err(|e| e.to_string())?;
        if prog.is_some() {
            return Err("Export already in progress".to_string());
        }
        state.settings.lock().map_err(|e| e.to_string())?.clone()
    };
    {
        let mut prog = state.export_progress.lock().map_err(|e| e.to_string())?;
        *prog = Some(ExportProgress {
            stage: "starting".to_string(),
            progress: 0.0,
            output_path: None,
        });
    }
    let export_progress = state.export_progress.clone();
    let handle = app_handle;

    // Fire-and-forget: tokio::spawn returns immediately, heavy work runs in background
    tokio::spawn(async move {
        let ep = Arc::clone(&export_progress);
        let h = handle.clone();

        let result = tokio::task::spawn_blocking(move || {
            let progress_cb: crate::export::encoder::ProgressFn = {
                let handle = h.clone();
                let export_progress = Arc::clone(&ep);
                Box::new(move |stage: &str, p: f64| {
                    let prog = ExportProgress {
                        stage: stage.to_string(),
                        progress: p,
                        output_path: None,
                    };
                    if let Ok(mut lock) = export_progress.lock() {
                        *lock = Some(prog.clone());
                    }
                    let _ = handle.emit("export-progress", prog);
                })
            };

            crate::export::encoder::export(
                &recording_id,
                &format,
                &quality,
                &settings,
                Some(&progress_cb),
            )
        }).await;

        // Clear progress and emit result
        if let Ok(mut lock) = export_progress.lock() {
            *lock = None;
        }
        match result {
            Ok(Ok(path)) => {
                let _ = handle.emit("export-complete", serde_json::json!({ "output_path": path }));
            }
            Ok(Err(e)) => {
                let _ = handle.emit("export-error", serde_json::json!({ "message": e.to_string() }));
            }
            Err(e) => {
                let _ = handle.emit("export-error", serde_json::json!({ "message": e.to_string() }));
            }
        }
    });

    log::info!("export_recording command returned immediately (background task spawned)");
    Ok(())
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
    save_settings_to_disk(&new_settings)?;
    let mut settings = state.settings.lock().map_err(|e| e.to_string())?;
    *settings = new_settings;
    Ok(())
}

#[tauri::command]
pub fn delete_recording(recording_id: String) -> Result<(), String> {
    crate::recording::session::delete_recording(&recording_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_recording_thumbnail(recording_id: String) -> Result<String, String> {
    crate::export::encoder::generate_thumbnail(&recording_id).map_err(|e| e.to_string())
}

/// Get zoom keyframes for a recording (for Timeline UI).
#[tauri::command]
pub fn get_zoom_keyframes(
    recording_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<crate::engine::zoom_planner::ZoomKeyframe>, String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?;
    crate::export::encoder::generate_keyframes_for_recording(&recording_id, &settings)
        .map_err(|e| e.to_string())
}

/// Get scene debug info for a recording (for Timeline UI visualization).
#[tauri::command]
pub fn get_recording_scenes(
    recording_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<crate::engine::scene_splitter::Scene>, String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?;
    crate::export::encoder::get_recording_scenes(&recording_id, &settings)
        .map_err(|e| e.to_string())
}

/// Get recording events for Timeline UI visualization.
#[tauri::command]
pub fn get_recording_events(
    recording_id: String,
) -> Result<Vec<crate::config::TimelineEvent>, String> {
    crate::export::encoder::get_recording_events(&recording_id)
        .map_err(|e| e.to_string())
}

/// Apply scene edits (merge/split) and get updated scenes + keyframes.
#[tauri::command]
pub fn apply_scene_edits(
    recording_id: String,
    edits: Vec<crate::engine::scene_splitter::SceneEditOp>,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?;
    let (scenes, keyframes) =
        crate::export::encoder::apply_scene_edits_for_recording(&recording_id, edits, &settings)
            .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "scenes": scenes,
        "keyframes": keyframes,
    }))
}

/// Export with custom keyframes from timeline UI.
#[tauri::command]
pub async fn export_with_keyframes(
    recording_id: String,
    keyframes: Vec<crate::engine::zoom_planner::ZoomKeyframe>,
    format: ExportFormat,
    quality: QualityPreset,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let settings = {
        let prog = state.export_progress.lock().map_err(|e| e.to_string())?;
        if prog.is_some() {
            return Err("Export already in progress".to_string());
        }
        state.settings.lock().map_err(|e| e.to_string())?.clone()
    };
    {
        let mut prog = state.export_progress.lock().map_err(|e| e.to_string())?;
        *prog = Some(ExportProgress {
            stage: "starting".to_string(),
            progress: 0.0,
            output_path: None,
        });
    }
    let export_progress = state.export_progress.clone();
    let handle = app_handle;

    tokio::spawn(async move {
        let ep = Arc::clone(&export_progress);
        let h = handle.clone();

        let result = tokio::task::spawn_blocking(move || {
            let progress_cb: crate::export::encoder::ProgressFn = {
                let handle = h.clone();
                let export_progress = Arc::clone(&ep);
                Box::new(move |stage: &str, p: f64| {
                    let prog = ExportProgress {
                        stage: stage.to_string(),
                        progress: p,
                        output_path: None,
                    };
                    if let Ok(mut lock) = export_progress.lock() {
                        *lock = Some(prog.clone());
                    }
                    let _ = handle.emit("export-progress", prog);
                })
            };

            crate::export::encoder::export_with_custom_keyframes(
                &recording_id,
                keyframes,
                &format,
                &quality,
                &settings,
                Some(&progress_cb),
            )
        }).await;

        if let Ok(mut lock) = export_progress.lock() {
            *lock = None;
        }
        match result {
            Ok(Ok(path)) => {
                let _ = handle.emit("export-complete", serde_json::json!({ "output_path": path }));
            }
            Ok(Err(e)) => {
                let _ = handle.emit("export-error", serde_json::json!({ "message": e.to_string() }));
            }
            Err(e) => {
                let _ = handle.emit("export-error", serde_json::json!({ "message": e.to_string() }));
            }
        }
    });

    log::info!("export_with_keyframes command returned immediately (background task spawned)");
    Ok(())
}

/// List visible windows for window recording mode selection.
#[tauri::command]
pub fn list_windows() -> Result<Vec<WindowInfo>, String> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::*;
        use windows::Win32::Foundation::*;

        let mut windows: Vec<WindowInfo> = Vec::new();

        unsafe {
            let _ = EnumWindows(
                Some(enum_window_callback),
                LPARAM(&mut windows as *mut Vec<WindowInfo> as isize),
            );
        }

        Ok(windows)
    }

    #[cfg(not(windows))]
    {
        Ok(Vec::new())
    }
}

#[cfg(windows)]
unsafe extern "system" fn enum_window_callback(
    hwnd: windows::Win32::Foundation::HWND,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::BOOL {
    use windows::Win32::UI::WindowsAndMessaging::*;
    use windows::Win32::Foundation::*;

    let windows = &mut *(lparam.0 as *mut Vec<WindowInfo>);

    // Only include visible windows with titles
    if !IsWindowVisible(hwnd).as_bool() {
        return TRUE;
    }

    let mut title = [0u16; 512];
    let len = GetWindowTextW(hwnd, &mut title);
    if len == 0 {
        return TRUE;
    }

    let title_str = String::from_utf16_lossy(&title[..len as usize]);
    if title_str.is_empty() {
        return TRUE;
    }

    // Skip certain system windows
    let skip_titles = ["Program Manager", "Windows Input Experience", "Settings"];
    if skip_titles.contains(&title_str.as_str()) {
        return TRUE;
    }

    let mut rect = RECT::default();
    if GetWindowRect(hwnd, &mut rect).is_ok() {
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        // Skip tiny windows (likely hidden or system)
        if width > 100 && height > 100 {
            windows.push(WindowInfo {
                hwnd: hwnd.0 as isize,
                title: title_str,
                rect: [rect.left as f64, rect.top as f64, rect.right as f64, rect.bottom as f64],
            });
        }
    }

    TRUE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_serialization_roundtrip() {
        let settings = AppSettings::default();
        let json = serde_json::to_string_pretty(&settings).expect("serialize");
        let deserialized: AppSettings = serde_json::from_str(&json).expect("deserialize");
        // Verify key fields survive roundtrip
        assert_eq!(deserialized.recording.fps, settings.recording.fps);
        assert_eq!(deserialized.effects.auto_zoom_enabled, settings.effects.auto_zoom_enabled);
        assert!((deserialized.effects.max_zoom - settings.effects.max_zoom).abs() < 0.01);
        assert!((deserialized.effects.text_input_zoom_level - settings.effects.text_input_zoom_level).abs() < 0.01);
    }

    #[test]
    fn test_settings_file_write_and_read() {
        let temp_dir = tempfile::TempDir::new().expect("create temp dir");
        let path = temp_dir.path().join("settings.json");

        let mut settings = AppSettings::default();
        settings.effects.max_zoom = 4.0;
        settings.recording.fps = 24;

        // Write
        let json = serde_json::to_string_pretty(&settings).expect("serialize");
        std::fs::write(&path, &json).expect("write");

        // Read back
        let content = std::fs::read_to_string(&path).expect("read");
        let loaded: AppSettings = serde_json::from_str(&content).expect("deserialize");

        assert_eq!(loaded.recording.fps, 24);
        assert!((loaded.effects.max_zoom - 4.0).abs() < 0.01);
    }

    #[test]
    fn test_settings_path_exists() {
        let path = settings_file_path();
        // Should point to a reasonable location
        let path_str = path.to_string_lossy();
        assert!(path_str.contains("Snappi"), "Settings path should contain 'Snappi': {}", path_str);
        assert!(path_str.ends_with("settings.json"), "Settings path should end with 'settings.json': {}", path_str);
    }
}
