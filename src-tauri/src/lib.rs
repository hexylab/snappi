pub mod commands;
pub mod config;
pub mod engine;
pub mod export;
pub mod recording;
pub mod shortcuts;
pub mod tray;


#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(commands::AppState::default())
        .setup(|app| {
            let handle = app.handle().clone();
            tray::setup_tray(&handle)?;
            shortcuts::setup_shortcuts(&handle)?;

            // Create recordings directory
            if let Some(video_dir) = dirs::video_dir() {
                let recordings_dir = video_dir.join("Snappi");
                std::fs::create_dir_all(&recordings_dir).ok();
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::start_recording,
            commands::stop_recording,
            commands::pause_recording,
            commands::resume_recording,
            commands::get_recording_state,
            commands::get_recordings_list,
            commands::export_recording,
            commands::get_export_progress,
            commands::get_settings,
            commands::save_settings,
            commands::delete_recording,
            commands::get_recording_thumbnail,
            commands::list_windows,
            commands::get_zoom_keyframes,
            commands::get_recording_scenes,
            commands::export_with_keyframes,
            commands::get_recording_events,
            commands::apply_scene_edits,
            commands::compute_activity_center,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
