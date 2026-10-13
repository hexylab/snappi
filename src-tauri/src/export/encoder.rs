use crate::camera::{CameraState, CameraTrack};
use crate::config::defaults::OutputStyle;
use crate::config::{AppSettings, ExportFormat, QualityPreset, RecordingEvent};
use crate::engine::compositor::{ClickEffect, Compositor, CursorKind, KeyOverlay};
use crate::engine::cursor_smoother::CursorSmoother;
use crate::engine::frame_differ::{self, ChangeRegion};
use crate::engine::preprocessor::preprocess;
use crate::engine::spring::AnimatedViewport;
use crate::engine::scene_splitter::{self, split_into_scenes, Scene};
use crate::engine::zoom_planner::{generate_zoom_plan, ZoomKeyframe};
use crate::export::presets::EncodingParams;
use crate::recording_data::RecordingData;
use anyhow::Result;
use chrono::DateTime;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Progress callback: (stage, progress 0.0-1.0)
pub type ProgressFn = Box<dyn Fn(&str, f64) + Send>;

/// 中間合成フレームの JPEG 品質 (1-100)。
/// 最終出力は H.264 等でさらに圧縮されるため、中間段階で 95 を切る必要はほぼ無い。
/// BMP (1920x1080 で約 5.9MB) から JPEG q=95 (約 0.4MB) へ約 15 倍のディスク削減。
const INTERMEDIATE_JPEG_QUALITY: u8 = 95;

/// エクスポート時のフレーム差分のサンプリング間隔（フレーム数）
const EXPORT_DIFF_SAMPLE_INTERVAL: u64 = 5;
/// タイムライン UI 用のフレーム差分のサンプリング間隔（応答性重視で粗くする）
const UI_DIFF_SAMPLE_INTERVAL: u64 = 10;

/// RGB イメージを JPEG として高品質で保存するヘルパー。
fn save_rgb_as_jpeg(
    img: &image::RgbImage,
    path: &std::path::Path,
    quality: u8,
) -> Result<()> {
    let file = std::fs::File::create(path)?;
    let mut writer = std::io::BufWriter::new(file);
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut writer, quality);
    encoder.encode(
        img.as_raw(),
        img.width(),
        img.height(),
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(())
}

/// 書き出し先のパスを決める。ファイル名は録画開始時刻（RFC3339）から YYYYMMDD_hhmmss とし、
/// 同名ファイルが既にあれば `_2`, `_3` … を付けて上書きを避ける。
fn unique_output_path(output_dir: &Path, start_time: &str, format: &ExportFormat) -> PathBuf {
    let ext = match format {
        ExportFormat::Mp4 => "mp4",
        ExportFormat::Gif => "gif",
        ExportFormat::WebM => "webm",
    };
    let stem = match DateTime::parse_from_rfc3339(start_time) {
        Ok(dt) => dt.format("%Y%m%d_%H%M%S").to_string(),
        // Fallback: use current time
        Err(_) => chrono::Local::now().format("%Y%m%d_%H%M%S").to_string(),
    };

    let first = output_dir.join(format!("{}.{}", stem, ext));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| output_dir.join(format!("{}_{}.{}", stem, n, ext)))
        .find(|p| !p.exists())
        .expect("unbounded range always yields a free name")
}

pub fn export(
    recording_id: &str,
    format: &ExportFormat,
    quality: &QualityPreset,
    settings: &AppSettings,
    progress: Option<&ProgressFn>,
) -> Result<String> {
    let data = RecordingData::load_by_id(recording_id)?;

    log::info!("Starting effects composition for recording {}", recording_id);
    if let Some(cb) = progress { cb("analyzing", 0.0); }
    let keyframes = plan_export_keyframes(&data, settings);

    export_recording_data(&data, &keyframes, format, quality, settings, progress)
}

/// Export with custom keyframes (from Timeline UI edits).
pub fn export_with_custom_keyframes(
    recording_id: &str,
    keyframes: Vec<ZoomKeyframe>,
    format: &ExportFormat,
    quality: &QualityPreset,
    settings: &AppSettings,
    progress: Option<&ProgressFn>,
) -> Result<String> {
    let data = RecordingData::load_by_id(recording_id)?;
    export_recording_data(&data, &keyframes, format, quality, settings, progress)
}

fn export_recording_data(
    data: &RecordingData,
    keyframes: &[ZoomKeyframe],
    format: &ExportFormat,
    quality: &QualityPreset,
    settings: &AppSettings,
    progress: Option<&ProgressFn>,
) -> Result<String> {
    let params = EncodingParams::from_preset(quality, data.width(), data.height());
    let style = OutputStyle::from_settings(&params, settings);

    let output_dir = PathBuf::from(&settings.output.save_directory);
    std::fs::create_dir_all(&output_dir)?;
    let output_path = unique_output_path(&output_dir, &data.meta.start_time, format);

    if let Some(cb) = progress { cb("composing", 0.0); }
    let temp_dir = compose_frames(data, settings, style, keyframes, params.fps, progress)?;
    let composed_frames_dir = temp_dir.path().join("frames");
    log::info!("Effects composition complete ({} fps), encoding...", params.fps);

    if let Some(cb) = progress { cb("encoding", 0.8); }
    let ffmpeg = find_ffmpeg()?;
    match format {
        ExportFormat::Mp4 => encode_mp4(&ffmpeg, &composed_frames_dir, &output_path, &params, &data.dir)?,
        ExportFormat::Gif => encode_gif(&ffmpeg, &composed_frames_dir, &output_path, &params)?,
        ExportFormat::WebM => encode_webm(&ffmpeg, &composed_frames_dir, &output_path, &params, &data.dir)?,
    }
    // temp_dir dropped here → composed frames cleaned up automatically

    if let Some(cb) = progress { cb("complete", 1.0); }
    log::info!("Export complete: {}", output_path.display());
    Ok(output_path.to_string_lossy().to_string())
}

/// シーン分割と、フレーム差分によるシーン BBox の拡張を行う。
/// 返り値の ChangeRegion は zoom_planner のアイドル判定にも使う。
fn analyze_scenes(
    data: &RecordingData,
    events: &[RecordingEvent],
    settings: &AppSettings,
    diff_sample_interval: u64,
) -> (Vec<Scene>, Vec<ChangeRegion>) {
    let (w, h) = (data.width() as f64, data.height() as f64);
    let max_zoom = settings.effects.max_zoom;
    let mut scenes = split_into_scenes(events, w, h, max_zoom);

    let mut change_regions = Vec::new();
    if settings.effects.auto_zoom_enabled {
        let diff_config = frame_differ::DiffConfig {
            sample_interval: diff_sample_interval,
            ..frame_differ::DiffConfig::default()
        };
        match frame_differ::detect_frame_changes(
            &data.frames_dir(),
            &data.frame_times,
            &extract_mouse_positions(events),
            data.width(),
            data.height(),
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
                        &mut scenes, &change_regions, w, h, max_zoom,
                    );
                }
            }
            Err(e) => {
                log::warn!("Frame diff analysis failed, using event-only BBox: {}", e);
            }
        }
    }

    (scenes, change_regions)
}

/// エクスポート用のズームキーフレームを生成する。
fn plan_export_keyframes(data: &RecordingData, settings: &AppSettings) -> Vec<ZoomKeyframe> {
    let preprocessed = preprocess(&data.events);
    log::info!(
        "Preprocessed {} raw events → {} thinned events, {} drags detected",
        data.events.len(),
        preprocessed.events.len(),
        preprocessed.drags.len(),
    );
    let events = preprocessed.events;

    let (mut scenes, change_regions) =
        analyze_scenes(data, &events, settings, EXPORT_DIFF_SAMPLE_INTERVAL);

    // Phase A (Issue #23): ui_events.jsonl の矩形を各シーンに紐付け
    crate::engine::ui_context::attach_ui_rects_to_scenes(
        &mut scenes,
        &events,
        data.width() as f64,
        data.height() as f64,
    );

    let zoom_keyframes = if settings.effects.auto_zoom_enabled {
        generate_zoom_plan(&scenes, &data.meta, &settings.effects, &change_regions)
    } else {
        Vec::new()
    };
    log::info!(
        "Analyzed {} events → {} scenes → {} zoom keyframes",
        events.len(),
        scenes.len(),
        zoom_keyframes.len(),
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
    zoom_keyframes
}

/// 旧エンジン（v4）のカメラトラックを求める（評価用, Issue #28）。
/// `compose_frames` と同じく、出力時刻 t までのキーフレームを適用してからバネを dt 進め、
/// フレーム内にクランプしたビューポートをカメラ状態とする。
pub fn legacy_camera_track(data: &RecordingData, settings: &AppSettings, fps: u32) -> CameraTrack {
    let keyframes = plan_export_keyframes(data, settings);
    let (w, h) = (data.width() as f64, data.height() as f64);
    let mut viewport = AnimatedViewport::new(w, h);
    let dt = 1.0 / fps.max(1) as f64;
    let mut kf_index = 0;
    let frames = data
        .output_frame_times(fps)
        .into_iter()
        .map(|time_ms| {
            while kf_index < keyframes.len() && keyframes[kf_index].time_ms <= time_ms {
                let kf = &keyframes[kf_index];
                match &kf.spring_hint {
                    Some(hint) => viewport.set_target_with_half_life(
                        kf.target_x,
                        kf.target_y,
                        kf.zoom_level,
                        hint.zoom_half_life,
                        hint.pan_half_life,
                    ),
                    None => viewport.set_target(kf.target_x, kf.target_y, kf.zoom_level),
                }
                kf_index += 1;
            }
            viewport.update(dt);
            let vp = viewport.current_viewport(w, h);
            CameraState {
                center_x: vp.x + vp.width / 2.0,
                center_y: vp.y + vp.height / 2.0,
                zoom: vp.zoom,
            }
        })
        .collect();
    CameraTrack { fps, frames }
}

/// Generate zoom keyframes for a recording (used by Timeline UI).
pub fn generate_keyframes_for_recording(
    recording_id: &str,
    settings: &AppSettings,
) -> Result<Vec<ZoomKeyframe>> {
    let data = RecordingData::load_by_id(recording_id)?;
    let events = preprocess(&data.events).events;

    let (mut scenes, change_regions) =
        analyze_scenes(&data, &events, settings, UI_DIFF_SAMPLE_INTERVAL);

    // Phase A (Issue #23): ui_events.jsonl の矩形情報を各シーンに付与。
    // scene.ui_rect が立ったシーンは zoom_planner で矩形ベースのズーム計算に切り替わる。
    crate::engine::ui_context::attach_ui_rects_to_scenes(
        &mut scenes,
        &data.events,
        data.width() as f64,
        data.height() as f64,
    );

    let keyframes = if settings.effects.auto_zoom_enabled {
        generate_zoom_plan(&scenes, &data.meta, &settings.effects, &change_regions)
    } else {
        Vec::new()
    };

    Ok(keyframes)
}

/// Get scene debug info for a recording (used by Timeline UI).
pub fn get_recording_scenes(recording_id: &str, settings: &AppSettings) -> Result<Vec<Scene>> {
    let data = RecordingData::load_by_id(recording_id)?;
    let events = preprocess(&data.events).events;
    let (scenes, _) = analyze_scenes(&data, &events, settings, UI_DIFF_SAMPLE_INTERVAL);
    Ok(scenes)
}

/// Apply scene edits (merge/split) and regenerate keyframes.
///
/// Loads events, creates auto-detected scenes, applies edits, then runs zoom_planner.
pub fn apply_scene_edits_for_recording(
    recording_id: &str,
    edits: Vec<crate::engine::scene_splitter::SceneEditOp>,
    settings: &AppSettings,
) -> Result<(Vec<Scene>, Vec<ZoomKeyframe>)> {
    let data = RecordingData::load_by_id(recording_id)?;
    let events = preprocess(&data.events).events;
    let (w, h) = (data.width() as f64, data.height() as f64);

    let (scenes, change_regions) =
        analyze_scenes(&data, &events, settings, UI_DIFF_SAMPLE_INTERVAL);

    // Apply manual edits
    let mut edited_scenes = scene_splitter::apply_scene_edits(
        &scenes,
        &edits,
        &events,
        w,
        h,
        settings.effects.max_zoom,
    );

    // Re-apply frame diff expansion to edited scenes (merge/split recomputes bbox from
    // activity points only, so frame_diff regions need to be re-applied)
    if settings.effects.frame_diff_enabled && !change_regions.is_empty() && !edits.is_empty() {
        scene_splitter::expand_scenes_with_change_regions(
            &mut edited_scenes,
            &change_regions,
            w,
            h,
            settings.effects.max_zoom,
        );
    }

    // Phase A (Issue #23): edited_scenes にも UI 矩形を紐付け
    crate::engine::ui_context::attach_ui_rects_to_scenes(&mut edited_scenes, &events, w, h);

    // Regenerate keyframes from edited scenes
    let keyframes = if settings.effects.auto_zoom_enabled {
        generate_zoom_plan(&edited_scenes, &data.meta, &settings.effects, &change_regions)
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
    let data = RecordingData::load_by_id(recording_id)?;
    let preprocessed = preprocess(&data.events);

    Ok(scene_splitter::compute_activity_center(
        &preprocessed.events,
        start_ms,
        end_ms,
        data.width() as f64,
        data.height() as f64,
        settings.effects.max_zoom,
    ))
}

/// Get recording events for Timeline UI (lightweight representation).
pub fn get_recording_events(recording_id: &str) -> Result<Vec<crate::config::TimelineEvent>> {
    let data = RecordingData::load_by_id(recording_id)?;
    let mut timeline_events = Vec::new();

    for event in &data.events {
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

// --- Effects composition pipeline ---

/// エフェクトを合成した出力フレームを一時ディレクトリに書き出す。
///
/// 出力は `fps` で等間隔に作る。出力フレーム k（時刻 t = k / fps）には「t 以前で最も新しい
/// 元フレーム」を使い、カーソル・クリック・キー表示・カメラもすべて時刻 t で評価する。
/// これにより実キャプチャが可変フレームレートでも、映像とイベントの時刻が揃う。
fn compose_frames(
    data: &RecordingData,
    settings: &AppSettings,
    style: OutputStyle,
    zoom_keyframes: &[ZoomKeyframe],
    fps: u32,
    progress: Option<&ProgressFn>,
) -> Result<tempfile::TempDir> {
    if data.frame_count() == 0 {
        return Err(anyhow::anyhow!("No frames found in recording"));
    }

    let events = preprocess(&data.events).events;

    // カーソル位置（座標は RecordingData でキャプチャ領域基準に変換済み）
    let raw_positions = extract_mouse_positions(&events);
    let cursor_positions = if settings.effects.cursor_smoothing && !raw_positions.is_empty() {
        CursorSmoother::new().smooth(&raw_positions)
    } else {
        raw_positions
    };
    let cursor_shapes = extract_cursor_shapes(&events);

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

    let mut compositor = Compositor::new(style, data.width(), data.height());
    compositor.set_motion_blur(settings.effects.motion_blur_enabled);

    let temp_dir = tempfile::TempDir::new()?;
    let composed_frames_dir = temp_dir.path().join("frames");
    std::fs::create_dir_all(&composed_frames_dir)?;

    let output_times = data.output_frame_times(fps);
    let total = output_times.len();
    let dt = 1.0 / fps.max(1) as f64;
    log::info!(
        "Composing {} output frames at {} fps from {} captured frames",
        total,
        fps,
        data.frame_count(),
    );

    let mut kf_index = 0;
    let mut output_frame_count: u64 = 0;
    // 同じ元フレームが連続して使われることが多いため、直前に読んだフレームを使い回す
    let mut current: Option<(u64, image::RgbaImage)> = None;

    for (k, &time_ms) in output_times.iter().enumerate() {
        // Apply any zoom keyframes that have been reached
        while kf_index < zoom_keyframes.len() && zoom_keyframes[kf_index].time_ms <= time_ms {
            compositor.apply_keyframe(&zoom_keyframes[kf_index]);
            kf_index += 1;
        }

        let frame_idx = data.frame_index_at(time_ms);
        if current.as_ref().map(|(idx, _)| *idx) != Some(frame_idx) {
            match image::open(data.frame_path(frame_idx)) {
                Ok(img) => current = Some((frame_idx, img.to_rgba8())),
                // 読めないフレームは直前のフレームで代用する
                Err(e) => log::warn!("Frame {} could not be loaded: {}", frame_idx, e),
            }
        }
        let Some((_, raw_frame)) = current.as_ref() else {
            continue;
        };

        let cursor_pos = find_cursor_at_time(&cursor_positions, time_ms);
        let cursor_kind = cursor_kind_at_time(&cursor_shapes, time_ms);
        let active_key = key_overlays.iter().rfind(|ko| ko.is_visible(time_ms));

        let composed = compositor.compose_frame(
            raw_frame,
            time_ms,
            cursor_pos,
            cursor_kind,
            &click_effects,
            active_key,
            dt,
        );

        // 合成済みフレームを JPEG q=95 で保存（アルファ非対応のため RGB に変換）。
        // シーケンス番号にギャップを作らないため output_frame_count を使う。
        let rgb_frame = image::DynamicImage::ImageRgba8(composed).to_rgb8();
        let output_path = composed_frames_dir.join(format!("frame_{:08}.jpg", output_frame_count));
        save_rgb_as_jpeg(&rgb_frame, &output_path, INTERMEDIATE_JPEG_QUALITY)?;
        output_frame_count += 1;

        if k % 10 == 0 {
            log::info!("Composing frame {}/{}", k, total);
            if let Some(cb) = progress {
                cb("composing", (k as f64 / total as f64) * 0.8);
            }
        }
    }

    log::info!("Composed {} frames", output_frame_count);
    Ok(temp_dir)
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

/// カーソル形状の変化 `(t, 種類)` を時刻順に取り出す。
fn extract_cursor_shapes(events: &[RecordingEvent]) -> Vec<(u64, CursorKind)> {
    events
        .iter()
        .filter_map(|e| match e {
            RecordingEvent::CursorShape { t, shape } => Some((*t, CursorKind::from_shape(shape))),
            _ => None,
        })
        .collect()
}

/// 時刻 `time_ms` のカーソル形状。記録が無い（旧形式の録画）・最初の記録より前は矢印。
fn cursor_kind_at_time(shapes: &[(u64, CursorKind)], time_ms: u64) -> CursorKind {
    let idx = shapes.partition_point(|&(t, _)| t <= time_ms);
    if idx == 0 {
        CursorKind::Arrow
    } else {
        shapes[idx - 1].1
    }
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

// --- Thumbnail generation ---

pub fn generate_thumbnail(recording_id: &str) -> Result<String> {
    use crate::recording_data::{frame_path, read_frame_count};

    let recording_dir = crate::recording_data::recording_dir(recording_id);
    let meta: crate::config::RecordingMeta =
        serde_json::from_str(&std::fs::read_to_string(recording_dir.join("meta.json"))?)?;

    let frames_dir = recording_dir.join("frames");
    let frame_count = read_frame_count(&recording_dir, &meta).max(1);
    let target_frame = (frame_count as f64 * 0.3) as u64;

    // Try target frame, then fallback to frame 0. frame_path が .jpg/.png を
    // 解決するので、旧録画（PNG 保存）と新録画（JPEG 保存）の両方に対応する。
    let target_path = frame_path(&frames_dir, target_frame);
    let frame_path = if target_path.exists() {
        target_path
    } else {
        let fallback = frame_path(&frames_dir, 0);
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

pub(crate) fn find_ffmpeg() -> Result<String> {
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

    // Input: 合成済みフレーム（プリセットの fps で等間隔に作成済み）
    cmd.args(["-y", "-framerate"])
        .arg(params.fps.to_string())
        .args(["-i"])
        .arg(
            frames_dir
                .join("frame_%08d.jpg")
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
) -> Result<()> {
    let palette_path = output.with_extension("palette.png");
    let width = params.canvas_width.min(640);

    // Pass 1: Generate palette
    Command::new(ffmpeg)
        .args(["-y", "-framerate"])
        .arg(params.fps.to_string())
        .args(["-i"])
        .arg(
            frames_dir
                .join("frame_%08d.jpg")
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
        .arg(params.fps.to_string())
        .args(["-i"])
        .arg(
            frames_dir
                .join("frame_%08d.jpg")
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
) -> Result<()> {
    let mut cmd = Command::new(ffmpeg);

    // Input: 合成済みフレーム（プリセットの fps で等間隔に作成済み）
    cmd.args(["-y", "-framerate"])
        .arg(params.fps.to_string())
        .args(["-i"])
        .arg(
            frames_dir
                .join("frame_%08d.jpg")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unique_output_path_avoids_overwrite() {
        let tmp = tempfile::TempDir::new().unwrap();
        let start = "2026-10-13T09:08:07+09:00";

        let first = unique_output_path(tmp.path(), start, &ExportFormat::Mp4);
        assert_eq!(first.file_name().unwrap(), "20261013_090807.mp4");
        std::fs::write(&first, b"").unwrap();

        let second = unique_output_path(tmp.path(), start, &ExportFormat::Mp4);
        assert_eq!(second.file_name().unwrap(), "20261013_090807_2.mp4");
        std::fs::write(&second, b"").unwrap();

        let third = unique_output_path(tmp.path(), start, &ExportFormat::Mp4);
        assert_eq!(third.file_name().unwrap(), "20261013_090807_3.mp4");

        // 形式が違えば別名にしない
        let gif = unique_output_path(tmp.path(), start, &ExportFormat::Gif);
        assert_eq!(gif.file_name().unwrap(), "20261013_090807.gif");
    }

    #[test]
    fn test_cursor_kind_at_time() {
        let shapes = vec![(100, CursorKind::IBeam), (500, CursorKind::Hand), (900, CursorKind::Arrow)];
        assert_eq!(cursor_kind_at_time(&shapes, 0), CursorKind::Arrow);
        assert_eq!(cursor_kind_at_time(&shapes, 100), CursorKind::IBeam);
        assert_eq!(cursor_kind_at_time(&shapes, 499), CursorKind::IBeam);
        assert_eq!(cursor_kind_at_time(&shapes, 600), CursorKind::Hand);
        assert_eq!(cursor_kind_at_time(&shapes, 1000), CursorKind::Arrow);
        // 形状の記録が無い旧録画は矢印
        assert_eq!(cursor_kind_at_time(&[], 300), CursorKind::Arrow);
    }
}
