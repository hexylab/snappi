use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager,
};

pub fn setup_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let start_recording = MenuItem::with_id(app, "start_recording", "Start Recording (Ctrl+Shift+R)", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&start_recording, &settings, &quit])?;

    // Embed icon at compile time and decode to RGBA
    let icon_png = include_bytes!("../icons/icon.png");
    let img = image::load_from_memory(icon_png)?.to_rgba8();
    let (width, height) = img.dimensions();
    let icon = Image::new_owned(img.into_raw(), width, height);

    TrayIconBuilder::new()
        .icon(icon)
        .menu(&menu)
        .tooltip("Snappi - Screen Recorder")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "start_recording" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.emit("tray-start-recording", ());
                }
            }
            "settings" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.emit("tray-open-settings", ());
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}
