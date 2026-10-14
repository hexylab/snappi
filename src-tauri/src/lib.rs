pub mod camera;
pub mod changes;
pub mod commands;
pub mod config;
pub mod engine;
pub mod eval;
pub mod export;
pub mod project;
pub mod recording;
pub mod recording_data;
pub mod shortcuts;
pub mod tray;
pub mod window_bar;


#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // log::warn! などを標準エラーに出す（RUST_LOG で変えられる）
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("snappi=info,snappi_lib=info,warn"))
        .try_init();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::AppState::default())
        .manage(window_bar::WindowBarState::default())
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
            commands::get_camera_track,
            commands::get_recording_events,
            commands::load_project,
            commands::save_project,
            commands::get_preview_data,
            commands::auto_project,
            commands::show_in_folder,
            commands::open_file,
            commands::set_recording_bar,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
