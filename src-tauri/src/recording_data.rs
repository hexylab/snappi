//! 録画データローダー (Issue #27)。
//!
//! 録画ディレクトリの meta.json・events.jsonl・ui_events.jsonl・frame_timestamps.txt・
//! window_rects.jsonl を1か所で読み込み、エクスポートや解析が使う形に正規化する。
//!
//! - イベントの座標は**キャプチャ領域基準（フレーム左上が原点）**に一度だけ変換して保持する。
//!   ウィンドウ録画では各イベント時刻のウィンドウ位置を使うため、録画中にウィンドウを
//!   動かしても座標がずれない。
//! - フレーム時刻は frame_timestamps.txt の実測値を使い、無い旧形式の録画では等間隔とみなす。

use crate::recording::capture::{FRAME_TIMESTAMPS_FILE, WINDOW_RECTS_FILE};
use crate::config::{RecordingEvent, RecordingMeta};
use anyhow::Result;
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// 録画 ID から録画ディレクトリのパスを返す。
pub fn recording_dir(recording_id: &str) -> PathBuf {
    dirs::video_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Snappi")
        .join("recordings")
        .join(recording_id)
}

/// 録画フレームのパスを解決する。新しい録画は `.jpg`、旧録画は `.png` で保存されている
/// ため両方をチェックする。ファイルが見つからない場合は `.jpg` のパスを返す（呼び出し側が
/// エラーを出す想定）。
pub fn frame_path(frames_dir: &Path, idx: u64) -> PathBuf {
    let jpg = frames_dir.join(format!("frame_{:08}.jpg", idx));
    if jpg.exists() {
        return jpg;
    }
    let png = frames_dir.join(format!("frame_{:08}.png", idx));
    if png.exists() {
        return png;
    }
    jpg
}

/// 録画フレーム数。meta.json の frame_count を優先し、旧録画は frame_count.txt にフォールバックする。
pub fn read_frame_count(dir: &Path, meta: &RecordingMeta) -> u64 {
    if let Some(fc) = meta.frame_count {
        return fc as u64;
    }
    std::fs::read_to_string(dir.join("frame_count.txt"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// window_rects.jsonl の1行
#[derive(Debug, Clone, Deserialize)]
struct WindowRectSample {
    t: u64,
    rect: [f64; 4],
}

/// キャプチャ領域の左上座標（画面座標）の時系列 `(t, x, y)`。時刻順。
/// 空の場合は原点 (0, 0)（ディスプレイ録画）を意味する。
type CaptureOrigins = Vec<(u64, f64, f64)>;

pub struct RecordingData {
    pub dir: PathBuf,
    pub meta: RecordingMeta,
    /// events.jsonl と ui_events.jsonl のイベント。時刻順、座標はキャプチャ領域基準。
    pub events: Vec<RecordingEvent>,
    /// 各フレームの録画時刻 (ms)。要素数はフレーム数と一致する。
    pub frame_times: Vec<u64>,
}

impl RecordingData {
    pub fn load_by_id(recording_id: &str) -> Result<Self> {
        Self::load(&recording_dir(recording_id))
    }

    pub fn load(dir: &Path) -> Result<Self> {
        let meta: RecordingMeta =
            serde_json::from_str(&std::fs::read_to_string(dir.join("meta.json"))?)?;

        let frame_count = read_frame_count(dir, &meta);
        let frame_times = match read_frame_timestamps(dir) {
            Some(ts) if ts.len() as u64 >= frame_count => ts[..frame_count as usize].to_vec(),
            _ => {
                log::info!("frame_timestamps.txt が無い（または不足）ため等間隔のフレーム時刻を使用");
                evenly_spaced_frame_times(frame_count, meta.duration_ms)
            }
        };

        let mut events = read_jsonl::<RecordingEvent>(&dir.join("events.jsonl"));
        events.extend(read_jsonl::<RecordingEvent>(&dir.join("ui_events.jsonl")));
        events.sort_by_key(crate::engine::analyzer::event_timestamp);

        let origins = capture_origins(dir, &meta);
        to_capture_coords(&mut events, &origins);

        Ok(Self {
            dir: dir.to_path_buf(),
            meta,
            events,
            frame_times,
        })
    }

    pub fn width(&self) -> u32 {
        self.meta.screen_width
    }

    pub fn height(&self) -> u32 {
        self.meta.screen_height
    }

    pub fn frame_count(&self) -> u64 {
        self.frame_times.len() as u64
    }

    pub fn frames_dir(&self) -> PathBuf {
        self.dir.join("frames")
    }

    pub fn frame_path(&self, idx: u64) -> PathBuf {
        frame_path(&self.frames_dir(), idx)
    }

    /// 時刻 `t_ms` 以前で最も新しいフレームの番号。最初のフレームより前なら 0。
    pub fn frame_index_at(&self, t_ms: u64) -> u64 {
        frame_index_at(&self.frame_times, t_ms)
    }

    /// 出力フレームの時刻列 (ms)。`fps` で等間隔に録画の長さ分を並べる。
    pub fn output_frame_times(&self, fps: u32) -> Vec<u64> {
        let end_ms = self
            .meta
            .duration_ms
            .max(self.frame_times.last().copied().unwrap_or(0));
        output_frame_times(end_ms, fps)
    }
}

fn read_jsonl<T: serde::de::DeserializeOwned>(path: &Path) -> Vec<T> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// frame_timestamps.txt を読む。ファイルが無い・壊れている場合は None。
fn read_frame_timestamps(dir: &Path) -> Option<Vec<u64>> {
    let content = std::fs::read_to_string(dir.join(FRAME_TIMESTAMPS_FILE)).ok()?;
    content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| l.parse::<u64>().ok())
        .collect()
}

/// 旧形式の録画用: 録画時間をフレーム数で等分したフレーム時刻。
fn evenly_spaced_frame_times(frame_count: u64, duration_ms: u64) -> Vec<u64> {
    if frame_count == 0 {
        return Vec::new();
    }
    (0..frame_count)
        .map(|i| i * duration_ms / frame_count)
        .collect()
}

fn frame_index_at(frame_times: &[u64], t_ms: u64) -> u64 {
    frame_times.partition_point(|&ft| ft <= t_ms).saturating_sub(1) as u64
}

fn output_frame_times(end_ms: u64, fps: u32) -> Vec<u64> {
    let fps = fps.max(1) as u64;
    let count = ((end_ms * fps + 500) / 1000).max(1);
    (0..count).map(|k| (k * 1000 + fps / 2) / fps).collect()
}

/// 録画モードに応じたキャプチャ領域の左上座標の時系列を求める。
fn capture_origins(dir: &Path, meta: &RecordingMeta) -> CaptureOrigins {
    let initial = meta.window_initial_rect.map(|r| vec![(0, r[0], r[1])]);
    match meta.recording_mode.as_deref() {
        Some("window") => {
            let history: CaptureOrigins = read_jsonl::<WindowRectSample>(&dir.join(WINDOW_RECTS_FILE))
                .into_iter()
                .map(|s| (s.t, s.rect[0], s.rect[1]))
                .collect();
            if history.is_empty() {
                // window_rects.jsonl が無い旧形式は録画開始時の位置で固定
                initial.unwrap_or_default()
            } else {
                history
            }
        }
        Some("area") => initial.unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// 時刻 `t` のキャプチャ領域左上座標。最初の記録より前は最初の値を使う。
fn origin_at(origins: &CaptureOrigins, t: u64) -> (f64, f64) {
    let idx = origins.partition_point(|&(ot, _, _)| ot <= t).saturating_sub(1);
    origins.get(idx).map(|&(_, x, y)| (x, y)).unwrap_or((0.0, 0.0))
}

/// 画面座標のイベントをキャプチャ領域基準の座標に変換する。
fn to_capture_coords(events: &mut [RecordingEvent], origins: &CaptureOrigins) {
    if origins.is_empty() {
        return;
    }
    for event in events.iter_mut() {
        let (ox, oy) = origin_at(origins, crate::engine::analyzer::event_timestamp(event));
        translate_event(event, -ox, -oy);
    }
}

fn translate_event(event: &mut RecordingEvent, dx: f64, dy: f64) {
    let translate_rect = |rect: &mut [f64; 4]| {
        rect[0] += dx;
        rect[1] += dy;
        rect[2] += dx;
        rect[3] += dy;
    };
    match event {
        RecordingEvent::MouseMove { x, y, .. }
        | RecordingEvent::Click { x, y, .. }
        | RecordingEvent::ClickRelease { x, y, .. }
        | RecordingEvent::Scroll { x, y, .. } => {
            *x += dx;
            *y += dy;
        }
        RecordingEvent::Focus { rect, .. }
        | RecordingEvent::WindowFocus { rect, .. }
        | RecordingEvent::UiFocus { rect, .. }
        | RecordingEvent::UiMenuOpen { rect, .. }
        | RecordingEvent::UiDialogOpen { rect, .. } => translate_rect(rect),
        RecordingEvent::Key { .. }
        | RecordingEvent::UiMenuClose { .. }
        | RecordingEvent::UiDialogClose { .. }
        | RecordingEvent::CursorShape { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_meta(mode: &str, rect: Option<[f64; 4]>) -> RecordingMeta {
        RecordingMeta {
            version: 2,
            id: "test".into(),
            screen_width: 800,
            screen_height: 600,
            fps: 30,
            start_time: "2026-01-01T00:00:00+09:00".into(),
            duration_ms: 1000,
            has_audio: false,
            monitor_scale: 1.0,
            recording_dir: String::new(),
            recording_mode: Some(mode.into()),
            window_title: None,
            window_initial_rect: rect,
            frame_count: Some(4),
        }
    }

    fn write_recording(dir: &Path, meta: &RecordingMeta, events: &str) {
        std::fs::write(dir.join("meta.json"), serde_json::to_string(meta).unwrap()).unwrap();
        std::fs::write(dir.join("events.jsonl"), events).unwrap();
    }

    fn click_pos(e: &RecordingEvent) -> (f64, f64) {
        match e {
            RecordingEvent::Click { x, y, .. } => (*x, *y),
            _ => panic!("click ではない: {:?}", e),
        }
    }

    #[test]
    fn test_window_mode_uses_rect_at_event_time() {
        let tmp = tempfile::TempDir::new().unwrap();
        let meta = test_meta("window", Some([100.0, 50.0, 900.0, 650.0]));
        write_recording(
            tmp.path(),
            &meta,
            concat!(
                r#"{"type":"click","t":100,"btn":"left","x":150,"y":80}"#, "\n",
                r#"{"type":"click","t":600,"btn":"left","x":450,"y":280}"#, "\n",
            ),
        );
        // t=500 でウィンドウが (400, 250) に移動
        std::fs::write(
            tmp.path().join(WINDOW_RECTS_FILE),
            concat!(
                r#"{"t":0,"rect":[100,50,900,650]}"#, "\n",
                r#"{"t":500,"rect":[400,250,1200,850]}"#, "\n",
            ),
        )
        .unwrap();

        let data = RecordingData::load(tmp.path()).unwrap();
        assert_eq!(click_pos(&data.events[0]), (50.0, 30.0));
        assert_eq!(click_pos(&data.events[1]), (50.0, 30.0));
    }

    #[test]
    fn test_window_mode_without_history_falls_back_to_initial_rect() {
        let tmp = tempfile::TempDir::new().unwrap();
        let meta = test_meta("window", Some([100.0, 50.0, 900.0, 650.0]));
        write_recording(
            tmp.path(),
            &meta,
            r#"{"type":"click","t":100,"btn":"left","x":150,"y":80}"#,
        );
        let data = RecordingData::load(tmp.path()).unwrap();
        assert_eq!(click_pos(&data.events[0]), (50.0, 30.0));
    }

    #[test]
    fn test_area_mode_offsets_points_and_rects() {
        let tmp = tempfile::TempDir::new().unwrap();
        let meta = test_meta("area", Some([200.0, 100.0, 1000.0, 700.0]));
        write_recording(
            tmp.path(),
            &meta,
            r#"{"type":"click","t":100,"btn":"left","x":250,"y":130}"#,
        );
        std::fs::write(
            tmp.path().join("ui_events.jsonl"),
            r#"{"type":"ui_focus","t":50,"control":"Edit","name":"","rect":[300,200,400,240]}"#,
        )
        .unwrap();

        let data = RecordingData::load(tmp.path()).unwrap();
        // ui_events も読み込まれ、時刻順に並ぶ
        match &data.events[0] {
            RecordingEvent::UiFocus { rect, .. } => assert_eq!(*rect, [100.0, 100.0, 200.0, 140.0]),
            e => panic!("ui_focus ではない: {:?}", e),
        }
        assert_eq!(click_pos(&data.events[1]), (50.0, 30.0));
    }

    #[test]
    fn test_display_mode_keeps_coordinates() {
        let tmp = tempfile::TempDir::new().unwrap();
        let meta = test_meta("display", None);
        write_recording(
            tmp.path(),
            &meta,
            r#"{"type":"click","t":100,"btn":"left","x":250,"y":130}"#,
        );
        let data = RecordingData::load(tmp.path()).unwrap();
        assert_eq!(click_pos(&data.events[0]), (250.0, 130.0));
    }

    #[test]
    fn test_frame_times_from_timestamps_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_recording(tmp.path(), &test_meta("display", None), "");
        std::fs::write(tmp.path().join(FRAME_TIMESTAMPS_FILE), "5\n40\n110\n300\n").unwrap();
        let data = RecordingData::load(tmp.path()).unwrap();
        assert_eq!(data.frame_times, vec![5, 40, 110, 300]);
    }

    #[test]
    fn test_frame_times_fallback_to_evenly_spaced() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_recording(tmp.path(), &test_meta("display", None), "");
        let data = RecordingData::load(tmp.path()).unwrap();
        assert_eq!(data.frame_times, vec![0, 250, 500, 750]);

        // 行数がフレーム数に足りない場合も等間隔にフォールバック
        std::fs::write(tmp.path().join(FRAME_TIMESTAMPS_FILE), "5\n40\n").unwrap();
        let data = RecordingData::load(tmp.path()).unwrap();
        assert_eq!(data.frame_times, vec![0, 250, 500, 750]);
    }

    #[test]
    fn test_frame_index_at_picks_latest_frame_not_after_t() {
        let times = [0, 40, 110, 300];
        assert_eq!(frame_index_at(&times, 0), 0);
        assert_eq!(frame_index_at(&times, 39), 0);
        assert_eq!(frame_index_at(&times, 40), 1);
        assert_eq!(frame_index_at(&times, 299), 2);
        assert_eq!(frame_index_at(&times, 10_000), 3);
        // 最初のフレームより前は先頭フレーム
        assert_eq!(frame_index_at(&[20, 50], 5), 0);
    }

    #[test]
    fn test_output_frame_times_are_evenly_spaced_at_fps() {
        assert_eq!(output_frame_times(1000, 4), vec![0, 250, 500, 750]);
        let t = output_frame_times(1000, 30);
        assert_eq!(t.len(), 30);
        assert_eq!(t[1], 33);
        assert_eq!(t[2], 67);
        // 長さ 0 でも1フレームは出す
        assert_eq!(output_frame_times(0, 30), vec![0]);
    }

    #[test]
    fn test_sampling_with_variable_capture_rate() {
        // 実キャプチャが設定より遅い（約 10fps）録画を 30fps で書き出す場合、
        // 各出力時刻にはその時刻以前で最新の元フレームが割り当てられる。
        let frame_times = [0, 95, 210, 300];
        let picked: Vec<u64> = output_frame_times(400, 30)
            .into_iter()
            .map(|t| frame_index_at(&frame_times, t))
            .collect();
        assert_eq!(picked.len(), 12);
        assert_eq!(&picked[..4], &[0, 0, 0, 1]); // t=0,33,67,100
        assert_eq!(picked[6], 1); // t=200
        assert_eq!(picked[7], 2); // t=233
        assert_eq!(picked[9], 3); // t=300
    }
}
