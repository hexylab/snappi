# Snappi エフェクトエンジン設計書

**Status**: Final (チーム議論による合意版)
**Date**: 2026-02-07
**Authors**: Ken (実用主義), Yuki (体験追求), Rio (理論派) — team-lead 集約

---

## 0. 目標

ユーザーがデスクトップ操作（クリック、ウィンドウ操作、文字入力等）を録画するだけで、
**操作がわかりやすい動画**が自動的にエクスポートされるエフェクトエンジンを構築する。

Screen Studio (Mac) のWindows版代替として、全自動でプロ品質の動画を生成する。

---

## 1. 全体アーキテクチャ

```
Recording Phase                    Export Phase
===============                    ============

[FrameCapture] → frames/           events.jsonl + frames/
[EventCollector] → events.jsonl          │
  mouse_move (10ms, 生データ保持)         ▼
  click + click_release             ┌──────────────┐
  key (modifiers付き)               │ Preprocessor │  mouse_move間引き, ドラッグ検出
  scroll                            └──────┬───────┘
                                           ▼
                                    ┌──────────────┐
                                    │   Analyzer   │  セマンティックセグメント分割
                                    └──────┬───────┘
                                           ▼
                                    ┌──────────────┐
                                    │ ZoomPlanner  │  ズームキーフレーム生成
                                    └──────┬───────┘
                                           ▼
                                    ┌──────────────┐
                                    │  Compositor  │  フレーム毎合成:
                                    │              │   1. キーフレーム適用
                                    │              │   2. Dead Zone カーソル追従
                                    │              │   3. スプリング更新 (解析解)
                                    │              │   4. crop + scale
                                    │              │   5. エフェクト描画
                                    └──────┬───────┘
                                           ▼
                                    ┌──────────────┐
                                    │   Encoder    │  FFmpeg
                                    └──────────────┘
```

### 設計原則

1. **録画側は最小限の変更** — 生データを保持し、Preprocessorでエクスポート時に加工する (Ken)
2. **解析解スプリング物理** — フレームレート非依存、無条件安定 (Rio)
3. **カメラワーク理論の適用** — ズームは「視聴者の注意を導くカメラ」(Yuki)
4. **段階的実装** — 既存コードの骨格を活かし、壊れている部分から修正

### 変更ファイル一覧

| ファイル | 変更内容 | Phase |
|---------|---------|-------|
| `src-tauri/src/recording/events.rs` | modifier追跡, ClickRelease追加 | 1 |
| `src-tauri/src/config/mod.rs` | ClickRelease イベント型追加 | 1 |
| `src-tauri/src/engine/preprocessor.rs` | **新規**: mouse_move間引き, ドラッグ検出 | 1 |
| `src-tauri/src/engine/mod.rs` | preprocessor モジュール追加 | 1 |
| `src-tauri/src/engine/analyzer.rs` | Idle検出修正 + 段階的Idle | 1 |
| `src-tauri/src/engine/spring.rs` | 解析解スプリング + half-life | 1 |
| `src-tauri/src/engine/compositor.rs` | Dead Zone追従, 非対称スプリング, click ring easing, 角丸AA | 1 |
| `src-tauri/src/engine/cursor_smoother.rs` | 双方向スムージング | 2 |
| `src-tauri/src/engine/zoom_planner.rs` | ズームアウト強化, ヒステリシス | 2 |
| `src-tauri/src/config/defaults.rs` | half-life パラメータ, プリセット | 2 |
| `src-tauri/src/export/encoder.rs` | Preprocessor呼び出し統合 | 1 |

---

## 2. イベント記録の改善

### 2.1 modifier状態追跡 (`events.rs`)

**問題**: `modifiers: vec![]` で常に空。Ctrl+Cが「C」と区別できない。

**解決**: ビットフラグで修飾キー状態を管理。

```rust
// events.rs のリスナー内
let modifier_state = Arc::new(Mutex::new(0u8));
// bit 0: Ctrl, bit 1: Shift, bit 2: Alt, bit 3: Meta(Win)

// KeyPress時: modifier キーならフラグをセット
// KeyRelease時: modifier キーならフラグをクリア
// 通常キーのKeyPress時: 現在のフラグから modifiers Vec を構築
```

rdev の `KeyRelease` イベントも処理し、`LShift`/`RShift` は `"Shift"` に正規化する。

### 2.2 ClickRelease イベント追加

**目的**: ドラッグ操作の正確な検出。

```rust
// config/mod.rs に追加
#[serde(rename = "click_release")]
ClickRelease { t: u64, btn: String, x: f64, y: f64 },
```

`rdev::EventType::ButtonRelease` をキャプチャし、ClickRelease として記録する。

### 2.3 mouse_move の処理方針

**録画側は変更しない**（10ms毎の記録を維持）。

理由:
- 生データを保持すればアルゴリズム改善時に再処理できる
- 10ms毎の記録はカーソルスムージングに好都合
- Preprocessor（エクスポート時）で間引けば十分

---

## 3. Preprocessor（新規モジュール）

`src-tauri/src/engine/preprocessor.rs` を新規作成。

### 3.1 mouse_move 間引き

```rust
pub fn thin_mouse_moves(
    events: &[RecordingEvent],
    distance_threshold: f64,  // default: 3.0px
) -> Vec<RecordingEvent> {
    // 1. significant events (Click/Key/Scroll) の前後100msを「保護領域」とする
    // 2. mouse_move は直前からの移動距離 >= distance_threshold のみ保持
    // 3. 200ms以上間隔が空いたら停止位置として1回記録
}
```

これにより:
- 手のふるえ（3px未満の微動）が除去される
- アイドル期間に自然な時間ギャップが生まれる
- significant events 近辺のカーソル位置は保持される

### 3.2 ドラッグ検出

```rust
pub struct DragEvent {
    pub start_ms: u64,
    pub end_ms: u64,
    pub start_x: f64, pub start_y: f64,
    pub end_x: f64, pub end_y: f64,
}

pub fn detect_drags(events: &[RecordingEvent]) -> Vec<DragEvent> {
    // Click → MouseMove(累積距離 > 20px) → ClickRelease パターンでドラッグを検出
    // ClickRelease がない場合は、Click後にmouse_moveが50px以上続くパターンで近似
}
```

---

## 4. セマンティック分析（Analyzer改修）

### 4.1 Idle検出の修正（最重要修正）

**問題**: mouse_move が10ms毎なので500ms閾値を超えるギャップが発生しない。

**解決**: **significant events（Click/Key/Scroll/ClickRelease）のみ**で時間ギャップを計測。

```rust
fn detect_idle_periods(events: &[RecordingEvent]) -> Vec<Segment> {
    let significant_events: Vec<_> = events.iter()
        .filter(|e| !matches!(e, RecordingEvent::MouseMove { .. }))
        .collect();

    // significant_events 間のギャップでアイドル判定
}
```

### 4.2 段階的Idle

アイドルの長さに応じて異なるズームアウト動作を行う:

| 段階 | 時間 | ズーム動作 |
|------|------|-----------|
| 短い思考 | 800ms - 2000ms | ズーム維持（変化なし） |
| 中程度の間 | 2000ms - 5000ms | ゆっくりズームアウト → 1.2x |
| 長い間 | 5000ms+ | 完全ズームアウト → 1.0x |

### 4.3 TextInput検出（Phase 1: Click→Keyパターン）

Focusイベントに頼らず、Click直後（500ms以内）にKeyイベントが続くパターンを TextInput として検出する。

```rust
// Click の後に Key が続くパターン
RecordingEvent::Click { t, x, y, .. } => {
    // 後続500ms以内のKeyイベントをスキャン
    // Keyが見つかれば TextInput セグメントを生成（クリック位置中心）
}
```

Phase 2 で Windows UI Automation (`IUIAutomation`) による正確なフォーカス追跡を実装する。

### 4.4 セグメントタイプ

Phase 1 は既存の5種を維持:

```rust
pub enum SegmentType {
    Click,       // 単発クリック
    TextInput,   // テキスト入力（Click→Keyパターン）
    Scroll,      // スクロール操作
    Idle,        // 操作なし（段階的: Short/Medium/Long）
    RapidAction, // 高速連続操作
}
```

Phase 2 で `Drag`, `KeyboardShortcut` 等を追加検討。

---

## 5. スプリング物理: 解析解（Rio方式）

### 5.1 なぜ解析解か

| 方式 | 安定性 | dt依存性 | 計算量 | 精度 |
|------|--------|---------|--------|------|
| Forward Euler (現状) | 条件付き | 高い | O(1) | 低い |
| サブステッピング | 改善 | 軽減 | O(N) | 中 |
| **解析解 (採用)** | **無条件安定** | **なし** | **O(1)** | **正確** |

### 5.2 臨界減衰スプリング（Critically Damped）

```rust
pub struct Spring {
    pub position: f64,
    pub velocity: f64,
    pub target: f64,
}

impl Spring {
    pub fn update(&mut self, half_life: f64, dt: f64) {
        let y = (4.0 * LN_2) / half_life.max(1e-5);
        let y_half = y / 2.0;
        let j0 = self.position - self.target;
        let j1 = self.velocity + j0 * y_half;
        let eydt = (-y_half * dt).exp();

        self.position = eydt * (j0 + j1 * dt) + self.target;
        self.velocity = eydt * (self.velocity - j1 * y_half * dt);
    }

    pub fn snap(&mut self, value: f64) {
        self.position = value;
        self.target = value;
        self.velocity = 0.0;
    }
}

const LN_2: f64 = 0.693147180559945;
```

### 5.3 half-life パラメータ化

`tension/friction/mass` の代わりに **half-life（秒）** を使用。
「ターゲットまでの距離が半分になるまでの時間」という直感的な意味を持つ。

```
damping = (4 * ln(2)) / half_life
stiffness = damping^2 / 4    (臨界減衰)
```

### 5.4 コンテキスト別 half-life

| コンテキスト | half-life | 説明 |
|-------------|-----------|------|
| ビューポート パン | 0.15s | カーソル追従（レスポンシブ） |
| ズームイン | 0.20s | 素早くズーム |
| ズームアウト | 0.35s | ゆっくり引く（映画的） |
| カーソルスムージング | 0.05s | 微ジッター除去のみ |

### 5.5 ズームイン/アウト非対称スプリング

ズームインは素早く（0.20s）、ズームアウトはゆっくり（0.35s）。
TransitionType に応じてズーム用スプリングの half-life を切り替える。

```rust
let zoom_half_life = match current_transition {
    TransitionType::SpringIn => 0.20,
    TransitionType::SpringOut => 0.35,
    _ => 0.25,
};
```

### 5.6 ユーザー向けプリセット (Phase 2)

Screen Studio互換の4段階:

| プリセット | pan half-life | zoom_in half-life | zoom_out half-life |
|-----------|--------------|-------------------|-------------------|
| Slow | 0.25s | 0.35s | 0.55s |
| Mellow (default) | 0.15s | 0.20s | 0.35s |
| Quick | 0.10s | 0.12s | 0.25s |
| Rapid | 0.08s | 0.10s | 0.18s |

### 5.7 不足減衰スプリング (Phase 2)

Quick/Rapid プリセットで微小なオーバーシュート（弾力感）を表現するために、
Phase 2 で不足減衰（underdamped）の解析解を追加する。

```
x(t) = A * e^(-zeta*omega*t) * cos(omega_d * t + phi) + g
where omega_d = omega * sqrt(1 - zeta^2)
```

Phase 1 は臨界減衰のみでシンプルに実装する。

---

## 6. ビューポート管理: Dead Zone / Soft Zone モデル（Rio方式）

### 6.1 ３層ゾーンモデル

```
┌─────────────────────────────────────────┐
│  PUSH ZONE (強制追従)                    │
│  ┌───────────────────────────────────┐  │
│  │  SOFT ZONE (段階的追従, smoothstep) │  │
│  │  ┌───────────────────────────┐    │  │
│  │  │  DEAD ZONE (追従なし)     │    │  │
│  │  │       cursor ●           │    │  │
│  │  └───────────────────────────┘    │  │
│  └───────────────────────────────────┘  │
└─────────────────────────────────────────┘
```

| ゾーン | 正規化距離 | デフォルト | 動作 |
|--------|-----------|-----------|------|
| Dead Zone | d < 0.3 | 0.3 | カメラ静止（テキスト入力時の安定性） |
| Soft Zone | 0.3 ≤ d < 0.7 | 0.7 | smoothstepで段階的に追従強度が増加 |
| Push Zone | 0.7 ≤ d < 1.0 | 1.0 | フル追従（カーソルがビューポート外に出ない） |
| 外側 | d ≥ 1.0 | - | 即座にリフレーム |

### 6.2 追従強度関数

```rust
fn follow_strength(d: f64, dead_zone: f64, soft_zone: f64) -> f64 {
    if d < dead_zone {
        0.0
    } else if d < soft_zone {
        let t = (d - dead_zone) / (soft_zone - dead_zone);
        t * t * (3.0 - 2.0 * t)  // smoothstep (C^1連続)
    } else {
        1.0
    }
}
```

### 6.3 フレーム毎の処理フロー

```
1. キーフレーム適用 (ZoomPlannerから)
2. カーソルの正規化オフセット計算
   dx = (cursor.x - viewport_center.x) / (vp_width / 2)
   dy = (cursor.y - viewport_center.y) / (vp_height / 2)
   d = sqrt(dx^2 + dy^2)
3. follow_strength(d) でカメラ追従量を決定
4. ビューポートターゲットを更新
5. スプリング物理でビューポートをアニメーション
6. 画面端クランプ
```

### 6.4 Ken方式との比較

Ken のマージンベースチェック（80%安全領域）はDead Zone + Push Zone の2層モデルに相当するが、
smoothstep による Soft Zone がないため境界をまたいだ瞬間に不連続な挙動が生じる。
Rio の3層モデルはsmoothstep 1つ追加するだけで解決でき、コスト増加はほぼゼロ。

---

## 7. 自動ズームのロジック

### 7.1 ズームイン条件

| トリガー | ズームレベル | 遷移 | 備考 |
|---------|-----------|------|------|
| 左クリック | 2.0x (default_zoom) | SpringIn | 操作の95%はクリックで始まる |
| テキスト入力 | rect-fitまたは2.5x | SpringIn | Click→Key パターンで検出 |
| スクロール | 1.3x | Smooth | スクロール中は少し寄る |
| ラピッドクリック (3+回) | 1.8x | SpringIn | ダブルクリック選択等 |
| ドラッグ (Phase 2) | 1.5x | Smooth | ドラッグ範囲に追従 |

### 7.2 ズームアウト条件（現在の最大の弱点の修正）

| トリガー | 条件 | 目標ズーム | 遷移 |
|---------|------|-----------|------|
| 段階的Idle | 2000-5000ms | 1.2x | SpringOut (slow) |
| 長Idle | 5000ms+ | 1.0x | SpringOut (slow) |
| 大距離移動 | カーソルがビューポート中心から30%以上離れた | 1.0x | SpringOut |
| 最大ズーム継続 | 同一ズームが5秒以上 | 1.0x | SpringOut |
| セグメント終了 | TextInput/Scroll の終了後 | 段階的に1.0x | SpringOut |

### 7.3 ズームヒステリシス (Phase 2)

ズーム振動を防ぐため、ズーム変更後800ms以内のズームアウトを抑制する。

```rust
if current_zoom > 1.0 && time_since_last_zoom_change < 800 {
    // ズームアウトを抑制
}
```

### 7.4 カット判定

画面の60%以上離れた位置への移動はカット（スプリングなし、即座に遷移）とする。
Phase 1 ではカット閾値を0.5→0.6に調整。
Phase 2 で「ブリッジパターン」（ズームアウト→パン→ズームイン）を検討。

### 7.5 ズーム抑制ルール

- 500ms以内に3回以上Clickがある場合: 1回目でズームイン、以降はビューポート追従のみ
- テキスト入力中: ズームレベル固定（入力中のズーム変更は最悪の体験）
- スクロール中: 浅めズーム維持

---

## 8. カーソルスムージング

### 8.1 Phase 1: スプリングベース（実タイムスタンプ使用）

現在の `dt = 1/60` 固定を実際のタイムスタンプ差に修正:

```rust
pub fn smooth(&mut self, raw_positions: &[(u64, f64, f64)]) -> Vec<(u64, f64, f64)> {
    let half_life = 0.05;  // 50ms — 微ジッター除去のみ
    self.spring_x.snap(raw_positions[0].1);
    self.spring_y.snap(raw_positions[0].2);

    for i in 0..raw_positions.len() {
        let dt = if i > 0 {
            (raw_positions[i].0 - raw_positions[i-1].0) as f64 / 1000.0
        } else {
            0.0
        };
        self.spring_x.target = raw_positions[i].1;
        self.spring_y.target = raw_positions[i].2;
        self.spring_x.update(half_life, dt);
        self.spring_y.update(half_life, dt);
        result.push((raw_positions[i].0, self.spring_x.position, self.spring_y.position));
    }
}
```

### 8.2 Phase 2: 双方向スムージング（Rio方式）

オフライン処理の利点を活用し、零位相フィルタを実装:

```
Pass 1 (forward): t=0→T でスプリング適用 → forward_positions[]
Pass 2 (reverse): t=T→0 でスプリング適用 → reverse_positions[]
Final: position[i] = (forward[i] + reverse[i]) / 2.0
```

位相遅延ゼロで、スプリングの滑らかさを維持。

### 8.3 ジッター除去（Preprocessor内）

スプリング適用前に変位ゲートを適用:

```
displacement < 2.0px AND velocity < 50px/s → 手の震え → 前の位置を保持
```

---

## 9. エフェクト一覧

### 9.1 クリックリング

**改善** (Phase 1): ease-out cubic イージングを適用（1行の変更）

```rust
fn eased_progress(linear: f64) -> f64 {
    1.0 - (1.0 - linear).powi(3)  // ease-out cubic
}
```

Phase 2: 二重リング（内リング300ms + 外リング500ms）、左右クリック色分け

### 9.2 キーバッジ

Phase 1: modifier修正により `Ctrl+C` 等が正しく表示される。
Phase 2: ab_glyph によるフォントレンダリング、スライドインアニメーション。

### 9.3 カーソル描画

Phase 1: 現状の三角形カーソルを維持（機能的に十分）。
Phase 2: プリレンダリングPNG画像、サイズ自動調整、1.5秒静止時フェードアウト。

### 9.4 角丸

**改善** (Phase 1): アンチエイリアス追加。サブピクセルalpha計算で数行の修正。

```rust
// 角の境界ピクセルで distance - radius の端数をalpha値に変換
let alpha = (1.0 - (dist - r).max(0.0).min(1.0)) * 255.0;
```

### 9.5 背景・シャドウ

Phase 1: 現状維持（グラデーション + リニアシャドウ）。
Phase 2: ガウシアンシャドウ近似、背景プリセット6種。
Phase 3: インセットシャドウ（画面内側の奥行き感）。

### 9.6 モーションブラー (Phase 3-4)

3対象: カーソルスミア → パンブラー → ズームラジアルブラー
品質設定: Off / Subtle / Cinematic

---

## 10. パフォーマンス最適化

### 10.1 Phase 1 最適化

| 項目 | 現状 | 改善 | 効果 |
|------|------|------|------|
| リサイズフィルタ | Lanczos3 | Triangle (バイリニア) | 2-3倍高速 |
| 背景+シャドウ | 毎フレーム計算 | 初回キャッシュ → clone | 大幅改善 |
| スプリング | O(N) サブステップ | O(1) 解析解 | 改善 |

### 10.2 Phase 2 最適化

- 角丸マスクのキャッシュ
- rayon による並列フレーム合成（スプリング状態を事前計算 → フレーム合成を並列化）

### 10.3 計算量

- 前処理（全イベント）: O(n)
- フレームあたり: O(W*H) — 画像リサンプリングが支配的
- スプリング更新: O(1) per spring
- メモリ: ~20MB working set（1フレーム入力 + 1フレーム出力）

---

## 11. 実装フェーズ

### Phase 1: 致命的バグ修正 + 基盤（1-2日）

| # | 項目 | ファイル | 提案元 |
|---|------|---------|--------|
| 1 | modifier状態追跡 | `events.rs` | Ken |
| 2 | ClickRelease追加 | `events.rs`, `config/mod.rs` | Rio |
| 3 | Preprocessor追加 | `preprocessor.rs` (新規) | Ken |
| 4 | Idle検出修正 + 段階的Idle | `analyzer.rs` | Ken + Yuki |
| 5 | 解析解スプリング + half-life | `spring.rs` | Rio |
| 6 | Dead Zone カーソル追従 | `compositor.rs` | Rio |
| 7 | ズームイン/アウト非対称スプリング | `compositor.rs` | Yuki |
| 8 | クリックリングイージング | `compositor.rs` | Yuki |
| 9 | 角丸AA | `compositor.rs` | Ken + Yuki |

### Phase 2: ズーム品質 + 視覚改善（1-2日）

| # | 項目 | ファイル |
|---|------|---------|
| 10 | ズームアウト条件強化 + ヒステリシス | `zoom_planner.rs` |
| 11 | 双方向カーソルスムージング | `cursor_smoother.rs` |
| 12 | Click→Key TextInput検出強化 | `analyzer.rs` |
| 13 | ドラッグ検出 + ズーム | `preprocessor.rs`, `zoom_planner.rs` |
| 14 | スプリングプリセットUI | `defaults.rs`, フロントエンド |
| 15 | 不足減衰スプリング（Quick/Rapid） | `spring.rs` |

### Phase 3: 視覚品質（追加1-2日）

| # | 項目 |
|---|------|
| 16 | クリックリング二重化 + 左右色分け |
| 17 | キーバッジフォントレンダリング |
| 18 | カーソル画像 + 自動非表示 |
| 19 | 背景プリセット + ガウシアンシャドウ |
| 20 | カーソルモーションブラー |

### Phase 4: 将来

| # | 項目 |
|---|------|
| 21 | Windows UI Automation Focus追跡 |
| 22 | ブリッジパターン（中距離ズームアウト→パン→ズームイン） |
| 23 | 画面モーションブラー |
| 24 | rayon並列フレーム処理 |
| 25 | インセットシャドウ |

---

## 12. 検証方法

1. `npx tauri build` でビルド成功確認
2. テスト録画（10秒程度）: クリック数回 → 文字入力 → スクロール → 別ウィンドウへ移動
3. エクスポートされた動画で以下を確認:
   - クリック時にズームインされるか
   - カーソルがビューポート外に出ないか（Dead Zone追従）
   - アイドル時にズームアウトするか（段階的）
   - ズームアウトがゆっくり、ズームインが速いか（非対称）
   - クリックリングが滑らかか（ease-out cubic）
   - 角丸にギザギザがないか（AA）
   - Ctrl+C 等のキーバッジが表示されるか（modifier修正）
4. スプリング物理が安定しているか（高tension値でも発散しない）

---

## 13. 設計の意思決定記録

### スプリング物理

- **Ken提案**: サブステッピング（シンプル、実績あり）
- **Yuki提案**: サブステッピング + プリセット
- **Rio提案**: 解析解 + half-life
- **結論**: Rio の解析解を採用。全員合意。理由: O(1)、dt非依存、無条件安定、コード量同等。

### ビューポート追従

- **Ken提案**: 10%マージンの矩形チェック
- **Yuki提案**: 15%マージンの矩形チェック
- **Rio提案**: Dead Zone / Soft Zone / Push Zone + smoothstep
- **結論**: Rio のモデルを採用。Ken/Yuki 合意。理由: smoothstep追加のコストはほぼゼロで、境界の不連続性を解消。

### mouse_move処理

- **Ken提案**: Preprocessor（エクスポート時に間引き、録画側は変更なし）
- **Yuki提案**: 録画側で間引き
- **Rio提案**: 録画側で変位ベース
- **結論**: Ken の Preprocessor 方式を採用。Rio 合意。理由: 生データ保持で再処理可能。

### Focus検出

- **Ken提案**: Phase 1 は Click→Key パターンで代替
- **Yuki提案**: Phase 1 で UI Automation
- **Rio提案**: Phase 5 で UI Automation
- **結論**: Ken の段階的アプローチ。Click→Key で80%カバー、UI Automationは Phase 4 で。

### モーションブラー

- **Ken**: MVP外
- **Yuki**: Phase 4 で3種類
- **Rio**: 慎重に Phase 4 以降
- **結論**: Phase 3 でカーソルのみ、Phase 4 で画面全体。Yuki案ベース。

---

## 14. 参考資料

- [Screen Studio Auto Zoom Guide](https://screen.studio/guide/auto-zoom)
- [Screen Studio Animations & Motion](https://screen.studio/guide/animations-motion)
- [Cap OSS (CapSoftware/Cap)](https://github.com/CapSoftware/Cap) — spring_mass_damper.rs, zoom_focus_interpolation.rs
- [Ryan Juckett "Damped Springs"](https://www.ryanjuckett.com/damped-springs/)
- [Daniel Holden "Spring-It-On"](https://theorangeduck.com/page/spring-roll-call) — half-life parameterization
- [Unity Cinemachine](https://docs.unity3d.com/Packages/com.unity.cinemachine@3.1/manual/CinemachinePositionComposer.html) — Dead Zone / Soft Zone model
- [Josh Comeau: Spring Physics](https://www.joshwcomeau.com/animation/a-friendly-introduction-to-spring-physics/)
- [Cursorful](https://cursorful.com) — zoom trigger thresholds
