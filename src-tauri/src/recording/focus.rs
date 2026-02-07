use crate::config::RecordingEvent;
use anyhow::Result;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Track foreground window changes using Windows API polling.
/// Writes WindowFocus events to window_events.jsonl when the
/// foreground window changes.
pub fn track_focus(
    is_running: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    output_dir: &Path,
) -> Result<()> {
    log::info!("Window focus tracking thread started");

    #[cfg(windows)]
    {
        use std::io::Write;
        use windows::Win32::Foundation::RECT;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowRect, GetWindowTextW,
        };

        let events_path = output_dir.join("window_events.jsonl");
        let mut file = std::fs::File::create(&events_path)?;
        let start_time = std::time::Instant::now();
        let mut last_hwnd: isize = 0;

        while is_running.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(100));

            if is_paused.load(Ordering::SeqCst) {
                continue;
            }

            unsafe {
                let hwnd = GetForegroundWindow();
                let hwnd_raw = hwnd.0 as isize;

                if hwnd_raw == last_hwnd || hwnd_raw == 0 {
                    continue;
                }

                // Get window title
                let mut title_buf = [0u16; 256];
                let len = GetWindowTextW(hwnd, &mut title_buf);
                let title = String::from_utf16_lossy(&title_buf[..len as usize]);

                // Skip empty title windows (desktop, system)
                if title.is_empty() {
                    continue;
                }

                last_hwnd = hwnd_raw;

                // Get window rect
                let mut rect = RECT::default();
                if GetWindowRect(hwnd, &mut rect).is_ok() {
                    let t = start_time.elapsed().as_millis() as u64;
                    let event = RecordingEvent::WindowFocus {
                        t,
                        title,
                        rect: [
                            rect.left as f64,
                            rect.top as f64,
                            rect.right as f64,
                            rect.bottom as f64,
                        ],
                    };

                    if let Ok(json) = serde_json::to_string(&event) {
                        let _ = writeln!(file, "{}", json);
                        let _ = file.flush();
                    }
                }
            }
        }
    }

    #[cfg(not(windows))]
    {
        let _ = (is_running, is_paused, output_dir);
    }

    log::info!("Window focus tracking stopped");
    Ok(())
}
