//! カメラトラックの品質メトリクス M1〜M7 (Issue #28)。定義は `docs/camera-spec-v5.md` を参照。

use crate::camera::{self, spec, ActivityKind, CameraTrack};
use crate::recording_data::RecordingData;
use std::fmt;

/// 各メトリクスの合格基準
#[derive(Debug, Clone)]
pub struct Criteria {
    /// M1: 寄っている間にカーソルがビューポート内にある割合の下限
    pub m1_min_ratio: f64,
    /// M2: クリック時刻にクリック位置がビューポート内にある割合の下限
    pub m2_min_ratio: f64,
    /// M3: ズーム状態の切り替え頻度の上限（回/分）
    pub m3_max_switches_per_min: f64,
    /// M4: 寄っている区間の最短長の下限 (ms)
    pub m4_min_segment_ms: u64,
    /// M5: カメラ中心の速度の上限（画面幅/秒）
    pub m5_max_speed: f64,
    /// M6: ビューポートがフレーム外にはみ出すフレーム数の上限
    pub m6_max_frames: usize,
    /// M7: アクティビティのまとまりの開始時にズームが完了している割合の下限
    pub m7_min_ratio: f64,
}

impl Default for Criteria {
    fn default() -> Self {
        Self {
            m1_min_ratio: 0.98,
            m2_min_ratio: 1.0,
            m3_max_switches_per_min: 6.0,
            m4_min_segment_ms: spec::MIN_SEGMENT_MS,
            m5_max_speed: 3.0,
            m6_max_frames: 0,
            m7_min_ratio: 0.9,
        }
    }
}

/// メトリクスの計測値。対象が無い指標（寄る区間が無い等）は None で、常に合格とする。
#[derive(Debug, Clone, PartialEq)]
pub struct Metrics {
    pub m1_cursor_in_view: Option<f64>,
    pub m2_click_in_view: Option<f64>,
    pub m3_switches_per_min: f64,
    pub m4_min_segment_ms: Option<u64>,
    pub m5_max_speed: f64,
    pub m6_out_of_frame_frames: usize,
    pub m7_zoom_ready: Option<f64>,
}

/// 1つのメトリクスの判定結果
#[derive(Debug, Clone)]
pub struct MetricResult {
    pub id: &'static str,
    pub name: &'static str,
    pub value: String,
    pub criterion: String,
    pub passed: bool,
}

impl fmt::Display for MetricResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {:<40} {:>10}  (基準 {})",
            if self.passed { "OK  " } else { "FAIL" },
            self.id,
            self.name,
            self.value,
            self.criterion,
        )
    }
}

/// メトリクス上で「寄っている」フレームが連続する範囲 `[start, end)` の一覧
pub fn zoomed_runs(track: &CameraTrack) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut start = None;
    for (k, s) in track.frames.iter().enumerate() {
        match (s.zoom >= spec::ZOOMED_THRESHOLD, start) {
            (true, None) => start = Some(k),
            (false, Some(st)) => {
                runs.push((st, k));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(st) = start {
        runs.push((st, track.frames.len()));
    }
    runs
}

/// 寄る/引くの切り替え回数（隣り合うフレームで「寄っている」かどうかが変わった回数）
pub fn zoom_switches(track: &CameraTrack) -> usize {
    track
        .frames
        .windows(2)
        .filter(|p| (p[0].zoom >= spec::ZOOMED_THRESHOLD) != (p[1].zoom >= spec::ZOOMED_THRESHOLD))
        .count()
}

impl Metrics {
    pub fn compute(data: &RecordingData, track: &CameraTrack) -> Self {
        let (w, h) = (data.width() as f64, data.height() as f64);
        let in_frame = |(x, y): (f64, f64)| x >= 0.0 && x <= w && y >= 0.0 && y <= h;
        let activities = camera::extract_activities(&data.events, data.meta.duration_ms);

        // M1: 寄っているフレームのうち、カーソルがビューポート内にある割合。
        // カーソルがキャプチャ領域外にある（ウィンドウ録画で窓の外にある）フレームは除く。
        let m1 = ratio(track.frames.iter().enumerate().filter_map(|(k, s)| {
            if s.zoom < spec::ZOOMED_THRESHOLD {
                return None;
            }
            let pos = camera::cursor_at(&data.events, track.time_ms(k)).filter(|&p| in_frame(p))?;
            Some(s.viewport(w, h).contains(pos.0, pos.1))
        }));

        // M2: クリック時刻にクリック位置がビューポート内にある割合
        let m2 = ratio(activities.iter().filter(|a| a.kind == ActivityKind::Click).filter_map(|a| {
            let pos = a.pos.filter(|&p| in_frame(p))?;
            let s = track.state_at(a.t)?;
            Some(s.viewport(w, h).contains(pos.0, pos.1))
        }));

        // M3: 寄る/引くの切り替え回数（1分未満の録画は1分として数える）
        let minutes = (track.time_ms(track.frames.len()) as f64 / 60_000.0).max(1.0);
        let m3 = zoom_switches(track) as f64 / minutes;

        // M4: 寄っている区間の最短長
        let m4 = zoomed_runs(track)
            .iter()
            .map(|&(s, e)| track.time_ms(e) - track.time_ms(s))
            .min();

        // M5: カメラ中心の最大速度（画面幅/秒）
        let m5 = (1..track.frames.len())
            .map(|k| {
                let (a, b) = (track.frames[k - 1], track.frames[k]);
                let dt = (track.time_ms(k) - track.time_ms(k - 1)).max(1) as f64 / 1000.0;
                (b.center_x - a.center_x).hypot(b.center_y - a.center_y) / w / dt
            })
            .fold(0.0, f64::max);

        // M6: ビューポートがフレーム外にはみ出すフレーム数（0.5px の誤差は許す）
        let m6 = track
            .frames
            .iter()
            .filter(|s| !s.viewport(w, h).is_within(w, h, 0.5))
            .count();

        // M7: アクティビティのまとまりの最初の時刻に、そのまとまりの間の最大ズームの
        // 95% まで寄り終えている割合。まとまりの間まったく寄らない場合も未完了とする。
        let m7 = ratio(
            camera::group_activities(&activities, spec::MERGE_GAP_MS)
                .into_iter()
                .filter_map(|(t0, t1)| {
                    let (k0, k1) = (track.index_at(t0)?, track.index_at(t1)?);
                    let peak = track.frames[k0..=k1].iter().map(|s| s.zoom).fold(1.0, f64::max);
                    let ready = track.frames[k0].zoom >= peak - 0.05 * (peak - 1.0);
                    Some(peak >= spec::ZOOMED_THRESHOLD && ready)
                }),
        );

        Self {
            m1_cursor_in_view: m1,
            m2_click_in_view: m2,
            m3_switches_per_min: m3,
            m4_min_segment_ms: m4,
            m5_max_speed: m5,
            m6_out_of_frame_frames: m6,
            m7_zoom_ready: m7,
        }
    }

    pub fn evaluate(&self, c: &Criteria) -> Vec<MetricResult> {
        let pct = |v: Option<f64>| v.map_or("-".to_string(), |v| format!("{:.2}%", v * 100.0));
        vec![
            MetricResult {
                id: "M1",
                name: "寄っている間カーソルがビューポート内",
                value: pct(self.m1_cursor_in_view),
                criterion: format!(">= {:.0}%", c.m1_min_ratio * 100.0),
                passed: self.m1_cursor_in_view.map_or(true, |v| v >= c.m1_min_ratio),
            },
            MetricResult {
                id: "M2",
                name: "クリック位置がビューポート内",
                value: pct(self.m2_click_in_view),
                criterion: format!(">= {:.0}%", c.m2_min_ratio * 100.0),
                passed: self.m2_click_in_view.map_or(true, |v| v >= c.m2_min_ratio),
            },
            MetricResult {
                id: "M3",
                name: "ズーム切り替え頻度 (回/分)",
                value: format!("{:.1}", self.m3_switches_per_min),
                criterion: format!("<= {:.1}", c.m3_max_switches_per_min),
                passed: self.m3_switches_per_min <= c.m3_max_switches_per_min,
            },
            MetricResult {
                id: "M4",
                name: "寄っている区間の最短長 (ms)",
                value: self.m4_min_segment_ms.map_or("-".to_string(), |v| v.to_string()),
                criterion: format!(">= {}", c.m4_min_segment_ms),
                passed: self.m4_min_segment_ms.map_or(true, |v| v >= c.m4_min_segment_ms),
            },
            MetricResult {
                id: "M5",
                name: "カメラ最大速度 (画面幅/秒)",
                value: format!("{:.2}", self.m5_max_speed),
                criterion: format!("<= {:.1}", c.m5_max_speed),
                passed: self.m5_max_speed <= c.m5_max_speed,
            },
            MetricResult {
                id: "M6",
                name: "フレーム外にはみ出すフレーム数",
                value: self.m6_out_of_frame_frames.to_string(),
                criterion: format!("<= {}", c.m6_max_frames),
                passed: self.m6_out_of_frame_frames <= c.m6_max_frames,
            },
            MetricResult {
                id: "M7",
                name: "最初のアクティビティでズーム完了",
                value: pct(self.m7_zoom_ready),
                criterion: format!(">= {:.0}%", c.m7_min_ratio * 100.0),
                passed: self.m7_zoom_ready.map_or(true, |v| v >= c.m7_min_ratio),
            },
        ]
    }
}

/// bool 列の true の割合。空なら None。
fn ratio(items: impl Iterator<Item = bool>) -> Option<f64> {
    let (mut hit, mut total) = (0usize, 0usize);
    for ok in items {
        total += 1;
        hit += ok as usize;
    }
    (total > 0).then(|| hit as f64 / total as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::CameraState;
    use crate::config::{RecordingEvent, RecordingMeta};

    const W: f64 = 1000.0;
    const H: f64 = 500.0;

    fn data(events: Vec<RecordingEvent>, duration_ms: u64) -> RecordingData {
        RecordingData {
            dir: Default::default(),
            meta: RecordingMeta {
                version: 2,
                id: "t".into(),
                screen_width: W as u32,
                screen_height: H as u32,
                fps: 10,
                start_time: String::new(),
                duration_ms,
                has_audio: false,
                monitor_scale: 1.0,
                recording_dir: String::new(),
                recording_mode: None,
                window_title: None,
                window_initial_rect: None,
                frame_count: Some(0),
            },
            events,
            frame_times: vec![],
        }
    }

    fn cam(cx: f64, cy: f64, zoom: f64) -> CameraState {
        CameraState { center_x: cx, center_y: cy, zoom }
    }

    /// 10fps・10秒: 2.0〜6.0秒だけ (250, 125) に 2倍で寄るトラック
    fn sample_track() -> CameraTrack {
        let frames = (0..100)
            .map(|k| if (20..60).contains(&k) { cam(250.0, 125.0, 2.0) } else { cam(W / 2.0, H / 2.0, 1.0) })
            .collect();
        CameraTrack { fps: 10, frames }
    }

    #[test]
    fn test_metrics_on_simple_track() {
        let events = vec![
            RecordingEvent::Click { t: 2000, btn: "left".into(), x: 200.0, y: 100.0 },
            RecordingEvent::Key { t: 3000, key: "Character".into(), modifiers: vec![] },
            // 寄っている間にビューポート外 (x=600) へカーソルが出る（4.0〜5.0秒）
            RecordingEvent::MouseMove { t: 4000, x: 600.0, y: 100.0 },
            RecordingEvent::MouseMove { t: 5000, x: 200.0, y: 100.0 },
            // 全体表示中のクリック
            RecordingEvent::Click { t: 7000, btn: "left".into(), x: 900.0, y: 400.0 },
        ];
        let m = Metrics::compute(&data(events, 10_000), &sample_track());
        // 寄っている 40 フレーム中、2.0〜4.0秒と5.0〜6.0秒の 30 フレームでカーソルが内側
        assert_eq!(m.m1_cursor_in_view, Some(30.0 / 40.0));
        assert_eq!(m.m2_click_in_view, Some(1.0));
        assert_eq!(m.m3_switches_per_min, 2.0);
        assert_eq!(m.m4_min_segment_ms, Some(4000));
        // 1フレームで (500,250)→(250,125): 279.5px / 1000px / 0.1s
        assert!((m.m5_max_speed - 2.795).abs() < 0.01);
        assert_eq!(m.m6_out_of_frame_frames, 0);
        // まとまり: [2000..3000] は開始時に寄り終えている / [7000] は寄らない
        assert_eq!(m.m7_zoom_ready, Some(0.5));

        let results = m.evaluate(&Criteria::default());
        let failed: Vec<_> = results.iter().filter(|r| !r.passed).map(|r| r.id).collect();
        assert_eq!(failed, vec!["M1", "M7"]);
    }

    #[test]
    fn test_out_of_frame_and_empty_metrics() {
        let track = CameraTrack { fps: 10, frames: vec![cam(100.0, 250.0, 2.0), cam(500.0, 250.0, 2.0)] };
        let m = Metrics::compute(&data(vec![], 200), &track);
        assert_eq!(m.m6_out_of_frame_frames, 1);
        // カーソル・クリック・アクティビティが無ければ None（合格扱い）
        assert_eq!(m.m1_cursor_in_view, None);
        assert_eq!(m.m2_click_in_view, None);
        assert_eq!(m.m7_zoom_ready, None);
    }

    #[test]
    fn test_zoomed_runs() {
        let track = CameraTrack {
            fps: 10,
            frames: vec![cam(0.0, 0.0, 1.3), cam(0.0, 0.0, 1.0), cam(0.0, 0.0, 2.0), cam(0.0, 0.0, 2.0)],
        };
        assert_eq!(zoomed_runs(&track), vec![(0, 1), (2, 4)]);
    }
}
