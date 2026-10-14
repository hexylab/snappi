//! 評価シナリオごとの期待値 `expect.json`。
//!
//! ```json
//! {
//!   "description": "操作 → 10秒放置 → 操作",
//!   "checks": [
//!     { "kind": "zoomed_out", "from_ms": 9000, "to_ms": 14000 },
//!     { "kind": "zoomed_in", "from_ms": 2500, "to_ms": 5000, "min_ratio": 0.9 },
//!     { "kind": "visible", "t_ms": 3000, "x": 400, "y": 300 },
//!     { "kind": "max_switches", "max": 4 }
//!   ]
//! }
//! ```

use crate::camera::{extract_activities, group_activities, spec, ActivityKind, CameraTrack, Viewport};
use crate::eval::metrics::zoom_switches;
use crate::recording_data::RecordingData;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const EXPECT_FILE: &str = "expect.json";

/// 下書きの期待時間帯に持たせる余裕 (ms)。遷移の終わり際・始まり際をチェックから外す
const DRAFT_MARGIN_MS: u64 = 500;
/// 下書きで zoomed_in / zoomed_out を置く時間帯の最短長 (ms)
const DRAFT_MIN_SPAN_MS: u64 = 1000;
/// 下書きで「遠い移動」とみなす距離（既定倍率のビューポート幅に対する倍数）
const DRAFT_FAR_JUMP_VIEWPORTS: f64 = 1.5;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Expectation {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub checks: Vec<Check>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Check {
    /// `[from_ms, to_ms]` の間、全体表示（寄っていない）であること
    ZoomedOut {
        from_ms: u64,
        to_ms: u64,
        #[serde(default = "default_ratio")]
        min_ratio: f64,
    },
    /// `[from_ms, to_ms]` の間、寄っていること
    ZoomedIn {
        from_ms: u64,
        to_ms: u64,
        #[serde(default = "default_ratio")]
        min_ratio: f64,
    },
    /// 時刻 `t_ms` に点 `(x, y)`（キャプチャ領域基準）がビューポート内に写っていること
    Visible { t_ms: u64, x: f64, y: f64 },
    /// 録画全体での寄る/引くの切り替え回数の上限
    MaxSwitches { max: usize },
    /// `[from_ms, to_ms]` の間、主役の矩形 `rect` = `[l, t, r, b]` がビューポートに収まっていること
    Framed {
        from_ms: u64,
        to_ms: u64,
        rect: [f64; 4],
        #[serde(default = "default_ratio")]
        min_ratio: f64,
    },
    /// `[from_ms, to_ms]` の間、ビューポートが矩形 `rect`（操作中のウィンドウ）の中に収まっていること
    Within {
        from_ms: u64,
        to_ms: u64,
        rect: [f64; 4],
        #[serde(default = "default_ratio")]
        min_ratio: f64,
    },
}

/// framed / within で許す誤差 (px)
const RECT_TOLERANCE_PX: f64 = 4.0;

fn default_ratio() -> f64 {
    1.0
}

#[derive(Debug, Clone)]
pub struct CheckResult {
    pub description: String,
    pub passed: bool,
}

impl Expectation {
    /// 録画ディレクトリの expect.json を読む。無ければ None。
    pub fn load(dir: &Path) -> anyhow::Result<Option<Self>> {
        let path = dir.join(EXPECT_FILE);
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(serde_json::from_str(&std::fs::read_to_string(path)?)?))
    }

    pub fn evaluate(&self, track: &CameraTrack, width: f64, height: f64) -> Vec<CheckResult> {
        self.checks
            .iter()
            .map(|c| c.evaluate(track, width, height))
            .collect()
    }

    /// 実録画を評価データに取り込むときの expect.json の下書きを、操作の時刻と位置から作る。
    ///
    /// - クリックごとに、その位置が写っていること（visible）
    /// - 操作のまとまり（R3・R4 で前後を延ばした区間）の最初から最後の操作まで寄っていること（zoomed_in, 90%）
    /// - 区間の外で、遷移の余裕を除いた時間帯は全体表示であること（zoomed_out）
    /// - 切り替え回数が区間数の2倍以下であること（max_switches）
    ///
    /// ビューポート幅の `DRAFT_FAR_JUMP_VIEWPORTS` 倍以上離れた移動を含む区間には zoomed_in を置かず、
    /// そうした録画には max_switches も置かない。下書きなので、主役（framed）・ウィンドウ（within）など
    /// 「編集者ならこう映す」は人が録画フレームを見て書き足すこと。
    pub fn draft(data: &RecordingData, description: &str) -> Self {
        let (w, h) = (data.width() as f64, data.height() as f64);
        let duration = data.meta.duration_ms;
        let activities = extract_activities(&data.events, duration);
        let far_jump = w / spec::ZOOM_STANDARD * DRAFT_FAR_JUMP_VIEWPORTS;

        // R3/R4 に従って区間 [start, end] を作り、隙間が MIN_GAP 未満なら結合する
        let mut segments: Vec<DraftSegment> = Vec::new();
        for (first, last) in group_activities(&activities, spec::MERGE_GAP_MS) {
            let start = first.saturating_sub(spec::LEAD_MS);
            let end = (last + spec::HOLD_MS).max(start + spec::MIN_SEGMENT_MS).min(duration);
            match segments.last_mut() {
                Some(prev) if start < prev.end + spec::MIN_GAP_MS => {
                    prev.end = prev.end.max(end);
                    prev.last = last;
                }
                _ => segments.push(DraftSegment { start, end, first, last, has_far_jump: false }),
            }
        }
        for seg in &mut segments {
            let positions: Vec<(f64, f64)> = activities
                .iter()
                .filter(|a| (seg.first..=seg.last).contains(&a.t))
                .filter_map(|a| a.pos)
                .collect();
            seg.has_far_jump = positions
                .windows(2)
                .any(|p| (p[1].0 - p[0].0).hypot(p[1].1 - p[0].1) >= far_jump);
        }

        let mut checks = Vec::new();
        for a in activities.iter().filter(|a| a.kind == ActivityKind::Click) {
            if let Some((x, y)) = a.pos.filter(|&(x, y)| (0.0..=w).contains(&x) && (0.0..=h).contains(&y)) {
                checks.push(Check::Visible { t_ms: a.t, x: x.round(), y: y.round() });
            }
        }
        for seg in segments.iter().filter(|s| !s.has_far_jump) {
            if seg.last >= seg.first + DRAFT_MIN_SPAN_MS {
                checks.push(Check::ZoomedIn { from_ms: seg.first, to_ms: seg.last, min_ratio: 0.9 });
            }
        }
        // 区間の外（録画冒頭・区間の間・末尾）は全体表示
        let mut out_from = 0;
        for seg in &segments {
            push_zoomed_out(&mut checks, out_from, seg.start.saturating_sub(DRAFT_MARGIN_MS));
            out_from = seg.end + spec::ZOOM_OUT_MS + DRAFT_MARGIN_MS;
        }
        push_zoomed_out(&mut checks, out_from, duration.saturating_sub(spec::END_GUARD_MS));
        if !segments.iter().any(|s| s.has_far_jump) {
            checks.push(Check::MaxSwitches { max: segments.len() * 2 });
        }

        Self { description: description.to_string(), checks }
    }
}

struct DraftSegment {
    start: u64,
    end: u64,
    first: u64,
    last: u64,
    has_far_jump: bool,
}

fn push_zoomed_out(checks: &mut Vec<Check>, from_ms: u64, to_ms: u64) {
    if to_ms >= from_ms + DRAFT_MIN_SPAN_MS {
        checks.push(Check::ZoomedOut { from_ms, to_ms, min_ratio: 1.0 });
    }
}

impl Check {
    fn evaluate(&self, track: &CameraTrack, width: f64, height: f64) -> CheckResult {
        let (description, passed) = match *self {
            Check::ZoomedOut { from_ms, to_ms, min_ratio } => {
                let r = zoomed_ratio(track, from_ms, to_ms).map(|r| 1.0 - r);
                (
                    format!("{}〜{}ms 全体表示 {} (基準 >= {:.0}%)", from_ms, to_ms, fmt_ratio(r), min_ratio * 100.0),
                    r.is_some_and(|r| r >= min_ratio),
                )
            }
            Check::ZoomedIn { from_ms, to_ms, min_ratio } => {
                let r = zoomed_ratio(track, from_ms, to_ms);
                (
                    format!("{}〜{}ms 寄っている {} (基準 >= {:.0}%)", from_ms, to_ms, fmt_ratio(r), min_ratio * 100.0),
                    r.is_some_and(|r| r >= min_ratio),
                )
            }
            Check::Visible { t_ms, x, y } => {
                let ok = track
                    .state_at(t_ms)
                    .is_some_and(|s| s.viewport(width, height).contains(x, y));
                (format!("{}ms に ({:.0}, {:.0}) が写っている", t_ms, x, y), ok)
            }
            Check::MaxSwitches { max } => {
                let switches = zoom_switches(track);
                (format!("切り替え {} 回 (基準 <= {})", switches, max), switches <= max)
            }
            Check::Framed { from_ms, to_ms, rect, min_ratio } => {
                let r = frame_ratio(track, from_ms, to_ms, |vp| {
                    vp.x - RECT_TOLERANCE_PX <= rect[0]
                        && vp.y - RECT_TOLERANCE_PX <= rect[1]
                        && vp.x + vp.width + RECT_TOLERANCE_PX >= rect[2]
                        && vp.y + vp.height + RECT_TOLERANCE_PX >= rect[3]
                }, width, height);
                (
                    format!("{}〜{}ms 主役 {:?} が枠内 {} (基準 >= {:.0}%)", from_ms, to_ms, rect, fmt_ratio(r), min_ratio * 100.0),
                    r.is_some_and(|r| r >= min_ratio),
                )
            }
            Check::Within { from_ms, to_ms, rect, min_ratio } => {
                let r = frame_ratio(track, from_ms, to_ms, |vp| {
                    vp.x + RECT_TOLERANCE_PX >= rect[0]
                        && vp.y + RECT_TOLERANCE_PX >= rect[1]
                        && vp.x + vp.width - RECT_TOLERANCE_PX <= rect[2]
                        && vp.y + vp.height - RECT_TOLERANCE_PX <= rect[3]
                }, width, height);
                (
                    format!("{}〜{}ms 枠がウィンドウ {:?} の中 {} (基準 >= {:.0}%)", from_ms, to_ms, rect, fmt_ratio(r), min_ratio * 100.0),
                    r.is_some_and(|r| r >= min_ratio),
                )
            }
        };
        CheckResult { description, passed }
    }
}

/// `[from_ms, to_ms]` のフレームのうち寄っている割合。該当フレームが無ければ None。
fn zoomed_ratio(track: &CameraTrack, from_ms: u64, to_ms: u64) -> Option<f64> {
    let zooms: Vec<bool> = (0..track.frames.len())
        .filter(|&k| (from_ms..=to_ms).contains(&track.time_ms(k)))
        .map(|k| track.frames[k].zoom >= spec::ZOOMED_THRESHOLD)
        .collect();
    (!zooms.is_empty()).then(|| zooms.iter().filter(|&&z| z).count() as f64 / zooms.len() as f64)
}

/// `[from_ms, to_ms]` のフレームのうち、ビューポートが `pred` を満たす割合。該当フレームが無ければ None。
fn frame_ratio(
    track: &CameraTrack,
    from_ms: u64,
    to_ms: u64,
    pred: impl Fn(&Viewport) -> bool,
    width: f64,
    height: f64,
) -> Option<f64> {
    let hits: Vec<bool> = (0..track.frames.len())
        .filter(|&k| (from_ms..=to_ms).contains(&track.time_ms(k)))
        .map(|k| pred(&track.frames[k].viewport(width, height)))
        .collect();
    (!hits.is_empty()).then(|| hits.iter().filter(|&&h| h).count() as f64 / hits.len() as f64)
}

fn fmt_ratio(r: Option<f64>) -> String {
    r.map_or("-".to_string(), |r| format!("{:.1}%", r * 100.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::CameraState;

    #[test]
    fn test_checks() {
        // 10fps・3秒: 1.0〜2.0秒だけ寄る
        let frames = (0..30)
            .map(|k| {
                let zoom = if (10..20).contains(&k) { 2.0 } else { 1.0 };
                CameraState { center_x: 100.0, center_y: 50.0, zoom }
            })
            .collect();
        let track = CameraTrack { fps: 10, frames };
        let exp: Expectation = serde_json::from_str(
            r#"{"checks":[
                {"kind":"zoomed_out","from_ms":0,"to_ms":900},
                {"kind":"zoomed_in","from_ms":1000,"to_ms":1900},
                {"kind":"zoomed_in","from_ms":500,"to_ms":1900,"min_ratio":0.9},
                {"kind":"visible","t_ms":1500,"x":190,"y":50},
                {"kind":"visible","t_ms":500,"x":190,"y":50},
                {"kind":"max_switches","max":1},
                {"kind":"framed","from_ms":1000,"to_ms":1900,"rect":[60,30,140,70]},
                {"kind":"framed","from_ms":1000,"to_ms":1900,"rect":[10,30,140,70]},
                {"kind":"within","from_ms":1000,"to_ms":1900,"rect":[40,20,160,80]},
                {"kind":"within","from_ms":0,"to_ms":1900,"rect":[40,20,160,80],"min_ratio":0.5}
            ]}"#,
        )
        .unwrap();
        let passed: Vec<bool> = exp.evaluate(&track, 200.0, 100.0).iter().map(|r| r.passed).collect();
        // 寄っている間のビューポートは [50, 25, 150, 75]
        assert_eq!(passed, vec![true, true, false, false, true, false, true, false, true, true]);
    }

    fn testdata(name: &str) -> RecordingData {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata/recordings").join(name);
        RecordingData::load(&dir).unwrap()
    }

    #[test]
    fn test_draft_idle_gaps() {
        // クリック+入力 → 10秒放置 → クリック+入力。2区間・間は全体表示
        let exp = Expectation::draft(&testdata("idle-gaps"), "idle");
        assert_eq!(exp.description, "idle");
        let zoomed_in = exp.checks.iter().filter(|c| matches!(c, Check::ZoomedIn { .. })).count();
        assert_eq!(zoomed_in, 2);
        assert!(exp.checks.iter().any(|c| matches!(c, Check::MaxSwitches { max: 4 })));
        let gap_out = exp.checks.iter().any(|c| {
            matches!(*c, Check::ZoomedOut { from_ms, to_ms, .. } if to_ms - from_ms >= 5000 && from_ms > 3000)
        });
        assert!(gap_out, "放置中の zoomed_out がない: {:?}", exp.checks);
        let visible = exp.checks.iter().filter(|c| matches!(c, Check::Visible { .. })).count();
        assert_eq!(visible, 2);
    }

    #[test]
    fn test_draft_far_jumps_skips_zoomed_in_and_switch_limit() {
        let exp = Expectation::draft(&testdata("far-jumps"), "");
        assert!(!exp.checks.iter().any(|c| matches!(c, Check::ZoomedIn { .. } | Check::MaxSwitches { .. })));
        let visible = exp.checks.iter().filter(|c| matches!(c, Check::Visible { .. })).count();
        assert_eq!(visible, 6);
    }

    #[test]
    fn test_draft_round_trips_as_json() {
        let exp = Expectation::draft(&testdata("vscode-typing"), "typing");
        let back: Expectation = serde_json::from_str(&serde_json::to_string(&exp).unwrap()).unwrap();
        assert_eq!(back.checks.len(), exp.checks.len());
    }
}
