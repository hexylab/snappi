use crate::config::defaults::OutputStyle;
use crate::config::{AppSettings, ExportFormat, QualityPreset, RecordingEvent, RecordingMeta};
use crate::engine::compositor::{ClickEffect, Compositor, KeyOverlay};
use crate::engine::cursor_smoother::CursorSmoother;
use crate::engine::preprocessor::preprocess;
use crate::engine::frame_differ;
use crate::engine::scene_splitter::{self, split_into_scenes};
use crate::engine::zoom_planner::generate_zoom_plan;
use chrono::DateTime;
use crate::export::presets::EncodingParams;
use anyhow::Result;
use std::process::Command;

/// Progress callback: (stage, progress 0.0-1.0)
pub type ProgressFn = Box<dyn Fn(&str, f64) + Send>;

/// Generate export filename from recording start_time (RFC3339) as YYYYMMDD_hhmmss.
fn export_filename(start_time: &str, format: &ExportFormat) -> String {
    let ext = match format {
        ExportFormat::Mp4 => "mp4",
        ExportFormat::Gif => "gif",
        ExportFormat::WebM => "webm",
    };
    if let Ok(dt) = DateTime::parse_from_rfc3339(start_time) {
        format!("{}.{}", dt.format("%Y%m%d_%H%M%S"), ext)
    } else {
        // Fallback: use current time
        let now = chrono::Local::now();
        format!("{}.{}", now.format("%Y%m%d_%H%M%S"), ext)
    }
}

pub fn export(
    recording_id: &str,
    format: &ExportFormat,
    quality: &QualityPreset,
    settings: &AppSettings,
    progress: Option<&ProgressFn>,
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

    let output_path = output_dir.join(export_filename(&meta.start_time, format));

    // Compose frames with effects engine
    log::info!("Starting effects composition for recording {}", recording_id);
    if let Some(cb) = progress { cb("composing", 0.0); }
    let (temp_dir, actual_fps) = compose_frames(&recording_dir, &meta, settings, style, progress)?;
    let composed_frames_dir = temp_dir.path().join("frames");
    log::info!("Effects composition complete (actual fps: {:.1}), encoding...", actual_fps);

    if let Some(cb) = progress { cb("encoding", 0.8); }
    let ffmpeg = find_ffmpeg()?;

    match format {
        ExportFormat::Mp4 => {
            encode_mp4(&ffmpeg, &composed_frames_dir, &output_path, &params, &recording_dir, actual_fps)?;
        }
        ExportFormat::Gif => {
            encode_gif(&ffmpeg, &composed_frames_dir, &output_path, &params, actual_fps)?;
        }
        ExportFormat::WebM => {
            encode_webm(&ffmpeg, &composed_frames_dir, &output_path, &params, &recording_dir, actual_fps)?;
        }
    }
    // temp_dir dropped here → composed frames cleaned up automatically

    if let Some(cb) = progress { cb("complete", 1.0); }
    log::info!("Export complete: {}", output_path.display());
    Ok(output_path.to_string_lossy().to_string())
}

/// Generate zoom keyframes for a recording (used by Timeline UI).
pub fn generate_keyframes_for_recording(
    recording_id: &str,
    settings: &AppSettings,
) -> Result<Vec<crate::engine::zoom_planner::ZoomKeyframe>> {
    let recording_dir = dirs::video_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Snappi")
        .join("recordings")
        .join(recording_id);

    let meta_path = recording_dir.join("meta.json");
    let meta_str = std::fs::read_to_string(&meta_path)?;
    let meta: RecordingMeta = serde_json::from_str(&meta_str)?;

    let raw_events = load_events(&recording_dir).unwrap_or_default();
    let preprocessed = preprocess(&raw_events);
    let events = preprocessed.events;

    let mut scenes = split_into_scenes(
        &events,
        meta.screen_width as f64,
        meta.screen_height as f64,
        settings.effects.max_zoom,
    );

    // Frame diff pre-pass (coarser sampling for UI responsiveness)
    let mut change_regions: Vec<frame_differ::ChangeRegion> = Vec::new();
    if settings.effects.auto_zoom_enabled {
        let frames_dir = recording_dir.join("frames");
        let frame_count = read_frame_count(&recording_dir);
        let cursor_for_diff = extract_mouse_positions(&events);
        let diff_config = frame_differ::DiffConfig {
            sample_interval: 10,
            ..frame_differ::DiffConfig::default()
        };
        if let Ok(diff_result) = frame_differ::detect_frame_changes(
            &frames_dir,
            frame_count,
            meta.duration_ms,
            &cursor_for_diff,
            meta.screen_width,
            meta.screen_height,
            &diff_config,
        ) {
            change_regions = diff_result.regions;
            if settings.effects.frame_diff_enabled {
                scene_splitter::expand_scenes_with_change_regions(
                    &mut scenes,
                    &change_regions,
                    meta.screen_width as f64,
                    meta.screen_height as f64,
                    settings.effects.max_zoom,
                );
            }
        }
    }

    let keyframes = if settings.effects.auto_zoom_enabled {
        generate_zoom_plan(&scenes, &meta, &settings.effects, &change_regions)
    } else {
        Vec::new()
    };

    Ok(keyframes)
}

/// Get scene debug info for a recording (used by Timeline UI).
pub fn get_recording_scenes(
    recording_id: &str,
    settings: &AppSettings,
) -> Result<Vec<crate::engine::scene_splitter::Scene>> {
    let recording_dir = dirs::video_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Snappi")
        .join("recordings")
        .join(recording_id);

    let meta_path = recording_dir.join("meta.json");
    let meta_str = std::fs::read_to_string(&meta_path)?;
    let meta: RecordingMeta = serde_json::from_str(&meta_str)?;

    let raw_events = load_events(&recording_dir).unwrap_or_default();
    let preprocessed = preprocess(&raw_events);
    let events = preprocessed.events;

    let mut scenes = split_into_scenes(
        &events,
        meta.screen_width as f64,
        meta.screen_height as f64,
        settings.effects.max_zoom,
    );

    // Frame diff pre-pass (coarser sampling for UI responsiveness)
    if settings.effects.auto_zoom_enabled {
        let frames_dir = recording_dir.join("frames");
        let frame_count = read_frame_count(&recording_dir);
        let cursor_for_diff = extract_mouse_positions(&events);
        let diff_config = frame_differ::DiffConfig {
            sample_interval: 10,
            ..frame_differ::DiffConfig::default()
        };
        if let Ok(diff_result) = frame_differ::detect_frame_changes(
            &frames_dir,
            frame_count,
            meta.duration_ms,
            &cursor_for_diff,
            meta.screen_width,
            meta.screen_height,
            &diff_config,
        ) {
            if settings.effects.frame_diff_enabled {
                scene_splitter::expand_scenes_with_change_regions(
                    &mut scenes,
                    &diff_result.regions,
                    meta.screen_width as f64,
                    meta.screen_height as f64,
                    settings.effects.max_zoom,
                );
            }
        }
    }

    Ok(scenes)
}

/// Apply scene edits (merge/split) and regenerate keyframes.
///
/// Loads events, creates auto-detected scenes, applies edits, then runs zoom_planner.
pub fn apply_scene_edits_for_recording(
    recording_id: &str,
    edits: Vec<crate::engine::scene_splitter::SceneEditOp>,
    settings: &AppSettings,
) -> Result<(
    Vec<crate::engine::scene_splitter::Scene>,
    Vec<crate::engine::zoom_planner::ZoomKeyframe>,
)> {
    let recording_dir = dirs::video_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Snappi")
        .join("recordings")
        .join(recording_id);

    let meta_path = recording_dir.join("meta.json");
    let meta_str = std::fs::read_to_string(&meta_path)?;
    let meta: RecordingMeta = serde_json::from_str(&meta_str)?;

    let raw_events = load_events(&recording_dir).unwrap_or_default();
    let preprocessed = preprocess(&raw_events);
    let events = preprocessed.events;

    let mut scenes = split_into_scenes(
        &events,
        meta.screen_width as f64,
        meta.screen_height as f64,
        settings.effects.max_zoom,
    );

    // Frame diff expansion (same as get_recording_scenes)
    let mut change_regions: Vec<frame_differ::ChangeRegion> = Vec::new();
    if settings.effects.auto_zoom_enabled {
        let frames_dir = recording_dir.join("frames");
        let frame_count = read_frame_count(&recording_dir);
        let cursor_for_diff = extract_mouse_positions(&events);
        let diff_config = frame_differ::DiffConfig {
            sample_interval: 10,
            ..frame_differ::DiffConfig::default()
        };
        if let Ok(diff_result) = frame_differ::detect_frame_changes(
            &frames_dir,
            frame_count,
            meta.duration_ms,
            &cursor_for_diff,
            meta.screen_width,
            meta.screen_height,
            &diff_config,
        ) {
            change_regions = diff_result.regions;
            if settings.effects.frame_diff_enabled {
                scene_splitter::expand_scenes_with_change_regions(
                    &mut scenes,
                    &change_regions,
                    meta.screen_width as f64,
                    meta.screen_height as f64,
                    settings.effects.max_zoom,
                );
            }
        }
    }

    // Apply manual edits
    let mut edited_scenes = scene_splitter::apply_scene_edits(
        &scenes,
        &edits,
        &events,
        meta.screen_width as f64,
        meta.screen_height as f64,
        settings.effects.max_zoom,
    );

    // Re-apply frame diff expansion to edited scenes (merge/split recomputes bbox from
    // activity points only, so frame_diff regions need to be re-applied)
    if settings.effects.frame_diff_enabled && !change_regions.is_empty() && !edits.is_empty() {
        scene_splitter::expand_scenes_with_change_regions(
            &mut edited_scenes,
            &change_regions,
            meta.screen_width as f64,
            meta.screen_height as f64,
            settings.effects.max_zoom,
        );
    }

    // Regenerate keyframes from edited scenes
    let keyframes = if settings.effects.auto_zoom_enabled {
        generate_zoom_plan(&edited_scenes, &meta, &settings.effects, &change_regions)
    } else {
        Vec::new()
    };

    Ok((edited_scenes, keyframes))
}

/// Compute activity center for a time range (used by frontend segment merge/add).
pub fn compute_activity_center_for_recording(
    recording_id: &str,
    start_ms: u64,
    end_ms: u64,
    settings: &AppSettings,
) -> Result<(f64, f64, f64)> {
    let recording_dir = dirs::video_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Snappi")
        .join("recordings")
        .join(recording_id);

    let meta_path = recording_dir.join("meta.json");
    let meta_str = std::fs::read_to_string(&meta_path)?;
    let meta: RecordingMeta = serde_json::from_str(&meta_str)?;

    let raw_events = load_events(&recording_dir).unwrap_or_default();
    let preprocessed = preprocess(&raw_events);

    Ok(scene_splitter::compute_activity_center(
        &preprocessed.events,
        start_ms,
        end_ms,
        meta.screen_width as f64,
        meta.screen_height as f64,
        settings.effects.max_zoom,
    ))
}

/// Get recording events for Timeline UI (lightweight representation).
pub fn get_recording_events(recording_id: &str) -> Result<Vec<crate::config::TimelineEvent>> {
    let recording_dir = dirs::video_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Snappi")
        .join("recordings")
        .join(recording_id);

    let raw_events = load_events(&recording_dir).unwrap_or_default();
    let mut timeline_events = Vec::new();

    for event in &raw_events {
        let te = match event {
            RecordingEvent::Click { t, btn, x, y } => Some(crate::config::TimelineEvent {
                time_ms: *t,
                event_type: "click".to_string(),
                x: Some(*x),
                y: Some(*y),
                label: Some(btn.clone()),
            }),
            RecordingEvent::Key { t, key, modifiers } => {
                let label = if !modifiers.is_empty() {
                    format!("{}+{}", modifiers.join("+"), key)
                } else {
                    key.clone()
                };
                Some(crate::config::TimelineEvent {
                    time_ms: *t,
                    event_type: "key".to_string(),
                    x: None,
                    y: None,
                    label: Some(label),
                })
            }
            RecordingEvent::Scroll { t, x, y, dy, .. } => Some(crate::config::TimelineEvent {
                time_ms: *t,
                event_type: "scroll".to_string(),
                x: Some(*x),
                y: Some(*y),
                label: Some(if *dy > 0.0 { "up" } else { "down" }.to_string()),
            }),
            RecordingEvent::Focus { t, name, rect, .. } => {
                let cx = (rect[0] + rect[2]) / 2.0;
                let cy = (rect[1] + rect[3]) / 2.0;
                Some(crate::config::TimelineEvent {
                    time_ms: *t,
                    event_type: "focus".to_string(),
                    x: Some(cx),
                    y: Some(cy),
                    label: Some(name.clone()),
                })
            }
            RecordingEvent::WindowFocus { t, title, rect } => {
                let cx = (rect[0] + rect[2]) / 2.0;
                let cy = (rect[1] + rect[3]) / 2.0;
                Some(crate::config::TimelineEvent {
                    time_ms: *t,
                    event_type: "window_focus".to_string(),
                    x: Some(cx),
                    y: Some(cy),
                    label: Some(title.clone()),
                })
            }
            _ => None,
        };
        if let Some(te) = te {
            timeline_events.push(te);
        }
    }

    Ok(timeline_events)
}

/// Export with custom keyframes (from Timeline UI edits).
pub fn export_with_custom_keyframes(
    recording_id: &str,
    keyframes: Vec<crate::engine::zoom_planner::ZoomKeyframe>,
    format: &ExportFormat,
    quality: &QualityPreset,
    settings: &AppSettings,
    progress: Option<&ProgressFn>,
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
    let style = crate::config::defaults::OutputStyle::from_settings(&params, settings);

    let output_dir = std::path::PathBuf::from(&settings.output.save_directory);
    std::fs::create_dir_all(&output_dir)?;

    let output_path = output_dir.join(export_filename(&meta.start_time, format));

    if let Some(cb) = progress { cb("composing", 0.0); }
    let (temp_dir, actual_fps) = compose_frames_with_keyframes(
        &recording_dir, &meta, settings, style, keyframes, progress,
    )?;
    let composed_frames_dir = temp_dir.path().join("frames");

    if let Some(cb) = progress { cb("encoding", 0.8); }
    let ffmpeg = find_ffmpeg()?;
    match format {
        ExportFormat::Mp4 => encode_mp4(&ffmpeg, &composed_frames_dir, &output_path, &params, &recording_dir, actual_fps)?,
        ExportFormat::Gif => encode_gif(&ffmpeg, &composed_frames_dir, &output_path, &params, actual_fps)?,
        ExportFormat::WebM => encode_webm(&ffmpeg, &composed_frames_dir, &output_path, &params, &recording_dir, actual_fps)?,
    }

    if let Some(cb) = progress { cb("complete", 1.0); }
    Ok(output_path.to_string_lossy().to_string())
}

/// Compose frames using custom keyframes (for timeline UI).
fn compose_frames_with_keyframes(
    recording_dir: &std::path::Path,
    meta: &RecordingMeta,
    settings: &AppSettings,
    style: crate::config::defaults::OutputStyle,
    zoom_keyframes: Vec<crate::engine::zoom_planner::ZoomKeyframe>,
    progress: Option<&ProgressFn>,
) -> Result<(tempfile::TempDir, f64)> {
    let raw_events = load_events(recording_dir).unwrap_or_default();
    let preprocessed = preprocess(&raw_events);
    let events = preprocessed.events;

    let frame_count = read_frame_count(recording_dir);
    if frame_count == 0 {
        return Err(anyhow::anyhow!("No frames found in recording"));
    }

    let actual_fps = if meta.duration_ms > 0 && frame_count > 1 {
        (frame_count as f64 * 1000.0) / meta.duration_ms as f64
    } else {
        meta.fps as f64
    };
    let frame_time_step_ms = if frame_count > 1 && meta.duration_ms > 0 {
        meta.duration_ms / frame_count
    } else if meta.fps > 0 {
        1000 / meta.fps as u64
    } else {
        33
    };
    let dt = 1.0 / actual_fps.max(1.0);

    let raw_positions = extract_mouse_positions(&events);
    // Adjust for window mode
    let adjusted_positions = if meta.recording_mode.as_deref() == Some("window") {
        if let Some(ref rect) = meta.window_initial_rect {
            raw_positions.into_iter()
                .map(|(t, x, y)| (t, x - rect[0], y - rect[1]))
                .collect()
        } else {
            raw_positions
        }
    } else {
        raw_positions
    };
    let cursor_positions = if settings.effects.cursor_smoothing && !adjusted_positions.is_empty() {
        CursorSmoother::new().smooth(&adjusted_positions)
    } else {
        adjusted_positions
    };

    let click_effects = if settings.effects.click_ring_enabled {
        let mut effects = extract_click_effects(&events, 400);
        if meta.recording_mode.as_deref() == Some("window") {
            if let Some(ref rect) = meta.window_initial_rect {
                for eff in &mut effects {
                    eff.x -= rect[0];
                    eff.y -= rect[1];
                }
            }
        }
        effects
    } else {
        Vec::new()
    };
    let key_overlays = if settings.effects.key_badge_enabled {
        extract_key_overlays(&events, 1500)
    } else {
        Vec::new()
    };

    let mut compositor = Compositor::new(style, meta.screen_width, meta.screen_height);
    compositor.set_motion_blur(settings.effects.motion_blur_enabled);

    let temp_dir = tempfile::TempDir::new()?;
    let composed_frames_dir = temp_dir.path().join("frames");
    std::fs::create_dir_all(&composed_frames_dir)?;

    let frames_dir = recording_dir.join("frames");
    let mut kf_index = 0;
    let mut output_frame_count: u64 = 0;

    for frame_idx in 0..frame_count {
        let frame_time_ms = frame_idx * frame_time_step_ms;

        while kf_index < zoom_keyframes.len() && zoom_keyframes[kf_index].time_ms <= frame_time_ms {
            compositor.apply_keyframe(&zoom_keyframes[kf_index]);
            kf_index += 1;
        }

        let frame_path = frames_dir.join(format!("frame_{:08}.png", frame_idx));
        let raw_frame = match image::open(&frame_path) {
            Ok(img) => img.to_rgba8(),
            Err(_) => continue,
        };

        let cursor_pos = find_cursor_at_time(&cursor_positions, frame_time_ms);
        let active_key = key_overlays.iter().rfind(|ko| ko.is_visible(frame_time_ms));

        let composed = compositor.compose_frame(&raw_frame, frame_time_ms, cursor_pos, &click_effects, active_key, dt);
        let rgb_frame = image::DynamicImage::ImageRgba8(composed).to_rgb8();
        let output_path = composed_frames_dir.join(format!("frame_{:08}.bmp", output_frame_count));
        rgb_frame.save_with_format(&output_path, image::ImageFormat::Bmp)?;
        output_frame_count += 1;

        if frame_idx % 10 == 0 {
            if let Some(cb) = progress {
                let p = (frame_idx as f64 / frame_count as f64) * 0.8;
                cb("composing", p);
            }
        }
    }

    let final_fps = if output_frame_count > 0 && meta.duration_ms > 0 {
        (output_frame_count as f64 * 1000.0) / meta.duration_ms as f64
    } else {
        actual_fps
    };

    Ok((temp_dir, final_fps))
}

// --- Effects composition pipeline ---

fn compose_frames(
    recording_dir: &std::path::Path,
    meta: &RecordingMeta,
    settings: &AppSettings,
    style: OutputStyle,
    progress: Option<&ProgressFn>,
) -> Result<(tempfile::TempDir, f64)> {
    let raw_events = load_events(recording_dir).unwrap_or_default();

    // Preprocess: thin mouse moves and detect drags
    let preprocessed = preprocess(&raw_events);
    let events = preprocessed.events;
    log::info!(
        "Preprocessed {} raw events → {} thinned events, {} drags detected",
        raw_events.len(),
        events.len(),
        preprocessed.drags.len(),
    );

    // Read frame count first (needed for timing calculation)
    let frame_count = read_frame_count(recording_dir);
    if frame_count == 0 {
        return Err(anyhow::anyhow!("No frames found in recording"));
    }

    // Calculate actual recording framerate from real data
    // Events use real-time timestamps (ms from recording start),
    // so frame timing must match the actual recording duration
    let actual_fps = if meta.duration_ms > 0 && frame_count > 1 {
        (frame_count as f64 * 1000.0) / meta.duration_ms as f64
    } else {
        meta.fps as f64
    };
    let frame_time_step_ms = if frame_count > 1 && meta.duration_ms > 0 {
        meta.duration_ms / frame_count
    } else if meta.fps > 0 {
        1000 / meta.fps as u64
    } else {
        33
    };
    let dt = 1.0 / actual_fps.max(1.0);

    // 1. Split events into scenes and generate lookahead zoom plan
    let mut scenes = split_into_scenes(
        &events,
        meta.screen_width as f64,
        meta.screen_height as f64,
        settings.effects.max_zoom,
    );

    // 1.5. Frame diff pre-pass: expand BBoxes with visual change regions
    // Also collect change_regions for idle detection in zoom_planner
    let mut change_regions: Vec<frame_differ::ChangeRegion> = Vec::new();
    if settings.effects.auto_zoom_enabled {
        if let Some(cb) = progress { cb("analyzing", 0.0); }
        let cursor_for_diff = extract_mouse_positions(&events);
        let diff_config = frame_differ::DiffConfig::default();
        match frame_differ::detect_frame_changes(
            &recording_dir.join("frames"),
            frame_count,
            meta.duration_ms,
            &cursor_for_diff,
            meta.screen_width,
            meta.screen_height,
            &diff_config,
        ) {
            Ok(diff_result) => {
                log::info!(
                    "Frame diff: {} change regions detected ({} pairs analyzed, {} excluded)",
                    diff_result.regions.len(),
                    diff_result.pairs_analyzed,
                    diff_result.pairs_excluded,
                );
                change_regions = diff_result.regions;
                if settings.effects.frame_diff_enabled {
                    scene_splitter::expand_scenes_with_change_regions(
                        &mut scenes,
                        &change_regions,
                        meta.screen_width as f64,
                        meta.screen_height as f64,
                        settings.effects.max_zoom,
                    );
                }
            }
            Err(e) => {
                log::warn!("Frame diff analysis failed, using event-only BBox: {}", e);
            }
        }
    }

    let zoom_keyframes = if settings.effects.auto_zoom_enabled {
        generate_zoom_plan(&scenes, meta, &settings.effects, &change_regions)
    } else {
        Vec::new()
    };
    log::info!(
        "Analyzed {} events → {} scenes → {} zoom keyframes (actual_fps: {:.1}, frame_step: {}ms)",
        events.len(),
        scenes.len(),
        zoom_keyframes.len(),
        actual_fps,
        frame_time_step_ms,
    );
    for scene in &scenes {
        log::info!(
            "  Scene #{}: {}ms-{}ms | bbox({:.0},{:.0} {:.0}x{:.0}) | center({:.0},{:.0}) | zoom {:.2}x | {} events",
            scene.id,
            scene.start_ms,
            scene.end_ms,
            scene.bbox.x, scene.bbox.y, scene.bbox.width, scene.bbox.height,
            scene.center_x, scene.center_y,
            scene.zoom_level,
            scene.event_count,
        );
    }

    // 2. Smooth cursor positions (and adjust for window mode)
    let raw_positions = extract_mouse_positions(&events);
    // For window mode: convert screen coords → window-relative coords
    let adjusted_positions = if meta.recording_mode.as_deref() == Some("window") {
        if let Some(ref rect) = meta.window_initial_rect {
            let win_x = rect[0];
            let win_y = rect[1];
            log::info!("Window mode: adjusting cursor coords by offset ({}, {})", win_x, win_y);
            raw_positions.into_iter()
                .map(|(t, x, y)| (t, x - win_x, y - win_y))
                .collect()
        } else {
            raw_positions
        }
    } else {
        raw_positions
    };
    let cursor_positions = if settings.effects.cursor_smoothing && !adjusted_positions.is_empty() {
        CursorSmoother::new().smooth(&adjusted_positions)
    } else {
        adjusted_positions
    };

    // 3. Build effect lists (also adjust for window mode)
    let click_effects = if settings.effects.click_ring_enabled {
        let mut effects = extract_click_effects(&events, style.click_ring_duration_ms);
        if meta.recording_mode.as_deref() == Some("window") {
            if let Some(ref rect) = meta.window_initial_rect {
                for eff in &mut effects {
                    eff.x -= rect[0];
                    eff.y -= rect[1];
                }
            }
        }
        effects
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
    compositor.set_motion_blur(settings.effects.motion_blur_enabled);

    // 5. Create temp directory for composed frames
    let temp_dir = tempfile::TempDir::new()?;
    let composed_frames_dir = temp_dir.path().join("frames");
    std::fs::create_dir_all(&composed_frames_dir)?;

    let frames_dir = recording_dir.join("frames");
    let mut kf_index = 0;
    let mut output_frame_count: u64 = 0;

    // 6. Process each frame
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
        // Use sequential output counter to avoid gaps in FFmpeg sequence
        // Convert RGBA→RGB to avoid alpha channel issues with FFmpeg
        let rgb_frame = image::DynamicImage::ImageRgba8(composed).to_rgb8();
        let output_path = composed_frames_dir.join(format!("frame_{:08}.bmp", output_frame_count));
        rgb_frame.save_with_format(&output_path, image::ImageFormat::Bmp)?;
        output_frame_count += 1;

        if frame_idx % 10 == 0 {
            log::info!("Composing frame {}/{}", frame_idx, frame_count);
            if let Some(cb) = progress {
                let p = (frame_idx as f64 / frame_count as f64) * 0.8;
                cb("composing", p);
            }
        }
    }

    // Recalculate fps based on actual output frame count (in case some frames were skipped)
    let final_fps = if output_frame_count > 0 && meta.duration_ms > 0 {
        (output_frame_count as f64 * 1000.0) / meta.duration_ms as f64
    } else {
        actual_fps
    };
    log::info!("Composed {} frames (final fps: {:.1})", output_frame_count, final_fps);

    Ok((temp_dir, final_fps))
}

fn load_events(recording_dir: &std::path::Path) -> Result<Vec<RecordingEvent>> {
    let mut events = Vec::new();

    // Load main events
    let events_path = recording_dir.join("events.jsonl");
    if events_path.exists() {
        let content = std::fs::read_to_string(&events_path)?;
        for line in content.lines() {
            let line = line.trim();
            if !line.is_empty() {
                if let Ok(event) = serde_json::from_str::<RecordingEvent>(line) {
                    events.push(event);
                }
            }
        }
    }

    // Note: window_events.jsonl is no longer loaded for the zoom pipeline.
    // The 2-state zoom model (Overview ↔ WorkArea) does not use window focus tracking.

    // Sort by timestamp
    events.sort_by_key(|e| crate::engine::analyzer::event_timestamp(e));
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

/// Find cursor position at a given time with linear interpolation between samples.
fn find_cursor_at_time(positions: &[(u64, f64, f64)], time_ms: u64) -> Option<(f64, f64)> {
    if positions.is_empty() {
        return None;
    }
    match positions.binary_search_by_key(&time_ms, |&(t, _, _)| t) {
        Ok(i) => Some((positions[i].1, positions[i].2)),
        Err(0) => Some((positions[0].1, positions[0].2)),
        Err(i) if i >= positions.len() => {
            let last = &positions[positions.len() - 1];
            Some((last.1, last.2))
        }
        Err(i) => {
            // Linear interpolation between positions[i-1] and positions[i]
            let (t0, x0, y0) = positions[i - 1];
            let (t1, x1, y1) = positions[i];
            let dt = (t1 - t0) as f64;
            if dt <= 0.0 {
                return Some((x0, y0));
            }
            let t = (time_ms - t0) as f64 / dt;
            Some((x0 + (x1 - x0) * t, y0 + (y1 - y0) * t))
        }
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
    input_fps: f64,
) -> Result<()> {
    let mut cmd = Command::new(ffmpeg);

    // Input: composed BMP frames at actual recording framerate
    cmd.args(["-y", "-framerate"])
        .arg(format!("{:.2}", input_fps))
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
        .args(["-movflags", "+faststart"])
        .args(["-r"])
        .arg(params.fps.to_string());

    if has_audio {
        cmd.args(["-c:a", "aac", "-b:a", "128k", "-shortest"]);
    }

    cmd.arg(output.to_string_lossy().to_string());

    log::info!("FFmpeg MP4 command: {:?}", cmd);
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
    input_fps: f64,
) -> Result<()> {
    let palette_path = output.with_extension("palette.png");
    let width = params.canvas_width.min(640);

    // Pass 1: Generate palette
    Command::new(ffmpeg)
        .args(["-y", "-framerate"])
        .arg(format!("{:.2}", input_fps))
        .args(["-i"])
        .arg(
            frames_dir
                .join("frame_%08d.bmp")
                .to_string_lossy()
                .to_string(),
        )
        .args(["-vf"])
        .arg(format!("fps=15,scale={}:-1:flags=lanczos,palettegen", width))
        .arg(palette_path.to_string_lossy().to_string())
        .output()?;

    // Pass 2: Generate GIF with palette
    Command::new(ffmpeg)
        .args(["-y", "-framerate"])
        .arg(format!("{:.2}", input_fps))
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
            "fps=15,scale={}:-1:flags=lanczos[x];[x][1:v]paletteuse",
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
    input_fps: f64,
) -> Result<()> {
    let mut cmd = Command::new(ffmpeg);

    // Input: composed BMP frames at actual recording framerate
    cmd.args(["-y", "-framerate"])
        .arg(format!("{:.2}", input_fps))
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
        .args(["-b:v", "0"])
        .args(["-r"])
        .arg(params.fps.to_string());

    if has_audio {
        cmd.args(["-c:a", "libopus", "-shortest"]);
    }

    cmd.arg(output.to_string_lossy().to_string());

    log::info!("FFmpeg WebM command: {:?}", cmd);
    let result = cmd.output()?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(anyhow::anyhow!("FFmpeg WebM encoding failed: {}", stderr));
    }

    Ok(())
}
