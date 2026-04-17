use super::analyzer::{Rect, ScoredSegment};
use super::scene_splitter::Scene;
use crate::config::RecordingEvent;

/// A scored segment enriched with UI context information.
#[derive(Debug, Clone)]
pub struct UiEnrichedSegment {
    pub scored: ScoredSegment,
    /// UI element rectangle (from UI Automation) that corresponds to this segment
    pub ui_rect: Option<Rect>,
    /// Additional importance boost from UI event correlation
    pub ui_importance_boost: f64,
}

/// Time window (ms) for matching UI events to scored segments
const UI_MATCH_WINDOW_MS: u64 = 200;

/// Enrich scored segments with UI context from UI Automation events.
/// For each segment, find temporally close UI events and:
/// 1. Boost importance score based on UI event type
/// 2. Attach UI element rectangle for precision zoom targeting
pub fn enrich_with_ui_context(
    scored_segments: &[ScoredSegment],
    ui_events: &[RecordingEvent],
) -> Vec<UiEnrichedSegment> {
    scored_segments
        .iter()
        .map(|scored| {
            let seg_time = scored.segment.start_ms;
            let mut best_boost: f64 = 0.0;
            let mut best_rect: Option<Rect> = None;

            for event in ui_events {
                let (event_time, boost, rect) = match event {
                    RecordingEvent::UiFocus { t, rect, .. } => {
                        (*t, 0.3, Some(rect_from_array(rect)))
                    }
                    RecordingEvent::UiMenuOpen { t, rect, .. } => {
                        (*t, 0.5, Some(rect_from_array(rect)))
                    }
                    RecordingEvent::UiDialogOpen { t, rect, .. } => {
                        (*t, 0.6, Some(rect_from_array(rect)))
                    }
                    _ => continue,
                };

                let time_diff = if event_time > seg_time {
                    event_time - seg_time
                } else {
                    seg_time - event_time
                };

                if time_diff <= UI_MATCH_WINDOW_MS && boost > best_boost {
                    best_boost = boost;
                    best_rect = rect;
                }
            }

            UiEnrichedSegment {
                scored: ScoredSegment {
                    segment: scored.segment.clone(),
                    importance: (scored.importance + best_boost).min(1.0),
                },
                ui_rect: best_rect,
                ui_importance_boost: best_boost,
            }
        })
        .collect()
}

fn rect_from_array(arr: &[f64; 4]) -> Rect {
    Rect {
        x: arr[0],
        y: arr[1],
        width: arr[2] - arr[0],
        height: arr[3] - arr[1],
    }
}

// ==================================================================
// Scene-level UI enrichment (Phase A: Issue #23)
//
// シーン単位で UI Automation の矩形情報を紐付ける。zoom_planner が
// scene.ui_rect を使ってズーム中心/倍率をテキストボックスやボタンの
// 矩形に基づいて決められるようにする。
// ==================================================================

/// UI 矩形として採用する最小サイズ（px）。
/// これより小さい要素（チェックボックスの小さなラベル等）は点ベースに任せる。
pub const MIN_UI_RECT_SIZE: f64 = 60.0;

/// UI 矩形として採用しない最大面積比（画面全体との比率）。
/// これを超える rect は「実質的に画面全体」なので、bbox の方が情報量が多い。
pub const MAX_UI_RECT_SCREEN_RATIO: f64 = 0.85;

/// シーン開始前/終了後にも UI イベントを拾う猶予 (ms)。
/// タイピング前のフォーカスや、ダイアログ出現が少し早まるケースに対応。
const SCENE_UI_MATCH_WINDOW_MS: u64 = 500;

/// UI 要素の「ズームターゲットとしての有用性」スコア。
/// 同じシーンに複数 UI イベントがある場合、スコアが高い方を採用する。
fn ui_event_priority(event: &RecordingEvent) -> u32 {
    match event {
        // ダイアログは単体で完結した UI なので最優先（ダイアログ全体をフレーミングしたい）
        RecordingEvent::UiDialogOpen { .. } => 100,
        // メニューも単体 UI
        RecordingEvent::UiMenuOpen { .. } => 80,
        // フォーカス（テキスト入力・ボタン等） — 最も一般的
        RecordingEvent::UiFocus { control, .. } => {
            // 特定のコントロールタイプは優先度が高い
            match control.as_str() {
                "Edit" | "Document" | "ComboBox" => 70,   // テキスト入力系
                "Button" | "Hyperlink" | "MenuItem" | "CheckBox" | "RadioButton" => 50,
                "Pane" | "Window" | "Group" => 30,          // コンテナ系は優先度低
                _ => 40,
            }
        }
        _ => 0,
    }
}

/// UI イベントが有効な矩形と時刻を持っていれば返す。
fn extract_ui_rect(event: &RecordingEvent) -> Option<(u64, Rect)> {
    match event {
        RecordingEvent::UiFocus { t, rect, .. }
        | RecordingEvent::UiDialogOpen { t, rect, .. }
        | RecordingEvent::UiMenuOpen { t, rect, .. } => {
            Some((*t, rect_from_array(rect)))
        }
        _ => None,
    }
}

/// UI 矩形がズーム用として有効かを判定する。
/// 小さすぎる要素や、画面全体を覆うような大きな要素は除外する。
pub fn is_ui_rect_useful(rect: &Rect, screen_w: f64, screen_h: f64) -> bool {
    if rect.width < MIN_UI_RECT_SIZE || rect.height < MIN_UI_RECT_SIZE {
        return false;
    }
    let screen_area = screen_w * screen_h;
    if screen_area <= 0.0 {
        return false;
    }
    let rect_area = rect.width * rect.height;
    if rect_area / screen_area > MAX_UI_RECT_SCREEN_RATIO {
        return false;
    }
    // 負の座標や画面外の rect は除外
    if rect.x < -rect.width || rect.y < -rect.height {
        return false;
    }
    true
}

/// 各シーンに、そのシーン期間中にフォーカスされていた UI 要素の矩形を紐付ける。
///
/// マッチング規則:
/// - 時刻 t が `scene.start_ms - window` 〜 `scene.end_ms + window` の範囲にある UI イベントを候補にする
/// - 複数候補があれば `ui_event_priority()` が最大のものを選ぶ
/// - 同優先度なら、シーン期間内に発火したものを優先（範囲外より）
/// - 無効な矩形（小さすぎる・画面全体を覆う）は無視して次の候補を探す
///
/// UI 情報が取れなかったシーン（`scene.ui_rect = None`）は、従来の
/// bbox ベースのズーム計算にフォールバックする。
pub fn attach_ui_rects_to_scenes(
    scenes: &mut [Scene],
    events: &[RecordingEvent],
    screen_w: f64,
    screen_h: f64,
) {
    // UI イベントだけを抜き出して (時刻, 優先度, 矩形, イベント内発火フラグ用のイベント参照) にする
    let ui_candidates: Vec<(u64, u32, Rect)> = events
        .iter()
        .filter_map(|e| {
            let (t, rect) = extract_ui_rect(e)?;
            if !is_ui_rect_useful(&rect, screen_w, screen_h) {
                return None;
            }
            Some((t, ui_event_priority(e), rect))
        })
        .collect();

    if ui_candidates.is_empty() {
        return;
    }

    for scene in scenes.iter_mut() {
        let window_lo = scene.start_ms.saturating_sub(SCENE_UI_MATCH_WINDOW_MS);
        let window_hi = scene.end_ms.saturating_add(SCENE_UI_MATCH_WINDOW_MS);

        // 候補を (優先度, within_scene) で並べ替えて最良を選ぶ
        let mut best: Option<(u32, bool, Rect)> = None;
        for (t, prio, rect) in &ui_candidates {
            if *t < window_lo || *t > window_hi {
                continue;
            }
            let within = *t >= scene.start_ms && *t <= scene.end_ms;
            let candidate = (*prio, within, rect.clone());
            best = match best {
                None => Some(candidate),
                Some(ref current) => {
                    // シーン期間内 > 期間外 > 優先度高 > 優先度低
                    let better = match (current.1, candidate.1) {
                        (false, true) => true,
                        (true, false) => false,
                        _ => candidate.0 > current.0,
                    };
                    if better { Some(candidate) } else { best }
                }
            };
        }

        if let Some((_, _, rect)) = best {
            scene.ui_rect = Some(rect);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::analyzer::{FocusPoint, Segment, SegmentType};

    #[test]
    fn test_enrich_boosts_importance() {
        let segs = vec![ScoredSegment {
            segment: Segment {
                segment_type: SegmentType::Click,
                start_ms: 1000,
                end_ms: 1100,
                focus_point: Some(FocusPoint { x: 500.0, y: 300.0, region: None }),
                idle_level: None,
                window_rect: None,
                window_changed: false,
            },
            importance: 0.5,
        }];

        let ui_events = vec![RecordingEvent::UiFocus {
            t: 1050,
            control: "Button".to_string(),
            name: "OK".to_string(),
            rect: [480.0, 280.0, 560.0, 320.0],
            automation_id: "btnOk".to_string(),
        }];

        let enriched = enrich_with_ui_context(&segs, &ui_events);
        assert_eq!(enriched.len(), 1);
        assert!((enriched[0].scored.importance - 0.8).abs() < 0.01);
        assert!(enriched[0].ui_rect.is_some());
    }

    #[test]
    fn test_enrich_no_match_no_boost() {
        let segs = vec![ScoredSegment {
            segment: Segment {
                segment_type: SegmentType::Click,
                start_ms: 1000,
                end_ms: 1100,
                focus_point: Some(FocusPoint { x: 500.0, y: 300.0, region: None }),
                idle_level: None,
                window_rect: None,
                window_changed: false,
            },
            importance: 0.5,
        }];

        // UI event far away in time
        let ui_events = vec![RecordingEvent::UiFocus {
            t: 5000,
            control: "Button".to_string(),
            name: "OK".to_string(),
            rect: [480.0, 280.0, 560.0, 320.0],
            automation_id: "btnOk".to_string(),
        }];

        let enriched = enrich_with_ui_context(&segs, &ui_events);
        assert_eq!(enriched.len(), 1);
        assert!((enriched[0].scored.importance - 0.5).abs() < 0.01);
        assert!(enriched[0].ui_rect.is_none());
    }

    #[test]
    fn test_dialog_highest_boost() {
        let segs = vec![ScoredSegment {
            segment: Segment {
                segment_type: SegmentType::Click,
                start_ms: 1000,
                end_ms: 1100,
                focus_point: Some(FocusPoint { x: 500.0, y: 300.0, region: None }),
                idle_level: None,
                window_rect: None,
                window_changed: false,
            },
            importance: 0.3,
        }];

        let ui_events = vec![
            RecordingEvent::UiFocus {
                t: 1050,
                control: "Edit".to_string(),
                name: "".to_string(),
                rect: [480.0, 280.0, 560.0, 320.0],
                automation_id: "".to_string(),
            },
            RecordingEvent::UiDialogOpen {
                t: 1080,
                control: "Dialog".to_string(),
                name: "Save".to_string(),
                rect: [300.0, 200.0, 700.0, 500.0],
            },
        ];

        let enriched = enrich_with_ui_context(&segs, &ui_events);
        assert!((enriched[0].scored.importance - 0.9).abs() < 0.01, "Dialog should give highest boost");
        // Dialog rect should be chosen (higher boost)
        let r = enriched[0].ui_rect.as_ref().unwrap();
        assert!((r.x - 300.0).abs() < 0.01);
    }

    // ---- attach_ui_rects_to_scenes のテスト（Phase A） ----

    fn make_scene(id: u32, start_ms: u64, end_ms: u64) -> Scene {
        Scene {
            id,
            start_ms,
            end_ms,
            bbox: Rect { x: 0.0, y: 0.0, width: 100.0, height: 100.0 },
            center_x: 50.0,
            center_y: 50.0,
            zoom_level: 1.0,
            event_count: 1,
            ui_rect: None,
        }
    }

    #[test]
    fn test_attach_ui_rect_basic_focus() {
        let mut scenes = vec![make_scene(0, 1000, 2000)];
        let events = vec![RecordingEvent::UiFocus {
            t: 1500,
            control: "Edit".to_string(),
            name: "Search".to_string(),
            rect: [100.0, 100.0, 500.0, 180.0],  // 400x80: MIN_UI_RECT_SIZE=60 をクリア
            automation_id: "".to_string(),
        }];

        attach_ui_rects_to_scenes(&mut scenes, &events, 1920.0, 1080.0);
        let r = scenes[0].ui_rect.as_ref().expect("ui_rect should be attached");
        assert!((r.width - 400.0).abs() < 0.01);
        assert!((r.height - 80.0).abs() < 0.01);
    }

    #[test]
    fn test_attach_ui_rect_ignores_tiny_rects() {
        let mut scenes = vec![make_scene(0, 1000, 2000)];
        // 30x30 は MIN_UI_RECT_SIZE (60) 未満
        let events = vec![RecordingEvent::UiFocus {
            t: 1500,
            control: "Button".to_string(),
            name: "x".to_string(),
            rect: [0.0, 0.0, 30.0, 30.0],
            automation_id: "".to_string(),
        }];

        attach_ui_rects_to_scenes(&mut scenes, &events, 1920.0, 1080.0);
        assert!(scenes[0].ui_rect.is_none(), "tiny rect must be ignored");
    }

    #[test]
    fn test_attach_ui_rect_ignores_screen_sized_rects() {
        let mut scenes = vec![make_scene(0, 1000, 2000)];
        // 画面全体を覆うペイン（フルスクリーンアプリ等）
        let events = vec![RecordingEvent::UiFocus {
            t: 1500,
            control: "Pane".to_string(),
            name: "".to_string(),
            rect: [0.0, 0.0, 1920.0, 1080.0],
            automation_id: "".to_string(),
        }];

        attach_ui_rects_to_scenes(&mut scenes, &events, 1920.0, 1080.0);
        assert!(scenes[0].ui_rect.is_none(), "screen-sized rect must be ignored");
    }

    #[test]
    fn test_attach_ui_rect_prefers_dialog_over_focus() {
        let mut scenes = vec![make_scene(0, 1000, 2000)];
        let events = vec![
            RecordingEvent::UiFocus {
                t: 1200,
                control: "Edit".to_string(),
                name: "".to_string(),
                rect: [100.0, 100.0, 300.0, 200.0],
                automation_id: "".to_string(),
            },
            RecordingEvent::UiDialogOpen {
                t: 1500,
                control: "Window".to_string(),
                name: "Save As".to_string(),
                rect: [400.0, 200.0, 1200.0, 700.0],
            },
        ];

        attach_ui_rects_to_scenes(&mut scenes, &events, 1920.0, 1080.0);
        let r = scenes[0].ui_rect.as_ref().unwrap();
        // ダイアログが選ばれる（優先度 100 > Edit の 70）
        assert!((r.x - 400.0).abs() < 0.01, "dialog rect should be selected");
    }

    #[test]
    fn test_attach_ui_rect_prefers_within_scene() {
        let mut scenes = vec![make_scene(0, 1000, 2000)];
        let events = vec![
            // 境界外（同優先度）
            RecordingEvent::UiFocus {
                t: 800,
                control: "Edit".to_string(),
                name: "".to_string(),
                rect: [100.0, 100.0, 300.0, 200.0],
                automation_id: "".to_string(),
            },
            // シーン期間内（優先される）
            RecordingEvent::UiFocus {
                t: 1500,
                control: "Edit".to_string(),
                name: "".to_string(),
                rect: [600.0, 100.0, 900.0, 200.0],
                automation_id: "".to_string(),
            },
        ];

        attach_ui_rects_to_scenes(&mut scenes, &events, 1920.0, 1080.0);
        let r = scenes[0].ui_rect.as_ref().unwrap();
        assert!((r.x - 600.0).abs() < 0.01, "within-scene event should be preferred");
    }

    #[test]
    fn test_attach_ui_rect_skips_far_events() {
        let mut scenes = vec![make_scene(0, 1000, 2000)];
        let events = vec![RecordingEvent::UiFocus {
            t: 5000,  // シーン終了後 3 秒以上経過（猶予 500ms を超える）
            control: "Edit".to_string(),
            name: "".to_string(),
            rect: [100.0, 100.0, 500.0, 200.0],
            automation_id: "".to_string(),
        }];

        attach_ui_rects_to_scenes(&mut scenes, &events, 1920.0, 1080.0);
        assert!(scenes[0].ui_rect.is_none(), "far event must not match");
    }

    #[test]
    fn test_is_ui_rect_useful() {
        // 通常サイズ → OK
        let r = Rect { x: 100.0, y: 100.0, width: 300.0, height: 100.0 };
        assert!(is_ui_rect_useful(&r, 1920.0, 1080.0));

        // 小さすぎる → NG
        let r = Rect { x: 100.0, y: 100.0, width: 40.0, height: 40.0 };
        assert!(!is_ui_rect_useful(&r, 1920.0, 1080.0));

        // 画面全体 → NG
        let r = Rect { x: 0.0, y: 0.0, width: 1920.0, height: 1080.0 };
        assert!(!is_ui_rect_useful(&r, 1920.0, 1080.0));
    }
}
