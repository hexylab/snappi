# Snappi 自動エフェクト仕様書 v4 — シーンベース先読みズーム

**Status**: 実装済み
**Previous**: `auto-effects-spec-v3.md`（3段階ズームモデル — 設計のみ）
**Last Updated**: 2026-02-08

---

## 1. 概要

### 1.1 背景と動機

v3仕様で導入した3段階ズームモデル（Overview / Window / WorkArea）は、v2のクリック座標追従方式に比べて大きく改善されたが、**スライディングウィンドウによる逐次的クラスタリング** に依存する設計には根本的な問題が残っていた:

1. **ズーム追従が1秒遅れる**: クラスタが「安定」するまで1000ms待つため、ボタンをクリックした瞬間にカメラが寄れない
2. **文字入力の位置検出が不正確**: Keyイベントに座標がないため「最後のマウス位置」を使用。ターミナルのように「どこをクリックしても文字入力できる」UIでは、クリック位置と入力位置が一致しない
3. **最初の操作のズーム位置が反映されない**: クラスタ安定待ちにより、録画冒頭の操作にカメラが追従しない
4. **操作箇所が画面外に出る**: Spring物理の遅延とクラスタ安定待ちの複合で、実際の操作位置にカメラが到達するまで時間がかかる

### 1.2 核心的な気づき: エクスポートはオフライン処理

Snappiの2パス方式では、エクスポート時にはすべてのイベントが確定している。にもかかわらず、v3では「リアルタイム処理かのように」イベントを逐次的にクラスタリングしていた。

**v4の根本的な変更**: 全イベントを先読みし、グローバルに最適なズーム計画を立てる。

### 1.3 新しいアプローチ: シーンベース先読みズーム

```
v3: 時系列 → 逐次クラスタリング → 安定待ち → ズーム（遅延あり）
v4: 全イベント読み込み → シーン分割 → 先読みズーム計画 → 先行カメラ移動（遅延なし）
```

「シーン」とは、連続するユーザー操作の期間を表す。アイドル区間やウィンドウ切り替えで自然に区切られる。

### 1.4 設計原則

| # | 原則 | 説明 |
|---|------|------|
| 1 | **グローバル先読み** | 全イベントを事前に読み込んでからズーム計画を立てる |
| 2 | **先行カメラ移動** | シーン開始「前」にカメラを動かし始める（遅延ゼロを実現） |
| 3 | **最初のシーンは即座** | t=0にキーフレームを配置し、録画冒頭から正しいズーム位置を保証 |
| 4 | **3段階Tierは維持** | Overview (1.0x) / Window / WorkArea の3段階ズームレベルはv3と同じ |
| 5 | **カーソル追従なし** | v3から引き続き、カーソル追従（Push Zone）は使用しない |
| 6 | **Spring物理は共通** | Critically Damped Springはv2/v3と同じエンジンを使用 |

---

## 2. パイプライン

### 2.1 全体フロー

```
[録画フェーズ] ── 変更なし
  events.jsonl, frames/*.png, audio.wav, meta.json

        ↓

[前処理] ── 変更なし
  preprocessor.rs: thin_mouse_moves(), detect_drags()

        ↓

[1. シーン分割] ── 新規 (scene_splitter.rs)
  全イベント読み込み
    → アクティビティポイント抽出
    → Idle gap (≥1500ms) / ウィンドウ変更 でシーン分割
    → 大シーンの空間分割
    → Vec<Scene> 出力

        ↓

[2. 先読みズーム計画] ── 全面書き換え (zoom_planner.rs)
  Vec<Scene> + RecordingMeta + EffectsSettings
    → 各シーンに先行キーフレーム配置
    → Idle区間にズームアウトキーフレーム配置
    → Vec<ZoomKeyframe> 出力

        ↓

[3. フレーム合成] ── 変更なし (compositor.rs)
  Vec<ZoomKeyframe> → Spring物理 → フレームごとの (zoom, pan_x, pan_y) 算出
  → 画像クロップ＋エフェクト描画

        ↓

[エクスポート] ── 呼び出しAPI変更 (encoder.rs)
  合成済みフレーム → FFmpeg → MP4/GIF/WebM
```

### 2.2 変更ファイル一覧

| ファイル | 変更内容 | 状態 |
|---------|---------|------|
| `engine/scene_splitter.rs` | **新規**: シーン分割・分析モジュール | 新規作成 |
| `engine/zoom_planner.rs` | **全面書き換え**: シーンベース先読みプランナー | 書き換え |
| `engine/activity_cluster.rs` | **除外**: mod.rsから削除（ファイルは残存） | 非コンパイル |
| `engine/mod.rs` | `activity_cluster` → `scene_splitter` に変更 | 変更済み |
| `export/encoder.rs` | 新API呼び出しに変更 | 変更済み |
| `engine/compositor.rs` | 変更なし（ZoomKeyframe消費は同一） | 変更なし |
| `engine/spring.rs` | 変更なし（物理エンジンは共通） | 変更なし |

### 2.3 廃止されたコンポーネント

| コンポーネント | v3での役割 | v4での代替 |
|-------------|----------|----------|
| `activity_cluster.rs` | スライディングウィンドウクラスタリング | `scene_splitter.rs` のグローバルシーン分割 |
| `analyzer.rs` の `analyze_events()` | セグメント分類・スコアリング | 不使用（`event_timestamp()` のみ使用） |
| `analyzer.rs` の `drags_to_segments()` | ドラッグセグメント変換 | 不使用 |
| クラスタ安定待ち (1000ms) | WorkArea確定のための遅延 | 不要（シーンは全イベントから事前計算） |

---

## 3. シーン分割 (`scene_splitter.rs`)

### 3.1 Scene 構造体

```rust
pub struct Scene {
    pub id: u32,          // 一意のシーンID
    pub start_ms: u64,    // 最初のイベント時刻
    pub end_ms: u64,      // 最後のイベント時刻
    pub bbox: Rect,       // パディング済みバウンディングボックス
    pub center_x: f64,    // BBox中心X
    pub center_y: f64,    // BBox中心Y
    pub zoom_level: f64,  // 最適ズーム倍率
    pub window_rect: Option<Rect>,  // 所属ウィンドウ矩形
    pub event_count: usize, // 含まれるイベント数
}
```

### 3.2 アクティビティポイント抽出

イベントからアクティビティポイント（座標付き時刻点）を抽出する。

| イベント | 座標の取得元 | 備考 |
|---------|-----------|------|
| Click | クリック座標 (x, y) | クラスタの主要形成要素 |
| ClickRelease | リリース座標 (x, y) | ドラッグ終了位置 |
| Key | 直近2秒以内のClick座標 or ウィンドウ中心 | **v4の重要改善点** |
| Scroll | マウス座標 (x, y) | スクロール操作 |
| Focus | UI要素の中心座標 | フォーカス変更 |
| WindowFocus | — (座標なし) | ウィンドウコンテキスト更新のみ |

#### Keyイベントの座標決定ロジック（v4の改善）

```
Keyイベント発生時:
  1. 直近2秒以内にClickがある場合
     → そのClick座標を使用（クリック→入力のパターン）
  2. Clickがない or 2秒以上前の場合
     → 現在のWindowFocusのウィンドウ中心座標を使用
  3. どちらもない場合
     → イベントを無視（座標が不明）
```

これにより、以下の問題が解決される:
- **テキストエディタ**: クリック位置に入力 → Click座標を使用（正確）
- **ターミナル**: クリックせずに入力開始 → ウィンドウ中心を使用（適切）
- **フォーム**: フォーカスが変わる → Focus座標を使用（適切）

### 3.3 シーン分割ロジック

#### パラメータ

```
SCENE_GAP_MS                = 1500ms     // アイドルギャップ閾値
SUB_SCENE_SPATIAL_JUMP_PX   = 400px      // サブシーン分割の空間閾値
SUB_SCENE_TIME_GAP_MS       = 500ms      // サブシーン分割の時間閾値
BBOX_PADDING                = 80px       // バウンディングボックスの余白
MIN_BBOX_SIZE               = 200px      // BBoxの最小サイズ（幅・高さ）
MAX_BBOX_SCREEN_FRACTION    = 0.5        // 画面面積の最大割合（これを超えるとサブシーン分割）
RECENT_CLICK_WINDOW_MS      = 2000ms     // Keyイベントに使う「最近のクリック」の時間窓
```

#### Phase 1: 時間・ウィンドウによるシーン分割

```
入力: points[] — 全アクティビティポイントの時系列配列
出力: raw_groups[] — ポイントのグループ（各グループが1つのシーン候補）

処理:
  current_group = [points[0]]

  for i in 1..points.len():
    time_gap = points[i].time_ms - points[i-1].time_ms
    window_changed = windows_differ(points[i].window, points[i-1].window)

    if time_gap >= SCENE_GAP_MS || window_changed:
      raw_groups.push(current_group)
      current_group = new group

    current_group.push(points[i])

  raw_groups.push(current_group)
```

シーン分割の2つの条件:
1. **アイドルギャップ ≥ 1500ms**: ユーザーが操作を一時停止した
2. **ウィンドウ変更**: 別のウィンドウに操作が移った（50pxの余裕で微小変動を無視）

#### Phase 2: 大シーンの空間分割

Phase 1で生成されたシーンのBBoxが画面面積の50%を超える場合、空間的なジャンプでサブシーンに分割する。

```
分割条件: time_gap ≥ 500ms AND spatial_distance ≥ 400px

例: VSCode操作
  ┌─────────────────────────────────┐
  │ サイドバー          エディタ      │
  │ [Click群]  ←400px→  [Click群]  │
  │ Sub-Scene A        Sub-Scene B │
  └─────────────────────────────────┘
```

分割が成功しなかった場合（分割点が見つからない）は、単一シーンのまま維持する。

### 3.4 BBox計算

```rust
fn compute_bbox(points: &[&ActivityPoint]) -> Rect {
    let min_x / max_x / min_y / max_y = 全ポイントの最小最大;
    let raw_w = max_x - min_x;
    let raw_h = max_y - min_y;
    let w = raw_w.max(MIN_BBOX_SIZE);   // 最小200px保証
    let h = raw_h.max(MIN_BBOX_SIZE);
    let cx = min_x + raw_w / 2.0;
    let cy = min_y + raw_h / 2.0;

    Rect {
        x: cx - w/2 - BBOX_PADDING,     // 80pxパディング
        y: cy - h/2 - BBOX_PADDING,
        width: w + BBOX_PADDING * 2,
        height: h + BBOX_PADDING * 2,
    }
}
```

### 3.5 ズーム倍率計算

```rust
fn calc_scene_zoom(bbox: &Rect, screen_w: f64, screen_h: f64, max_zoom: f64) -> f64 {
    let fit_zoom = min(screen_w / bbox.width, screen_h / bbox.height);
    fit_zoom.clamp(1.2, max_zoom)  // 1.2x 〜 max_zoom の範囲
}

fn calc_window_zoom(window_rect: &Rect, screen_w: f64, screen_h: f64, max_zoom: f64) -> f64 {
    // 5%パディング付き
    let fit_zoom = min(screen_w / padded_w, screen_h / padded_h);
    fit_zoom.clamp(1.1, max_zoom)  // 1.1x 〜 max_zoom の範囲
}
```

### 3.6 公開API

```rust
/// 全イベントからシーンに分割する。
pub fn split_into_scenes(
    events: &[RecordingEvent],
    screen_w: f64,
    screen_h: f64,
    max_zoom: f64,
) -> Vec<Scene>

/// ウィンドウに対するズーム倍率を計算する。
pub fn calc_window_zoom(
    window_rect: &Rect,
    screen_w: f64,
    screen_h: f64,
    max_zoom: f64,
) -> f64
```

---

## 4. 先読みズームプランナー (`zoom_planner.rs`)

### 4.1 基本概念: 先行カメラ移動

v3ではイベント発生後にクラスタが安定してからカメラが動いた。v4ではシーンが事前に確定しているため、シーン開始「前」にカメラを動かし始める。

```
v3: イベント → [1000ms安定待ち] → カメラ移動開始
v4: [先行カメラ移動開始] → シーン開始（カメラは既に到着済み）
```

先行時間はSpring half-lifeから逆算する:

```
先行時間 = pan_half_life × speed_scale × 3.0 × 1000ms

3 half-lifeでSpringは約87.5%に到達するため、
シーン開始時にはカメラがほぼ目標位置に到着している。
```

### 4.2 データ型

```rust
pub enum TransitionType {
    SpringIn,   // ズームイン（概要→詳細）
    SpringOut,  // ズームアウト（詳細→概要）
    Smooth,     // 同レベル間のスムーズな遷移
}

pub struct SpringHint {
    pub zoom_half_life: f64,  // ズーム用Spring half-life (秒)
    pub pan_half_life: f64,   // パン用Spring half-life (秒)
}

pub struct ZoomKeyframe {
    pub time_ms: u64,                     // キーフレーム時刻
    pub target_x: f64,                    // ズーム先の中心X
    pub target_y: f64,                    // ズーム先の中心Y
    pub zoom_level: f64,                  // ズーム倍率
    pub transition: TransitionType,       // 遷移タイプ
    pub spring_hint: Option<SpringHint>,  // Spring物理パラメータ
}
```

### 4.3 Spring Half-life パラメータ

| 遷移コンテキスト | zoom_half_life | pan_half_life | 用途 |
|---------------|---------------|-------------|------|
| 初回/Idle後のズームイン | 0.20s | 0.20s | 素早くフォーカス |
| シーン間のスムーズ遷移 | 0.25s | 0.25s | 落ち着いた移動 |
| Idle→ウィンドウレベル | 0.35s | 0.30s | ゆっくり引く |
| Idle→Overview (1.0x) | 0.40s | 0.35s | さらにゆっくり引く |

全パラメータに `AnimationSpeed` のスケール係数が掛けられる:

```
Slow:   ×1.5
Mellow: ×1.0（デフォルト）
Quick:  ×0.7
Rapid:  ×0.5
```

### 4.4 キーフレーム生成アルゴリズム

```
入力: scenes[] — 時系列のシーン配列
出力: plan[] — ZoomKeyframe配列

処理:
for (i, scene) in scenes:
    prev = scenes[i-1] if i > 0
    gap = scene.start - prev.end

    // ─── (A) Idle区間のズームアウト ───
    if gap >= idle_overview_ms && is_display_mode:
        // 長Idle → Overview (1.0x) に戻る
        emit SpringOut(screen_center, zoom=1.0) at prev.end + offset

    else if gap >= idle_zoom_out_ms:
        // 中Idle → ウィンドウレベルに戻る
        emit SpringOut(window_center, window_zoom) at prev.end + offset

    // ─── (B) シーンへの先行ズームイン ───
    if is_first:
        // 最初のシーン → t=0 にキーフレーム強制配置
        emit SpringIn(scene.center, scene.zoom) at t=0

    else if gap >= idle_zoom_out_ms:
        // Idle後 → SpringIn で素早くフォーカス
        emit SpringIn(scene.center, scene.zoom) at anticipated_time

    else:
        // 近接シーン → Smooth で滑らかに遷移
        emit Smooth(scene.center, scene.zoom) at anticipated_time

// 録画末尾のIdle
if remaining_time >= idle_overview_ms && is_display_mode:
    emit SpringOut(screen_center, zoom=1.0) at last_scene.end + offset
```

### 4.5 先行時間の計算

```rust
let anticipation_ms = (pan_hl * scale * 3.0 * 1000.0) as u64;

let kf_time = if is_first {
    0  // 最初のシーンは常にt=0
} else {
    let earliest = prev_scene.end_ms;          // 前のシーン終了時
    let anticipated = scene.start_ms - anticipation_ms; // 先行開始時刻
    let min_after_last = last_kf.time_ms + 200;        // 最小キーフレーム間隔

    max(anticipated, earliest, min_after_last)
};
```

### 4.6 Idle区間の処理

| 区間 | 条件 | アクション | 遷移タイプ |
|------|------|---------|----------|
| 中Idle | gap ≥ `idle_zoom_out_ms` (5000ms) | ウィンドウレベルにズームアウト | SpringOut |
| 長Idle | gap ≥ `idle_overview_ms` (8000ms) | Overview (1.0x) にズームアウト | SpringOut |
| 末尾Idle | 録画末尾に `idle_overview_ms` 以上の余り | Overview (1.0x) にズームアウト | SpringOut |

ウィンドウモード (`recording_mode == "window"`) では、Overview (1.0x) への遷移は発生しない。

### 4.7 キーフレーム重複除去

200ms未満の間隔のキーフレームは重複として除去する。近接するキーフレームが存在する場合、後のキーフレーム（より新しい決定）を優先する。

### 4.8 公開API

```rust
/// シーンベースの先読みズーム計画を生成する。
pub fn generate_zoom_plan(
    scenes: &[Scene],
    meta: &RecordingMeta,
    settings: &EffectsSettings,
) -> Vec<ZoomKeyframe>
```

---

## 5. 遷移パターンの具体例

### 5.1 基本的な操作シーケンス

```
時刻  イベント                  生成されるキーフレーム
0ms   [録画開始]                KF: t=0, zoom=2.0x, center=(500,300), SpringIn
      Click (500, 300)
500   Click (520, 310)
1000  Click (490, 290)
                                 ← Scene 0 (0ms〜1000ms) ─
                                    zoom=2.0x at (505, 300)

                                 [1500ms アイドル]

2500  Click (800, 600)           KF: t=2000, zoom=1.8x, center=(810,610), Smooth
3000  Click (810, 610)           (先行: 2500-600=1900 → clamped to 2000)
3500  Click (820, 620)
                                 ← Scene 1 (2500ms〜3500ms) ─
                                    zoom=1.8x at (810, 610)
```

### 5.2 ウィンドウ切り替え

```
時刻  イベント                  生成されるキーフレーム
0ms   WindowFocus: VSCode        KF: t=0, zoom=2.5x, center=(400,300), SpringIn
100   Click (400, 300)
...
3000  [最後のイベント]
                                 ← Scene 0 (VSCode内) ─

3200  WindowFocus: Chrome
3300  Click (1200, 500)          ← ウィンドウ変更でシーン分割 ─
                                 KF: t=3000, zoom=2.0x, center=(1200,500), SpringIn
...                              (先行: ウィンドウ変更直後なので最短で配置)
```

### 5.3 長いアイドル後の復帰

```
時刻   イベント                  生成されるキーフレーム
0ms    Click (500, 300)          KF: t=0, zoom=2.0x, SpringIn
...
3000   [最後のイベント]
                                 ← Scene 0 ─

                                 KF: t=5000, zoom=1.0, center=(960,540), SpringOut
                                 (長Idle: 8000ms超 → Overview)

15000  Click (1500, 800)         KF: t=14400, zoom=2.0x, center=(1500,800), SpringIn
                                 (先行: 15000 - 600 = 14400)
                                 ← Scene 1 ─
```

### 5.4 ターミナルでの文字入力

```
時刻   イベント                  生成されるキーフレーム
0ms    WindowFocus: Terminal      KF: t=0, zoom=1.8x, center=(500,400), SpringIn
       (rect: [100,100,900,700])
       [Clickなし]
3000   Key 'l'                   ← ウィンドウ中心 (500,400) を使用
3050   Key 's'                      ∵ 直近2秒以内のClickなし
3100   Key ' '
3150   Key '-'
...
                                 ← Scene 0 (3000ms〜) ─
                                    ウィンドウ中心でズーム
```

---

## 6. アニメーション

### 6.1 Spring物理

v2/v3と同じ **Critically Damped Spring** エンジンを使用する。変更は一切ない。

Compositorは `Vec<ZoomKeyframe>` を受け取り、各フレーム時刻でのSpring状態 (`zoom`, `pan_x`, `pan_y`) を計算する。

### 6.2 全遷移がSpring物理

v4ではすべてのカメラ遷移がSpring物理で行われる。即時切り替え（Cut）は存在しない。

```
TransitionType::SpringIn  → 概要から詳細へ（ズームイン）
TransitionType::SpringOut → 詳細から概要へ（ズームアウト）
TransitionType::Smooth    → 同レベル間のパン/ズーム
```

### 6.3 AnimationSpeed プリセット

EffectsSettingsの `animation_speed` で全体の速度を調整:

| プリセット | スケール係数 | 体感 |
|-----------|------------|------|
| Slow | ×1.5 | ゆっくり |
| Mellow | ×1.0 | デフォルト |
| Quick | ×0.7 | 素早い |
| Rapid | ×0.5 | 高速 |

---

## 7. ユーザー設定

### 7.1 関連するEffectsSettings

| 設定項目 | デフォルト値 | v4での役割 |
|---------|-----------|----------|
| `auto_zoom_enabled` | true | ズーム機能のON/OFF |
| `max_zoom` | 3.0 | ズーム倍率の上限 |
| `animation_speed` | Mellow | Spring half-lifeのスケール |
| `idle_zoom_out_ms` | 5000 | 中Idle閾値（ウィンドウレベルへ） |
| `idle_overview_ms` | 8000 | 長Idle閾値（Overviewへ） |
| `click_ring_enabled` | true | クリックリング表示（変更なし） |
| `key_badge_enabled` | true | キーバッジ表示（変更なし） |
| `cursor_smoothing` | true | カーソルスムージング（変更なし） |

### 7.2 `auto_zoom_enabled = false` の場合

ズーム計画の生成をスキップし、空の `Vec<ZoomKeyframe>` を返す。カメラは常に全画面表示 (zoom=1.0x) のまま。

---

## 8. エクスポート統合 (`encoder.rs`)

### 8.1 呼び出しフロー

```rust
// 1. イベント読み込み
let events = load_events(&recording_dir)?;

// 2. シーン分割（グローバル解析）
let scenes = split_into_scenes(
    &events,
    meta.screen_width as f64,
    meta.screen_height as f64,
    settings.effects.max_zoom,
);

// 3. ズーム計画生成
let zoom_keyframes = if settings.effects.auto_zoom_enabled {
    generate_zoom_plan(&scenes, meta, &settings.effects)
} else {
    Vec::new()
};

// 4. フレーム合成（既存のcompositor）
for frame in frames {
    compositor.compose_frame(frame, &zoom_keyframes, ...);
}
```

### 8.2 v3からの呼び出し変更

```
v3: analyze_events() → drags_to_segments() → cluster_activities() → generate_zoom_plan()
v4: split_into_scenes() → generate_zoom_plan()
```

パイプラインが大幅に簡略化された。

---

## 9. テスト

### 9.1 scene_splitter のテスト（10件）

| テスト | 検証内容 |
|-------|---------|
| `test_empty_events` | 空イベントで空シーン |
| `test_single_click_one_scene` | 1クリックで1シーン |
| `test_nearby_clicks_one_scene` | 近接クリック群が1シーンに |
| `test_idle_gap_splits_scenes` | 1500ms+のギャップでシーン分割 |
| `test_window_change_splits_scenes` | ウィンドウ変更でシーン分割 |
| `test_key_events_use_click_position` | Keyイベントが直近Click座標を使用 |
| `test_key_events_use_window_center_without_recent_click` | 2秒超のClickなし→ウィンドウ中心 |
| `test_key_events_use_recent_click_within_window` | 2秒以内のClick→Click座標を優先 |
| `test_zoom_level_in_range` | ズーム倍率が1.2〜max_zoom |
| `test_large_scene_splits_spatially` | 大BBoxのシーンがサブシーン分割 |
| `test_scene_has_window_rect` | ウィンドウ矩形が伝搬 |

### 9.2 zoom_planner のテスト（10件）

| テスト | 検証内容 |
|-------|---------|
| `test_empty_scenes_no_keyframes` | シーンなし→キーフレームなし |
| `test_single_scene_keyframe_at_zero` | 最初のシーンがt=0にKF |
| `test_two_scenes_with_anticipation` | 2シーン目のKFがシーン開始前 |
| `test_idle_gap_generates_zoomout` | Idle区間にSpringOutキーフレーム |
| `test_long_idle_overview_in_display_mode` | 長Idle→zoom=1.0 |
| `test_no_overview_in_window_mode` | ウィンドウモードではOverviewなし |
| `test_close_scenes_smooth_transition` | 近接シーン→Smooth遷移 |
| `test_no_cut_transitions` | Cut遷移は存在しない |
| `test_trailing_idle_zoomout` | 録画末尾のIdle→ズームアウト |
| `test_medium_idle_zoomout_with_window` | 中Idle→ウィンドウレベル（1.0xではない） |

---

## 付録A: v3との主要な差分まとめ

| 項目 | v3（前仕様） | v4（本仕様） |
|------|-----------|------------|
| イベント処理 | スライディングウィンドウ（逐次的） | **全イベント先読み（グローバル）** |
| ズーム単位 | ActivityCluster（時間窓ベース） | **Scene（自然な操作区間）** |
| ズーム開始タイミング | クラスタ安定後（1000ms遅延） | **シーン開始前（先行カメラ移動）** |
| 最初の操作 | クラスタ安定まで未対応 | **t=0にキーフレーム強制配置** |
| Keyイベント座標 | 最後のマウス位置 | **直近2秒のClick or ウィンドウ中心** |
| シーン分割条件 | TIME_WINDOW (3000ms) 超過で自然消滅 | **Idle gap ≥1500ms or ウィンドウ変更** |
| 大クラスタの処理 | BBox面積2倍制限で新クラスタに | **画面50%超 + 空間jump + time gapでサブシーン** |
| パイプライン | analyzer → cluster → zoom_planner | **scene_splitter → zoom_planner** |
| 後方互換性 | ZoomModel::V2/V3 切り替え | **v3を完全置換（切り替えフラグなし）** |
| Spring物理 | 同一 | 同一（パラメータのみ調整） |
| カーソル追従 | 廃止（v3で廃止済み） | 廃止（維持） |

## 付録B: パラメータ一覧

### scene_splitter.rs

| パラメータ | 値 | 説明 |
|-----------|-----|------|
| `SCENE_GAP_MS` | 1500ms | シーン分割のアイドルギャップ閾値 |
| `SUB_SCENE_SPATIAL_JUMP_PX` | 400px | サブシーン分割の空間距離閾値 |
| `SUB_SCENE_TIME_GAP_MS` | 500ms | サブシーン分割の時間ギャップ閾値 |
| `BBOX_PADDING` | 80px | BBox上下左右のパディング |
| `MIN_BBOX_SIZE` | 200px | BBox最小寸法 |
| `MAX_BBOX_SCREEN_FRACTION` | 0.5 | サブシーン分割を検討するBBox面積の画面比率 |
| `RECENT_CLICK_WINDOW_MS` | 2000ms | Keyイベントに使う「最近のClick」の時間窓 |

### zoom_planner.rs

| パラメータ | 値 | 説明 |
|-----------|-----|------|
| `ANTICIPATION_HALF_LIVES` | 3.0 | 先行カメラ移動の半減期倍率 |
| `MIN_KEYFRAME_INTERVAL_MS` | 200ms | キーフレーム最小間隔 |
| `ZOOM_IN` / `ZOOM_IN_PAN` | 0.20s | 初回/Idle後のズームイン半減期 |
| `SCENE_TO_SCENE_ZOOM` / `_PAN` | 0.25s | シーン間遷移の半減期 |
| `IDLE_ZOOMOUT_ZOOM` / `_PAN` | 0.35s / 0.30s | 中Idleズームアウトの半減期 |
| `OVERVIEW_ZOOM` / `_PAN` | 0.40s / 0.35s | Overview遷移の半減期 |
