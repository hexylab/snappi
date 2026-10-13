//! カメラトラックの品質メトリクス M1〜M7 (Issue #28, #39)。定義は `docs/camera-spec-v6.md` §8 を参照。

use crate::camera::{self, spec, ActivityKind, CameraTrack};
use crate::recording_data::RecordingData;
use std::fmt;

/// M1: クリック・ドラッグ・スクロールからこの時間内のカーソルを「操作中のカーソル」とみなす
pub const ACTIVE_CURSOR_MS: u64 = 1000;
/// M7: 寄り終えたかを、操作からこの時間内の最大倍率と比べて判定する
pub const ZOOM_READY_WINDOW_MS: u64 = 1000;

/// 各メトリクスの合格基準
#[derive(Debug, Clone)]
pub struct Criteria {
    /// M1: 寄っている間に操作中のカーソルがビューポート内にある割合の下限
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
    /// M7: 寄る区間の最初の操作時にズームが完了している割合の下限
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

        // M1: 寄っているフレームのうち、操作中（直近 ACTIVE_CURSOR_MS 以内にクリック・ドラッグ・スクロール）の
        // カーソルがビューポート内にある割合。止まっているマウスは主役ではないので数えない。
        // カーソルがキャプチャ領域外にある（ウィンドウ録画で窓の外にある）フレームは除く。
        let pointer_times: Vec<u64> = activities.iter().filter(|a| a.pos.is_some()).map(|a| a.t).collect();
        let m1 = ratio(track.frames.iter().enumerate().filter_map(|(k, s)| {
            if s.zoom < spec::ZOOMED_THRESHOLD {
                return None;
            }
            let t = track.time_ms(k);
            let i = pointer_times.partition_point(|&p| p <= t);
            let active = i > 0 && t - pointer_times[i - 1] <= ACTIVE_CURSOR_MS;
            if !active {
                return None;
            }
            let pos = camera::cursor_at(&data.events, t).filter(|&p| in_frame(p))?;
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

        // M7: 寄っている区間（連続する寄っているフレーム）ごとに、寄り始めてから ZOOM_IN_MS 以上あとの
        // 最初の操作の時刻に、その後 ZOOM_READY_WINDOW_MS の最大倍率の 95% まで寄り終えている割合。
        // 寄り始め = 区間の直前で最後に全体表示（zoom 1.0）だったフレーム。直前の画面の切り替えやクリックを
        // 全体表示で見せるために寄り始めが遅れた場合、遷移中の操作は数えない。
        // 寄らないと判断した操作は対象にしない（寄るべき場面は expect.json で確かめる）。
        let m7 = ratio(zoomed_runs(track).into_iter().filter_map(|(k0, k1)| {
            let ks = (0..k0).rev().find(|&k| track.frames[k].zoom <= 1.0 + 1e-9).unwrap_or(0);
            let t_ready = track.time_ms(ks) + spec::ZOOM_IN_MS;
            let t1 = track.time_ms(k1);
            let a = activities.iter().find(|a| a.t >= t_ready.max(track.time_ms(k0)) && a.t < t1)?;
            let ka = track.index_at(a.t)?;
            let kb = track.index_at(a.t + ZOOM_READY_WINDOW_MS)?;
            let peak = track.frames[ka..=kb].iter().map(|s| s.zoom).fold(1.0, f64::max);
            Some(track.frames[ka].zoom >= peak - 0.05 * (peak - 1.0))
        }));

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
                name: "寄っている間 操作中のカーソルが枠内",
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
                name: "寄る区間の最初の操作でズーム完了",
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
            changes: vec![],
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
            // 操作していない間にカーソルがビューポート外 (x=600) に出ても数えない（3.0〜4.5秒）
            RecordingEvent::MouseMove { t: 3500, x: 600.0, y: 100.0 },
            // ビューポート外でスクロール（4.5〜5.0秒は操作中のカーソルが外）
            RecordingEvent::Scroll { t: 4500, x: 600.0, y: 100.0, dx: 0.0, dy: -1.0 },
            RecordingEvent::MouseMove { t: 5000, x: 200.0, y: 100.0 },
            // 全体表示中のクリック
            RecordingEvent::Click { t: 7000, btn: "left".into(), x: 900.0, y: 400.0 },
        ];
        let m = Metrics::compute(&data(events, 10_000), &sample_track());
        // 操作中の寄っているフレームは 2.0〜3.0秒と 4.5〜5.5秒の 22 フレーム。うち 4.5〜4.9秒の 5 フレームが外
        assert_eq!(m.m1_cursor_in_view, Some(17.0 / 22.0));
        assert_eq!(m.m2_click_in_view, Some(1.0));
        assert_eq!(m.m3_switches_per_min, 2.0);
        assert_eq!(m.m4_min_segment_ms, Some(4000));
        // 1フレームで (500,250)→(250,125): 279.5px / 1000px / 0.1s
        assert!((m.m5_max_speed - 2.795).abs() < 0.01);
        assert_eq!(m.m6_out_of_frame_frames, 0);
        // 寄っている区間 [2000, 6000) の最初の操作（2000ms）で寄り終えている。7000ms のクリックは寄らないので対象外
        assert_eq!(m.m7_zoom_ready, Some(1.0));

        let results = m.evaluate(&Criteria::default());
        let failed: Vec<_> = results.iter().filter(|r| !r.passed).map(|r| r.id).collect();
        assert_eq!(failed, vec!["M1"]);
    }

    #[test]
    fn test_m7_detects_late_zoom() {
        // 1.5秒から寄り始め、2.0秒の操作の時点ではまだ寄っている途中（1.4 → 2.0）
        let frames = (0..60)
            .map(|k| {
                let zoom = match k {
                    15..=19 => 1.3,
                    20..=24 => 1.4,
                    25..=49 => 2.0,
                    _ => 1.0,
                };
                cam(250.0, 125.0, zoom)
            })
            .collect();
        let events = vec![RecordingEvent::Click { t: 2000, btn: "left".into(), x: 200.0, y: 100.0 }];
        let m = Metrics::compute(&data(events, 6000), &CameraTrack { fps: 10, frames });
        assert_eq!(m.m7_zoom_ready, Some(0.0));
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
