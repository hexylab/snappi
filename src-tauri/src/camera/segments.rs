//! ズーム区間の自動生成 (Issue #29, #39)。仕様 `docs/camera-spec-v6.md` §4〜§5。
//!
//! 編集者の考え方で「ショット」を決める:
//! 1. 操作ごとの主役（`subject`）を時間順にまとめ、余白付きで収まる大きさで寄る（S3）
//! 2. 同じ場所・同じ目的の一連の操作は 1 つの場面として、ショットを切らない（S4）
//! 3. 意味のない操作（画面が変化しないクリック）や画面の切り替えでは寄らない（S2）
//! 4. 枠は操作しているウィンドウの中に収める（S5）
//!
//! 各ショットは `zoom: Some(倍率)`・`focus: Fixed { 枠の中心 }` の ZoomSegment になる。

use super::config::CameraConfig;
use super::subject::{distance, fit_zoom, subjects, Subject, SubjectInput};
use super::{extract_activities, spec, ActivityKind};
use crate::changes::Rect;
use crate::config::RecordingEvent;
use crate::recording_data::RecordingData;
use serde::{Deserialize, Serialize};

/// 寄っている間に映す場所の決め方
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]
pub enum FocusMode {
    /// カーソル・入力位置を追従する（v5 の F1。旧 project.json 互換）
    Auto,
    /// 枠の中心（キャプチャ領域基準の px）。自動生成したショットはこれを使う
    Fixed { x: f64, y: f64 },
}

/// 寄っている時間帯。`start_ms` で寄り始め、`end_ms` で引き始める。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]
pub struct ZoomSegment {
    /// 編集・保存用の安定 ID。自動生成した区間は `auto-<start_ms>`。
    pub id: String,
    #[cfg_attr(feature = "ts-export", ts(type = "number"))]
    pub start_ms: u64,
    #[cfg_attr(feature = "ts-export", ts(type = "number"))]
    pub end_ms: u64,
    /// None ならズーム強さの既定倍率
    pub zoom: Option<f64>,
    pub focus: FocusMode,
}

/// 録画からズーム区間（ショット）を自動生成する。ズーム強さが Off なら空。
pub fn auto_segments(data: &RecordingData, config: &CameraConfig) -> Vec<ZoomSegment> {
    if !config.auto_segments {
        return Vec::new();
    }
    plan(data, config)
        .into_iter()
        .filter(|s| s.zoom > 1.0)
        .map(|s| ZoomSegment {
            id: format!("auto-{}", s.start),
            start_ms: s.start,
            end_ms: s.end,
            zoom: Some(round2(s.zoom)),
            focus: FocusMode::Fixed { x: s.center.0.round(), y: s.center.1.round() },
        })
        .collect()
}

/// 計画したショット。zoom 1.0 は全体表示のショット（区間にはならない）
#[derive(Debug, Clone, PartialEq)]
pub struct Shot {
    pub start: u64,
    pub end: u64,
    /// ショットに含まれる操作の時刻の範囲
    pub first: u64,
    pub last: u64,
    pub zoom: f64,
    pub center: (f64, f64),
    /// ショットで映す主役の和
    pub subject: Rect,
    /// 全体表示のショットで、全体表示を保つ必要がある最後の時刻（画面の切り替え・クリックの位置を見せる）
    pub keep_until: u64,
}

/// 録画のショットを計画する（全体表示のショットを含む）。デバッグ表示にも使う。
pub fn plan(data: &RecordingData, config: &CameraConfig) -> Vec<Shot> {
    let (w, h) = (data.width() as f64, data.height() as f64);
    let activities = extract_activities(&data.events, data.meta.duration_ms);
    let subs = subjects(&SubjectInput {
        activities: &activities,
        changes: &data.changes,
        width: w,
        height: h,
        zoom: config.zoom,
    });
    let windows = window_rects(&data.events, w, h);
    plan_shots(&subs, &windows, data.meta.duration_ms, config.zoom, w, h)
}

/// 計画中のショット
#[derive(Debug, Clone)]
struct Draft {
    first: u64,
    last: u64,
    last_seen: u64,
    union: Rect,
    /// 画面が変化した操作を含むか（含まなければ寄らない）
    meaningful: bool,
    /// 画面の切り替え（寄らない）
    wide: bool,
    /// 最後のクリックの時刻
    last_click: Option<u64>,
    /// このショットで一番寄れていた倍率
    best_zoom: f64,
}

impl Draft {
    fn new(s: &Subject) -> Self {
        Self {
            first: s.t,
            last: s.t,
            last_seen: s.last_seen,
            union: s.rect,
            meaningful: s.responded,
            wide: s.wide,
            last_click: (s.kind == ActivityKind::Click).then_some(s.t),
            best_zoom: 1.0,
        }
    }

    fn add(&mut self, s: &Subject) {
        self.last = s.t;
        self.last_seen = self.last_seen.max(s.last_seen);
        self.union = self.union.union(&s.rect);
        self.meaningful |= s.responded;
        if s.kind == ActivityKind::Click {
            self.last_click = Some(s.t);
        }
    }

    fn zoom(&self, max_zoom: f64, w: f64, h: f64) -> f64 {
        if self.wide || !self.meaningful {
            return 1.0;
        }
        let z = fit_zoom(&self.union, w, h).min(max_zoom);
        if z < spec::MIN_SHOT_ZOOM {
            1.0
        } else {
            z
        }
    }
}

/// 主役（時刻順）からショットを作る
fn plan_shots(subs: &[Subject], windows: &[(u64, Rect)], duration: u64, max_zoom: f64, w: f64, h: f64) -> Vec<Shot> {
    // S3 / S4: 主役を時間順にまとめる
    let mut drafts: Vec<Draft> = Vec::new();
    for s in subs {
        let Some(cur) = drafts.last_mut() else {
            drafts.push(Draft::new(s));
            continue;
        };
        let gap = s.t.saturating_sub(cur.last);
        let mut merged = cur.clone();
        merged.add(s);
        // まとめても十分に寄れるか（倍率が大きく下がるなら次のショットへ移る）。少しずつ下がり続けないよう、
        // このショットで一番寄れていた倍率と比べる
        let cur_zoom = cur.zoom(max_zoom, w, h);
        let best = cur.best_zoom.max(cur_zoom);
        let fits = fit_zoom(&merged.union, w, h) >= spec::MIN_SHOT_ZOOM.max(best * spec::SHOT_ZOOM_KEEP);
        // スクロールで主役（操作位置の周りの内容）が少しずれただけなら、枠を広げずにそのまま続ける。
        // 新しく現れたもの（ダイアログ等）は切らないよう、スクロール以外には使わない
        let covered = s.kind == ActivityKind::Scroll && !cur.wide && gap < spec::SCENE_BRIDGE_MS && cur_zoom > 1.0 && {
            let view = view_rect(&cur.union, cur_zoom, w, h);
            s.rect.intersect(&view).map_or(0.0, |r| r.area()) >= spec::SHOT_COVER_RATIO * s.rect.area()
                && (s.rect.area() > 0.0 || view.contains(&s.rect, 0.0))
        };
        let join = if s.wide || cur.wide || covered {
            false
        } else if gap < spec::MERGE_GAP_MS {
            // 続けて操作している: 主役がまとめて収まるなら同じショット。意味のない操作どうしも続ける
            fits || (!cur.meaningful && !s.responded)
        } else if gap < spec::SCENE_BRIDGE_MS && cur.meaningful {
            // 間が空いても、同じ場所（今の枠に収まる）の操作なら場面を続ける
            cur_zoom > 1.0 && fit_zoom(&merged.union, w, h) >= cur_zoom * 0.95
        } else {
            false
        };
        if join {
            *cur = merged;
            cur.best_zoom = best;
        } else if covered {
            let union = cur.union;
            cur.add(s);
            cur.union = union;
            cur.best_zoom = best;
        } else {
            drafts.push(Draft::new(s));
        }
    }

    // 時間帯（R3）と枠（S5）を決める
    let mut shots: Vec<Shot> = drafts
        .iter()
        .map(|d| {
            let zoom = d.zoom(max_zoom, w, h);
            let start = d.first.saturating_sub(spec::LEAD_MS);
            let end = (d.last + spec::HOLD_MS).max(d.last_seen + spec::RESPONSE_HOLD_MS).min(duration);
            let bound = window_at(windows, d.first)
                .filter(|b| b.contains(&d.union, spec::WINDOW_TOLERANCE_PX))
                .unwrap_or(Rect::new(0.0, 0.0, w, h));
            let zoom = inside_window_zoom(&d.union, &bound, zoom, max_zoom, w, h);
            let center = frame_center(&d.union, &bound, zoom, w, h);
            let keep_until = if d.wide { d.last } else { d.last_click.unwrap_or(0) };
            Shot { start, end, first: d.first, last: d.last, zoom, center, subject: d.union, keep_until }
        })
        .collect();

    arrange(&mut shots, duration);
    shots
}

/// 隣り合うショットの時間帯を調整する。
/// - 重なる・隙間が MIN_GAP 未満のショットは境目でつなぐ（寄る→引く→寄るの往復を防ぐ）
/// - 全体表示のショットに重なる寄ったショットは、全体表示が始まる前に引き始める
/// - 寄ったショットが MIN_SEGMENT より短ければ前後に延ばし、延ばせなければ寄らない
fn arrange(shots: &mut Vec<Shot>, duration: u64) {
    for i in 1..shots.len() {
        let (a, b) = shots.split_at_mut(i);
        let (prev, next) = (a.last_mut().unwrap(), &mut b[0]);
        // S4: 近い場所の操作が続くなら、間が空いても寄ったまま次のショットへ移る
        let same_place = next.first.saturating_sub(prev.last) < spec::SCENE_BRIDGE_MS
            && distance(&prev.subject, &next.subject) <= spec::NEAR_PX;
        if prev.end + spec::MIN_GAP_MS > next.start || (same_place && prev.zoom > 1.0 && next.zoom > 1.0) {
            if prev.zoom > 1.0 && next.zoom > 1.0 {
                // 寄ったまま次のショットへ移る。境目は次の操作の LEAD 前（前のショットの最後の操作より後）
                let boundary = next.start.max(prev.last + 1).min(next.first);
                prev.end = boundary;
                next.start = boundary;
            } else if prev.zoom > 1.0 {
                // 次は全体表示: 次の操作までに引き終える
                prev.end = prev.end.min(next.first.saturating_sub(spec::ZOOM_OUT_MS)).max(prev.last);
            } else if next.zoom > 1.0 {
                // 前は全体表示: 全体表示を保つ必要がある時刻（画面の切り替え・クリック）の後に寄り始める
                next.start = next.start.max(prev.keep_until.saturating_add(1)).min(next.first);
            }
        }
    }
    // 最短長: 孤立した寄ったショットだけを延ばす（つながったショットは合わせて数える）
    for i in 0..shots.len() {
        if shots[i].zoom <= 1.0 {
            continue;
        }
        let joined_prev = i > 0 && shots[i - 1].zoom > 1.0 && shots[i - 1].end == shots[i].start;
        let joined_next = i + 1 < shots.len() && shots[i + 1].zoom > 1.0 && shots[i + 1].start == shots[i].end;
        if joined_prev || joined_next {
            continue;
        }
        let next_limit = shots.get(i + 1).map_or(duration, |n| n.first.saturating_sub(spec::ZOOM_OUT_MS));
        let prev_limit = if i > 0 { shots[i - 1].keep_until } else { 0 };
        let s = &mut shots[i];
        if s.end < s.start + spec::MIN_SEGMENT_MS {
            s.end = (s.start + spec::MIN_SEGMENT_MS).min(next_limit.max(s.end));
        }
        if s.end < s.start + spec::MIN_SEGMENT_MS {
            s.start = s.end.saturating_sub(spec::MIN_SEGMENT_MS).max(prev_limit.min(s.start));
        }
        if s.end < s.start + spec::MIN_SEGMENT_MS {
            s.zoom = 1.0;
        }
    }
}

/// S5: 寄るショットで枠がウィンドウ `bound` より大きくなるなら、主役が（余白なしで）収まる範囲で
/// 倍率を上げて枠をウィンドウの中に収める（ウィンドウの外の別アプリを映さない）。
fn inside_window_zoom(subject: &Rect, bound: &Rect, zoom: f64, max_zoom: f64, w: f64, h: f64) -> f64 {
    if zoom <= 1.0 || bound.width() >= w - 1.0 && bound.height() >= h - 1.0 {
        return zoom;
    }
    let z_in = (w / bound.width()).max(h / bound.height());
    let fits = subject.width() <= w / z_in && subject.height() <= h / z_in;
    if z_in > zoom && z_in <= max_zoom && fits {
        z_in
    } else {
        zoom
    }
}

/// S5: 主役 `subject` を倍率 `zoom` で映す枠の中心。主役の中心を基本に、枠をウィンドウ `bound` の中、
/// さらにフレームの中に収める。主役は枠から外さない。
fn frame_center(subject: &Rect, bound: &Rect, zoom: f64, w: f64, h: f64) -> (f64, f64) {
    let (vw, vh) = (w / zoom, h / zoom);
    let axis = |c: f64, s0: f64, s1: f64, b0: f64, b1: f64, view: f64, size: f64| {
        let half = view / 2.0;
        // ウィンドウに収める（ウィンドウより枠が大きければウィンドウの中央）
        let c = if b1 - b0 >= view { c.clamp(b0 + half, b1 - half) } else { (b0 + b1) / 2.0 };
        // 主役を外さない
        let c = if s1 - s0 <= view { c.clamp(s1 - half, s0 + half) } else { (s0 + s1) / 2.0 };
        // フレームに収める
        c.clamp(half, size - half)
    };
    let (cx, cy) = subject.center();
    (
        axis(cx, subject.l, subject.r, bound.l, bound.r, vw, w),
        axis(cy, subject.t, subject.b, bound.t, bound.b, vh, h),
    )
}

/// 主役 `subject` を倍率 `zoom` で中央に映したときの枠（フレーム内に収めたもの）
fn view_rect(subject: &Rect, zoom: f64, w: f64, h: f64) -> Rect {
    let (hw, hh) = (w / zoom / 2.0, h / zoom / 2.0);
    let (cx, cy) = subject.center();
    let (cx, cy) = (cx.clamp(hw, w - hw), cy.clamp(hh, h - hh));
    Rect::new(cx - hw, cy - hh, cx + hw, cy + hh)
}

/// 前面ウィンドウの矩形の履歴 `(t, rect)`（フレーム内に切り詰め済み）
fn window_rects(events: &[RecordingEvent], w: f64, h: f64) -> Vec<(u64, Rect)> {
    let frame = Rect::new(0.0, 0.0, w, h);
    events
        .iter()
        .filter_map(|e| match e {
            RecordingEvent::WindowFocus { t, rect, .. } => {
                Some((*t, Rect::from_array(*rect).intersect(&frame).unwrap_or(frame)))
            }
            _ => None,
        })
        .collect()
}

/// 時刻 `t` の前面ウィンドウの矩形
fn window_at(windows: &[(u64, Rect)], t: u64) -> Option<Rect> {
    let i = windows.partition_point(|&(wt, _)| wt <= t);
    i.checked_sub(1).map(|i| windows[i].1)
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: f64 = 1920.0;
    const H: f64 = 1080.0;

    fn sub(t: u64, r: [f64; 4], responded: bool) -> Subject {
        Subject { t, kind: ActivityKind::Click, rect: Rect::from_array(r), responded, wide: false, last_seen: t }
    }

    fn scroll(t: u64, r: [f64; 4]) -> Subject {
        Subject { kind: ActivityKind::Scroll, ..sub(t, r, true) }
    }

    fn shots(subs: &[Subject]) -> Vec<Shot> {
        plan_shots(subs, &[], 30_000, 1.8, W, H)
    }

    #[test]
    fn test_typing_shot_frames_subject_with_max_zoom() {
        let s = shots(&[sub(2000, [400.0, 48.0, 700.0, 120.0], true), sub(2500, [400.0, 48.0, 800.0, 320.0], true)]);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].zoom, 1.8);
        assert_eq!((s[0].start, s[0].end), (1400, 4000));
        // 主役は枠に入り、枠はフレーム内
        let (vw, vh) = (W / 1.8, H / 1.8);
        let (cx, cy) = s[0].center;
        assert!(cx - vw / 2.0 <= 400.0 && cx + vw / 2.0 >= 800.0 && cy - vh / 2.0 >= 0.0);
    }

    #[test]
    fn test_meaningless_clicks_stay_overview() {
        let s = shots(&[sub(2000, [90.0, 80.0, 90.0, 80.0], false), sub(4000, [1800.0, 900.0, 1800.0, 900.0], false)]);
        assert!(s.iter().all(|s| s.zoom == 1.0));
    }

    #[test]
    fn test_subject_too_large_is_overview() {
        let s = shots(&[sub(2000, [100.0, 100.0, 1800.0, 1000.0], true)]);
        assert_eq!(s[0].zoom, 1.0);
    }

    #[test]
    fn test_same_place_after_pause_continues_scene() {
        // メニュー → (5 秒) → 同じダイアログの中でクリック: 寄ったまま 1 ショット
        let s = shots(&[sub(3000, [576.0, 80.0, 1552.0, 640.0], true), sub(8000, [780.0, 470.0, 790.0, 480.0], true)]);
        assert_eq!(s.len(), 1);
        assert!(s[0].zoom > 1.25);
    }

    #[test]
    fn test_menu_then_dialog_stays_zoomed() {
        // メニュー（3.7 秒眺める）→ 近くに開いたダイアログ: 間で全体に戻らない
        let s = shots(&[sub(3500, [576.0, 146.0, 874.0, 592.0], true), sub(7967, [592.0, 80.0, 1552.0, 640.0], true)]);
        assert!(s.iter().all(|s| s.zoom > 1.0));
        assert!(s.windows(2).all(|p| p[0].end == p[1].start), "{:?}", s);
        // 遠い場所なら全体に戻る
        let s = shots(&[sub(3500, [576.0, 146.0, 874.0, 592.0], true), sub(7967, [1500.0, 700.0, 1800.0, 1000.0], true)]);
        assert_eq!(s.len(), 2);
        assert!(s[0].end < s[1].start);
    }

    #[test]
    fn test_small_drift_does_not_move_shot() {
        // 主役の大部分が今の枠に入っていれば枠を広げない
        let s = shots(&[scroll(3000, [400.0, 300.0, 1000.0, 800.0]), scroll(4000, [400.0, 360.0, 1000.0, 1000.0])]);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].subject, Rect::new(400.0, 300.0, 1000.0, 800.0));
        assert_eq!(s[0].last, 4000);
    }

    #[test]
    fn test_far_place_after_pause_is_new_scene_with_overview_between() {
        let s = shots(&[sub(3000, [1300.0, 80.0, 1700.0, 320.0], true), sub(13_000, [370.0, 690.0, 720.0, 940.0], true)]);
        assert_eq!(s.len(), 2);
        assert!(s[0].end + spec::MIN_GAP_MS <= s[1].start);
    }

    #[test]
    fn test_adjacent_zoomed_shots_are_joined() {
        // 続けて操作しているが、まとめると収まらない → 寄ったまま次のショットへ移る
        let s = shots(&[sub(3000, [50.0, 50.0, 400.0, 200.0], true), sub(4000, [1500.0, 850.0, 1850.0, 1000.0], true)]);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].end, s[1].start);
        assert!(s[0].zoom > 1.0 && s[1].zoom > 1.0);
    }

    #[test]
    fn test_frame_center_keeps_view_inside_window() {
        // 主役はウィンドウの左端近く: 枠はウィンドウの左端に合わせる（左の別ウィンドウを映さない）
        let bound = Rect::new(463.0, 94.0, 1643.0, 980.0);
        let subject = Rect::new(560.0, 450.0, 900.0, 500.0);
        let (cx, _) = frame_center(&subject, &bound, 1.8, W, H);
        assert!((cx - (463.0 + W / 1.8 / 2.0)).abs() < 1e-6);
    }

    #[test]
    fn test_zoom_up_to_keep_view_inside_window() {
        let bound = Rect::new(463.0, 94.0, 1643.0, 980.0);
        let subject = Rect::new(464.0, 244.0, 1273.0, 878.0);
        // 1.57 では枠（1223px）がウィンドウ（1180px）より広い → 1.63 に上げる
        let z = inside_window_zoom(&subject, &bound, 1.57, 1.8, W, H);
        assert!((z - W / 1180.0).abs() < 1e-9);
        // 上げると主役が収まらないなら上げない
        assert_eq!(inside_window_zoom(&Rect::new(464.0, 100.0, 1273.0, 900.0), &bound, 1.3, 1.8, W, H), 1.3);
    }

    #[test]
    fn test_short_isolated_shot_is_extended() {
        let s = shots(&[sub(5000, [400.0, 400.0, 500.0, 450.0], true)]);
        assert!(s[0].end - s[0].start >= spec::MIN_SEGMENT_MS);
    }
}
