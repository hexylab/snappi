use crate::camera::{self, CameraState, CameraTrack};
use crate::config::defaults::OutputStyle;
use crate::config::{AppSettings, ExportFormat, QualityPreset, RecordingEvent};
use crate::engine::compositor::{ClickEffect, Compositor, CursorKind, KeyOverlay};
use crate::engine::cursor_smoother::CursorSmoother;
use crate::export::presets::EncodingParams;
use crate::project::{Project, Trim};
use crate::recording_data::RecordingData;
use rayon::prelude::*;
use std::ops::Range;
use anyhow::Result;
use chrono::DateTime;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Progress callback: (stage, progress 0.0-1.0)
pub type ProgressFn = Box<dyn Fn(&str, f64) + Send>;

/// 書き出しを中止したときのエラー（`anyhow::Error::is::<Cancelled>()` で見分ける）
#[derive(Debug)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("書き出しを中止しました")
    }
}

impl std::error::Error for Cancelled {}

/// 一度に並列で合成するフレーム数。元フレームと合成済みフレームをこの数だけメモリに置く
const COMPOSE_BATCH: usize = 8;

/// GIF のフレームレート
const GIF_FPS: u32 = 15;

/// GIF の最大の幅（px）
const GIF_MAX_WIDTH: u32 = 640;

/// 合成が終わるまでの進捗の割合（残りは FFmpeg がエンコードを終えるまで）
const COMPOSE_PROGRESS: f64 = 0.95;

/// MP4・WebM の色の変換。RGB を動画で標準の BT.709・4:2:0 にし、色の情報をファイルに記録する。
/// 記録しないとプレーヤーによって色の解釈が変わり、4:2:0 にしないと VP9 が再生できない環境の多い
/// Profile 1 になる。
const VIDEO_COLOR_FILTER: &str = "scale=out_color_matrix=bt709:out_range=tv,format=yuv420p,\
     setparams=range=tv:colorspace=bt709:color_primaries=bt709:color_trc=bt709";

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

/// 録画を書き出す。ズーム区間とトリムは録画の `project.json`（プレビューの編集内容）を使い、
/// 無ければ設定のズーム強さで区間を自動生成して録画全体を書き出す。
/// 合成したフレームは一時ファイルを介さず FFmpeg の標準入力に流し、合成とエンコードを並行して進める。
/// `cancel` が立つと中止して `Cancelled` を返す（失敗・中止のときは途中までの動画ファイルを消す）。
pub fn export(
    recording_id: &str,
    format: &ExportFormat,
    quality: &QualityPreset,
    settings: &AppSettings,
    progress: Option<&ProgressFn>,
    cancel: &AtomicBool,
) -> Result<String> {
    let data = RecordingData::load_by_id(recording_id)?;

    log::info!("Starting effects composition for recording {}", recording_id);
    if let Some(cb) = progress { cb("analyzing", 0.0); }
    let strength = settings.effects.zoom_strength;
    let project = match Project::load(&data.dir)? {
        Some(project) => project,
        None => {
            log::info!("project.json が無いため自動生成した区間で書き出す");
            Project::auto(&data, strength)
        }
    };
    let config = project.camera_config(strength);
    let trim = project.trim.clamped(data.meta.duration_ms);
    log::info!("{} zoom segments, trim {}ms-{}ms", project.segments.len(), trim.start_ms, trim.end_ms);
    for seg in &project.segments {
        log::info!("  {}: {}ms-{}ms zoom={:?} focus={:?}", seg.id, seg.start_ms, seg.end_ms, seg.zoom, seg.focus);
    }

    let params = EncodingParams::from_preset(quality, data.width(), data.height());
    let style = OutputStyle::from_settings(&params, settings);
    // カメラは録画全体でシミュレーションしてからトリムの範囲を切り出す（トリムで動きが変わらないように）
    let track = camera::camera_track(&data, &project.segments, &config, params.fps);
    let frames = trimmed_frame_range(&track, trim);
    if frames.is_empty() {
        return Err(anyhow::anyhow!("書き出す範囲にフレームがありません"));
    }
    let audio_offset_ms = track.time_ms(frames.start);

    let output_dir = PathBuf::from(&settings.output.save_directory);
    std::fs::create_dir_all(&output_dir)?;
    let output_path = unique_output_path(&output_dir, &data.meta.start_time, format);

    let ffmpeg = find_ffmpeg()?;
    // GIF は GIF_FPS で書き出すので、その時刻のフレームだけを合成する。ただし動きのブレは
    // 直前のフレームに依るので、有効なら全フレームを合成して GIF のフレームだけを FFmpeg に渡す
    let (frames, gif_keep, input_fps): (Vec<usize>, Option<Vec<bool>>, u32) = match format {
        ExportFormat::Gif => {
            let picked = gif_frames(frames.clone(), params.fps);
            if settings.effects.motion_blur_enabled {
                let keep = frames.clone().map(|k| picked.binary_search(&k).is_ok()).collect();
                (frames.collect(), Some(keep), GIF_FPS)
            } else {
                (picked, None, GIF_FPS)
            }
        }
        _ => (frames.collect(), None, params.fps),
    };
    let job = EncodeJob {
        ffmpeg: &ffmpeg,
        input: RawVideoInput { width: style.canvas_width, height: style.canvas_height, fps: input_fps },
        params: &params,
        recording_dir: &data.dir,
        audio_offset_ms,
        output: &output_path,
        cancel,
    };

    if let Some(cb) = progress { cb("composing", 0.0); }
    let result = match format {
        ExportFormat::Mp4 | ExportFormat::WebM => {
            encode_video(&job, format, progress, |sink| compose_frames(&data, settings, style, &track, &frames, cancel, progress, sink))
        }
        ExportFormat::Gif => encode_gif(&job, progress, |sink| match &gif_keep {
            Some(keep) => {
                let mut index = 0;
                let mut keep_sink = |frame: &[u8]| {
                    index += 1;
                    if keep[index - 1] { sink(frame) } else { Ok(()) }
                };
                compose_frames(&data, settings, style, &track, &frames, cancel, progress, &mut keep_sink)
            }
            None => compose_frames(&data, settings, style, &track, &frames, cancel, progress, sink),
        }),
    };
    if let Err(e) = result {
        // 途中までできたファイルを残さない（書き出し先は新しく決めた名前なので既存のファイルではない）
        let _ = std::fs::remove_file(&output_path);
        return Err(e);
    }

    if let Some(cb) = progress { cb("complete", 1.0); }
    log::info!("Export complete: {}", output_path.display());
    Ok(output_path.to_string_lossy().to_string())
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

/// 合成でフレームに重ねるもの（カーソル・クリック・キー表示）の時系列。
/// 書き出しとプレビュー（`export::preview`）が同じものを使う。
pub struct EffectTimeline {
    /// カーソル位置 `(t, x, y)`。設定によりスムージング済み
    pub cursor_positions: Vec<(u64, f64, f64)>,
    /// カーソル形状の変化 `(t, 種類)`
    pub cursor_shapes: Vec<(u64, CursorKind)>,
    pub click_effects: Vec<ClickEffect>,
    pub key_overlays: Vec<KeyOverlay>,
}

impl EffectTimeline {
    pub fn new(data: &RecordingData, settings: &AppSettings, style: &OutputStyle) -> Self {
        let events = &data.events;

        // カーソル位置（座標は RecordingData でキャプチャ領域基準に変換済み）
        let raw_positions = extract_mouse_positions(events);
        let cursor_positions = if settings.effects.cursor_smoothing && !raw_positions.is_empty() {
            CursorSmoother::new().smooth(&raw_positions)
        } else {
            raw_positions
        };

        let click_effects = if settings.effects.click_ring_enabled {
            extract_click_effects(events, style.click_ring_duration_ms)
        } else {
            Vec::new()
        };
        let key_overlays = if settings.effects.key_badge_enabled {
            extract_key_overlays(events, style.key_badge_duration_ms)
        } else {
            Vec::new()
        };

        Self {
            cursor_positions,
            cursor_shapes: extract_cursor_shapes(events),
            click_effects,
            key_overlays,
        }
    }
}

/// トラックのうち、時刻がトリムの範囲 `[start_ms, end_ms)` に入るフレームの番号の範囲
fn trimmed_frame_range(track: &CameraTrack, trim: Trim) -> Range<usize> {
    let n = track.frames.len();
    let start = (0..n).find(|&k| track.time_ms(k) >= trim.start_ms).unwrap_or(n);
    let end = (start..n).find(|&k| track.time_ms(k) >= trim.end_ms).unwrap_or(n);
    start..end
}

/// GIF（`GIF_FPS`）の各フレームの時刻に最も近いトラックのフレーム（トラックは `track_fps`）
fn gif_frames(frames: Range<usize>, track_fps: u32) -> Vec<usize> {
    let n = frames.len();
    (0..)
        .map(|j: usize| (j as f64 * track_fps as f64 / GIF_FPS as f64).round() as usize)
        .take_while(|&k| k < n)
        .map(|k| frames.start + k)
        .collect()
}

/// 1 つの出力フレームの合成に使うもの
struct FrameJob {
    time_ms: u64,
    camera: CameraState,
    /// 使う元フレームの番号
    source: u64,
    cursor_pos: Option<(f64, f64)>,
    cursor_kind: CursorKind,
    /// 表示中のキー表示（`key_overlays` の添え字）
    key_overlay: Option<usize>,
    /// クリックの輪を描いている途中か
    clicking: bool,
}

impl FrameJob {
    /// 2 つのフレームが同じ絵になるか（同じ元フレーム・カメラ・カーソル・キー表示で、クリックの輪が無い）
    fn looks_same(&self, source: u64, other: &FrameJob, other_source: u64) -> bool {
        !self.clicking
            && !other.clicking
            && source == other_source
            && self.camera == other.camera
            && self.cursor_pos == other.cursor_pos
            && self.cursor_kind == other.cursor_kind
            && self.key_overlay == other.key_overlay
    }
}

/// エフェクトを合成した出力フレーム（RGB 24bit、キャンバスの大きさ）を時刻順に `sink` に渡し、渡した数を返す。
///
/// 出力はカメラトラックの fps で等間隔に作る。出力フレーム k（時刻 t）には「t 以前で最も新しい
/// 元フレーム」を使い、カーソル・クリック・キー表示も時刻 t で評価し、カメラはトラックの k 番目を使う。
/// これにより実キャプチャが可変フレームレートでも、映像とイベントの時刻が揃う。
/// 書き出すのはトラックのうち `frames` のフレーム（トリムの範囲。GIF では間引いたもの）だけ。
///
/// 速くするために次のようにする（どれも合成の結果は変えない）:
/// - `COMPOSE_BATCH` 個ずつ、元フレームの読み込みと合成を並列に行う（動きのブレが有効なときは
///   直前のフレームを使うので合成は順に行う）
/// - 直前のフレームと同じ絵になるフレームは合成せず、直前の結果を使い回す
/// - `sink` は別のスレッドで呼ぶので、FFmpeg への書き込み（エンコード）と合成が並行して進む
#[allow(clippy::too_many_arguments)]
fn compose_frames(
    data: &RecordingData,
    settings: &AppSettings,
    style: OutputStyle,
    track: &CameraTrack,
    frames: &[usize],
    cancel: &AtomicBool,
    progress: Option<&ProgressFn>,
    sink: &mut (dyn FnMut(&[u8]) -> Result<()> + Send),
) -> Result<u64> {
    if data.frame_count() == 0 {
        return Err(anyhow::anyhow!("No frames found in recording"));
    }

    let EffectTimeline { cursor_positions, cursor_shapes, click_effects, key_overlays } =
        EffectTimeline::new(data, settings, &style);

    let mut compositor = Compositor::new(style, data.width(), data.height());
    compositor.set_motion_blur(settings.effects.motion_blur_enabled);
    let sequential = compositor.motion_blur_enabled();

    let jobs: Vec<FrameJob> = frames
        .iter()
        .map(|&k| {
            let time_ms = track.time_ms(k);
            FrameJob {
                time_ms,
                camera: track.frames[k],
                source: data.frame_index_at(time_ms),
                cursor_pos: find_cursor_at_time(&cursor_positions, time_ms),
                cursor_kind: cursor_kind_at_time(&cursor_shapes, time_ms),
                key_overlay: key_overlays.iter().rposition(|ko| ko.is_visible(time_ms)),
                clicking: click_effects.iter().any(|e| e.is_active(time_ms)),
            }
        })
        .collect();

    let total = jobs.len();
    log::info!(
        "Composing {} output frames at {} fps from {} captured frames",
        total,
        track.fps,
        data.frame_count(),
    );

    std::thread::scope(|scope| {
        let (tx, rx) = std::sync::mpsc::sync_channel::<Arc<Vec<u8>>>(COMPOSE_BATCH);
        let writer = scope.spawn(move || -> Result<()> {
            for frame in rx {
                sink(&frame)?;
            }
            Ok(())
        });

        let produced = (|| -> Result<u64> {
            let mut written: u64 = 0;
            // 直前に読めた元フレーム（読めない元フレームはこれで代用する）
            let mut current: Option<(u64, Arc<image::RgbaImage>)> = None;
            // 直前に出したフレーム（`jobs` の添え字と元フレームの番号）とその絵
            let mut last_job: Option<(usize, u64)> = None;
            let mut last_frame: Option<Arc<Vec<u8>>> = None;

            for (batch_index, batch) in jobs.chunks(COMPOSE_BATCH).enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    return Err(Cancelled.into());
                }
                let batch_start = batch_index * COMPOSE_BATCH;

                // (1) このバッチで新しく使う元フレームを並列に読む
                let mut sources: Vec<u64> = batch
                    .iter()
                    .map(|job| job.source)
                    .filter(|&s| current.as_ref().map(|(idx, _)| *idx) != Some(s))
                    .collect();
                sources.dedup();
                let loaded: Vec<(u64, Option<Arc<image::RgbaImage>>)> = sources
                    .into_par_iter()
                    .map(|idx| match image::open(data.frame_path(idx)) {
                        Ok(img) => (idx, Some(Arc::new(img.to_rgba8()))),
                        Err(e) => {
                            log::warn!("Frame {} could not be loaded: {}", idx, e);
                            (idx, None)
                        }
                    })
                    .collect();

                // (2) 各フレームの元フレームを決め、合成するか直前の絵を使い回すかを決める
                enum Plan {
                    Render(usize, Arc<image::RgbaImage>),
                    Repeat,
                }
                let mut plans: Vec<Plan> = Vec::with_capacity(batch.len());
                for (i, job) in batch.iter().enumerate() {
                    if current.as_ref().map(|(idx, _)| *idx) != Some(job.source) {
                        if let Some((_, Some(img))) = loaded.iter().find(|(idx, _)| *idx == job.source) {
                            current = Some((job.source, Arc::clone(img)));
                        }
                    }
                    let Some((source, image)) = current.as_ref() else {
                        continue;
                    };
                    let repeat = !sequential
                        && last_job.is_some_and(|(prev, prev_source)| job.looks_same(*source, &jobs[prev], prev_source));
                    plans.push(if repeat { Plan::Repeat } else { Plan::Render(i, Arc::clone(image)) });
                    last_job = Some((batch_start + i, *source));
                }

                // (3) 合成する
                let rendered: Vec<Arc<Vec<u8>>> = if sequential {
                    plans
                        .iter()
                        .filter_map(|plan| match plan {
                            Plan::Render(i, image) => {
                                let job = &batch[*i];
                                let canvas = compositor.compose_frame(
                                    image, job.time_ms, job.camera, job.cursor_pos, job.cursor_kind,
                                    &click_effects, job.key_overlay.map(|k| &key_overlays[k]),
                                );
                                Some(Arc::new(rgba_to_rgb(canvas)))
                            }
                            Plan::Repeat => None,
                        })
                        .collect()
                } else {
                    let compositor = &compositor;
                    plans
                        .par_iter()
                        .filter_map(|plan| match plan {
                            Plan::Render(i, image) => {
                                let job = &batch[*i];
                                let canvas = compositor.render(
                                    image, job.time_ms, job.camera, job.cursor_pos, job.cursor_kind,
                                    &click_effects, job.key_overlay.map(|k| &key_overlays[k]),
                                );
                                Some(Arc::new(rgba_to_rgb(canvas)))
                            }
                            Plan::Repeat => None,
                        })
                        .collect()
                };

                // (4) 時刻順に書き出しのスレッドへ渡す
                let mut rendered = rendered.into_iter();
                for plan in &plans {
                    let frame = match plan {
                        Plan::Render(..) => rendered.next().expect("合成した数と Render の数は同じ"),
                        Plan::Repeat => Arc::clone(last_frame.as_ref().expect("Repeat の前には必ずフレームがある")),
                    };
                    last_frame = Some(Arc::clone(&frame));
                    if tx.send(frame).is_err() {
                        // 書き出しのスレッドが止まった（FFmpeg が終了した）。エラーはそちらで返す
                        return Ok(written);
                    }
                    written += 1;
                }

                let done = (batch_start + batch.len()).min(total);
                log::info!("Composing frame {}/{}", done, total);
                if let Some(cb) = progress {
                    cb("composing", done as f64 / total as f64 * COMPOSE_PROGRESS);
                }
            }
            Ok(written)
        })();

        drop(tx);
        let written = writer.join().map_err(|_| anyhow::anyhow!("書き出しのスレッドが異常終了しました"))?;
        // 書き出し側のエラー（FFmpeg が途中で終了した等）を優先して返す
        written?;
        let count = produced?;
        log::info!("Composed {} frames", count);
        Ok(count)
    })
}

/// RGBA の画像からアルファを除いた RGB 24bit の画素列
fn rgba_to_rgb(img: image::RgbaImage) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(img.width() as usize * img.height() as usize * 3);
    for px in img.as_raw().chunks_exact(4) {
        rgb.extend_from_slice(&px[..3]);
    }
    rgb
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

/// 時刻 `time_ms` のカーソル形状。最初の記録より前（記録が無い場合も）は矢印。
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
    let data = RecordingData::load_by_id(recording_id)?;
    let recording_dir = &data.dir;
    let target_frame = (data.frame_count().max(1) as f64 * 0.3) as u64;

    // Try target frame, then fallback to frame 0.
    let target_path = data.frame_path(target_frame);
    let frame_path = if target_path.exists() {
        target_path
    } else {
        let fallback = data.frame_path(0);
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

// --- FFmpeg encoding (合成済みフレームを標準入力で受け取る。拡大縮小はしない) ---

/// 標準入力から渡す映像（RGB 24bit の生の画素）の形式
struct RawVideoInput {
    width: u32,
    height: u32,
    fps: u32,
}

impl RawVideoInput {
    /// FFmpeg の入力オプション（標準入力から読む）
    fn add_input(&self, cmd: &mut Command) {
        cmd.args(["-f", "rawvideo", "-pix_fmt", "rgb24", "-s"])
            .arg(format!("{}x{}", self.width, self.height))
            .arg("-framerate")
            .arg(self.fps.to_string())
            .args(["-i", "-"]);
    }
}

/// エンコードに使うもの（形式によらず共通）
struct EncodeJob<'a> {
    ffmpeg: &'a str,
    input: RawVideoInput,
    params: &'a EncodingParams,
    recording_dir: &'a Path,
    audio_offset_ms: u64,
    output: &'a Path,
    cancel: &'a AtomicBool,
}

/// フレームを標準入力で受け取って動いている FFmpeg のプロセス
struct FfmpegProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    /// 標準エラーを読み続けるスレッド（読まないとパイプが詰まって FFmpeg が止まる）
    stderr: Option<std::thread::JoinHandle<String>>,
    what: &'static str,
}

impl FfmpegProcess {
    fn spawn(mut cmd: Command, what: &'static str) -> Result<Self> {
        log::info!("FFmpeg {} command: {:?}", what, cmd);
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdin = child.stdin.take();
        let stderr = child.stderr.take().map(|mut pipe| {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = pipe.read_to_end(&mut buf);
                String::from_utf8_lossy(&buf).into_owned()
            })
        });
        Ok(Self { child, stdin, stderr, what })
    }

    /// 標準入力に 1 フレームを書く
    fn write_frame(&mut self, frame: &[u8]) -> Result<()> {
        let stdin = self.stdin.as_mut().ok_or_else(|| anyhow::anyhow!("FFmpeg の入力が閉じています"))?;
        stdin.write_all(frame)?;
        Ok(())
    }

    fn stderr_text(&mut self) -> String {
        self.stderr.take().and_then(|h| h.join().ok()).unwrap_or_default()
    }

    /// 入力を閉じて終了を待つ。待っている間に中止されたらプロセスを終わらせて `Cancelled` を返す
    fn finish(mut self, cancel: &AtomicBool) -> Result<()> {
        drop(self.stdin.take());
        let status = loop {
            if cancel.load(Ordering::Relaxed) {
                self.kill();
                return Err(Cancelled.into());
            }
            match self.child.try_wait()? {
                Some(status) => break status,
                None => std::thread::sleep(std::time::Duration::from_millis(50)),
            }
        };
        let stderr = self.stderr_text();
        if !status.success() {
            return Err(anyhow::anyhow!("FFmpeg {} encoding failed: {}", self.what, stderr));
        }
        Ok(())
    }

    /// フレームを渡している途中で失敗・中止したときにプロセスを終わらせ、元のエラーを FFmpeg の出力と合わせて返す
    fn abort(mut self, error: anyhow::Error) -> anyhow::Error {
        self.kill();
        if error.is::<Cancelled>() {
            return error;
        }
        let stderr = self.stderr_text();
        if error.is::<std::io::Error>() && !stderr.is_empty() {
            // 書き込みに失敗した = FFmpeg が先に終了した。原因は FFmpeg の出力にある
            anyhow::anyhow!("FFmpeg {} encoding failed: {}", self.what, stderr)
        } else {
            error
        }
    }

    fn kill(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `compose` に FFmpeg の標準入力へ書き込む `sink` を渡してフレームを流し込み、エンコードの終了まで待つ
fn run_ffmpeg<F>(
    cmd: Command,
    what: &'static str,
    cancel: &AtomicBool,
    progress: Option<&ProgressFn>,
    compose: F,
) -> Result<()>
where
    F: FnOnce(&mut (dyn FnMut(&[u8]) -> Result<()> + Send)) -> Result<u64>,
{
    let mut process = FfmpegProcess::spawn(cmd, what)?;
    let composed = compose(&mut |frame: &[u8]| process.write_frame(frame));
    match composed {
        Ok(0) => Err(process.abort(anyhow::anyhow!("書き出すフレームがありません"))),
        Ok(_) => {
            if let Some(cb) = progress {
                cb("encoding", COMPOSE_PROGRESS);
            }
            process.finish(cancel)
        }
        Err(e) => Err(process.abort(e)),
    }
}

/// 録画の音声を入力に加える。トリムに合わせて `offset_ms` から読む（`-ss` は入力オプション
/// なので `-i` の前に置く）。音声が無い・空なら何もせず false を返す。
fn add_audio_input(cmd: &mut Command, recording_dir: &Path, offset_ms: u64) -> bool {
    let audio_path = recording_dir.join("audio.wav");
    let has_audio = std::fs::metadata(&audio_path)
        .map(|m| m.len() > 44)
        .unwrap_or(false);
    if has_audio {
        if offset_ms > 0 {
            cmd.arg("-ss").arg(format!("{:.3}", offset_ms as f64 / 1000.0));
        }
        cmd.arg("-i").arg(audio_path.to_string_lossy().to_string());
    }
    has_audio
}

/// MP4（H.264）または WebM（VP9）にエンコードする
fn encode_video<F>(job: &EncodeJob, format: &ExportFormat, progress: Option<&ProgressFn>, compose: F) -> Result<()>
where
    F: FnOnce(&mut (dyn FnMut(&[u8]) -> Result<()> + Send)) -> Result<u64>,
{
    let mut cmd = Command::new(job.ffmpeg);
    cmd.arg("-y");
    // Input: 合成済みフレーム（プリセットの fps で等間隔に作成済み）
    job.input.add_input(&mut cmd);
    let has_audio = add_audio_input(&mut cmd, job.recording_dir, job.audio_offset_ms);

    // Output options (composed frames are already at final canvas resolution)
    cmd.args(["-vf", VIDEO_COLOR_FILTER]);
    let what = match format {
        ExportFormat::WebM => {
            // VP9 は既定では 1 スレッドに近く遅いので、行単位のマルチスレッドと速度優先の設定にする
            cmd.args(["-c:v", "libvpx-vp9"])
                .args(["-crf"])
                .arg(job.params.crf.to_string())
                .args(["-b:v", "0"])
                .args(["-row-mt", "1", "-deadline", "good", "-cpu-used", "4"]);
            "WebM"
        }
        _ => {
            cmd.args(["-c:v", "libx264"])
                .args(["-crf"])
                .arg(job.params.crf.to_string())
                .args(["-preset", "medium"])
                .args(["-pix_fmt", "yuv420p"])
                .args(["-movflags", "+faststart"]);
            "MP4"
        }
    };
    cmd.args(["-r"]).arg(job.params.fps.to_string());

    if has_audio {
        match format {
            ExportFormat::WebM => cmd.args(["-c:a", "libopus", "-shortest"]),
            _ => cmd.args(["-c:a", "aac", "-b:a", "128k", "-shortest"]),
        };
    }

    cmd.arg(job.output.to_string_lossy().to_string());
    run_ffmpeg(cmd, what, job.cancel, progress, compose)
}

/// GIF にエンコードする。パレットを作るには全フレームが要るので、1 回目で縮小したフレームを
/// 可逆圧縮（FFV1）で一時ファイルに保存しながらパレットを作り、2 回目でパレットを使って GIF にする。
fn encode_gif<F>(job: &EncodeJob, progress: Option<&ProgressFn>, compose: F) -> Result<()>
where
    F: FnOnce(&mut (dyn FnMut(&[u8]) -> Result<()> + Send)) -> Result<u64>,
{
    let temp_dir = tempfile::TempDir::new()?;
    let scaled_path = temp_dir.path().join("scaled.mkv");
    let palette_path = temp_dir.path().join("palette.png");
    let width = job.params.canvas_width.min(GIF_MAX_WIDTH);

    // Pass 1: 縮小したフレームの保存とパレットの生成
    let mut cmd = Command::new(job.ffmpeg);
    cmd.arg("-y");
    job.input.add_input(&mut cmd);
    cmd.args(["-filter_complex"])
        .arg(format!("[0:v]scale={}:-1:flags=lanczos,split[v][p];[p]palettegen[pal]", width))
        .args(["-map", "[v]", "-c:v", "ffv1"])
        .arg(scaled_path.to_string_lossy().to_string())
        .args(["-map", "[pal]"])
        .arg(palette_path.to_string_lossy().to_string());
    run_ffmpeg(cmd, "GIF", job.cancel, progress, compose)?;

    // Pass 2: パレットを使って GIF にする
    let mut cmd = Command::new(job.ffmpeg);
    cmd.arg("-y")
        .arg("-i")
        .arg(scaled_path.to_string_lossy().to_string())
        .arg("-i")
        .arg(palette_path.to_string_lossy().to_string())
        .args(["-lavfi", "[0:v][1:v]paletteuse"])
        .arg(job.output.to_string_lossy().to_string());
    FfmpegProcess::spawn(cmd, "GIF")?.finish(job.cancel)
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
    fn test_trimmed_frame_range() {
        let track = CameraTrack {
            fps: 10,
            frames: vec![crate::camera::CameraState::overview(10.0, 10.0); 20], // 0, 100, ..., 1900ms
        };
        assert_eq!(trimmed_frame_range(&track, Trim { start_ms: 0, end_ms: 2000 }), 0..20);
        assert_eq!(trimmed_frame_range(&track, Trim { start_ms: 250, end_ms: 1000 }), 3..10);
        assert_eq!(trimmed_frame_range(&track, Trim { start_ms: 300, end_ms: 301 }), 3..4);
        assert!(trimmed_frame_range(&track, Trim { start_ms: 1950, end_ms: 2000 }).is_empty());
    }

    #[test]
    fn test_cursor_kind_at_time() {
        let shapes = vec![(100, CursorKind::IBeam), (500, CursorKind::Hand), (900, CursorKind::Arrow)];
        assert_eq!(cursor_kind_at_time(&shapes, 0), CursorKind::Arrow);
        assert_eq!(cursor_kind_at_time(&shapes, 100), CursorKind::IBeam);
        assert_eq!(cursor_kind_at_time(&shapes, 499), CursorKind::IBeam);
        assert_eq!(cursor_kind_at_time(&shapes, 600), CursorKind::Hand);
        assert_eq!(cursor_kind_at_time(&shapes, 1000), CursorKind::Arrow);
        // 形状の記録が無ければ矢印
        assert_eq!(cursor_kind_at_time(&[], 300), CursorKind::Arrow);
    }

    #[test]
    fn test_gif_frames() {
        // 30fps → 1 つおき、60fps → 3 つおき（トリムの開始からの位置で数える）
        assert_eq!(gif_frames(10..20, 30), vec![10, 12, 14, 16, 18]);
        assert_eq!(gif_frames(0..10, 60), vec![0, 4, 8]);
        // 24fps → GIF の各時刻に最も近いフレーム（0, 1.6, 3.2, 4.8, 6.4 → 0, 2, 3, 5, 6）
        assert_eq!(gif_frames(0..7, 24), vec![0, 2, 3, 5, 6]);
        assert!(gif_frames(5..5, 30).is_empty());
    }

    #[test]
    fn test_rgba_to_rgb_drops_alpha() {
        let img = image::RgbaImage::from_raw(2, 1, vec![1, 2, 3, 4, 5, 6, 7, 0]).unwrap();
        assert_eq!(rgba_to_rgb(img), vec![1, 2, 3, 5, 6, 7]);
    }

    #[test]
    fn test_frame_job_looks_same() {
        let job = |camera_x: f64, clicking: bool| FrameJob {
            time_ms: 0,
            camera: CameraState { center_x: camera_x, center_y: 50.0, zoom: 1.0 },
            source: 0,
            cursor_pos: Some((10.0, 10.0)),
            cursor_kind: CursorKind::Arrow,
            key_overlay: None,
            clicking,
        };
        assert!(job(50.0, false).looks_same(3, &job(50.0, false), 3));
        // 元フレーム・カメラが違う、クリックの輪を描いている途中なら別の絵
        assert!(!job(50.0, false).looks_same(3, &job(50.0, false), 4));
        assert!(!job(50.0, false).looks_same(3, &job(51.0, false), 3));
        assert!(!job(50.0, true).looks_same(3, &job(50.0, false), 3));
        assert!(!job(50.0, false).looks_same(3, &job(50.0, true), 3));
    }

    /// 単色の RGB フレームを `count` 枚 `sink` に渡す（FFmpeg のテスト用）
    fn solid_frames(input: &RawVideoInput, count: u64, sink: &mut (dyn FnMut(&[u8]) -> Result<()> + Send)) -> Result<u64> {
        let frame = vec![128u8; (input.width * input.height * 3) as usize];
        for _ in 0..count {
            sink(&frame)?;
        }
        Ok(count)
    }

    /// テストに使う FFmpeg（見つからない環境ではテストを飛ばす）
    fn ffmpeg_for_test() -> Option<String> {
        let bundled = Path::new(env!("CARGO_MANIFEST_DIR")).join("ffmpeg").join("ffmpeg.exe");
        let ffmpeg = if bundled.exists() { Some(bundled.to_string_lossy().to_string()) } else { find_ffmpeg().ok() };
        if ffmpeg.is_none() {
            eprintln!("FFmpeg が見つからないためスキップ");
        }
        ffmpeg
    }

    /// 64x36 の録画を「軽量」で書き出すときの FFmpeg の入力
    fn test_input(params: &EncodingParams, fps: u32) -> RawVideoInput {
        RawVideoInput { width: params.canvas_width, height: params.canvas_height, fps }
    }

    #[test]
    fn test_encode_from_pipe() {
        let Some(ffmpeg) = ffmpeg_for_test() else { return };
        let tmp = tempfile::TempDir::new().unwrap();
        let params = EncodingParams::from_preset(&QualityPreset::Lightweight, 64, 36);
        let cancel = AtomicBool::new(false);
        for format in [ExportFormat::Mp4, ExportFormat::WebM, ExportFormat::Gif] {
            let output = tmp.path().join(format!("out_{:?}", format));
            let fps = if format == ExportFormat::Gif { GIF_FPS } else { params.fps };
            let output = output.with_extension(match format {
                ExportFormat::Mp4 => "mp4",
                ExportFormat::WebM => "webm",
                ExportFormat::Gif => "gif",
            });
            let job = EncodeJob {
                ffmpeg: &ffmpeg,
                input: test_input(&params, fps),
                params: &params,
                recording_dir: tmp.path(),
                audio_offset_ms: 0,
                output: &output,
                cancel: &cancel,
            };
            let compose = |sink: &mut (dyn FnMut(&[u8]) -> Result<()> + Send)| {
                solid_frames(&test_input(&params, fps), 12, sink)
            };
            match format {
                ExportFormat::Gif => encode_gif(&job, None, compose).unwrap(),
                _ => encode_video(&job, &format, None, compose).unwrap(),
            }
            assert!(std::fs::metadata(&output).unwrap().len() > 0, "{:?} が書き出されていない", format);
            if format != ExportFormat::Gif {
                // 多くの環境で再生できる 4:2:0 で、BT.709 の色の情報が記録されている
                let probe = Command::new(&ffmpeg).arg("-i").arg(&output).output().unwrap();
                let info = String::from_utf8_lossy(&probe.stderr);
                assert!(info.contains("yuv420p(tv, bt709"), "{:?}: {}", format, info);
            }
        }
    }

    #[test]
    fn test_encode_cancelled_while_composing() {
        let Some(ffmpeg) = ffmpeg_for_test() else { return };
        let tmp = tempfile::TempDir::new().unwrap();
        let output = tmp.path().join("out.mp4");
        let params = EncodingParams::from_preset(&QualityPreset::Lightweight, 64, 36);
        let cancel = AtomicBool::new(false);
        let job = EncodeJob {
            ffmpeg: &ffmpeg,
            input: test_input(&params, params.fps),
            params: &params,
            recording_dir: tmp.path(),
            audio_offset_ms: 0,
            output: &output,
            cancel: &cancel,
        };
        let err = encode_video(&job, &ExportFormat::Mp4, None, |sink| {
            solid_frames(&test_input(&params, params.fps), 3, sink)?;
            Err(Cancelled.into())
        })
        .unwrap_err();
        assert!(err.is::<Cancelled>(), "{}", err);
    }

    #[test]
    fn test_encode_reports_ffmpeg_error() {
        let Some(ffmpeg) = ffmpeg_for_test() else { return };
        let tmp = tempfile::TempDir::new().unwrap();
        // 存在しないフォルダには書き出せない → FFmpeg のエラーとして返る
        let output = tmp.path().join("no-such-dir").join("out.mp4");
        let params = EncodingParams::from_preset(&QualityPreset::Lightweight, 64, 36);
        let cancel = AtomicBool::new(false);
        let job = EncodeJob {
            ffmpeg: &ffmpeg,
            input: test_input(&params, params.fps),
            params: &params,
            recording_dir: tmp.path(),
            audio_offset_ms: 0,
            output: &output,
            cancel: &cancel,
        };
        let err = encode_video(&job, &ExportFormat::Mp4, None, |sink| {
            solid_frames(&test_input(&params, params.fps), 300, sink)
        })
        .unwrap_err();
        assert!(err.to_string().contains("FFmpeg MP4 encoding failed"), "{}", err);
    }
}
