# Snappi 自動エフェクト仕様書 v2

**Status**: 設計完了（未実装）
**Previous**: `auto-effects-spec.md`（現行仕様）
**Last Updated**: 2026-02-07

---

## 概要と設計思想

### 背景

現行の自動エフェクトエンジンは「マウスカーソルの動きに追従してカメラを移動する」方式を採用している。この方式では、マウスが少し動くだけでカメラが反応し、映像が激しく動いて視聴者にとって非常に見づらい映像になるという根本的な問題がある。

Screen Studioをはじめとする業界をリードするツールの調査から、**「カメラは動かさないのがデフォルト。動かす理由がある時だけ動かす」** という映像制作の基本原則に立ち返る必要があることが明らかになった。

### 設計原則

本仕様書では、以下の7つのUXデザイン原則に基づいて自動エフェクトを再設計する。

| # | 原則 | 説明 |
|---|------|------|
| 1 | **クリック＝意図のシグナル** | マウスの移動ではなくクリックをズームのトリガーとする。移動は「探索」、クリックは「決定」 |
| 2 | **重要度に基づくフィルタリング** | すべてのクリックでズームせず、文脈から重要度を評価して閾値を超えたもののみズーム |
| 3 | **最小保持時間の確保** | ズーム状態を十分な時間（最低1.5秒）保持し、視聴者がコンテンツを認識する時間を確保 |
| 4 | **Spring物理による自然な動き** | 解析的臨界減衰スプリングで自然な加減速を実現。非対称ズーム（イン:速、アウト:遅） |
| 5 | **デッドゾーンの拡大** | カーソルの小さな動きではカメラを動かさない。映像の安定性を最優先 |
| 6 | **ウィンドウスコープ** | 同じウィンドウ内の操作はズームレベルを維持し、パンのみ。ウィンドウ変更時のみズームリセット |
| 7 | **シーン遷移の知的処理** | 大きな位置ジャンプはズームではなくカット。スクロールはズームアウト |

### Screen Studioとの比較

| 要素 | Screen Studio | Snappi 現行 | Snappi v2（本仕様） |
|------|-------------|------------|-------------------|
| ズームトリガー | クリック位置のみ | 全セグメント（Click/TextInput/Scroll/RapidAction） | クリック + 重要度スコアリング + UI要素認識 |
| パン制御 | 不明（滑らか） | 3ゾーンカーソル追従（常時） | 3ゾーンカーソル追従（ズーム中のみ、デッドゾーン拡大） |
| ズーム頻度 | 低（クリックのみ） | 高（すべてのアクションでズーム） | 低（重要度閾値 + クールダウン） |
| アニメーション | Spring物理 + モーションブラー | Spring物理 | Spring物理（パラメータ最適化） |
| ウィンドウ認識 | あり | 部分的 | 完全対応（ウィンドウスコープ + UI Automation） |
| 録画モード | Display / Window / Area | Display（全画面）のみ | Display / Window（★新規） |
| UI要素認識 | なし（クリック位置のみ） | なし | あり（★Windows UI Automation API） |

---

## 1. 現在の実装の課題

### 課題1: すべてのクリックが等価に扱われている

`zoom_planner.rs` の現行実装では、すべての `SegmentType::Click` に対して一律 `default_zoom`（2.0x）でズームインする。メニュー操作の連続クリック、ボタン連打、フォーム入力のための複数フィールドクリックなど、文脈によって重要度が大きく異なるにもかかわらず、区別がない。

### 課題2: ズーム頻度の制御が不十分

`deduplicate_keyframes()` は300ms以内の時間的重複を排除するのみで、セマンティックな判断がない。結果として短時間に複数のズームイン/アウトが発生し、映像が落ち着かない。

### 課題3: カーソル追従が過敏

`compositor.rs` の3ゾーンモデル（Dead: 30%, Soft: 30-70%, Push: 70%+）はズーム中に常時カーソルを追従する。ズームのトリガーが多すぎることと相まって、映像が常に動き続ける。

### 課題4: ウィンドウコンテキストの活用不足

`WindowFocus` イベントは記録・分析されているが、「同じウィンドウ内のクリックはズームを維持」「ウィンドウが変わったらズームをリセット」といったスコープ制御がない。

### 課題5: スクロールとRapidActionの扱い

スクロールは1.2xにズーム、RapidActionは1.8xにズームするが、いずれも視聴者にとって有益でないケースが多い。スクロール中のズームは画面の動きを倍増させ、RapidAction（連続クリック）は通常UIの日常操作であり注目に値しない。

### 課題6: 画面全体キャプチャのみの対応

現在は画面全体のキャプチャしかサポートしていない。Screen Studioをはじめ、FocuSee、Rapidemo等の競合はすべてウィンドウ指定録画をサポートしている。特定のウィンドウに絞って録画することで、(1) 不要なウィンドウの映り込みを防止、(2) ズーム対象が明確になりエフェクト品質が向上、(3) 仮想背景の適用が容易になる。

### 課題7: UI要素レベルの認識がない

現在はクリック座標とウィンドウ矩形のみを記録しており、ドロップダウンメニューの展開、ダイアログの出現、テキストフィールドへのフォーカス移動といったUI要素レベルの変化を検出できない。クリック位置ベースではメニュー内のどの項目に注目すべきかの判断ができず、ズームの精度に限界がある。

---

## 2. 新しい自動エフェクトの全体像

### パイプライン変更

```
┌─────────────────────────────────────────────────────────────┐
│  録画フェーズ（★拡張）                                         │
│  ├─ capture.rs   : 画面/ウインドウキャプチャ → frame_XXXX.png │
│  ├─ events.rs    : 入力イベント   → events.jsonl             │
│  ├─ focus.rs     : ウインドウ変化 → window_events.jsonl      │
│  ├─ ★ui_tracker.rs : UI要素変化 → ui_events.jsonl           │
│  └─ audio.rs     : 音声          → audio.wav                │
└──────────────┬──────────────────────────────────────────────┘
               │
               v
┌─────────────────────────────────────────────────────────────┐
│  前処理 (preprocessor.rs) — 変更なし                          │
│  ├─ マウス移動の間引き（3px未満除去）                           │
│  └─ ドラッグ検出（Click→20px移動→Release）                    │
└──────────────┬──────────────────────────────────────────────┘
               │
               v
┌─────────────────────────────────────────────────────────────┐
│  イベント解析 (analyzer.rs) — 拡張                            │
│  ├─ フェーズ1: アイドル区間検出（既存）                         │
│  ├─ フェーズ2: アクションセグメント分類（既存）                  │
│  ├─ フェーズ3: ウインドウコンテキスト付与（既存）                │
│  └─ ★フェーズ4: 重要度スコアリング（新規）                     │
└──────────────┬──────────────────────────────────────────────┘
               │
               v
┌─────────────────────────────────────────────────────────────┐
│  ★クリッククラスタリング (click_cluster.rs) — 新規（オプション）│
│  └─ 空間的・時間的に近接するクリックをクラスタ化                 │
└──────────────┬──────────────────────────────────────────────┘
               │
               v
┌─────────────────────────────────────────────────────────────┐
│  ズーム計画 (zoom_planner.rs) — 大幅改修                      │
│  ├─ ★重要度閾値フィルタリング（新規）                          │
│  ├─ ★クールダウン制御（新規）                                 │
│  ├─ ★ウィンドウスコープ判定（新規）                            │
│  ├─ 2段階ズーム（既存）                                      │
│  └─ 重複除去・カット検出（既存）                               │
└──────────────┬──────────────────────────────────────────────┘
               │
               v
┌─────────────────────────────────────────────────────────────┐
│  フレーム合成 (compositor.rs) — パラメータ調整                  │
│  ├─ キーフレーム適用 → スプリングアニメーション                  │
│  ├─ デッドゾーンカーソル追従（★デッドゾーン拡大）                │
│  ├─ クロップ＆スケール                                        │
│  ├─ カーソル・クリックリング・キーバッジ描画                     │
│  └─ 角丸・影・背景合成                                        │
└──────────────┬──────────────────────────────────────────────┘
               │
               v
┌─────────────────────────────────────────────────────────────┐
│  エクスポート (encoder.rs) — 変更なし                          │
│  └─ BMP連番 → FFmpeg → MP4 / GIF / WebM                     │
└─────────────────────────────────────────────────────────────┘
```

### 変更ファイル一覧

| ファイル | 変更内容 | 影響度 |
|---------|---------|--------|
| `engine/analyzer.rs` | フェーズ4: `ScoredSegment` 追加、`score_segments()` 関数 | 中（新規追加） |
| `engine/click_cluster.rs` | 新規モジュール: クリッククラスタリング | 小（新規追加、オプション） |
| `engine/zoom_planner.rs` | `generate_zoom_plan_v2()`: 重要度閾値、クールダウン、ウィンドウスコープ | 大（既存ロジック置換） |
| `engine/compositor.rs` | デッドゾーン半径の拡大、パン追従パラメータ調整 | 小（定数変更） |
| `engine/spring.rs` | half-life定数の最適化 | 小（定数変更） |
| `config/mod.rs` | `EffectsSettings` に新パラメータ追加、`RecordingMode` 追加 | 中 |
| `config/defaults.rs` | 新パラメータのデフォルト値 | 小 |
| `export/encoder.rs` | `compose_frames()` 内の呼び出し変更 | 小 |
| ★`recording/capture.rs` | ウィンドウ指定キャプチャモードの追加 | 大（新規機能） |
| ★`recording/ui_tracker.rs` | 新規: UI Automation APIによるUI要素変化の追跡 | 大（新規モジュール） |
| ★`engine/ui_context.rs` | 新規: UI要素イベントの解析とズーム統合 | 中（新規モジュール） |

---

## 3. ズームトリガーの定義と優先順位

### 3.1 重要度スコアリング

各セグメントに0.0〜1.0の重要度スコア（`importance`）を付与する。スコアが閾値（デフォルト: 0.4）を超えたセグメントのみがズームのトリガーとなる。

#### スコア計算ルール

```
ScoredSegment {
    segment: Segment,     // 既存のセグメント
    importance: f64,      // 0.0 - 1.0
}
```

| セグメントタイプ | ベーススコア | 追加条件 | 追加スコア | 最終スコア例 |
|-----------------|------------|---------|-----------|------------|
| **Click** | 0.3 | ウィンドウ変更あり | +0.4 | 0.7 |
|  |  | 前セグメントとの距離 > 300px | +0.2 | 0.5 |
|  |  | Long Idle後の最初のクリック | +0.3 | 0.6 |
|  |  | Medium Idle後の最初のクリック | +0.15 | 0.45 |
| **TextInput** | 0.8 | （常に高重要度） | — | 0.8 |
| **Scroll** | 0.1 | （★大幅引き下げ: 現行0.2→0.1） | — | 0.1 |
| **RapidAction** | 0.1 | （★大幅引き下げ: 連打は無視） | — | 0.1 |
| **Idle (Long)** | 0.5 | （ズームアウトトリガー） | — | 0.5 |
| **Idle (Medium)** | 0.3 | — | — | 0.3 |
| **Idle (Short)** | 0.0 | （無視） | — | 0.0 |

#### 重要度閾値

```
ZOOM_IMPORTANCE_THRESHOLD = 0.4
```

- スコア < 0.4 のセグメントはズーム変更をトリガーしない
- 例外: `Idle(Long)` はスコアに関わらずズームアウトを許可（映像の「呼吸」のため）

#### 効果の予測

現行方式ではすべてのClick/TextInput/Scroll/RapidActionでズームが発生するが、重要度フィルタリングにより：
- 単独のClickの多くがフィルタされる（スコア0.3 < 閾値0.4）
- ウィンドウ変更を伴うClickは通過する（スコア0.7）
- 長い操作停止後の再開Clickは通過する（スコア0.6）
- TextInputは常に通過する（スコア0.8）
- ScrollとRapidActionはほぼ常にフィルタされる（スコア0.1）
- **結果: ズーム頻度が60-70%減少**

### 3.2 クールダウン制御

連続するズーム変更の最小インターバルを強制する。

```
MIN_ZOOM_CHANGE_INTERVAL_MS = 1500  // 1.5秒
```

- 前回のズーム変更から1.5秒未満のセグメントは、重要度に関わらずスキップ
- **例外**: ウィンドウ変更（`window_changed == true`）は常にクールダウンを無視して許可
- **例外**: `Idle(Long)` によるズームアウトはクールダウンを無視

### 3.3 ウィンドウスコープ

同一ウィンドウ内の操作は、ズームレベルを維持したままパン（カメラ移動）のみ行う。

```
判定ロジック:
1. セグメントのwindow_rectを取得
2. 前回のwindow_rectと比較（位置・サイズの差が50px以内なら同一ウィンドウ）
3. 同一ウィンドウ AND window_changed == false → ズームレベル維持、パンのみ
4. ウィンドウ変更あり → 通常の2段階ズーム処理
```

効果:
- 同じウィンドウ内でボタンを複数クリックしても、ズームレベルは一定
- ビューポートはクリック位置にゆっくりパンするのみ
- ウィンドウを切り替えた時だけ、新しいウィンドウにフィット→クリック位置にズーム

---

## 4. パン（カメラ移動）のロジック

### 4.1 デッドゾーンの拡大

カーソル追従のデッドゾーンを拡大し、映像の安定性を向上させる。

```
// 現行
DEAD_ZONE_RADIUS = 0.3   // ビューポートの30%
SOFT_ZONE_RADIUS = 0.7   // ビューポートの70%

// 新仕様
DEAD_ZONE_RADIUS = 0.4   // ビューポートの40%（★拡大）
SOFT_ZONE_RADIUS = 0.75  // ビューポートの75%（★拡大）
```

効果:
- デッドゾーン拡大により、カーソルが中央付近で動いてもカメラが反応しない
- ソフトゾーンの勾配が緩やかになり、追従開始時の動きがより滑らか

### 4.2 パン速度の低下

パンのSpring half-lifeを長くし、カメラ移動をより緩やかにする。

```
// 現行
VIEWPORT_PAN = 0.15s

// 新仕様
VIEWPORT_PAN = 0.22s  // ★47%遅く
```

### 4.3 シーン遷移の処理

大きな位置ジャンプはパンではなくカットで処理する。

```
// 現行
CUT_DISTANCE_THRESHOLD = 0.5  // 画面対角の50%

// 新仕様（変更なし、現行値を維持）
CUT_DISTANCE_THRESHOLD = 0.5
```

カット時の処理:
- `TransitionType::Cut` → `viewport.snap_to(x, y, zoom)` で瞬時移動
- パンのSpringをスキップし、映像的には「場面転換」として処理

---

## 5. アイドル状態の処理

### 5.1 アイドルレベルの再定義

```
// 現行
IDLE_SHORT_MS  = 800    // 800ms〜2s: 何もしない
IDLE_MEDIUM_MS = 2000   // 2s〜5s: 1.2xまでズームアウト
IDLE_LONG_MS   = 5000   // 5s+: 1.0xまで完全ズームアウト

// 新仕様
IDLE_SHORT_MS  = 1000   // ★延長: 1s〜3s: 何もしない
IDLE_MEDIUM_MS = 3000   // ★延長: 3s〜6s: 1.2xまでズームアウト
IDLE_LONG_MS   = 6000   // ★延長: 6s+: 1.0xまで完全ズームアウト
```

理由: 現行の閾値ではアイドル検出が過敏。ユーザーが画面を見ながら考えている時間（1-2秒）でもズームアウトが始まってしまう。閾値を延長することで、より自然な「呼吸」を実現する。

### 5.2 ズームアウトの段階的処理

```
Idle(Short):  ズーム維持（何もしない）
Idle(Medium): 現在のzoomから1.2xに向けて緩やかにズームアウト
              → SpringHint: ZOOM_OUT_SLOW (0.40s)
Idle(Long):   1.0xまで完全ズームアウト
              → SpringHint: ZOOM_OUT_SLOW (0.40s)
```

---

## 6. Spring物理パラメータの推奨値

### 6.1 half-life定数の最適化

全体的にアニメーションを「ゆったり」させ、映像の落ち着き感を向上させる。

```
// 定数名                  現行     新仕様     変更率    用途
VIEWPORT_PAN             0.15s → 0.22s    +47%    カーソル追従パン
WINDOW_PAN               0.20s → 0.25s    +25%    ウィンドウ表示用パン

ZOOM_IN_FAST             0.12s → 0.18s    +50%    クリック（重要度高）
ZOOM_IN                  0.20s → 0.25s    +25%    テキスト入力
ZOOM_IN_SLOW             0.28s → 0.30s    +7%     スクロール（ほぼ使用されない）

ZOOM_OUT                 0.25s → 0.35s    +40%    中程度アイドル
ZOOM_OUT_SLOW            0.35s → 0.40s    +14%    長時間アイドル

WINDOW_ZOOM              0.25s → 0.30s    +20%    ウィンドウレベルズーム
CURSOR_SMOOTHING         0.05s → 0.05s    変更なし  カーソルジッター除去
```

### 6.2 95%到達時間の比較

| 動作 | 現行95%到達 | 新仕様95%到達 | 体感 |
|------|-----------|-------------|------|
| クリックズームイン | ~0.5s | ~0.8s | 素早いがスムーズ |
| テキスト入力ズーム | ~0.9s | ~1.1s | ゆったり自然 |
| アイドルズームアウト | ~1.1s | ~1.5s | ゆっくり元に戻る |
| カーソル追従パン | ~0.6s | ~1.0s | 落ち着いた追従 |

### 6.3 非対称ズームの維持

ズームイン（速い）とズームアウト（遅い）の非対称性は維持する。これは人間の視覚系にとって自然：
- **ズームイン**: 注目を集めるため、素早く（0.18-0.25s）
- **ズームアウト**: 文脈に戻るため、ゆっくり（0.35-0.40s）

---

## 7. ウィンドウ認識とセマンティックズーム

### 7.1 ウィンドウスコープ判定

```rust
fn is_same_window(a: &Rect, b: &Rect) -> bool {
    (a.x - b.x).abs() < 50.0
    && (a.y - b.y).abs() < 50.0
    && (a.width - b.width).abs() < 50.0
    && (a.height - b.height).abs() < 50.0
}
```

### 7.2 ウィンドウ変更時の2段階ズーム（既存維持）

```
段階1 (アクション400ms前): ウィンドウ全体にフィットするズーム
  → TransitionType::WindowZoom
  → ズーム倍率 = calc_zoom_to_fit(window_rect) （最低1.2x, max_zoom上限）

段階2 (アクション時刻): アクション点へのズーム
  → 通常のセグメント別トランジション
```

### 7.3 ウィンドウサイズに応じたズーム倍率の動的決定

ウィンドウサイズに基づいてズーム倍率を調整する。小さなウィンドウにはより大きなズーム、フルスクリーンアプリには控えめなズーム。

```
calc_zoom_to_fit(window_rect, screen_w, screen_h, padding) -> f64:
    // ウィンドウが画面の80%以上 → 控えめなズーム (1.3-1.5x)
    // ウィンドウが画面の50-80% → 中程度のズーム (1.5-2.0x)
    // ウィンドウが画面の50%未満 → ウィンドウにフィット (2.0-3.0x)

    let window_ratio = (window_rect.width * window_rect.height)
                     / (screen_w * screen_h);

    if window_ratio > 0.8 {
        1.3  // フルスクリーンアプリ: 控えめ
    } else {
        let fit_zoom_w = screen_w / (window_rect.width + padding * 2);
        let fit_zoom_h = screen_h / (window_rect.height + padding * 2);
        fit_zoom_w.min(fit_zoom_h).min(max_zoom).max(1.2)
    }
```

---

## 8. クリッククラスタリング（Phase 3 オプション機能）

### 8.1 概要

空間的・時間的に近接するクリックをクラスタにまとめ、クラスタ単位でズーム判定を行う。個別のクリックごとにズームするのではなく、「操作の塊」として扱う。

### 8.2 クラスタリングパラメータ

```
CLUSTER_SPATIAL_EPS  = 200.0   // ピクセル: この距離以内のクリックを同一クラスタに
CLUSTER_TEMPORAL_EPS = 3000    // ms: この時間以内のクリックを同一クラスタに
```

### 8.3 クラスタからのズーム倍率決定

```
クラスタのクリック数に基づく控えめなズーム:
  1クリック:   1.8x  — 単発の重要なアクション
  2-3クリック: 1.6x  — 少数の関連操作
  4+クリック:  1.4x  — 多数の操作（広めのビュー）

ウィンドウ矩形がある場合はウィンドウフィットを優先。
```

### 8.4 実装優先度

クリッククラスタリングはPhase 3（オプション機能）として位置づける。Phase 1-2の改善だけで映像品質は大幅に向上するため、クラスタリングは追加的な品質向上策。

---

## 9. ウィンドウ指定録画モード

### 9.1 概要

Screen Studioは Display / Window / Area の3モードを提供している。Snappiにもウィンドウ指定録画モードを追加し、特定のウィンドウに絞った録画を可能にする。

### 9.2 録画モードの定義

```rust
pub enum RecordingMode {
    Display,           // 画面全体（現行の動作）
    Window(WindowTarget),  // 特定ウィンドウを指定
}

pub struct WindowTarget {
    pub hwnd: isize,       // ウィンドウハンドル
    pub title: String,     // ウィンドウタイトル（表示用）
    pub rect: Rect,        // 録画開始時のウィンドウ矩形
}
```

### 9.3 ウィンドウ指定録画の動作

#### 録画開始フロー

1. ユーザーが録画モードで「Window」を選択
2. 画面上のウィンドウ一覧をオーバーレイ表示（または右クリックリストで選択）
3. 対象ウィンドウをクリックして選択
4. 選択されたウィンドウの `HWND` を取得し、`WindowTarget` として保存
5. 録画開始

#### キャプチャの変更 (`capture.rs`)

```
Display モード（現行）:
  GetDC(None) → 画面全体をBitBlt

Window モード（新規）:
  1. GetWindowRect(hwnd) で最新のウィンドウ位置・サイズを取得
  2. PrintWindow(hwnd) または GetDC(hwnd) + BitBlt でウィンドウ内容をキャプチャ
  3. ウィンドウが最小化された場合 → 前フレームを再利用
  4. ウィンドウがリサイズされた場合 → 新しいサイズでキャプチャ（出力時にスケーリング）
```

**重要**: `PrintWindow` はウィンドウが他のウィンドウに隠れていてもキャプチャ可能（GDI BitBltとの最大の違い）。ただしハードウェアアクセラレーションされたウィンドウ（一部のゲーム等）では動作しない場合がある。

#### メタデータの変更 (`meta.json`)

```json
{
  "version": 2,
  "recording_mode": "window",
  "window_title": "Visual Studio Code",
  "window_initial_rect": [100, 50, 1820, 1030],
  ...
}
```

### 9.4 ウィンドウモード時の自動エフェクトの簡素化

ウィンドウ指定録画では、録画対象が1つのウィンドウに限定されるため、エフェクト処理が大幅に簡素化される。

| 処理 | Display モード | Window モード |
|------|--------------|-------------|
| ウィンドウ切替の検出 | 必要（複数ウィンドウ間） | 不要（対象は1つ） |
| 2段階ズーム | 必要（ウィンドウ→アクション点） | 不要（直接アクション点へ） |
| ウィンドウスコープ判定 | 必要 | 不要（全操作が同一ウィンドウ内） |
| 背景処理 | 他ウィンドウが映り込む | 仮想背景で美しく表示 |
| ズーム倍率の計算 | 画面全体に対する比率 | ウィンドウに対する比率 |

### 9.5 仮想背景

ウィンドウモードでは、ウィンドウの周囲に仮想背景を表示できる。これはScreen Studioの重要な機能の1つ。

```
ウィンドウキャプチャ（実コンテンツ）
  ↓
OutputStyle.background（グラデーション/単色/画像）のキャンバス上に配置
  ↓
角丸 + ドロップシャドウ適用
  ↓
ズーム・パンエフェクト適用
```

既存の背景・角丸・シャドウの仕組みはそのまま活用できる。

---

## 10. UI Automation連携によるスマートズーム

### 10.1 概要

Windows UI Automation APIを活用して、UI要素レベルの変化を検出し、ズームの精度を向上させる。**これはScreen Studioにはない機能であり、Snappiの大きな差別化要因となる。**

Screen Studioはクリック位置ベースのオートズームのみを実装しているが、Snappiはクリック位置に加えてUI要素の矩形情報を活用することで、「メニューが展開された→メニュー全体を映す」「ダイアログが出現した→ダイアログにフォーカスする」といった知的なズームが可能になる。

参考: macOS向けOSSのScreenize（`github.com/syi0808/screenize`）がAccessibility APIを使ったUI要素検出を実装しており、本セクションの設計に影響を与えている。

### 10.2 Windows UI Automation APIの活用

#### 検出するUIイベント

| UIイベント | 検出方法 | ズームへの応用 |
|-----------|---------|-------------|
| **フォーカス変更** | `IUIAutomation::AddFocusChangedEventHandler` | テキストフィールド、ボタン等にフォーカスが移動 → その要素の矩形にスマートズーム |
| **メニュー展開** | `StructureChangedEvent`（子要素の追加） | メニュー要素が出現 → メニュー全体の矩形にフィット |
| **ダイアログ出現** | `StructureChangedEvent` + ControlType判定 | ダイアログ出現 → ダイアログ矩形にフィット |
| **コンボボックス展開** | `PropertyChangedEvent`（ExpandCollapsePattern） | ドロップダウン展開 → 展開された領域にフィット |

#### Rust実装に使用するクレート

```toml
# Cargo.toml
[dependencies]
uiautomation = "0.x"   # Windows UIAutomation APIラッパー
# または
windows = { version = "0.x", features = ["UI_UIAutomation"] }
```

### 10.3 録画時のUI要素追跡 (`recording/ui_tracker.rs`)

録画中に専用スレッドでUI Automationイベントをリッスンし、`ui_events.jsonl` に記録する。

#### UIイベントの記録フォーマット

```jsonl
{"type":"ui_focus","t":1250,"control":"Edit","name":"Search","rect":[450,120,800,145],"automation_id":"searchBox"}
{"type":"ui_menu_open","t":2100,"control":"Menu","name":"File","rect":[5,25,200,350]}
{"type":"ui_menu_close","t":3400,"control":"Menu","name":"File"}
{"type":"ui_dialog_open","t":5200,"control":"Window","name":"Save As","rect":[400,200,1000,600]}
{"type":"ui_dialog_close","t":8100,"control":"Window","name":"Save As"}
{"type":"ui_combo_expand","t":9500,"control":"ComboBox","name":"Format","rect":[500,300,700,500]}
{"type":"ui_combo_collapse","t":10200,"control":"ComboBox","name":"Format"}
```

#### 追跡スレッドの動作

```
1. IUIAutomation::CreateInstance() でUIAutomationを初期化
2. FocusChangedEventHandler を登録
3. StructureChangedEventHandler を登録
4. PropertyChangedEventHandler を登録（ExpandCollapsePattern等）
5. イベント発生時:
   a. 対象要素の ControlType, Name, BoundingRectangle を取得
   b. タイムスタンプ付きで ui_events.jsonl に書き込み
6. 録画停止時: ハンドラを解除、スレッド終了
```

### 10.4 エクスポート時のUI要素活用 (`engine/ui_context.rs`)

#### UIイベントと入力イベントの統合

```
events.jsonl          (click, key, scroll, mouse_move)
window_events.jsonl   (window_focus)
ui_events.jsonl       (ui_focus, ui_menu_open, ui_dialog_open, ...)
     ↓ タイムスタンプ順にマージ
unified_events
     ↓ analyzer.rs（フェーズ1-4）
scored_segments
     ↓ ui_context.rs（UI要素コンテキストの付与）
enriched_segments
     ↓ zoom_planner.rs
zoom_keyframes
```

#### UIイベントによる重要度スコアの加算

```
// UI要素変化に基づく追加スコア
UIフォーカス変更（テキストフィールド） → importance += 0.3, zoom_target = 要素の矩形
メニュー展開                         → importance += 0.5, zoom_target = メニュー矩形
ダイアログ出現                       → importance += 0.6, zoom_target = ダイアログ矩形
コンボボックス展開                    → importance += 0.4, zoom_target = 展開領域の矩形
```

#### UIイベントによるズームターゲットの改善

従来はクリック座標（点）にズームしていたが、UI要素の矩形を取得できるため、**矩形にフィットするズーム**が可能になる。

```
従来: クリック(500, 300) → その座標に2.0xでズーム
新規: クリック(500, 300) + UI矩形[450,280,800,320]
      → 矩形全体が見えるようにフィットズーム（矩形サイズに応じた適切な倍率）
```

これにより：
- テキストフィールドをクリック → フィールド全体が見える倍率でズーム
- メニューをクリック → メニュー全体が見える倍率でズーム
- ダイアログが出現 → ダイアログ全体が見える倍率でズーム

### 10.5 制約事項と注意点

| 制約 | 影響 | 対策 |
|------|------|------|
| **権限**: UIAutomation APIは特別な権限不要 | なし | — |
| **パフォーマンス**: イベントハンドラのコールバック頻度 | 高頻度のフォーカス変更でCPU負荷 | 100msのデバウンス、同一要素の重複除去 |
| **非対応アプリ**: UIAutomationを実装していないアプリ | UI要素情報が取得できない | フォールバック: クリック位置ベースのズーム（現行動作） |
| **カスタムUI**: Electron等のカスタム描画アプリ | 正確な要素矩形が取れない場合がある | ウィンドウ矩形のみ使用にフォールバック |
| **スレッド安全性**: COMのアパートメントモデル | UIAutomationはSTAスレッドが必要 | 専用STAスレッドで動作、チャネルでデータ送信 |

### 10.6 後方互換性

- `ui_events.jsonl` がない録画（現行の全録画）: UI要素コンテキストなしで動作。クリック位置ベースのズームのみ（従来通り）
- UIAutomationが使えない環境: `ui_tracker` スレッドが起動失敗しても録画は正常に動作。エラーをログに記録するのみ

---

## 11. ユーザー設定パラメータ

### 9.1 EffectsSettings の拡張

```rust
pub struct EffectsSettings {
    // 既存パラメータ
    pub auto_zoom_enabled: bool,           // 自動ズームON/OFF（デフォルト: true）
    pub default_zoom_level: f64,           // クリック時ズーム倍率（デフォルト: 2.0）
    pub text_input_zoom_level: f64,        // テキスト入力時ズーム倍率（デフォルト: 2.5）
    pub max_zoom: f64,                     // ズーム倍率上限（デフォルト: 3.0）
    pub idle_timeout_ms: u64,             // ★廃止（アイドル閾値は内部定数に変更）
    pub click_ring_enabled: bool,          // クリックリングON/OFF（デフォルト: true）
    pub key_badge_enabled: bool,           // キーバッジON/OFF（デフォルト: true）
    pub cursor_smoothing: bool,            // カーソルスムージングON/OFF（デフォルト: true）

    // ★新規パラメータ
    pub zoom_intensity: ZoomIntensity,     // ズーム頻度プリセット（デフォルト: Balanced）
    pub animation_speed: AnimationSpeed,   // アニメーション速度プリセット（デフォルト: Mellow）
}
```

### 9.2 ZoomIntensity（ズーム頻度プリセット）

ユーザーが「どの程度頻繁にズームするか」を直感的に制御するプリセット。

```rust
pub enum ZoomIntensity {
    Minimal,    // 最小限: ウィンドウ変更時とTextInput時のみ
    Balanced,   // バランス: 重要度スコア0.4以上（デフォルト）
    Active,     // 積極的: 重要度スコア0.25以上
}
```

| プリセット | 重要度閾値 | クールダウン | 典型的ズーム頻度 | 推奨シーン |
|-----------|-----------|------------|----------------|-----------|
| Minimal | 0.6 | 3000ms | 1-2回/分 | プレゼンテーション、概要説明 |
| Balanced | 0.4 | 1500ms | 3-5回/分 | チュートリアル、デモ |
| Active | 0.25 | 800ms | 5-8回/分 | 詳細な操作説明 |

### 9.3 AnimationSpeed（アニメーション速度プリセット）

Screen Studioの4段階プリセットを参考にした速度設定。

```rust
pub enum AnimationSpeed {
    Slow,      // ゆっくり: 落ち着いた印象
    Mellow,    // まろやか: 自然なバランス（デフォルト）
    Quick,     // 素早い: テンポの良い印象
    Rapid,     // 高速: エネルギッシュな印象
}
```

| プリセット | zoom_in HL | zoom_out HL | pan HL | 体感 |
|-----------|-----------|------------|--------|------|
| Slow | 0.30s | 0.50s | 0.30s | ゆったり、プロフェッショナル |
| Mellow | 0.18s | 0.35s | 0.22s | 自然、心地よい（デフォルト） |
| Quick | 0.12s | 0.25s | 0.15s | テンポ良い、現行に近い |
| Rapid | 0.08s | 0.18s | 0.10s | 素早い、エネルギッシュ |

### 9.4 UI設定画面の変更

Settings.tsxのEffectsセクションに追加:

```
Recording
├─ ★Recording Mode: [Display | Window]  ← 新規
│   └─ (Window選択時) Target Window: [ウィンドウ選択UI]
│
Effects
├─ [✓] Auto Zoom
│   ├─ Zoom Level: [1.5x | 2.0x | 2.5x | 3.0x]  ← 既存
│   ├─ ★Zoom Frequency: [Minimal | Balanced | Active]  ← 新規
│   └─ ★Animation Speed: [Slow | Mellow | Quick | Rapid]  ← 新規
├─ ★[✓] Smart Zoom (UI Element Detection)  ← 新規（Phase 5）
├─ [✓] Click Ring       ← 既存
├─ [✓] Key Display      ← 既存
└─ [✓] Cursor Smoothing ← 既存
```

---

## 12. 実装ロードマップ

### Phase 1: 最小変更で最大効果（推定工数: 2-3時間）

**目標**: ズーム頻度を60-70%削減し、映像の安定性を劇的に改善する。

変更内容:
1. `zoom_planner.rs` に `MIN_ZOOM_CHANGE_INTERVAL_MS = 1500` を追加
2. `Scroll` と `RapidAction` のズームを無効化（スコア0.1 < 閾値0.4）
3. デッドゾーンの拡大: `DEAD_ZONE_RADIUS = 0.3 → 0.4`
4. Spring half-life の調整（定数変更のみ）

変更ファイル:
- `engine/zoom_planner.rs` — クールダウンロジック追加
- `engine/compositor.rs` — デッドゾーン定数変更
- `engine/spring.rs` — half-life定数変更

**これだけで現行の最大の問題（映像が激しく動く）が大幅に改善される。**

### Phase 2: 重要度スコアリング（推定工数: 3-4時間）

**目標**: ズームの「知性」を向上させ、意味のあるアクションにのみズームする。

変更内容:
1. `analyzer.rs` に `ScoredSegment` 型と `score_segments()` 関数を追加
2. `zoom_planner.rs` の `generate_zoom_plan()` をスコアベースに改修
3. ウィンドウスコープ判定の実装
4. `config/mod.rs` に `ZoomIntensity` enum を追加
5. フロントエンドに Zoom Frequency プリセットUIを追加

変更ファイル:
- `engine/analyzer.rs` — ScoredSegment追加
- `engine/zoom_planner.rs` — generate_zoom_plan() 改修
- `config/mod.rs` — ZoomIntensity追加
- `config/defaults.rs` — デフォルト値
- `src/lib/types.ts` — TypeScript型追加
- `src/components/Settings.tsx` — UI追加

### Phase 3: クラスタリングとアニメーション速度（推定工数: 3-4時間）

**目標**: さらなる品質向上と、ユーザーカスタマイズ性の拡大。

変更内容:
1. `engine/click_cluster.rs` 新規モジュール追加
2. `config/mod.rs` に `AnimationSpeed` enum を追加
3. AnimationSpeedに基づくSpring half-lifeの動的選択
4. フロントエンドに Animation Speed プリセットUIを追加
5. アイドル閾値の調整（IDLE_SHORT: 1000ms, IDLE_MEDIUM: 3000ms, IDLE_LONG: 6000ms）

### Phase 4: ウィンドウ指定録画モード（推定工数: 6-8時間）

**目標**: 特定のウィンドウに絞った録画を可能にし、エフェクト品質を向上させる。

変更内容:
1. `config/mod.rs` に `RecordingMode` enum と `WindowTarget` 型を追加
2. `recording/capture.rs` に `PrintWindow` / `GetDC(hwnd)` によるウィンドウキャプチャを追加
3. フロントエンドにウィンドウ選択UI（録画開始前のモード切り替え）
4. `meta.json` に `recording_mode` / `window_title` / `window_initial_rect` フィールド追加
5. `zoom_planner.rs` のウィンドウモード時の簡素化パス（2段階ズーム不要）

変更ファイル:
- `recording/capture.rs` — ウィンドウキャプチャモード追加
- `config/mod.rs` — RecordingMode, WindowTarget 追加
- `commands.rs` — 録画開始コマンドにモード引数追加
- フロントエンド — ウィンドウ選択UI
- `engine/zoom_planner.rs` — ウィンドウモード時の簡素化パス

### Phase 5: UI Automation連携（推定工数: 8-10時間）

**目標**: UI要素レベルの変化を検出し、Screen Studioを超えるスマートズームを実現する。

変更内容:
1. `recording/ui_tracker.rs` 新規モジュール追加（UI Automation イベントリスナー）
2. `engine/ui_context.rs` 新規モジュール追加（UIイベントの解析・ズーム統合）
3. `analyzer.rs` に UIイベントを統合した重要度スコア加算ロジック
4. `zoom_planner.rs` に UI要素矩形ベースのフィットズーム

変更ファイル:
- `recording/ui_tracker.rs` — 新規: UIAutomationイベントリスナー
- `engine/ui_context.rs` — 新規: UIイベント解析
- `engine/analyzer.rs` — UIイベント統合
- `engine/zoom_planner.rs` — 矩形フィットズーム
- `Cargo.toml` — `uiautomation` or `windows` クレートの UI Automation features 追加

**Cargo.toml依存追加**:
```toml
uiautomation = "0.x"
# または
windows = { features = ["UI_UIAutomation"] }
```

### Phase 6: 将来の拡張（優先度低）

- モーションブラー: ズーム・パン時のフレームブラー（実装コスト高）
- タイムラインUI: ズームポイントの手動追加/削除/タイミング調整
- ドラッグ対応ズーム: ドラッグ操作の範囲にフィットするズーム
- エリア指定録画: ドラッグで任意の範囲を指定して録画

---

## 13. 後方互換性

### 既存録画データとの互換性

- `window_events.jsonl` がない録画: 全セグメントで `window_rect: None`, `window_changed: false`。重要度スコアリングはウィンドウ関連の加点なしで動作
- `ui_events.jsonl` がない録画: UI要素コンテキストなしで動作。クリック位置ベースのズームのみ（従来通り）
- 既存の `EffectsSettings` にない新フィールド: Deserialize時にデフォルト値（`ZoomIntensity::Balanced`, `AnimationSpeed::Mellow`）を適用
- `meta.json` に `recording_mode` がない録画: `RecordingMode::Display` として扱う

### 既存コードとの互換性

- 現行の `generate_zoom_plan()` は `generate_zoom_plan_v1()` にリネームして残す
- 新実装は `generate_zoom_plan_v2()` として追加
- `EffectsSettings` のフラグ（`use_smart_zoom: bool` = デフォルトtrue）で切り替え

---

## 14. 評価基準

### 品質目標

| 指標 | 現行 | Phase 1後 | Phase 2後 | Phase 4-5後 | 目標 |
|------|------|----------|----------|-----------|------|
| ズーム回数/分 | 8-15回 | 3-5回 | 2-4回 | 2-4回 | 2-5回 |
| カメラ静止時間率 | ~30% | ~60% | ~70% | ~75% | 60-80% |
| ウィンドウ変更時の適切なズーム | 50% | 50% | 90% | 95% | 85%+ |
| UI要素へのフィットズーム精度 | — | — | — | 80%+ | 80%+ |
| 視聴者の快適性（主観） | 低 | 中 | 高 | 非常に高 | 高 |

### テスト方法

1. 同一の録画データに対して現行/新仕様でエクスポートし、映像を比較
2. ズームキーフレーム数をログ出力し、定量的に削減率を確認
3. 3つのユースケースでテスト:
   - チュートリアル動画（VS Code操作）
   - デモ動画（Webアプリ操作）
   - バグレポート（複数ウィンドウ切り替え）
