use super::clock::RecordingClock;
use crate::config::RecordingEvent;
use anyhow::Result;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// 前面ウィンドウの変化を記録するファイル名
pub const WINDOW_EVENTS_FILE: &str = "window_events.jsonl";

/// Track foreground window changes using Windows API polling.
/// 前面ウィンドウが変わったとき、または前面ウィンドウの矩形が変わったとき（開くアニメーションの
/// 終わり・移動・リサイズ）に WindowFocus イベントを window_events.jsonl に書く。
pub fn track_focus(
    is_running: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    output_dir: &Path,
    clock: Arc<RecordingClock>,
) -> Result<()> {
    log::info!("Window focus tracking thread started");

    #[cfg(windows)]
    {
        use std::io::Write;
        use windows::Win32::Foundation::RECT;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowRect, GetWindowTextW,
        };

        let events_path = output_dir.join(WINDOW_EVENTS_FILE);
        let mut file = std::fs::File::create(&events_path)?;
        let mut last_hwnd: isize = 0;
        let mut last_rect = [0i32; 4];

        while is_running.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(100));

            if is_paused.load(Ordering::SeqCst) {
                continue;
            }

            unsafe {
                let hwnd = GetForegroundWindow();
                let hwnd_raw = hwnd.0 as isize;

                if hwnd_raw == 0 {
                    continue;
                }

                let mut rect = RECT::default();
                if GetWindowRect(hwnd, &mut rect).is_err() {
                    continue;
                }
                let rect_arr = [rect.left, rect.top, rect.right, rect.bottom];
                if hwnd_raw == last_hwnd && rect_arr == last_rect {
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
                last_rect = rect_arr;

                {
                    let t = clock.now_ms();
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
        let _ = (is_running, is_paused, output_dir, clock);
    }

    log::info!("Window focus tracking stopped");
    Ok(())
}
