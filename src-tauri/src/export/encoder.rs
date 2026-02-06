use crate::config::defaults::OutputStyle;
use crate::config::{AppSettings, ExportFormat, QualityPreset, RecordingEvent, RecordingMeta};
use crate::engine::analyzer::analyze_events;
use crate::engine::compositor::{ClickEffect, Compositor, KeyOverlay};
use crate::engine::cursor_smoother::CursorSmoother;
use crate::engine::zoom_planner::generate_zoom_plan;
use crate::export::presets::EncodingParams;
use anyhow::Result;
use std::process::Command;

pub fn export(
    recording_id: &str,
    format: &ExportFormat,
    quality: &QualityPreset,
    settings: &AppSettings,
) -> Result<String> {
    let recording_dir = dirs::video_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Snappi")
        .join("recordings")
        .join(recording_id);

    let meta_path = recording_dir.join("meta.json");
    let meta_str = std::fs::read_to_string(&meta_path)?;
    let meta: RecordingMeta = serde_json::from_str(&meta_str)?;

    let params = EncodingParams::from_preset(quality, meta.screen_width, meta.screen_height);
    let style = OutputStyle::from_settings(&params, settings);

    let output_dir = std::path::PathBuf::from(&settings.output.save_directory);
    std::fs::create_dir_all(&output_dir)?;

    let output_path = match format {
        ExportFormat::Mp4 => output_dir.join(format!("{}.mp4", recording_id)),
        ExportFormat::Gif => output_dir.join(format!("{}.gif", recording_id)),
        ExportFormat::WebM => output_dir.join(format!("{}.webm", recording_id)),
    };

    // Compose frames with effects engine
    log::info!("Starting effects composition for recording {}", recording_id);
    let temp_dir = compose_frames(&recording_dir, &meta, settings, style, params.fps)?;
    let composed_frames_dir = temp_dir.path().join("frames");
    log::info!("Effects composition complete, encoding...");

    let ffmpeg = find_ffmpeg()?;

    match format {
        ExportFormat::Mp4 => {
            encode_mp4(&ffmpeg, &composed_frames_dir, &output_path, &params, &recording_dir)?;
        }
        ExportFormat::Gif => {
            encode_gif(&ffmpeg, &composed_frames_dir, &output_path, &params)?;
        }
        ExportFormat::WebM => {
            encode_webm(&ffmpeg, &composed_frames_dir, &output_path, &params, &recording_dir)?;
        }
    }
    // temp_dir dropped here → composed frames cleaned up automatically

    log::info!("Export complete: {}", output_path.display());
    Ok(output_path.to_string_lossy().to_string())
}

// --- Effects composition pipeline ---

fn compose_frames(
    recording_dir: &std::path::Path,
    meta: &RecordingMeta,
    settings: &AppSettings,
    style: OutputStyle,
    fps: u32,
) -> Result<tempfile::TempDir> {
    let events = load_events(recording_dir).unwrap_or_default();
    let frame_time_step_ms = if fps > 0 { 1000 / fps as u64 } else { 33 };
    let dt = 1.0 / fps.max(1) as f64;

    // 1. Analyze events and generate zoom plan
    let segments = analyze_events(&events);
    let zoom_keyframes = if settings.effects.auto_zoom_enabled {
        generate_zoom_plan(
            &segments,
            meta,
            settings.effects.default_zoom_level,
            settings.effects.text_input_zoom_level,
            settings.effects.max_zoom,
            settings.effects.idle_timeout_ms,
        )
    } else {
        Vec::new()
    };
    log::info!(
        "Analyzed {} events → {} segments → {} zoom keyframes",
        events.len(),
        segments.len(),
        zoom_keyframes.len()
    );

    // 2. Smooth cursor positions
    let raw_positions = extract_mouse_positions(&events);
    let cursor_positions = if settings.effects.cursor_smoothing && !raw_positions.is_empty() {
        CursorSmoother::new().smooth(&raw_positions)
    } else {
        raw_positions
    };

    // 3. Build effect lists
    let click_effects = if settings.effects.click_ring_enabled {
        extract_click_effects(&events, style.click_ring_duration_ms)
    } else {
        Vec::new()
    };
    let key_overlays = if settings.effects.key_badge_enabled {
        extract_key_overlays(&events, style.key_badge_duration_ms)
    } else {
        Vec::new()
    };

    // 4. Create compositor
    let mut compositor = Compositor::new(style, meta.screen_width, meta.screen_height);

    // 5. Create temp directory for composed frames
    let temp_dir = tempfile::TempDir::new()?;
    let composed_frames_dir = temp_dir.path().join("frames");
    std::fs::create_dir_all(&composed_frames_dir)?;

    // 6. Read frame count
    let frame_count = read_frame_count(recording_dir);
    if frame_count == 0 {
        return Err(anyhow::anyhow!("No frames found in recording"));
    }

    let frames_dir = recording_dir.join("frames");
    let mut kf_index = 0;

    // 7. Process each frame
    for frame_idx in 0..frame_count {
        let frame_time_ms = frame_idx * frame_time_step_ms;

        // Apply any zoom keyframes that have been reached
        while kf_index < zoom_keyframes.len()
            && zoom_keyframes[kf_index].time_ms <= frame_time_ms
        {
            compositor.apply_keyframe(&zoom_keyframes[kf_index]);
            kf_index += 1;
        }

        // Load raw frame
        let frame_path = frames_dir.join(format!("frame_{:08}.png", frame_idx));
        let raw_frame = match image::open(&frame_path) {
            Ok(img) => img.to_rgba8(),
            Err(_) => {
                log::warn!("Frame {} not found, skipping", frame_idx);
                continue;
            }
        };

        // Find cursor position for this frame time
        let cursor_pos = find_cursor_at_time(&cursor_positions, frame_time_ms);

        // Find active key overlay
        let active_key = key_overlays.iter().rfind(|ko| ko.is_visible(frame_time_ms));

        // Compose frame with all effects
        let composed = compositor.compose_frame(
            &raw_frame,
            frame_time_ms,
            cursor_pos,
            &click_effects,
            active_key,
            dt,
        );

        // Save composed frame as BMP (faster than PNG for temp files)
        let output_path = composed_frames_dir.join(format!("frame_{:08}.bmp", frame_idx));
        composed.save_with_format(&output_path, image::ImageFormat::Bmp)?;

        if frame_idx % 30 == 0 {
            log::info!("Composing frame {}/{}", frame_idx, frame_count);
        }
    }

    Ok(temp_dir)
}

fn load_events(recording_dir: &std::path::Path) -> Result<Vec<RecordingEvent>> {
    let events_path = recording_dir.join("events.jsonl");
    if !events_path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&events_path)?;
    let mut events = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if !line.is_empty() {
            if let Ok(event) = serde_json::from_str::<RecordingEvent>(line) {
                events.push(event);
            }
        }
    }
    Ok(events)
}

fn extract_mouse_positions(events: &[RecordingEvent]) -> Vec<(u64, f64, f64)> {
    events
        .iter()
        .filter_map(|e| match e {
            RecordingEvent::MouseMove { t, x, y } => Some((*t, *x, *y)),
            RecordingEvent::Click { t, x, y, .. } => Some((*t, *x, *y)),
            _ => None,
        })
        .collect()
}

fn extract_click_effects(events: &[RecordingEvent], duration_ms: u64) -> Vec<ClickEffect> {
    events
        .iter()
        .filter_map(|e| match e {
            RecordingEvent::Click { t, x, y, .. } => Some(ClickEffect {
                x: *x,
                y: *y,
                start_ms: *t,
                duration_ms,
            }),
            _ => None,
        })
        .collect()
}

fn extract_key_overlays(events: &[RecordingEvent], duration_ms: u64) -> Vec<KeyOverlay> {
    events
        .iter()
        .filter_map(|e| match e {
            RecordingEvent::Key { t, key, modifiers } => {
                // Show modifier combos (Ctrl+C etc.) and special keys only
                let display = if !modifiers.is_empty() {
                    format!("{}+{}", modifiers.join("+"), key)
                } else {
                    match key.as_str() {
                        "Return" | "Escape" | "Tab" | "Backspace" | "Delete" | "Space"
                        | "F1" | "F2" | "F3" | "F4" | "F5" | "F6" | "F7" | "F8" | "F9"
                        | "F10" | "F11" | "F12" => key.clone(),
                        _ => return None,
                    }
                };
                Some(KeyOverlay {
                    keys: display,
                    start_ms: *t,
                    duration_ms,
                })
            }
            _ => None,
        })
        .collect()
}

fn find_cursor_at_time(positions: &[(u64, f64, f64)], time_ms: u64) -> Option<(f64, f64)> {
    if positions.is_empty() {
        return None;
    }
    match positions.binary_search_by_key(&time_ms, |&(t, _, _)| t) {
        Ok(i) => Some((positions[i].1, positions[i].2)),
        Err(0) => Some((positions[0].1, positions[0].2)),
        Err(i) => Some((positions[i - 1].1, positions[i - 1].2)),
    }
}

fn read_frame_count(recording_dir: &std::path::Path) -> u64 {
    let frame_count_path = recording_dir.join("frame_count.txt");
    std::fs::read_to_string(&frame_count_path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

// --- Thumbnail generation ---

pub fn generate_thumbnail(recording_id: &str) -> Result<String> {
    let recording_dir = dirs::video_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Snappi")
        .join("recordings")
        .join(recording_id);

    let frames_dir = recording_dir.join("frames");
    let frame_count = read_frame_count(&recording_dir).max(1);
    let target_frame = (frame_count as f64 * 0.3) as u64;

    // Try target frame, then fallback to frame 0
    let frame_path = frames_dir.join(format!("frame_{:08}.png", target_frame));
    let frame_path = if frame_path.exists() {
        frame_path
    } else {
        let fallback = frames_dir.join("frame_00000000.png");
        if !fallback.exists() {
            return Err(anyhow::anyhow!("No frames found for thumbnail"));
        }
        fallback
    };

    let img = image::open(&frame_path)?;
    let thumb_width = 640u32;
    let thumb_height = (img.height() as f64 * (thumb_width as f64 / img.width() as f64)) as u32;
    let thumbnail = image::imageops::resize(
        &img,
        thumb_width,
        thumb_height.max(1),
        image::imageops::FilterType::Triangle,
    );

    let thumb_path = recording_dir.join("thumbnail.png");
    thumbnail.save(&thumb_path)?;

    Ok(thumb_path.to_string_lossy().to_string())
}

// --- FFmpeg discovery ---

fn find_ffmpeg() -> Result<String> {
    // Try bundled ffmpeg first (next to exe)
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()));

    if let Some(ref dir) = exe_dir {
        let bundled = dir.join("ffmpeg").join("ffmpeg.exe");
        if bundled.exists() {
            return Ok(bundled.to_string_lossy().to_string());
        }
        let beside = dir.join("ffmpeg.exe");
        if beside.exists() {
            return Ok(beside.to_string_lossy().to_string());
        }
    }

    // Try system PATH
    if Command::new("ffmpeg").arg("-version").output().is_ok() {
        return Ok("ffmpeg".to_string());
    }

    // Try winget install location
    if let Some(local_app) = std::env::var_os("LOCALAPPDATA") {
        let winget_dir = std::path::PathBuf::from(local_app)
            .join("Microsoft")
            .join("WinGet")
            .join("Packages");
        if winget_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&winget_dir) {
                for entry in entries.flatten() {
                    if entry.file_name().to_string_lossy().contains("FFmpeg") {
                        if let Some(path) = find_ffmpeg_in_dir(&entry.path()) {
                            return Ok(path);
                        }
                    }
                }
            }
        }
    }

    // Try common install locations
    let common_paths = [
        r"C:\ffmpeg\bin\ffmpeg.exe",
        r"C:\tools\ffmpeg\bin\ffmpeg.exe",
        r"C:\Program Files\ffmpeg\bin\ffmpeg.exe",
    ];
    for path in &common_paths {
        if std::path::Path::new(path).exists() {
            return Ok(path.to_string());
        }
    }

    Err(anyhow::anyhow!(
        "FFmpeg not found. Please install FFmpeg or place it in the ffmpeg/ directory."
    ))
}

fn find_ffmpeg_in_dir(dir: &std::path::Path) -> Option<String> {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.file_name().map(|n| n == "ffmpeg.exe").unwrap_or(false) {
                return Some(path.to_string_lossy().to_string());
            }
            if path.is_dir() {
                if let Some(found) = find_ffmpeg_in_dir(&path) {
                    return Some(found);
                }
            }
        }
    }
    None
}

// --- FFmpeg encoding (uses composed frames, no additional scaling) ---

fn encode_mp4(
    ffmpeg: &str,
    frames_dir: &std::path::Path,
    output: &std::path::Path,
    params: &EncodingParams,
    recording_dir: &std::path::Path,
) -> Result<()> {
    let mut cmd = Command::new(ffmpeg);

    // Input: composed BMP frames
    cmd.args(["-y", "-framerate"])
        .arg(params.fps.to_string())
        .args(["-i"])
        .arg(
            frames_dir
                .join("frame_%08d.bmp")
                .to_string_lossy()
                .to_string(),
        );

    // Add audio input if available and non-empty
    let audio_path = recording_dir.join("audio.wav");
    let has_audio = audio_path.exists()
        && std::fs::metadata(&audio_path)
            .map(|m| m.len() > 44)
            .unwrap_or(false);
    if has_audio {
        cmd.args(["-i"])
            .arg(audio_path.to_string_lossy().to_string());
    }

    // Output options (composed frames are already at final canvas resolution)
    cmd.args(["-c:v", "libx264"])
        .args(["-crf"])
        .arg(params.crf.to_string())
        .args(["-preset", "medium"])
        .args(["-pix_fmt", "yuv420p"])
        .args(["-movflags", "+faststart"]);

    if has_audio {
        cmd.args(["-c:a", "aac", "-b:a", "128k"]);
    }

    cmd.arg(output.to_string_lossy().to_string());

    let result = cmd.output()?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(anyhow::anyhow!("FFmpeg MP4 encoding failed: {}", stderr));
    }

    Ok(())
}

fn encode_gif(
    ffmpeg: &str,
    frames_dir: &std::path::Path,
    output: &std::path::Path,
    params: &EncodingParams,
) -> Result<()> {
    let palette_path = output.with_extension("palette.png");
    let fps = params.fps.min(15);
    let width = params.canvas_width.min(640);

    // Pass 1: Generate palette
    Command::new(ffmpeg)
        .args(["-y", "-framerate"])
        .arg(fps.to_string())
        .args(["-i"])
        .arg(
            frames_dir
                .join("frame_%08d.bmp")
                .to_string_lossy()
                .to_string(),
        )
        .args(["-vf"])
        .arg(format!("scale={}:-1:flags=lanczos,palettegen", width))
        .arg(palette_path.to_string_lossy().to_string())
        .output()?;

    // Pass 2: Generate GIF with palette
    Command::new(ffmpeg)
        .args(["-y", "-framerate"])
        .arg(fps.to_string())
        .args(["-i"])
        .arg(
            frames_dir
                .join("frame_%08d.bmp")
                .to_string_lossy()
                .to_string(),
        )
        .args(["-i"])
        .arg(palette_path.to_string_lossy().to_string())
        .args(["-lavfi"])
        .arg(format!(
            "scale={}:-1:flags=lanczos[x];[x][1:v]paletteuse",
            width
        ))
        .arg(output.to_string_lossy().to_string())
        .output()?;

    let _ = std::fs::remove_file(&palette_path);

    Ok(())
}

fn encode_webm(
    ffmpeg: &str,
    frames_dir: &std::path::Path,
    output: &std::path::Path,
    params: &EncodingParams,
    recording_dir: &std::path::Path,
) -> Result<()> {
    let mut cmd = Command::new(ffmpeg);

    // Input: composed BMP frames
    cmd.args(["-y", "-framerate"])
        .arg(params.fps.to_string())
        .args(["-i"])
        .arg(
            frames_dir
                .join("frame_%08d.bmp")
                .to_string_lossy()
                .to_string(),
        );

    // Add audio input if available and non-empty
    let audio_path = recording_dir.join("audio.wav");
    let has_audio = audio_path.exists()
        && std::fs::metadata(&audio_path)
            .map(|m| m.len() > 44)
            .unwrap_or(false);
    if has_audio {
        cmd.args(["-i"])
            .arg(audio_path.to_string_lossy().to_string());
    }

    // Output options
    cmd.args(["-c:v", "libvpx-vp9"])
        .args(["-crf"])
        .arg(params.crf.to_string())
        .args(["-b:v", "0"]);

    if has_audio {
        cmd.args(["-c:a", "libopus"]);
    }

    cmd.arg(output.to_string_lossy().to_string());

    let result = cmd.output()?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(anyhow::anyhow!("FFmpeg WebM encoding failed: {}", stderr));
    }

    Ok(())
}
