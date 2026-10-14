use super::clock::RecordingClock;
use crate::config::{AppSettings, RecordingInfo, RecordingMeta, RecordingMode};
use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

pub struct RecordingSession {
    id: String,
    recording_dir: std::path::PathBuf,
    is_running: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    /// 全スレッドが共有する録画時計（一時停止中は進まない）
    clock: Arc<RecordingClock>,
    /// 録画開始時刻（RFC3339）。meta.start_time と書き出しファイル名に使う。
    started_at: String,
    fps: u32,
    recording_mode: RecordingMode,
    /// キー入力のラベルを平文で events.jsonl に記録するか（既定: false）。
    record_key_labels: bool,
    /// PC の音・マイクを録音するか
    capture_system_audio: bool,
    capture_microphone: bool,
    /// 各キャプチャスレッドのJoinHandle。stop()時にjoinして取りこぼしを防ぐ
    thread_handles: Mutex<Vec<JoinHandle<()>>>,
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
            clock: Arc::new(RecordingClock::new()),
            started_at: chrono::Local::now().to_rfc3339(),
            fps: settings.recording.fps,
            recording_mode: settings.recording.recording_mode.clone(),
            record_key_labels: settings.recording.record_key_labels,
            capture_system_audio: settings.recording.capture_system_audio,
            capture_microphone: settings.recording.capture_microphone,
            thread_handles: Mutex::new(Vec::new()),
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn start(&self) -> Result<()> {
        self.is_running.store(true, Ordering::SeqCst);
        log::info!("Recording started: {}", self.id);

        let mut handles: Vec<JoinHandle<()>> = Vec::new();

        // Start capture thread (mode-dependent)
        let running = self.is_running.clone();
        let paused = self.is_paused.clone();
        let dir = self.recording_dir.clone();
        let fps = self.fps;
        let mode = self.recording_mode.clone();
        let clock = self.clock.clone();
        handles.push(std::thread::spawn(move || {
            let result = match mode {
                RecordingMode::Window { hwnd, .. } => {
                    super::capture::capture_window(running, paused, &dir, fps, hwnd, clock)
                }
                RecordingMode::Area { x, y, width, height } => {
                    super::capture::capture_area(running, paused, &dir, fps, x, y, width, height, clock)
                }
                RecordingMode::Display => {
                    super::capture::capture_screen(running, paused, &dir, fps, clock)
                }
            };
            if let Err(e) = result {
                log::error!("Capture error: {}", e);
            }
        }));

        // Start input event collection thread
        let running = self.is_running.clone();
        let paused = self.is_paused.clone();
        let dir = self.recording_dir.clone();
        let rec_keys = self.record_key_labels;
        let clock = self.clock.clone();
        handles.push(std::thread::spawn(move || {
            if let Err(e) = super::events::collect_events(running, paused, &dir, rec_keys, clock) {
                log::error!("Event collection error: {}", e);
            }
        }));

        // 音声（PC の音・マイクを別々の WAV に録り、停止時に audio.wav にまとめる）
        let sources = [
            (self.capture_system_audio, super::audio::AudioSource::System),
            (self.capture_microphone, super::audio::AudioSource::Microphone),
        ];
        for (_, source) in sources.into_iter().filter(|(enabled, _)| *enabled) {
            let running = self.is_running.clone();
            let paused = self.is_paused.clone();
            let dir = self.recording_dir.clone();
            handles.push(std::thread::spawn(move || {
                if let Err(e) = super::audio::capture_audio(source, running, paused, &dir) {
                    log::error!("Audio capture error ({:?}): {}", source, e);
                }
            }));
        }

        // Start window focus tracking thread
        let running = self.is_running.clone();
        let paused = self.is_paused.clone();
        let dir = self.recording_dir.clone();
        let clock = self.clock.clone();
        handles.push(std::thread::spawn(move || {
            if let Err(e) = super::focus::track_focus(running, paused, &dir, clock) {
                log::error!("Window focus tracking error: {}", e);
            }
        }));

        // Start UI Automation tracker thread (best-effort, failure doesn't block recording)
        let running = self.is_running.clone();
        let paused = self.is_paused.clone();
        let dir = self.recording_dir.clone();
        let clock = self.clock.clone();
        handles.push(std::thread::spawn(move || {
            if let Err(e) = super::ui_tracker::track_ui_events(running, paused, &dir, clock) {
                log::warn!("UI tracker error (non-fatal): {}", e);
            }
        }));

        // Store handles for join on stop
        if let Ok(mut stored) = self.thread_handles.lock() {
            *stored = handles;
        }

        Ok(())
    }

    pub fn stop(&self) -> Result<()> {
        self.is_running.store(false, Ordering::SeqCst);
        // duration_ms は「実効録画時間」（一時停止時間を除いた時間）。フレーム・イベントの
        // タイムスタンプと同じ時計で、スレッドの join 待ちを含まないよう停止直後に測る。
        let duration_ms = self.clock.now_ms();
        log::info!("Recording stopped: {}", self.id);

        // Join all capture threads to ensure buffered frames/audio are flushed.
        // rdev (events.rs) のグローバルフックは listen() がブロッキングのため
        // is_running を落としてもすぐには抜けない可能性がある。そこで全体に
        // タイムアウトを設け、超過した場合は強制的に次へ進む（記録済みデータは保全される）。
        let handles = self
            .thread_handles
            .lock()
            .map(|mut g| std::mem::take(&mut *g))
            .unwrap_or_default();

        const MAX_JOIN_WAIT_PER_THREAD_MS: u64 = 2000;
        for (i, handle) in handles.into_iter().enumerate() {
            // JoinHandle::join() はタイムアウト機能を持たないため、
            // Thread::is_finished() でポーリングしつつタイムアウトを実装する。
            let thread_name = handle.thread().name().map(|s| s.to_string());
            let start = std::time::Instant::now();
            let mut finished = false;

            while start.elapsed() < std::time::Duration::from_millis(MAX_JOIN_WAIT_PER_THREAD_MS) {
                if handle.is_finished() {
                    finished = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }

            if finished {
                if let Err(e) = handle.join() {
                    log::warn!("Thread #{} ({:?}) panicked during join: {:?}", i, thread_name, e);
                }
            } else {
                log::warn!(
                    "Thread #{} ({:?}) did not finish within {}ms; detaching",
                    i, thread_name, MAX_JOIN_WAIT_PER_THREAD_MS
                );
                // JoinHandle を drop するとスレッドはデタッチされ、プロセス終了まで動作を続ける
                drop(handle);
            }
        }

        log::info!("All recording threads joined (or timed out): {}", self.id);

        // Write metadata
        // Read actual screen dimensions from capture thread output
        let (screen_width, screen_height) = self.read_dimensions();

        // Read frame count from capture thread output
        let frame_count = self.read_frame_count();

        if let Err(e) = super::audio::finalize_audio(&self.recording_dir) {
            log::warn!("audio.wav を作れません: {:#}", e);
        }

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
            start_time: self.started_at.clone(),
            duration_ms,
            has_audio,
            monitor_scale: 1.0,
            recording_dir: self.recording_dir.to_string_lossy().to_string(),
            recording_mode: mode_str,
            window_title: win_title,
            window_initial_rect: win_rect,
            frame_count: Some(frame_count),
        };

        let meta_path = self.recording_dir.join("meta.json");
        let meta_json = serde_json::to_string_pretty(&meta)?;
        std::fs::write(meta_path, meta_json)?;

        // meta.json への統合が完了したので、冗長な中間ファイルを削除する。
        // 互換性: 読み込み側は meta.frame_count が None のとき frame_count.txt に
        // フォールバックするため、旧録画は従来通り動作する。
        let _ = std::fs::remove_file(self.recording_dir.join("dimensions.txt"));
        let _ = std::fs::remove_file(self.recording_dir.join("frame_count.txt"));

        // 画面の変化領域を求めておく（自動ズームの主役の手がかり, Issue #39）。
        // 失敗しても録画は使える（プレビューを開くときに作り直す）ので警告だけにする
        if let Err(e) = crate::changes::generate(&self.recording_dir) {
            log::warn!("changes.jsonl を作れません: {:#}", e);
        }

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

    fn read_frame_count(&self) -> u32 {
        let fc_path = self.recording_dir.join("frame_count.txt");
        std::fs::read_to_string(&fc_path)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(0)
    }

    pub fn pause(&self) -> Result<()> {
        // 時計を先に止めてからフラグを立てる（停止中に付くタイムスタンプが進まないように）
        self.clock.pause();
        self.is_paused.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub fn resume(&self) -> Result<()> {
        self.clock.resume();
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
                    // meta.frame_count を優先。旧録画 (None) は frame_count.txt にフォールバック。
                    let frame_count = meta.frame_count.unwrap_or_else(|| {
                        let fc_path = entry.path().join("frame_count.txt");
                        std::fs::read_to_string(&fc_path)
                            .ok()
                            .and_then(|s| s.trim().parse::<u32>().ok())
                            .unwrap_or(0)
                    });
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
