//! カメラトラックの計算 (Issue #29)。仕様 `docs/camera-spec-v5.md` §5 の F1〜F5 と §5.1 の遷移を、
//! 出力 fps で時刻を進めながら決定的にシミュレーションする。

use super::config::CameraConfig;
use super::segments::{FocusMode, ZoomSegment};
use super::{spec, CameraState, CameraTrack};
use crate::config::RecordingEvent;
use crate::engine::cursor_smoother::CursorSmoother;
use crate::engine::spring::Spring;
use crate::recording_data::RecordingData;

/// 区間に指定できる倍率の上限
const MAX_ZOOM: f64 = 5.0;
/// デッドゾーンの外側の余白（ビューポートに対する比率）。注目点を寄せるときの目安
const DEAD_ZONE_MARGIN: f64 = (1.0 - spec::DEAD_ZONE_RATIO) / 2.0;
/// カーソルをビューポートに残すとき、端からこの比率以上内側に置く
const KEEP_MARGIN: f64 = 0.05;

/// 区間列からカメラトラックを求める。出力フレーム k（時刻 `output_frame_time(k, fps)`）ごとの状態を返す。
pub fn camera_track(
    data: &RecordingData,
    segments: &[ZoomSegment],
    config: &CameraConfig,
    fps: u32,
) -> CameraTrack {
    let fps = fps.max(1);
    let focus = FocusSource::new(&data.events);
    let mut segments: Vec<&ZoomSegment> = segments.iter().filter(|s| s.end_ms > s.start_ms).collect();
    segments.sort_by_key(|s| s.start_ms);

    let (w, h) = (data.width() as f64, data.height() as f64);
    let dt = 1.0 / fps as f64;
    let times = data.output_frame_times(fps);
    // 重なっている場合は後から始まった区間を優先する
    let active: Vec<Option<&ZoomSegment>> = times
        .iter()
        .map(|&t| segments.iter().rev().find(|s| s.start_ms <= t && t < s.end_ms).copied())
        .collect();
    let corridors = keep_corridors(&times, &active, &focus, config, w, h, dt);

    let mut camera = Camera::new(w, h);
    let frames = times
        .iter()
        .zip(&active)
        .zip(&corridors)
        .map(|((&t, &seg), corridor)| camera.step(t, seg, corridor, &focus, config, dt))
        .collect();
    CameraTrack { fps, frames }
}

/// 区間の倍率（対数）
fn segment_zoom_log(seg: &ZoomSegment, config: &CameraConfig) -> f64 {
    seg.zoom.unwrap_or(config.zoom).clamp(1.0, MAX_ZOOM).ln()
}

/// ビューポートの中心が取りうる範囲 `[lo, hi]`（x, y それぞれ）
#[derive(Debug, Clone, Copy, PartialEq)]
struct Corridor {
    x: (f64, f64),
    y: (f64, f64),
}

impl Corridor {
    const FREE: Corridor = Corridor {
        x: (f64::NEG_INFINITY, f64::INFINITY),
        y: (f64::NEG_INFINITY, f64::INFINITY),
    };
}

/// 範囲 `[lo, hi]` に丸める。範囲が空（追いつけない）なら中央に置く
fn clamp_range(v: f64, (lo, hi): (f64, f64)) -> f64 {
    if lo <= hi {
        v.clamp(lo, hi)
    } else {
        (lo + hi) / 2.0
    }
}

/// 寄っている間、カーソルをビューポート内に収め続けるための中心位置の範囲を各フレームについて求める。
/// 後ろのフレームから速度上限付きで伝播させるので、速いカーソル移動に対してカメラが前もって動き出せる。
fn keep_corridors(
    times: &[u64],
    active: &[Option<&ZoomSegment>],
    focus: &FocusSource,
    config: &CameraConfig,
    w: f64,
    h: f64,
    dt: f64,
) -> Vec<Corridor> {
    let mut out: Vec<Corridor> = times
        .iter()
        .zip(active)
        .map(|(&t, seg)| {
            let Some(seg) = seg.filter(|s| s.focus == FocusMode::Auto) else {
                return Corridor::FREE;
            };
            // ウィンドウ録画でカーソルが窓の外にあるときは追わない
            let Some((px, py)) = focus.raw_cursor_at(t).filter(|&(x, y)| (0.0..=w).contains(&x) && (0.0..=h).contains(&y))
            else {
                return Corridor::FREE;
            };
            let z = segment_zoom_log(seg, config).exp();
            let (kx, ky) = (w / z * (0.5 - KEEP_MARGIN), h / z * (0.5 - KEEP_MARGIN));
            Corridor { x: (px - kx, px + kx), y: (py - ky, py + ky) }
        })
        .collect();
    // 軸ごとの速度上限。斜めに動いても合計が MAX_PAN_SPEED を超えないようにする
    let v = spec::MAX_PAN_SPEED * w * dt / std::f64::consts::SQRT_2;
    for k in (0..out.len().saturating_sub(1)).rev() {
        let next = out[k + 1];
        let c = &mut out[k];
        c.x = (c.x.0.max(next.x.0 - v), c.x.1.min(next.x.1 + v));
        c.y = (c.y.0.max(next.y.0 - v), c.y.1.min(next.y.1 + v));
    }
    out
}

/// 時刻順の位置サンプル `(t, x, y)` のうち、時刻 `t_ms` 以前で最新の位置。最初のサンプルより前は最初の位置。
/// マウスイベントは動いている間だけ記録されるため、サンプル間を補間せずに直前の位置を保つ。
fn position_at(positions: &[(u64, f64, f64)], t_ms: u64) -> Option<(f64, f64)> {
    let i = positions.partition_point(|&(t, _, _)| t <= t_ms).saturating_sub(1);
    positions.get(i).map(|&(_, x, y)| (x, y))
}

/// F1 の注目点を求めるためのイベントの索引
struct FocusSource {
    /// 記録されたカーソル位置
    raw_cursor: Vec<(u64, f64, f64)>,
    /// スムージング済みのカーソル位置
    cursor: Vec<(u64, f64, f64)>,
    clicks: Vec<(u64, f64, f64)>,
    keys: Vec<u64>,
}

impl FocusSource {
    fn new(events: &[RecordingEvent]) -> Self {
        let mut raw = Vec::new();
        let mut clicks = Vec::new();
        let mut keys = Vec::new();
        for e in events {
            match e {
                RecordingEvent::Click { t, x, y, .. } => {
                    raw.push((*t, *x, *y));
                    clicks.push((*t, *x, *y));
                }
                RecordingEvent::MouseMove { t, x, y }
                | RecordingEvent::ClickRelease { t, x, y, .. }
                | RecordingEvent::Scroll { t, x, y, .. } => raw.push((*t, *x, *y)),
                RecordingEvent::Key { t, .. } => keys.push(*t),
                _ => {}
            }
        }
        Self { cursor: CursorSmoother::new().smooth(&raw), raw_cursor: raw, clicks, keys }
    }

    /// 区間 `seg` の時刻 `t` の注目点
    fn point(&self, seg: &ZoomSegment, t: u64) -> Option<(f64, f64)> {
        match seg.focus {
            FocusMode::Fixed { x, y } => Some((x, y)),
            FocusMode::Auto => self.auto_point(seg.start_ms, t),
        }
    }

    /// F1: 文字入力中（直近のキー入力から MERGE_GAP 以内で、その後にクリックが無い間）は
    /// 区間内の直近のクリック位置、区間内にクリックが無ければ入力開始時のカーソル位置。
    /// それ以外はカーソル位置。
    fn auto_point(&self, seg_start: u64, t: u64) -> Option<(f64, f64)> {
        let key_end = self.keys.partition_point(|&k| k <= t);
        let last_key = key_end.checked_sub(1).map(|i| self.keys[i]).filter(|&k| k >= seg_start);
        let last_click = self.clicks[..self.clicks.partition_point(|c| c.0 <= t)]
            .last()
            .filter(|c| c.0 >= seg_start);
        if let Some(k) = last_key {
            let typing = t - k < spec::MERGE_GAP_MS && last_click.map_or(true, |c| c.0 <= k);
            if typing {
                if let Some(&(_, x, y)) = last_click {
                    return Some((x, y));
                }
                // 区間内にクリックが無い: 入力が途切れずに続いている範囲の最初のキー入力時のカーソル
                let mut i = key_end - 1;
                while i > 0 && self.keys[i - 1] >= seg_start && self.keys[i] - self.keys[i - 1] < spec::MERGE_GAP_MS {
                    i -= 1;
                }
                return self.cursor_at(self.keys[i]);
            }
        }
        self.cursor_at(t)
    }

    fn cursor_at(&self, t: u64) -> Option<(f64, f64)> {
        position_at(&self.cursor, t)
    }

    /// スムージング前のカーソル位置。速い移動ではスムージングの遅れが大きいため、
    /// カーソルを画面内に収める判定にはこちらを使う
    fn raw_cursor_at(&self, t: u64) -> Option<(f64, f64)> {
        position_at(&self.raw_cursor, t)
    }
}

/// ズームの遷移（§5.1, F4）
#[derive(Debug, Clone, Copy)]
struct Transition {
    start_t: u64,
    duration_ms: u64,
    /// 遷移前後の zoom（対数）
    from_log: f64,
    to_log: f64,
    /// 寄る（引く）先の点
    anchor: (f64, f64),
    /// 遷移開始時・終了時の、ビューポート内での anchor の相対位置
    u_from: (f64, f64),
    u_to: (f64, f64),
}

struct Camera {
    width: f64,
    height: f64,
    cx: Spring,
    cy: Spring,
    /// 現在の zoom（対数）
    zoom_log: f64,
    /// 現在の区間（ID と倍率の対数）。None は全体表示
    active: Option<(String, f64)>,
    transition: Option<Transition>,
    /// 直近の注目点（引くときの anchor に使う）
    last_focus: Option<(f64, f64)>,
}

impl Camera {
    fn new(width: f64, height: f64) -> Self {
        Self {
            width,
            height,
            cx: Spring::new(width / 2.0),
            cy: Spring::new(height / 2.0),
            zoom_log: 0.0,
            active: None,
            transition: None,
            last_focus: None,
        }
    }

    fn zoom(&self) -> f64 {
        self.zoom_log.exp()
    }

    fn step(
        &mut self,
        t: u64,
        seg: Option<&ZoomSegment>,
        corridor: &Corridor,
        focus: &FocusSource,
        config: &CameraConfig,
        dt: f64,
    ) -> CameraState {
        let next = seg.map(|s| (s.id.clone(), segment_zoom_log(s, config)));
        if next != self.active {
            self.on_segment_change(t, seg, next.as_ref().map_or(0.0, |n| n.1), focus);
            self.active = next;
        }

        let focus_point = seg.and_then(|s| focus.point(s, t));
        if let Some(tr) = self.transition {
            self.step_transition(t, tr);
        } else if self.zoom_log > 0.0 {
            if let Some(s) = seg {
                // 先読みした注目点に向けて早めに動き出し、バネの遅れを補う
                let t_ahead = (t + spec::PAN_LOOKAHEAD_MS).min(s.end_ms);
                // 自動追従ではカーソルを、固定位置では注目点を、ビューポートから外さない
                let (keep_ahead, keep_now) = match s.focus {
                    FocusMode::Auto => (focus.raw_cursor_at(t_ahead), focus.raw_cursor_at(t)),
                    FocusMode::Fixed { .. } => (focus_point, focus_point),
                };
                if let Some(ahead) = focus.point(s, t_ahead) {
                    self.retarget_pan(ahead, &[keep_ahead, keep_now]);
                }
            }
            self.step_pan(corridor, dt);
        }
        if focus_point.is_some() {
            self.last_focus = focus_point;
        }
        self.state()
    }

    /// 区間が変わったときに、倍率が変わるならズームの遷移を始める
    fn on_segment_change(&mut self, t: u64, seg: Option<&ZoomSegment>, to_log: f64, focus: &FocusSource) {
        if (to_log - self.zoom_log).abs() < 1e-9 && self.transition.is_none() {
            return;
        }
        let zooming_in = to_log > self.zoom_log;
        // 寄るときは区間の最初のアクティビティ（区間開始の LEAD 後）の注目点へ、
        // 引くときは直前の注目点を保ったまま引く
        let anchor = match seg {
            Some(s) if zooming_in => {
                let t_anchor = (s.start_ms + spec::LEAD_MS).min(s.end_ms.saturating_sub(1)).max(t);
                focus.point(s, t_anchor)
            }
            _ => None,
        }
        .or(self.last_focus)
        .unwrap_or((self.cx.position, self.cy.position));

        let vp = self.state().viewport(self.width, self.height);
        let u_from = ((anchor.0 - vp.x) / vp.width, (anchor.1 - vp.y) / vp.height);
        // anchor がビューポート内なら相対位置を保つ（F4）。外ならデッドゾーンの端へ寄せていく
        let settle = |u: f64| {
            if (0.0..=1.0).contains(&u) {
                u
            } else {
                u.clamp(DEAD_ZONE_MARGIN, 1.0 - DEAD_ZONE_MARGIN)
            }
        };
        self.transition = Some(Transition {
            start_t: t,
            duration_ms: if zooming_in { spec::ZOOM_IN_MS } else { spec::ZOOM_OUT_MS },
            from_log: self.zoom_log,
            to_log,
            anchor,
            u_from,
            u_to: (settle(u_from.0), settle(u_from.1)),
        });
    }

    fn step_transition(&mut self, t: u64, tr: Transition) {
        let s = ((t - tr.start_t) as f64 / tr.duration_ms.max(1) as f64).min(1.0);
        let e = ease_in_out_cubic(s);
        self.zoom_log = tr.from_log + (tr.to_log - tr.from_log) * e;
        let z = self.zoom();
        let (vw, vh) = (self.width / z, self.height / z);
        let ux = tr.u_from.0 + (tr.u_to.0 - tr.u_from.0) * e;
        let uy = tr.u_from.1 + (tr.u_to.1 - tr.u_from.1) * e;
        let (cx, cy) = self.clamp_center(tr.anchor.0 - ux * vw + vw / 2.0, tr.anchor.1 - uy * vh + vh / 2.0);
        self.cx.snap(cx);
        self.cy.snap(cy);
        if s >= 1.0 {
            self.zoom_log = tr.to_log;
            self.transition = None;
        }
    }

    /// F2: 注目点 `ahead`（先読み）が目標位置のデッドゾーンの外に出たら、内側に戻る位置へ目標を動かす。
    /// そのうえで `keep` の点を順に目標のビューポートから外さないようにする（後の点ほど優先）。
    fn retarget_pan(&mut self, ahead: (f64, f64), keep: &[Option<(f64, f64)>]) {
        let z = self.zoom();
        let (vw, vh) = (self.width / z, self.height / z);
        let follow = |target: f64, p: f64, view: f64| {
            let half = view * spec::DEAD_ZONE_RATIO / 2.0;
            target + (p - p.clamp(target - half, target + half))
        };
        let mut tx = follow(self.cx.target, ahead.0, vw);
        let mut ty = follow(self.cy.target, ahead.1, vh);
        for &(px, py) in keep.iter().flatten() {
            let (kx, ky) = (vw / 2.0 - vw * KEEP_MARGIN, vh / 2.0 - vh * KEEP_MARGIN);
            tx = tx.clamp(px - kx, px + kx);
            ty = ty.clamp(py - ky, py + ky);
        }
        // 目標をフレームの端ちょうどにするとバネが端に漸近するだけで届かないため、少し外側まで許す
        let (ex, ey) = (vw * KEEP_MARGIN, vh * KEEP_MARGIN);
        self.cx.set_target(tx.clamp(vw / 2.0 - ex, self.width - vw / 2.0 + ex));
        self.cy.set_target(ty.clamp(vh / 2.0 - ey, self.height - vh / 2.0 + ey));
    }

    /// F2: 臨界減衰バネで目標へ動かし、カーソルを収める範囲 `corridor`、速度上限、
    /// F3 のクランプを適用する
    fn step_pan(&mut self, corridor: &Corridor, dt: f64) {
        let half_life = spec::PAN_HALF_LIFE_MS as f64 / 1000.0;
        let (ox, oy) = (self.cx.position, self.cy.position);
        self.cx.update(half_life, dt);
        self.cy.update(half_life, dt);
        let (nx, ny) = (clamp_range(self.cx.position, corridor.x), clamp_range(self.cy.position, corridor.y));
        let (dx, dy) = (nx - ox, ny - oy);
        let max_step = spec::MAX_PAN_SPEED * self.width * dt;
        let r = (max_step / dx.hypot(dy).max(1e-9)).min(1.0);
        let (nx, ny) = self.clamp_center(ox + dx * r, oy + dy * r);
        // 速度上限・範囲で位置を変えた場合も、バネの速度を実際の動きに合わせる
        self.cx.velocity = (nx - ox) / dt;
        self.cy.velocity = (ny - oy) / dt;
        self.cx.position = nx;
        self.cy.position = ny;
    }

    /// F3: ビューポートがフレーム内に収まる中心位置に丸める
    fn clamp_center(&self, cx: f64, cy: f64) -> (f64, f64) {
        let z = self.zoom().max(1.0);
        let (hw, hh) = (self.width / z / 2.0, self.height / z / 2.0);
        (cx.clamp(hw, self.width - hw), cy.clamp(hh, self.height - hh))
    }

    fn state(&self) -> CameraState {
        let (center_x, center_y) = self.clamp_center(self.cx.position, self.cy.position);
        CameraState { center_x, center_y, zoom: self.zoom().max(1.0) }
    }
}

fn ease_in_out_cubic(s: f64) -> f64 {
    if s < 0.5 {
        4.0 * s * s * s
    } else {
        1.0 - (-2.0 * s + 2.0).powi(3) / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RecordingMeta;

    const W: f64 = 1920.0;
    const H: f64 = 1080.0;

    fn data(events: Vec<RecordingEvent>, duration_ms: u64) -> RecordingData {
        RecordingData {
            dir: Default::default(),
            meta: RecordingMeta {
                version: 2,
                id: "t".into(),
                screen_width: W as u32,
                screen_height: H as u32,
                fps: 30,
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

    fn seg(start_ms: u64, end_ms: u64) -> ZoomSegment {
        ZoomSegment { id: format!("s{}", start_ms), start_ms, end_ms, zoom: None, focus: FocusMode::Auto }
    }

    fn mv(t: u64, x: f64, y: f64) -> RecordingEvent {
        RecordingEvent::MouseMove { t, x, y }
    }

    fn click(t: u64, x: f64, y: f64) -> RecordingEvent {
        RecordingEvent::Click { t, btn: "left".into(), x, y }
    }

    fn key(t: u64) -> RecordingEvent {
        RecordingEvent::Key { t, key: "Character".into(), modifiers: vec![] }
    }

    fn track(events: Vec<RecordingEvent>, duration_ms: u64, segments: &[ZoomSegment]) -> CameraTrack {
        camera_track(&data(events, duration_ms), segments, &CameraConfig::default(), 30)
    }

    #[test]
    fn test_no_segments_stays_overview() {
        let t = track(vec![mv(0, 100.0, 100.0), click(2000, 100.0, 100.0)], 5000, &[]);
        assert_eq!(t.frames.len(), 150);
        assert!(t.frames.iter().all(|s| *s == CameraState::overview(W, H)));
    }

    #[test]
    fn test_zoom_in_and_out_timing() {
        let t = track(vec![mv(0, 960.0, 540.0)], 10_000, &[seg(2000, 6000)]);
        let z = |ms| t.state_at(ms).unwrap().zoom;
        assert_eq!(z(1900), 1.0);
        // ZOOM_IN = 500ms で寄り終える
        assert!((z(2500) - spec::ZOOM_STANDARD).abs() < 1e-9);
        assert!(z(2250) > 1.0 && z(2250) < spec::ZOOM_STANDARD);
        assert!((z(5990) - spec::ZOOM_STANDARD).abs() < 1e-9);
        // ZOOM_OUT = 700ms で引き終える
        assert!(z(6300) > 1.0);
        assert_eq!(z(6700), 1.0);
    }

    #[test]
    fn test_zoom_in_keeps_anchor_screen_position() {
        // F4: 最初のクリック位置 (480, 270) の画面上の相対位置を保ったまま寄る
        let events = vec![mv(0, 480.0, 270.0), click(2600, 480.0, 270.0)];
        let t = track(events, 10_000, &[seg(2000, 6000)]);
        for ms in [2000, 2100, 2200, 2300, 2400, 2500] {
            let s = t.state_at(ms).unwrap();
            let vp = s.viewport(W, H);
            assert!(((480.0 - vp.x) / vp.width - 0.25).abs() < 1e-6, "t={} vp={:?}", ms, vp);
            assert!(((270.0 - vp.y) / vp.height - 0.25).abs() < 1e-6);
        }
    }

    #[test]
    fn test_pan_follows_cursor_outside_dead_zone_only() {
        let mut events = vec![mv(0, 960.0, 540.0), click(2600, 960.0, 540.0)];
        // デッドゾーン内の小さな動きでは動かない
        events.push(mv(3500, 1000.0, 560.0));
        // 右端まで動かすとパンで追う
        events.push(mv(4500, 1800.0, 540.0));
        let t = track(events, 10_000, &[seg(2000, 8000)]);
        // （4500ms のジャンプは先読みで少し前から追い始めるので、その前の時刻で比べる）
        let s3 = t.state_at(3000).unwrap();
        let s4 = t.state_at(4100).unwrap();
        assert_eq!(s3.center_x, s4.center_x);
        assert!(t.state_at(4500).unwrap().center_x > s4.center_x);
        // 右端に寄せきる（F3: 注目点が中央に来なくてもフレーム内に収める）
        let s6 = t.state_at(6000).unwrap();
        assert!((s6.viewport(W, H).x + s6.viewport(W, H).width - W).abs() < 1.0, "{:?}", s6);
        assert!(s6.viewport(W, H).contains(1800.0, 540.0));
        // 速度上限とクランプ
        for k in 1..t.frames.len() {
            let (a, b) = (t.frames[k - 1], t.frames[k]);
            let speed = (b.center_x - a.center_x).hypot(b.center_y - a.center_y) / W * 30.0;
            assert!(speed <= spec::MAX_PAN_SPEED + 1e-6, "k={} speed={}", k, speed);
            assert!(b.viewport(W, H).is_within(W, H, 1e-6));
        }
    }

    #[test]
    fn test_fast_cursor_move_stays_in_view() {
        // 0.3 秒で画面の左上から右下へ動かしても、カーソルはビューポートから外れない
        let mut events = vec![mv(0, 100.0, 100.0), click(2600, 100.0, 100.0)];
        for i in 0..=10u32 {
            let r = i as f64 / 10.0;
            events.push(mv(4000 + i as u64 * 30, 100.0 + 1700.0 * r, 100.0 + 850.0 * r));
        }
        events.push(click(4400, 1800.0, 950.0));
        let t = track(events.clone(), 8000, &[seg(2000, 7000)]);
        for k in 0..t.frames.len() {
            let ms = t.time_ms(k);
            let s = t.frames[k];
            if (2500..7000).contains(&ms) {
                let p = crate::camera::cursor_at(&events, ms).unwrap();
                assert!(s.viewport(W, H).contains(p.0, p.1), "t={} cursor={:?} {:?}", ms, p, s);
            }
            if k > 0 {
                let prev = t.frames[k - 1];
                let speed = (s.center_x - prev.center_x).hypot(s.center_y - prev.center_y) / W * 30.0;
                assert!(speed <= spec::MAX_PAN_SPEED + 1e-6, "t={} speed={}", ms, speed);
            }
        }
    }

    #[test]
    fn test_typing_focuses_last_click_in_segment() {
        // クリック後に入力中はカーソルが離れてもクリック位置を映す
        let events = vec![
            mv(0, 400.0, 300.0),
            click(2600, 400.0, 300.0),
            key(3000),
            mv(3100, 1800.0, 1000.0),
            key(3500),
        ];
        let src = FocusSource::new(&events);
        let s = seg(2000, 9000);
        assert_eq!(src.auto_point(s.start_ms, 3200), Some((400.0, 300.0)));
        // 最後の入力から MERGE_GAP 経つとカーソルに戻る
        let p = src.auto_point(s.start_ms, 6600).unwrap();
        assert!((p.0 - 1800.0).abs() < 1.0);
    }

    #[test]
    fn test_typing_without_click_uses_cursor_at_typing_start() {
        let events = vec![mv(0, 300.0, 200.0), key(2000), mv(2100, 900.0, 900.0), key(2500)];
        let src = FocusSource::new(&events);
        let p = src.auto_point(1400, 2600).unwrap();
        assert!((p.0 - 300.0).abs() < 1.0 && (p.1 - 200.0).abs() < 1.0, "{:?}", p);
    }

    #[test]
    fn test_fixed_focus() {
        let s = ZoomSegment { focus: FocusMode::Fixed { x: 1700.0, y: 900.0 }, ..seg(1000, 5000) };
        let t = track(vec![mv(0, 100.0, 100.0)], 6000, &[s]);
        let st = t.state_at(3000).unwrap();
        assert!(st.viewport(W, H).contains(1700.0, 900.0));
    }

    #[test]
    fn test_segment_zoom_override() {
        let s = ZoomSegment { zoom: Some(3.0), ..seg(1000, 5000) };
        let t = track(vec![mv(0, 960.0, 540.0)], 6000, &[s]);
        assert!((t.state_at(3000).unwrap().zoom - 3.0).abs() < 1e-9);
    }

    #[test]
    fn test_position_at_holds_latest_sample() {
        let p = [(100, 0.0, 0.0), (200, 10.0, 20.0)];
        assert_eq!(position_at(&p, 0), Some((0.0, 0.0)));
        assert_eq!(position_at(&p, 150), Some((0.0, 0.0)));
        assert_eq!(position_at(&p, 200), Some((10.0, 20.0)));
        assert_eq!(position_at(&p, 300), Some((10.0, 20.0)));
        assert_eq!(position_at(&[], 0), None);
    }

    #[test]
    fn test_ease_in_out_cubic() {
        assert_eq!(ease_in_out_cubic(0.0), 0.0);
        assert_eq!(ease_in_out_cubic(0.5), 0.5);
        assert_eq!(ease_in_out_cubic(1.0), 1.0);
    }
}
