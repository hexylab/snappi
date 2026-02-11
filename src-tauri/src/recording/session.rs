use crate::config::{AppSettings, RecordingInfo, RecordingMeta, RecordingMode};
use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub struct RecordingSession {
    id: String,
    recording_dir: std::path::PathBuf,
    is_running: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    start_time: Arc<Mutex<Option<std::time::Instant>>>,
    fps: u32,
    recording_mode: RecordingMode,
}

impl RecordingSession {
    pub fn new(settings: &AppSettings) -> Result<Self> {
        let id = uuid::Uuid::new_v4().to_string();
        let base_dir = dirs::video_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("Snappi")
            .join("recordings")
            .join(&id);
        std::fs::create_dir_all(&base_dir)?;

        Ok(Self {
            id,
            recording_dir: base_dir,
            is_running: Arc::new(AtomicBool::new(false)),
            is_paused: Arc::new(AtomicBool::new(false)),
            start_time: Arc::new(Mutex::new(None)),
            fps: settings.recording.fps,
            recording_mode: settings.recording.recording_mode.clone(),
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn start(&self) -> Result<()> {
        self.is_running.store(true, Ordering::SeqCst);
        *self.start_time.lock().unwrap() = Some(std::time::Instant::now());
        log::info!("Recording started: {}", self.id);

        // Start capture thread (mode-dependent)
        let running = self.is_running.clone();
        let paused = self.is_paused.clone();
        let dir = self.recording_dir.clone();
        let fps = self.fps;
        let mode = self.recording_mode.clone();
        std::thread::spawn(move || {
            let result = match mode {
                RecordingMode::Window { hwnd, .. } => {
                    super::capture::capture_window(running, paused, &dir, fps, hwnd)
                }
                RecordingMode::Area { x, y, width, height } => {
                    super::capture::capture_area(running, paused, &dir, fps, x, y, width, height)
                }
                RecordingMode::Display => {
                    super::capture::capture_screen(running, paused, &dir, fps)
                }
            };
            if let Err(e) = result {
                log::error!("Capture error: {}", e);
            }
        });

        // Start input event collection thread
        let running = self.is_running.clone();
        let paused = self.is_paused.clone();
        let dir = self.recording_dir.clone();
        std::thread::spawn(move || {
            if let Err(e) = super::events::collect_events(running, paused, &dir) {
                log::error!("Event collection error: {}", e);
            }
        });

        // Start audio capture thread
        let running = self.is_running.clone();
        let paused = self.is_paused.clone();
        let dir = self.recording_dir.clone();
        std::thread::spawn(move || {
            if let Err(e) = super::audio::capture_audio(running, paused, &dir) {
                log::error!("Audio capture error: {}", e);
            }
        });

        // Start window focus tracking thread
        let running = self.is_running.clone();
        let paused = self.is_paused.clone();
        let dir = self.recording_dir.clone();
        std::thread::spawn(move || {
            if let Err(e) = super::focus::track_focus(running, paused, &dir) {
                log::error!("Window focus tracking error: {}", e);
            }
        });

        // Start UI Automation tracker thread (best-effort, failure doesn't block recording)
        let running = self.is_running.clone();
        let paused = self.is_paused.clone();
        let dir = self.recording_dir.clone();
        std::thread::spawn(move || {
            if let Err(e) = super::ui_tracker::track_ui_events(running, paused, &dir) {
                log::warn!("UI tracker error (non-fatal): {}", e);
            }
        });

        Ok(())
    }

    pub fn stop(&self) -> Result<()> {
        self.is_running.store(false, Ordering::SeqCst);
        log::info!("Recording stopped: {}", self.id);

        // Wait a moment for threads to finish
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Write metadata
        let duration_ms = self.start_time.lock().unwrap()
            .map(|t| t.elapsed().as_millis() as u64)
            .unwrap_or(0);

        // Read actual screen dimensions from capture thread output
        let (screen_width, screen_height) = self.read_dimensions();

        // Check if audio was actually captured (file exists and has data beyond WAV header)
        let audio_path = self.recording_dir.join("audio.wav");
        let has_audio = audio_path.exists()
            && std::fs::metadata(&audio_path).map(|m| m.len() > 44).unwrap_or(false);

        let (mode_str, win_title, win_rect) = match &self.recording_mode {
            RecordingMode::Display => (Some("display".to_string()), None, None),
            RecordingMode::Window { title, rect, .. } => {
                (Some("window".to_string()), Some(title.clone()), Some(*rect))
            }
            RecordingMode::Area { x, y, width, height } => {
                (Some("area".to_string()), None, Some([*x as f64, *y as f64, (*x + *width) as f64, (*y + *height) as f64]))
            }
        };

        let meta = RecordingMeta {
            version: 2,
            id: self.id.clone(),
            screen_width,
            screen_height,
            fps: self.fps,
            start_time: chrono::Local::now().to_rfc3339(),
            duration_ms,
            has_audio,
            monitor_scale: 1.0,
            recording_dir: self.recording_dir.to_string_lossy().to_string(),
            recording_mode: mode_str,
            window_title: win_title,
            window_initial_rect: win_rect,
        };

        let meta_path = self.recording_dir.join("meta.json");
        let meta_json = serde_json::to_string_pretty(&meta)?;
        std::fs::write(meta_path, meta_json)?;

        Ok(())
    }

    fn read_dimensions(&self) -> (u32, u32) {
        let dims_path = self.recording_dir.join("dimensions.txt");
        if let Ok(content) = std::fs::read_to_string(&dims_path) {
            let parts: Vec<&str> = content.trim().split('x').collect();
            if parts.len() == 2 {
                if let (Ok(w), Ok(h)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
                    return (w, h);
                }
            }
        }
        // Fallback to default
        (1920, 1080)
    }

    pub fn pause(&self) -> Result<()> {
        self.is_paused.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub fn resume(&self) -> Result<()> {
        self.is_paused.store(false, Ordering::SeqCst);
        Ok(())
    }
}

pub fn list_recordings() -> Result<Vec<RecordingInfo>> {
    let base_dir = dirs::video_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Snappi")
        .join("recordings");

    let mut recordings = Vec::new();

    if !base_dir.exists() {
        return Ok(recordings);
    }

    for entry in std::fs::read_dir(&base_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let meta_path = entry.path().join("meta.json");
            if meta_path.exists() {
                let content = std::fs::read_to_string(&meta_path)?;
                if let Ok(meta) = serde_json::from_str::<RecordingMeta>(&content) {
                    let fc_path = entry.path().join("frame_count.txt");
                    let frame_count = std::fs::read_to_string(&fc_path)
                        .ok()
                        .and_then(|s| s.trim().parse::<u32>().ok())
                        .unwrap_or(0);
                    let thumb_path = entry.path().join("thumbnail.png");
                    let thumbnail_path = if thumb_path.exists() {
                        Some(thumb_path.to_string_lossy().to_string())
                    } else {
                        // Auto-generate thumbnail if frames exist
                        crate::export::encoder::generate_thumbnail(&meta.id).ok()
                    };
                    recordings.push(RecordingInfo {
                        id: meta.id,
                        date: meta.start_time,
                        duration_ms: meta.duration_ms,
                        frame_count,
                        thumbnail_path,
                        recording_dir: meta.recording_dir,
                        screen_width: meta.screen_width,
                        screen_height: meta.screen_height,
                    });
                }
            }
        }
    }

    recordings.sort_by(|a, b| b.date.cmp(&a.date));
    Ok(recordings)
}

pub fn delete_recording(recording_id: &str) -> Result<()> {
    let recording_dir = dirs::video_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Snappi")
        .join("recordings")
        .join(recording_id);

    if recording_dir.exists() {
        std::fs::remove_dir_all(&recording_dir)?;
    }
    Ok(())
}
