# Snappi 自動エフェクト仕様書 v3 — 3段階ズームモデル

**Status**: 設計中
**Previous**: `auto-effects-spec-v2.md`（現行実装仕様）
**Last Updated**: 2026-02-08

---

## 1. 概要

### 1.1 背景と動機

現行のエフェクトエンジン（v2）は「個々のクリック座標にSpring物理でズームし、カーソル追従で微調整する」方式を採用している。この方式には以下の根本的な問題がある：

1. **カメラが動きすぎる**: クリックのたびにカメラが飛び、さらにカーソル追従で常時揺れ続ける
2. **視聴者が予測できない**: ズーム先が個々のクリック座標で決まるため、次にカメラがどこへ向かうか分からない
3. **ズームレベルが不安定**: 1.0x〜3.0xの間で細かく変動し、視聴体験が落ち着かない
4. **カーソル追従が混乱を招く**: Push Zoneによるビューポート微調整は、ユーザーの操作とは無関係にカメラが動くため理解しにくい

### 1.2 新しいアプローチ: 3段階ズームモデル

カメラの状態を **3つの明確なズームレベル** に限定し、状態遷移として管理する。

```
┌──────────────────────────────────────────────────┐
│  Tier 1: Overview（全画面表示）                     │
│  画面全体を表示。「何をしているか」の全体像。          │
│  zoom = 1.0x                                      │
├──────────────────────────────────────────────────┤
│  Tier 2: Window（アクティブウィンドウ表示）           │
│  操作中のウィンドウ全体を表示。                       │
│  zoom = ウィンドウサイズに応じた倍率                   │
├──────────────────────────────────────────────────┤
│  Tier 3: WorkArea（作業範囲表示）                    │
│  ウィンドウ内の具体的な操作領域を表示。                │
│  zoom = アクティビティクラスタに応じた倍率              │
│  ※ 複数のWorkAreaが存在しうる（クラスタごとに1つ）     │
└──────────────────────────────────────────────────┘
```

### 1.3 設計原則

| # | 原則 | 説明 |
|---|------|------|
| 1 | **離散的なズームレベル** | 連続的なズーム変動をやめ、3段階の明確なレベルで遷移する |
| 2 | **カーソル追従の廃止** | カーソルの動きでカメラを動かさない。カメラ移動は状態遷移のみ |
| 3 | **クラスタベースの作業範囲** | 個々のクリック座標ではなく、アクティビティの集合体（クラスタ）に基づいてズーム範囲を決定 |
| 4 | **予測可能な遷移** | 状態遷移のルールが明確で、視聴者がカメラの動きを予測できる |
| 5 | **最小限のカメラ移動** | 同一クラスタ内の操作ではカメラを動かさない |

### 1.4 録画モード別の動作

| 録画モード | 利用可能なTier | 説明 |
|-----------|--------------|------|
| **Display（全画面）** | Overview → Window → WorkArea | 3段階すべてを使用 |
| **Window（ウィンドウ指定）** | Window → WorkArea | Overviewを除外。録画対象が1ウィンドウに限定されるため |
| **Area（範囲指定）** | Window → WorkArea | 同上。指定範囲をWindowとして扱う |

---

## 2. 状態遷移モデル

### 2.1 Display モード（全画面録画）

```
                    最初のアクション
     Overview ─────────────────────→ Window
        ↑                               │
        │                               │ アクティビティが
        │ 長Idle(5s+)                   │ クラスタを形成(1s+)
        │                               ↓
        ←──── 中Idle(3s+) ──────── WorkArea[n]
                                        │  ↑
                                        │  │ 同一ウィンドウ内で
                                        │  │ 別クラスタへ移動
                                        │  ↓
                                   WorkArea[m]
                                        │
                                        │ ウィンドウ変更
                                        ↓
                                   Window(新)
                                        │
                                        ↓
                                   WorkArea[p]
```

### 2.2 Window / Area モード

```
                    最初のアクション
      Window ──────────────────────→ WorkArea[n]
        ↑                               │  ↑
        │                               │  │ 別クラスタへ移動
        │ 中Idle(3s+)                   │  ↓
        │                          WorkArea[m]
        │                               │
        ←───────── 長Idle(5s+) ─────────┘
```

### 2.3 遷移条件の詳細

| 遷移 | トリガー | アニメーション | 備考 |
|------|---------|-------------|------|
| Overview → Window | 最初のClick/Key/Scroll | SpringIn (0.25s) | ウィンドウ矩形にフィット |
| Window → WorkArea | アクティビティクラスタが安定（1秒以上同一領域で操作） | SpringIn (0.20s) | クラスタBBoxにフィット |
| WorkArea → WorkArea | 操作が別クラスタに移動（現クラスタから300px以上離れた操作が継続） | Smooth (0.30s) | 新クラスタBBoxにフィット |
| WorkArea → Window | ウィンドウ変更イベント | SpringOut (0.30s) → SpringIn (0.25s) | 一度ウィンドウ全体に戻ってから新ウィンドウへ |
| WorkArea → Overview | 長Idle (5秒以上操作なし) | SpringOut (0.40s) | 段階的：WorkArea → Window → Overview |
| Window → Overview | 中Idle (3秒以上操作なし) | SpringOut (0.35s) | 全画面表示に戻る |
| Overview → Overview | 維持 | — | 操作がない間は全画面表示を維持 |

### 2.4 段階的ズームアウト

Idle時のズームアウトは段階的に行う。WorkArea → 直接Overview ではなく、必ず中間状態を経由する。

```
WorkArea(3.0x) ──[3秒Idle]──→ Window(1.5x) ──[さらに2秒Idle]──→ Overview(1.0x)
```

これにより、短い休止（3秒）では操作中のウィンドウ全体が見え、長い休止（5秒+）で画面全体に戻る。

---

## 3. WorkArea（作業範囲）の決定

### 3.1 アクティビティクラスタリング

WorkAreaは **アクティビティクラスタ** に基づいて決定される。クラスタとは、時間的・空間的に近接する操作イベントの集合である。

#### 入力イベント

クラスタリングに使用するイベント：

| イベント | 座標の取得元 | 重み |
|---------|-----------|------|
| Click | クリック座標 (x, y) | 1.0 |
| TextInput | クリック座標（入力開始位置） | 1.0 |
| Scroll | マウス座標 (x, y) | 0.5 |
| Drag | 開始〜終了の中点 | 0.8 |
| MouseMove | マウス座標 | 0.1（密度制限後） |

MouseMoveは重みが低く、大量に発生するため、100ms間隔に間引いた上でクラスタの「範囲拡張」にのみ寄与する。クラスタの形成トリガーにはならない。

#### クラスタリングアルゴリズム

**スライディングウィンドウ + 空間クラスタリング** を採用する。

```
パラメータ:
  TIME_WINDOW       = 3000ms    // 直近3秒間のイベントを対象
  SPATIAL_RADIUS    = 300px     // この距離以内のイベントを同一クラスタに
  MIN_EVENTS        = 2         // クラスタ形成に必要な最小イベント数
  STABILITY_TIME    = 1000ms    // クラスタが安定したと判断するまでの時間
  BBOX_PADDING      = 50px      // バウンディングボックスの余白
  MIN_BBOX_SIZE     = 200px     // バウンディングボックスの最小サイズ（幅・高さ）
```

```
アルゴリズム:
1. 直近 TIME_WINDOW (3秒) 内の操作イベントを収集
2. 重み付きDBSCAN的クラスタリング:
   a. 各イベントの座標を点として扱う
   b. SPATIAL_RADIUS (300px) 以内の点を同一クラスタに統合
   c. イベント数が MIN_EVENTS (2) 以上のクラスタを有効とする
3. 各クラスタのバウンディングボックスを算出:
   a. 全イベント座標の min/max から矩形を決定
   b. BBOX_PADDING (50px) を上下左右に追加
   c. 最小サイズ MIN_BBOX_SIZE (200px) を保証
4. 最も最新のイベントを含むクラスタを「アクティブクラスタ」とする
5. アクティブクラスタが STABILITY_TIME (1秒) 以上変化しなければ WorkArea 確定
```

### 3.2 複数WorkAreaの管理

ウィンドウ内に複数の操作領域が存在する場合、複数のWorkAreaクラスタが形成される。

```
例: VSCode での操作

  ┌─────────────────────────────────────────┐
  │ VSCode                                  │
  │  ┌──────────┐  ┌────────────────────┐   │
  │  │WorkArea A│  │                    │   │
  │  │(サイドバー)│  │  WorkArea B       │   │
  │  │ファイル   │  │  (エディタ領域)     │   │
  │  │クリック群 │  │  テキスト入力 +     │   │
  │  │          │  │  スクロール          │   │
  │  └──────────┘  └────────────────────┘   │
  │                 ┌────────────────────┐   │
  │                 │  WorkArea C       │   │
  │                 │  (ターミナル)      │   │
  │                 │  コマンド入力       │   │
  │                 └────────────────────┘   │
  └─────────────────────────────────────────┘
```

操作が別のWorkAreaに移ると、カメラはSmooth遷移でそちらへ移動する。

#### WorkArea間の遷移判定

```
1. 現在のアクティブクラスタ: WorkArea[n]
2. 新しいイベントが発生 (x, y)
3. if イベントが WorkArea[n] のBBox内:
     → そのまま WorkArea[n] に留まる（カメラ固定）
4. else if イベントが別の既存クラスタ WorkArea[m] のBBox内:
     → WorkArea[m] に切り替え（Smooth遷移）
5. else:
     → 新しいクラスタの形成を開始
     → 形成中はカメラを維持し、STABILITY_TIME後に新WorkAreaへ遷移
```

### 3.3 WorkArea → ズーム倍率の計算

WorkAreaのバウンディングボックスから、適切なズーム倍率を計算する。

```rust
fn calc_workarea_zoom(
    bbox: &Rect,           // WorkAreaのバウンディングボックス
    screen_w: f64,         // 録画領域の幅
    screen_h: f64,         // 録画領域の高さ
    max_zoom: f64,         // ユーザー設定の最大ズーム
) -> f64 {
    let zoom_w = screen_w / bbox.width;
    let zoom_h = screen_h / bbox.height;
    let fit_zoom = zoom_w.min(zoom_h);  // アスペクト比を保持

    fit_zoom
        .min(max_zoom)    // 最大ズームを超えない
        .max(1.2)         // 最低でも1.2x（ズーム感を出す）
}
```

例:
- 画面 1920x1080、WorkArea 600x400 → zoom = 2.7x
- 画面 1920x1080、WorkArea 1200x800 → zoom = 1.35x
- 画面 1920x1080、WorkArea 200x150 → zoom = 3.0x（max_zoom制限）

---

## 4. Window（ウィンドウ）ズームの決定

### 4.1 アクティブウィンドウの追跡

WindowFocusイベントにより、現在操作中のウィンドウを特定する。

```
WindowFocusイベント:
  { type: "window_focus", t: 1500, title: "Visual Studio Code", rect: [100, 50, 1820, 1030] }
```

### 4.2 ウィンドウズーム倍率の計算

```rust
fn calc_window_zoom(
    window_rect: &Rect,
    screen_w: f64,
    screen_h: f64,
    max_zoom: f64,
) -> f64 {
    let padding = 0.05;  // 5%余白
    let padded_w = window_rect.width * (1.0 + padding * 2.0);
    let padded_h = window_rect.height * (1.0 + padding * 2.0);

    let zoom_w = screen_w / padded_w;
    let zoom_h = screen_h / padded_h;
    let fit_zoom = zoom_w.min(zoom_h);

    fit_zoom
        .min(max_zoom)
        .max(1.1)         // 最低でもわずかにズーム
}
```

### 4.3 ウィンドウモードでのWindow Tier

ウィンドウモード録画の場合、録画対象のウィンドウ自体が「Window」Tierとなる。この場合の「Window」は zoom = 1.0x（録画領域全体 = ウィンドウ全体）に相当する。

---

## 5. カーソル追従の廃止

### 5.1 現行のカーソル追従（削除対象）

```
現行: compositor.rs の apply_cursor_follow()
  - Dead Zone (d < 0.75): カメラ固定
  - Soft Zone (0.75 < d < 1.0): 段階的追従
  - Push Zone (d > 1.0): 最大30%シフト
```

**この機能を完全に廃止する。**

### 5.2 廃止の理由

1. WorkAreaクラスタがカーソルの典型的な活動範囲をカバーするため、カーソルが画面外に出ることが少ない
2. カーソル追従は予測不能なカメラ移動の主要因
3. 万一カーソルがWorkAreaのBBox外に出た場合は、クラスタの拡張か新クラスタの形成で対応

### 5.3 カーソルがWorkArea外に出た場合の対応

```
1. カーソルがWorkArea BBox外に移動
2. if 300ms以内にBBox内に戻る:
     → 何もしない（一時的な逸脱）
3. else if BBoxの近傍（SPATIAL_RADIUS以内）にとどまる:
     → WorkAreaのBBoxを拡張して含める
4. else:
     → 新しいクラスタの形成開始
     → STABILITY_TIME後に新WorkAreaへ遷移
```

---

## 6. アニメーション

### 6.1 Spring物理の継続利用

遷移アニメーションにはv2と同じ **Critically Damped Spring** を使用する。変更点はパラメータのみ。

### 6.2 遷移タイプ別のSpringパラメータ

| 遷移 | zoom half-life | pan half-life | 体感 |
|------|---------------|-------------|------|
| Overview → Window | 0.25s | 0.25s | しっかり寄る |
| Window → WorkArea | 0.20s | 0.20s | やや速くフォーカス |
| WorkArea → WorkArea（同一ウィンドウ） | 0.25s | 0.30s | 落ち着いて移動 |
| WorkArea → Window（ウィンドウ変更） | 0.30s | 0.30s | 一度引いてから |
| Window → Overview | 0.35s | 0.30s | ゆっくり引く |
| WorkArea → Window（Idle） | 0.40s | 0.35s | ゆっくり引く |
| Window → Overview（Idle） | 0.40s | 0.35s | ゆっくり引く |

### 6.3 ユーザー設定: AnimationSpeed

既存のAnimationSpeedプリセットを継続利用。half-lifeにスケール係数を掛ける。

```
Slow:   ×1.5  （ゆっくり）
Mellow: ×1.0  （デフォルト）
Quick:  ×0.7  （素早い）
Rapid:  ×0.5  （高速）
```

### 6.4 ウィンドウ変更時のシーケンス

ウィンドウ変更時は、直接新ウィンドウのWorkAreaに飛ぶのではなく、段階的に遷移する。すべてSpring物理による滑らかなアニメーションで行い、即時切り替え（Cut）は使用しない。

```
WorkArea[旧ウィンドウ]
  ──[SpringOut 0.30s]──→ Window[旧ウィンドウ]
  ──[SpringIn 0.30s]──→ Window[新ウィンドウ]
  ──[クラスタ安定後]──→ WorkArea[新ウィンドウ]
```

ウィンドウ間の距離が大きい場合でも、Spring物理で滑らかに移動する。Overview Tier経由にすることで移動距離を自然に吸収する。

```
距離が大きい場合:
WorkArea[旧] ──→ Window[旧] ──→ Overview ──→ Window[新] ──→ WorkArea[新]
              SpringOut       SpringOut     SpringIn      SpringIn
```

---

## 7. Idle処理

### 7.1 Idle検出

v2と同じアルゴリズムを使用。MouseMoveを除外した重要イベント間のギャップを計測。

```
IDLE_SHORT_MS   = 1000ms   // 1〜3秒: 何もしない
IDLE_MEDIUM_MS  = 3000ms   // 3〜5秒: Window Tierへ戻る
IDLE_LONG_MS    = 5000ms   // 5秒以上: Overview Tierへ戻る
```

### 7.2 Idle時の段階的ズームアウト

| 現在のTier | 中Idle (3秒) | 長Idle (5秒) |
|-----------|------------|------------|
| WorkArea | → Window | → Overview |
| Window | そのまま | → Overview |
| Overview | そのまま | そのまま |

### 7.3 Idle後の復帰

Idleからの復帰時も段階的に遷移する。

```
Overview状態で操作再開:
  1. 操作のウィンドウを特定 → Window Tierへ (SpringIn 0.25s)
  2. 1秒後、クラスタ安定 → WorkArea Tierへ (SpringIn 0.20s)

Window状態で操作再開:
  1. クラスタ形成開始
  2. 1秒後、クラスタ安定 → WorkArea Tierへ (SpringIn 0.20s)
```

---

## 8. パイプライン変更

### 8.1 全体フロー

```
[録画フェーズ] ── 変更なし
  events.jsonl, window_events.jsonl, ui_events.jsonl
  frames/*.png, audio.wav, meta.json

        ↓

[前処理] ── 変更なし
  preprocessor.rs: thin_mouse_moves(), detect_drags()

        ↓

[イベント解析] ── 簡略化
  analyzer.rs:
    Phase 1: Idle区間検出（既存維持）
    Phase 2: WindowFocus追跡（既存維持）
    Phase 3: アクティビティイベント抽出（★新規・簡略化）
             → Click, TextInput, Drag, Scroll を時系列で収集
             → 重要度スコアリングは廃止（Tierの遷移で代替）

        ↓

[★ アクティビティクラスタリング] ── 新規
  activity_cluster.rs:
    - スライディングウィンドウでイベント群をクラスタリング
    - 各クラスタのBBox + ズーム倍率を算出
    - 出力: Vec<ActivityCluster>

        ↓

[★ 3段階ズームプランナー] ── 全面書き換え
  zoom_planner_v3.rs:
    - 状態遷移マシン（Overview/Window/WorkArea）
    - クラスタ + ウィンドウ情報 + Idle区間からキーフレーム生成
    - 出力: Vec<ZoomKeyframe>（v2と同じ構造体）

        ↓

[フレーム合成] ── 微修正
  compositor.rs:
    - apply_cursor_follow() を削除
    - その他（クロップ、エフェクト描画）は変更なし

        ↓

[エクスポート] ── 変更なし
  encoder.rs → FFmpeg
```

### 8.2 変更ファイル一覧

| ファイル | 変更内容 | 影響度 |
|---------|---------|-------|
| `engine/activity_cluster.rs` | **新規**: アクティビティクラスタリングモジュール | 新規 |
| `engine/zoom_planner_v3.rs` | **新規**: 3段階ズームプランナー | 新規 |
| `engine/zoom_planner.rs` | v2として残す（後方互換用） | 変更なし |
| `engine/analyzer.rs` | セグメント分類の簡略化。スコアリングは維持（WorkArea内のUI優先度に転用可能） | 中 |
| `engine/compositor.rs` | `apply_cursor_follow()` を削除 | 小 |
| `engine/click_cluster.rs` | activity_cluster.rsに統合・発展 | 廃止予定 |
| `engine/mod.rs` | 新モジュールの公開 | 小 |
| `export/encoder.rs` | `compose_frames()` 内のプランナー呼び出しをv3に変更 | 小 |
| `config/mod.rs` | `EffectsSettings` にv3切り替えフラグ追加 | 小 |

---

## 9. データ構造

### 9.1 ActivityCluster（新規）

```rust
/// ウィンドウ内のアクティビティクラスタ（作業範囲）
#[derive(Debug, Clone)]
pub struct ActivityCluster {
    /// クラスタID（時系列で一意）
    pub id: u32,
    /// クラスタに含まれる最初のイベントの時刻
    pub start_ms: u64,
    /// クラスタに含まれる最後のイベントの時刻
    pub end_ms: u64,
    /// バウンディングボックス（パディング込み）
    pub bbox: Rect,
    /// バウンディングボックスの中心座標
    pub center_x: f64,
    pub center_y: f64,
    /// このクラスタに適したズーム倍率
    pub zoom_level: f64,
    /// クラスタに含まれるイベント数
    pub event_count: usize,
    /// 所属ウィンドウの矩形（分かる場合）
    pub window_rect: Option<Rect>,
}
```

### 9.2 ZoomTier（新規）

```rust
/// カメラの状態（3段階ズームモデル）
#[derive(Debug, Clone, PartialEq)]
pub enum ZoomTier {
    /// 画面全体を表示（zoom = 1.0x）
    Overview,
    /// アクティブウィンドウ全体を表示
    Window {
        window_rect: Rect,
        zoom_level: f64,
    },
    /// アクティビティクラスタ（作業範囲）を表示
    WorkArea {
        cluster_id: u32,
        bbox: Rect,
        zoom_level: f64,
    },
}
```

### 9.3 ZoomKeyframe（既存構造体を再利用）

```rust
/// v2と同じ構造。互換性を維持。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoomKeyframe {
    pub time_ms: u64,
    pub target_x: f64,       // ズーム先の中心X
    pub target_y: f64,       // ズーム先の中心Y
    pub zoom_level: f64,     // ズーム倍率
    pub transition: TransitionType,
    pub spring_hint: Option<SpringHint>,
}
```

ズームプランナーv3の出力は引き続き `Vec<ZoomKeyframe>` であり、compositorとの互換性を維持する。

### 9.4 TierTransition（新規、内部用）

```rust
/// 状態遷移の記録（デバッグ・Timeline表示用）
#[derive(Debug, Clone)]
pub struct TierTransition {
    pub time_ms: u64,
    pub from: ZoomTier,
    pub to: ZoomTier,
    pub reason: TransitionReason,
}

#[derive(Debug, Clone)]
pub enum TransitionReason {
    FirstAction,            // 最初の操作
    ClusterStabilized,      // クラスタが安定
    ClusterChanged,         // 別クラスタへ移動
    WindowChanged,          // ウィンドウ変更
    IdleMedium,             // 3秒Idle
    IdleLong,               // 5秒Idle
    IdleResume,             // Idle後の操作再開
}
```

---

## 10. アクティビティクラスタリングの詳細アルゴリズム

### 10.1 時系列処理

クラスタリングはオフライン処理（エクスポート時）で行うため、全イベントを事前に読み込んで処理できる。

```
入力: events[] — 全イベントの時系列配列
出力: clusters[] — 時系列順のActivityCluster配列

処理:
  active_clusters: Vec<(ActivityCluster, last_event_ms)> = []

  for event in events:
    if event は操作イベント (Click/TextInput/Drag/Scroll):
      (x, y) = event の座標

      // 既存クラスタとのマッチング
      matched_cluster = active_clusters.find(|c|
        c.bbox.contains_or_near(x, y, SPATIAL_RADIUS)
        && event.t - c.last_event_ms < TIME_WINDOW
      )

      if matched_cluster:
        // クラスタを更新（BBoxを拡張）
        matched_cluster.extend(x, y, event.t)
      else:
        // 新しいクラスタを作成
        new_cluster = ActivityCluster::new(x, y, event.t)
        active_clusters.push(new_cluster)

      // タイムアウトしたクラスタをfinalizeして出力
      active_clusters.retain(|c| event.t - c.last_event_ms < TIME_WINDOW)
```

### 10.2 BBoxの拡張ルール

```
新しいイベント座標 (x, y) が既存BBoxの外にある場合:

1. BBoxを拡張して (x, y) を含める
2. パディング (BBOX_PADDING = 50px) を再適用
3. 最小サイズ (MIN_BBOX_SIZE = 200px) を保証
4. ズーム倍率を再計算

ただし:
- BBox面積が元の2倍以上に拡大する場合は、新しいクラスタとして分離
- これにより、「画面全体に広がるクラスタ」が形成されることを防ぐ
```

### 10.3 クラスタの安定判定

```
クラスタが「安定」したと判断する条件:
1. MIN_EVENTS (2) 以上のイベントを含む
2. 最初のイベントから STABILITY_TIME (1000ms) 以上経過
3. 直近500msでBBoxが20%以上変化していない

安定前のクラスタ:
- カメラは前の状態を維持
- バックグラウンドでクラスタを形成し続ける

安定後:
- WorkArea Tierへの遷移キーフレームを生成
```

---

## 11. ズームプランナーv3の処理フロー

### 11.1 全体処理

```rust
pub fn generate_zoom_plan_v3(
    events: &[RecordingEvent],
    idle_segments: &[Segment],       // Idle区間
    window_events: &[RecordingEvent], // WindowFocusイベント
    clusters: &[ActivityCluster],     // アクティビティクラスタ
    meta: &RecordingMeta,
    settings: &EffectsSettings,
    recording_mode: &RecordingMode,
) -> Vec<ZoomKeyframe> {
    let mut plan: Vec<ZoomKeyframe> = Vec::new();
    let mut current_tier = match recording_mode {
        RecordingMode::Display => ZoomTier::Overview,
        _ => ZoomTier::Window { /* 録画領域全体 */ },
    };
    let mut current_window: Option<Rect> = None;
    let mut active_cluster_id: Option<u32> = None;

    // 全イベントを時系列で処理
    // ... 状態遷移に応じてキーフレームを生成
}
```

### 11.2 状態遷移マシンの擬似コード

```
for each time_point in timeline:

  // 1. Idle チェック
  if idle_segment が現在時刻をカバー:
    if idle.level == Medium && current_tier == WorkArea:
      → emit キーフレーム: Window Tierへ遷移
      current_tier = Window
    if idle.level == Long:
      if recording_mode == Display:
        → emit キーフレーム: Overview Tierへ遷移
        current_tier = Overview
      else:
        → emit キーフレーム: Window Tierへ遷移
        current_tier = Window
    continue

  // 2. ウィンドウ変更チェック
  if window_focus_event at this time:
    if current_tier == WorkArea:
      → emit キーフレーム: Window[旧] Tierへ遷移 (SpringOut)
    → emit キーフレーム: Window[新] Tierへ遷移 (SpringIn)
    current_tier = Window
    current_window = new_window
    active_cluster_id = None
    continue

  // 3. クラスタ遷移チェック
  active_cluster = clusters.find(|c| c.start_ms <= now && c.end_ms >= now && c.is_stable(now))

  if active_cluster && active_cluster.id != active_cluster_id:
    if current_tier == Overview:
      // Overview → Window → WorkArea（2段階）
      → emit キーフレーム: Window Tierへ遷移
      → emit キーフレーム（+delay）: WorkArea Tierへ遷移
    else:
      // Window or WorkArea → WorkArea
      → emit キーフレーム: WorkArea[cluster] Tierへ遷移
    current_tier = WorkArea { cluster }
    active_cluster_id = Some(active_cluster.id)
```

---

## 12. v2からの移行

### 12.1 切り替え方法

`EffectsSettings` にフラグを追加し、v2とv3を切り替え可能にする。

```rust
pub struct EffectsSettings {
    // ... 既存フィールド ...

    /// ズームモデルのバージョン（v2: 現行, v3: 3段階モデル）
    #[serde(default = "default_zoom_model")]
    pub zoom_model: ZoomModel,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ZoomModel {
    V2,  // 現行: クリック座標ベース + カーソル追従
    V3,  // 新: 3段階ズームモデル
}

fn default_zoom_model() -> ZoomModel {
    ZoomModel::V3
}
```

### 12.2 encoder.rs での分岐

```rust
let zoom_keyframes = if settings.effects.auto_zoom_enabled {
    match settings.effects.zoom_model {
        ZoomModel::V2 => generate_zoom_plan_v2(/* ... */),
        ZoomModel::V3 => generate_zoom_plan_v3(/* ... */),
    }
} else {
    Vec::new()
};
```

### 12.3 既存録画との互換性

v3は入力データ（events.jsonl, window_events.jsonl）に追加のフィールドを要求しない。既存の全録画データでv3プランナーを使用可能。

---

## 13. Timeline UIとの連携

### 13.1 Tier遷移の可視化

Timelineコンポーネントで、キーフレームの代わりにTier遷移を表示する。

```
タイムライン表示:
  [Overview]──[Window: VSCode]──[WorkArea: エディタ]──[WorkArea: ターミナル]──[Window]──[Overview]
       0s         1.5s              3.0s                  8.0s              12.0s     15.0s
```

### 13.2 VideoPlayerとの連携

VideoPlayerのシークバーにTier遷移マーカーを表示し、各Tier区間を色分けする。

```
色分け:
  Overview:  灰色
  Window:    青色
  WorkArea:  紫色
```

### 13.3 WorkAreaのBBox表示

プレビュー画面上に、現在のWorkAreaのバウンディングボックスを半透明の矩形で表示する。これにより、カメラがどこをズームするかが視覚的に分かる。

---

## 14. ユーザー設定

### 14.1 既存設定の継続

| 設定項目 | 説明 | v3での扱い |
|---------|------|----------|
| auto_zoom_enabled | ズームON/OFF | 維持 |
| default_zoom_level | デフォルトズーム倍率 | WorkAreaの最大ズーム倍率の参考値に |
| max_zoom | 最大ズーム倍率 | 維持（WorkAreaの倍率上限） |
| zoom_intensity | ズーム頻度 | クラスタ安定時間とIdleしきい値に影響 |
| animation_speed | アニメーション速度 | 維持 |
| click_ring_enabled | クリックリング | 維持（変更なし） |
| key_badge_enabled | キーバッジ | 維持（変更なし） |
| cursor_smoothing | カーソルスムージング | 維持（変更なし） |

### 14.2 ZoomIntensityのv3での解釈

| プリセット | クラスタ安定時間 | Idle Medium | Idle Long | 特徴 |
|-----------|--------------|-------------|-----------|------|
| Minimal | 2000ms | 5000ms | 8000ms | 非常に控えめ。ほぼWindowレベルに留まる |
| Balanced | 1000ms | 3000ms | 5000ms | デフォルト。自然な頻度で遷移 |
| Active | 500ms | 2000ms | 4000ms | 素早く作業範囲にフォーカス |

---

## 15. 実装ロードマップ

### Phase 1: コアロジック実装

1. `engine/activity_cluster.rs` — アクティビティクラスタリング
2. `engine/zoom_planner_v3.rs` — 3段階ズームプランナー（状態遷移マシン）
3. `engine/compositor.rs` — `apply_cursor_follow()` の削除
4. `export/encoder.rs` — v3プランナー呼び出し
5. `config/mod.rs` — `ZoomModel` 設定追加

### Phase 2: 検証・調整

1. 既存録画データでv2とv3のエクスポート結果を比較
2. クラスタリングパラメータの調整
3. Spring half-lifeの微調整

### Phase 3: UI統合

1. Settings.tsx — `ZoomModel` 切り替えUI
2. Timeline.tsx — Tier遷移表示
3. VideoPlayer.tsx — WorkArea BBox表示

---

## 16. 評価基準

### 品質目標

| 指標 | v2（現行） | v3（目標） |
|------|----------|----------|
| カメラ移動回数/分 | 8-15回 | 2-5回 |
| カメラ静止時間率 | ~30% | ~70% |
| ズームレベル変動 | 連続的（1.0〜3.0） | 離散的（3段階） |
| カーソル追従による微振動 | あり | なし |
| 視聴者の快適性 | 低 | 高 |

### テスト方法

1. 同一録画データでv2/v3をエクスポートし、映像を並べて比較
2. キーフレーム数のログ出力で定量比較
3. ユースケース別テスト:
   - VSCode操作（エディタ + ターミナル + サイドバー → 複数WorkArea）
   - Webブラウジング（タブ切り替え → ウィンドウ変更）
   - フォーム入力（テキスト欄クリック → 入力 → 次の欄 → 同一WorkArea内の遷移）

---

## 付録A: 現行v2との主要な差分まとめ

| 項目 | v2（現行） | v3（本仕様） |
|------|----------|-----------|
| ズームターゲット | 個々のクリック座標 | アクティビティクラスタのBBox中心 |
| ズームレベル | 連続値（1.0〜3.0） | 3段階（Overview / Window / WorkArea） |
| カーソル追従 | Push Zone (apply_cursor_follow) | **廃止** |
| 遷移トリガー | 各イベント（クリック、テキスト入力等） | 状態遷移（クラスタ安定 / ウィンドウ変更 / Idle） |
| キーフレーム数 | 多数（イベントごと） | 少数（状態遷移の数のみ） |
| 重要度スコアリング | あり（各セグメントに0.0〜1.0） | 不要（Tierの遷移で代替） |
| ウィンドウ変更時 | 2段階ズーム（ウィンドウ → クリック位置） | 段階的遷移（WorkArea → Window → Window → WorkArea） |
| Idle処理 | 画面中央へ直接ズームアウト | 段階的ズームアウト（WorkArea → Window → Overview） |
| 同一ウィンドウ内の操作 | パンのみ（ズーム維持） | WorkArea内なら完全静止。別クラスタならSmooth遷移 |
| アニメーション | Spring物理 | Spring物理（パラメータのみ変更） |
| 後方互換性 | — | ZoomModel設定でv2/v3を切り替え可能 |
