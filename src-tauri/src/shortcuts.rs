use tauri::AppHandle;
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::ShortcutState;

pub fn setup_shortcuts(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(desktop)]
    {
        use tauri_plugin_global_shortcut::GlobalShortcutExt;

        app.global_shortcut().on_shortcut("CmdOrCtrl+Shift+R", move |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.emit("shortcut-toggle-recording", ());
                }
            }
        })?;
    }

    Ok(())
}
