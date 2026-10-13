//! 操作ごとの主役（映すべきもの）の矩形 (Issue #39)。仕様 `docs/camera-spec-v6.md` §4 S1〜S2。
//!
//! 主役は「操作の結果として画面のどこが変わったか」で決める。マウスカーソルの位置は使わない
//! （クリック・ドラッグ・スクロールの位置だけを使う）。アプリごとの分岐は持たない。

use super::{spec, Activity, ActivityKind};
use crate::changes::{FrameChange, Rect};

/// 1つの操作の主役
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Subject {
    pub t: u64,
    pub kind: ActivityKind,
    /// 映すべき矩形（キャプチャ領域基準）
    pub rect: Rect,
    /// 操作で画面が変化したか（変化しないクリックは意味のない操作として寄らない）
    pub responded: bool,
    /// 画面の切り替え（場面転換）を起こしたか
    pub wide: bool,
    /// 反応が最後に見えた時刻（反応が無ければ操作の時刻）
    pub last_seen: u64,
}

/// 主役を求めるための入力
pub struct SubjectInput<'a> {
    pub activities: &'a [Activity],
    pub changes: &'a [FrameChange],
    pub width: f64,
    pub height: f64,
    /// 寄るときの倍率（スクロール等で主役が大きすぎるときの切り出しに使う）
    pub zoom: f64,
}

/// 各アクティビティの主役。反応の無いキー入力（修飾キーだけ等）は除く。
pub fn subjects(input: &SubjectInput) -> Vec<Subject> {
    let ambient = ambient_rects(input.activities, input.changes);
    let frame = Rect::new(0.0, 0.0, input.width, input.height);
    let mut out = Vec::new();
    for (i, a) in input.activities.iter().enumerate() {
        // 反応の時間帯: (t, t + RESPONSE] を、位置のある次の操作（クリック・ドラッグ等）で打ち切る。
        // 続けて打つキー入力・続けて回すスクロールでは打ち切らない（同じ変化が続く操作の主役に重なってよい）
        let next_pos_t = input.activities[i + 1..]
            .iter()
            .find(|b| b.pos.is_some() && b.t > a.t && !(a.kind == ActivityKind::Scroll && b.kind == ActivityKind::Scroll))
            .map(|b| b.t);
        let until = (a.t + spec::RESPONSE_MS).min(next_pos_t.unwrap_or(u64::MAX));
        let from = input.changes.partition_point(|c| c.t <= a.t);
        let to = input.changes.partition_point(|c| c.t <= until);
        let window = &input.changes[from..to.max(from)];
        let mut candidates: Vec<(Rect, u32, u64)> = window
            .iter()
            .flat_map(|c| c.rects.iter().map(move |r| (r.rect, r.cells, c.t)))
            .filter(|(r, cells, _)| *cells >= spec::MIN_CHANGE_CELLS && !is_ambient(r, &ambient))
            .filter_map(|(r, cells, t)| r.intersect(&frame).map(|r| (r, cells, t)))
            .collect();
        // 大きな変化（ダイアログ等）に付随する小さな片は除く（操作位置を含むものは残す）
        let max_cells = candidates.iter().map(|c| c.1).max().unwrap_or(0) as f64;
        candidates.retain(|(r, cells, _)| {
            *cells as f64 >= max_cells * spec::FRAGMENT_RATIO || a.pos.is_some_and(|(x, y)| r.contains_point(x, y))
        });

        // 位置の無いキー入力は、直前の入力の主役に近い変化（入力は同じ場所で続く）、無ければ最大の変化を種にする
        let prev_key = out
            .last()
            .filter(|p: &&Subject| p.kind == ActivityKind::Key && a.t - p.t < spec::MERGE_GAP_MS)
            .map(|p| p.rect);
        let seed = match (a.pos, prev_key) {
            (Some((x, y)), _) => Some(Rect::point(x, y)),
            (None, Some(prev)) => candidates
                .iter()
                .filter(|c| distance(&prev, &c.0) <= spec::NEAR_PX)
                .max_by_key(|c| c.1)
                .or_else(|| candidates.iter().max_by_key(|c| c.1))
                .map(|c| c.0),
            (None, None) => candidates.iter().max_by_key(|c| c.1).map(|c| c.0),
        };
        let Some(seed) = seed else {
            continue; // 反応の無いキー入力
        };
        let (response, last_seen) = grow(seed, &candidates);
        let responded = response.is_some();
        let mut rect = response.map_or(seed, |r| r.union(&seed));
        // 反応が寄れないほど大きい（画面・ページ・ペインの切り替え）なら場面転換
        let wide = response.is_some_and(|r| fit_zoom(&r, input.width, input.height) < spec::MIN_SHOT_ZOOM)
            && a.kind != ActivityKind::Scroll;
        if a.kind == ActivityKind::Scroll && !fits(&rect, input.zoom, input.width, input.height) {
            // スクロールしている領域が寄った枠より大きければ、操作位置の周りの枠に入る分だけを映す
            let (cx, cy) = a.pos.unwrap_or_else(|| seed.center());
            let (hw, hh) = (input.width / input.zoom / 2.0, input.height / input.zoom / 2.0);
            let around = Rect::new(cx - hw, cy - hh, cx + hw, cy + hh);
            rect = rect.intersect(&around).unwrap_or(seed);
        }
        out.push(Subject {
            t: a.t,
            kind: a.kind,
            rect,
            responded: responded || matches!(a.kind, ActivityKind::Drag),
            wide,
            last_seen: last_seen.unwrap_or(a.t),
        });
    }
    out
}

/// 種の矩形から、近い変化領域と大きな変化領域を取り込んで反応の矩形を広げる。
/// 取り込んだものが無ければ None。
fn grow(seed: Rect, candidates: &[(Rect, u32, u64)]) -> (Option<Rect>, Option<u64>) {
    let mut used = vec![false; candidates.len()];
    let mut union: Option<Rect> = None;
    let mut last_seen = None;
    loop {
        let base = union.map_or(seed, |u| u.union(&seed));
        let mut grew = false;
        for (k, (r, cells, t)) in candidates.iter().enumerate() {
            if used[k] {
                continue;
            }
            if distance(&base, r) <= spec::NEAR_PX || *cells >= spec::BIG_CHANGE_CELLS {
                used[k] = true;
                grew = true;
                union = Some(union.map_or(*r, |u| u.union(r)));
                last_seen = Some(last_seen.map_or(*t, |s: u64| s.max(*t)));
            }
        }
        if !grew {
            return (union, last_seen);
        }
    }
}

/// 矩形同士の距離（重なっていれば 0）
pub fn distance(a: &Rect, b: &Rect) -> f64 {
    let dx = (b.l - a.r).max(a.l - b.r).max(0.0);
    let dy = (b.t - a.b).max(a.t - b.b).max(0.0);
    dx.hypot(dy)
}

/// 操作の無い時間に変化した小さな領域（常に動いている場所）
fn ambient_rects(activities: &[Activity], changes: &[FrameChange]) -> Vec<Rect> {
    let idle = |t: u64| {
        let i = activities.partition_point(|a| a.t + spec::AMBIENT_IDLE_MS < t);
        activities.get(i).map_or(true, |a| a.t > t + spec::AMBIENT_IDLE_MS)
    };
    changes
        .iter()
        .filter(|c| idle(c.t))
        .flat_map(|c| c.rects.iter())
        .filter(|r| r.cells <= spec::AMBIENT_MAX_CELLS)
        .map(|r| r.rect)
        .collect()
}

fn is_ambient(r: &Rect, ambient: &[Rect]) -> bool {
    ambient.iter().any(|a| a.inflate(16.0).contains(r, 0.0))
}

/// 矩形を余白付きで映せる最大の倍率
pub fn fit_zoom(r: &Rect, width: f64, height: f64) -> f64 {
    let (mx, my) = (width * spec::SUBJECT_MARGIN, height * spec::SUBJECT_MARGIN);
    let zx = width / (r.width() + 2.0 * mx);
    let zy = height / (r.height() + 2.0 * my);
    zx.min(zy)
}

fn fits(r: &Rect, zoom: f64, width: f64, height: f64) -> bool {
    fit_zoom(r, width, height) >= zoom
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::changes::ChangeRect;

    const W: f64 = 1920.0;
    const H: f64 = 1080.0;

    fn change(t: u64, rects: &[([f64; 4], u32)]) -> FrameChange {
        FrameChange {
            t,
            rects: rects.iter().map(|&(r, cells)| ChangeRect { rect: Rect::from_array(r), cells }).collect(),
        }
    }

    fn act(t: u64, kind: ActivityKind, pos: Option<(f64, f64)>) -> Activity {
        Activity { t, kind, pos }
    }

    fn run(activities: &[Activity], changes: &[FrameChange]) -> Vec<Subject> {
        subjects(&SubjectInput { activities, changes, width: W, height: H, zoom: 1.8 })
    }

    #[test]
    fn test_typing_subject_is_changed_line_not_cursor() {
        // キー入力（位置なし）: 反応のうち最大の変化と、その近くの変化をまとめる。遠くの小さな変化は除く
        let acts = [act(1000, ActivityKind::Key, None)];
        let changes = [change(1400, &[([400.0, 48.0, 1184.0, 320.0], 171), ([1568.0, 1008.0, 1584.0, 1024.0], 4)])];
        let s = run(&acts, &changes);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].rect, Rect::new(400.0, 48.0, 1184.0, 320.0));
        assert!(s[0].responded && !s[0].wide);
        assert_eq!(s[0].last_seen, 1400);
    }

    #[test]
    fn test_typing_continues_near_previous_subject() {
        // 2回目の入力の反応では、遠くの大きな変化（スクロールバーの帯）より前の入力に近い変化を選ぶ
        let acts = [act(1000, ActivityKind::Key, None), act(2000, ActivityKind::Key, None)];
        let changes = [
            change(1400, &[([400.0, 48.0, 800.0, 120.0], 60)]),
            change(2400, &[([480.0, 64.0, 672.0, 320.0], 64), ([1888.0, 64.0, 1920.0, 992.0], 115)]),
        ];
        let s = run(&acts, &changes);
        assert_eq!(s[1].rect, Rect::new(480.0, 64.0, 672.0, 320.0));
    }

    #[test]
    fn test_click_without_response_is_not_meaningful() {
        let acts = [act(1000, ActivityKind::Click, Some((90.0, 80.0)))];
        let s = run(&acts, &[change(5000, &[([0.0, 0.0, 100.0, 100.0], 50)])]);
        assert_eq!(s[0].rect, Rect::point(90.0, 80.0));
        assert!(!s[0].responded);
    }

    #[test]
    fn test_click_includes_big_far_response() {
        // メニューの項目をクリック → 離れた場所に大きなダイアログ。遠くの細長い変化（スクロールバー）と小さな片は含めない
        let acts = [act(1000, ActivityKind::Click, Some((652.0, 380.0)))];
        let s = run(
            &acts,
            &[change(2100, &[([700.0, 80.0, 1552.0, 640.0], 547), ([1888.0, 64.0, 1920.0, 992.0], 115), ([576.0, 656.0, 608.0, 752.0], 12)])],
        );
        assert_eq!(s[0].rect, Rect::new(652.0, 80.0, 1552.0, 640.0));
    }

    #[test]
    fn test_ambient_changes_are_ignored() {
        // 操作の無い時間にも点滅している場所（キャレット）は主役にしない
        let blink = ([416.0, 64.0, 432.0, 96.0], 4);
        let changes = [change(100, &[blink]), change(10_000, &[blink])];
        let s = run(&[act(10_000 - 300, ActivityKind::Key, None)], &changes);
        assert!(s.is_empty());
    }

    #[test]
    fn test_scroll_of_large_pane_is_cut_around_pointer() {
        let acts = [act(1000, ActivityKind::Scroll, Some((780.0, 575.0)))];
        let s = run(&acts, &[change(1300, &[([368.0, 80.0, 1136.0, 880.0], 1500)])]);
        let r = s[0].rect;
        assert!(!s[0].wide);
        assert_eq!((r.l, r.r), (368.0, 1136.0));
        assert!((r.height() - H / 1.8).abs() < 1e-6 && r.contains_point(780.0, 575.0));
    }

    #[test]
    fn test_click_causing_screen_switch_is_wide() {
        let acts = [act(1000, ActivityKind::Click, Some((653.0, 620.0)))];
        let s = run(&acts, &[change(1500, &[([352.0, 32.0, 1136.0, 1008.0], 1139)])]);
        assert!(s[0].wide);
        // ページ全体の変化でも寄れる大きさなら場面転換ではない
        let s = run(&acts, &[change(1500, &[([464.0, 208.0, 1632.0, 976.0], 2555)])]);
        assert!(!s[0].wide);
    }

    #[test]
    fn test_response_window_stops_at_next_positioned_activity() {
        let acts = [act(1000, ActivityKind::Click, Some((100.0, 100.0))), act(1200, ActivityKind::Click, Some((1800.0, 900.0)))];
        // 2つ目のクリックの後の変化は、1つ目の反応ではない
        let s = run(&acts, &[change(1500, &[([1700.0, 800.0, 1900.0, 1000.0], 40)])]);
        assert!(!s[0].responded);
        assert!(s[1].responded);
    }

    #[test]
    fn test_fit_zoom() {
        // 幅 960 + 余白 2x48 → 1920/1056
        let z = fit_zoom(&Rect::new(0.0, 0.0, 960.0, 100.0), W, H);
        assert!((z - 1920.0 / 1056.0).abs() < 1e-9);
    }
}
