use crate::config::RecordingEvent;
use anyhow::Result;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

pub fn collect_events(
    is_running: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    output_dir: &Path,
) -> Result<()> {
    let events_path = output_dir.join("events.jsonl");
    let mut file = std::fs::File::create(&events_path)?;
    let start_time = Instant::now();
    let running = is_running.clone();
    let paused = is_paused.clone();

    log::info!("Event collection thread started");

    // Track last mouse position time for sampling
    let last_mouse_time = Arc::new(std::sync::Mutex::new(Instant::now()));
    let events_buffer = Arc::new(std::sync::Mutex::new(Vec::<RecordingEvent>::new()));

    let events_clone = events_buffer.clone();
    let last_mouse_clone = last_mouse_time.clone();
    let start = start_time;

    // Set up rdev listener in a separate thread
    let listener_running = running.clone();
    let listener_paused = paused.clone();
    let listener_handle = std::thread::spawn(move || {
        let _ = rdev::listen(move |event| {
            if !listener_running.load(Ordering::SeqCst) {
                return;
            }
            if listener_paused.load(Ordering::SeqCst) {
                return;
            }

            let t = start.elapsed().as_millis() as u64;

            let recording_event = match event.event_type {
                rdev::EventType::MouseMove { x, y } => {
                    let mut last = last_mouse_clone.lock().unwrap();
                    if last.elapsed().as_millis() >= 10 {
                        *last = Instant::now();
                        Some(RecordingEvent::MouseMove { t, x, y })
                    } else {
                        None
                    }
                }
                rdev::EventType::ButtonPress(btn) => {
                    if let Some(pos) = get_cursor_position() {
                        let btn_name = match btn {
                            rdev::Button::Left => "left",
                            rdev::Button::Right => "right",
                            rdev::Button::Middle => "middle",
                            _ => "unknown",
                        };
                        Some(RecordingEvent::Click {
                            t,
                            btn: btn_name.to_string(),
                            x: pos.0,
                            y: pos.1,
                        })
                    } else {
                        None
                    }
                }
                rdev::EventType::KeyPress(key) => {
                    let key_name = format!("{:?}", key);
                    Some(RecordingEvent::Key {
                        t,
                        key: key_name,
                        modifiers: vec![],
                    })
                }
                rdev::EventType::Wheel { delta_x, delta_y } => {
                    if let Some(pos) = get_cursor_position() {
                        Some(RecordingEvent::Scroll {
                            t,
                            x: pos.0,
                            y: pos.1,
                            dx: delta_x as f64,
                            dy: delta_y as f64,
                        })
                    } else {
                        None
                    }
                }
                _ => None,
            };

            if let Some(evt) = recording_event {
                if let Ok(mut buffer) = events_clone.lock() {
                    buffer.push(evt);
                }
            }
        });
    });

    // Flush events to file periodically
    while is_running.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(100));

        let mut buffer = events_buffer.lock().unwrap();
        for event in buffer.drain(..) {
            if let Ok(json) = serde_json::to_string(&event) {
                let _ = writeln!(file, "{}", json);
            }
        }
        let _ = file.flush();
    }

    // Final flush
    if let Ok(mut buffer) = events_buffer.lock() {
        for event in buffer.drain(..) {
            if let Ok(json) = serde_json::to_string(&event) {
                let _ = writeln!(file, "{}", json);
            }
        }
    }

    // The rdev listener thread will be cleaned up when the process exits
    drop(listener_handle);

    log::info!("Event collection stopped");
    Ok(())
}

fn get_cursor_position() -> Option<(f64, f64)> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
        use windows::Win32::Foundation::POINT;
        let mut point = POINT::default();
        unsafe {
            if GetCursorPos(&mut point).is_ok() {
                return Some((point.x as f64, point.y as f64));
            }
        }
    }
    None
}
