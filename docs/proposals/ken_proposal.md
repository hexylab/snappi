# Snappi エフェクトエンジン設計提案 - Ken

> 「動くものが正義」
> 完璧な設計書より、確実に動く実装を。

## 0. 現状の問題と方針

現状のコードを読んだ。骨格はできている。パイプライン構成（analyzer → zoom_planner → cursor_smoother → compositor）は正しい方向だし、全部作り直す必要はない。**壊れている部分を直して、足りない部分を足す**。それだけでいい。

### 致命的な問題（今すぐ直すべき）

| # | 問題 | 原因 | 影響度 |
|---|------|------|--------|
| 1 | アイドル検出が機能しない | mouse_move が10ms毎に記録され、500ms閾値を超えるギャップが発生しない | ズームアウトが発動しない → ずっとズームインしっぱなし |
| 2 | Focusイベントが未実装 | `focus.rs` が空スタブ | TextInput セグメントが生成されない |
| 3 | スプリング物理が不安定 | 明示的オイラー法を使用 | 大きなdtでエネルギーが発散 |
| 4 | カーソルがビューポート外で迷子 | ビューポートはキーフレーム時のみ移動 | ズーム中にカーソルが画面外に出る |
| 5 | modifiersが常に空 | rdev の KeyPress で修飾キー状態を追跡していない | Ctrl+C 等のショートカットバッジが出ない |

### 方針

- OSSの実装（[Cap](https://github.com/CapSoftware/Cap)、[Screenize](https://github.com/syi0808/screenize)、[ScreenArc](https://github.com/tamnguyenvan/screenarc)、[screen-demo](https://github.com/njraladdin/screen-demo)）を参考にする
- [Screen Studio](https://screen.studio/guide/auto-zoom) のUXを目標にする
- 過剰な抽象化を避け、各モジュールは単一ファイルで完結させる
- パフォーマンスは後回しにしない（BMPフレーム保存は正解、Lanczos3スケーリングは要検討）

---

## 1. 全体アーキテクチャ

```
Recording Time                 Export Time
=============                  ===========

events.jsonl ──┐
               │
frames/   ─────┤
               ▼
         ┌─────────────┐
         │  Preprocessor │  ← mouse_move間引き、modifier追跡補正
         └──────┬──────┘
                ▼
         ┌─────────────┐
         │   Analyzer   │  ← セグメント分割（Click/TextInput/Scroll/Idle/Drag）
         └──────┬──────┘
                ▼
         ┌─────────────┐
         │ ZoomPlanner  │  ← ズームキーフレーム生成
         └──────┬──────┘
                ▼
         ┌─────────────┐
         │  Compositor  │  ← フレーム毎に：
         │              │     1. ビューポート更新（スプリング物理）
         │              │     2. カーソル追従チェック
         │              │     3. crop + scale
         │              │     4. エフェクト描画
         └──────┬──────┘
                ▼
         ┌─────────────┐
         │   Encoder    │  ← FFmpeg
         └─────────────┘
```

**変更点**: 既存パイプラインに `Preprocessor` ステージを追加し、CursorSmootherの責務をCompositor内に統合する。

### なぜCursorSmootherを独立モジュールから外すか

現在の CursorSmoother は事前に全マウス位置をバッチ処理している。しかし実際には、カーソルスムージングとビューポート追従は密結合した処理であり、同じフレームループ内で一緒に処理すべき。分離するとカーソルの「見た目の位置」とビューポートの間にズレが生じる。

---

## 2. イベント記録の改善

### 2.1 mouse_move のサンプリング改善

**現状**: 10ms毎（100Hz）にすべて記録。idle検出の500ms閾値を絶対に超えない。

**修正**: イベント記録側ではなく、**Preprocessor側で間引く**。

```rust
// preprocessor.rs

/// mouse_moveイベントを間引く
/// - 移動距離が閾値未満のイベントを除去
/// - ただしクリック/キー/スクロールの前後100msは保持
pub fn thin_mouse_moves(events: &[RecordingEvent], distance_threshold: f64) -> Vec<RecordingEvent> {
    let significant_times: HashSet<u64> = events.iter()
        .filter_map(|e| match e {
            RecordingEvent::Click { t, .. } |
            RecordingEvent::Key { t, .. } |
            RecordingEvent::Scroll { t, .. } => Some(*t),
            _ => None,
        })
        .flat_map(|t| (t.saturating_sub(100)..=t + 100))
        .collect();

    let mut result = Vec::new();
    let mut last_x = f64::NAN;
    let mut last_y = f64::NAN;

    for event in events {
        match event {
            RecordingEvent::MouseMove { t, x, y } => {
                let dist = ((x - last_x).powi(2) + (y - last_y).powi(2)).sqrt();
                if dist >= distance_threshold || significant_times.contains(t) {
                    result.push(event.clone());
                    last_x = *x;
                    last_y = *y;
                }
            }
            _ => {
                result.push(event.clone());
            }
        }
    }
    result
}
```

**理由**: 録画側を触ると全体に影響する。Preprocessorで処理すれば、生データは保持しつつ分析精度を上げられる。10ms毎の記録は「カーソルスムージング」には実は好都合なので、生データ自体は悪くない。

### 2.2 修飾キー状態の追跡

**現状**: `modifiers: vec![]` で常に空。

**修正**: rdev の KeyPress/KeyRelease で修飾キー状態をビットフラグ管理する。

```rust
// events.rs のリスナー内

let modifier_state = Arc::new(std::sync::Mutex::new(0u8));
// bit 0: Ctrl, bit 1: Shift, bit 2: Alt, bit 3: Meta(Win)

// KeyPress/KeyRelease で修飾キーフラグを更新
// 通常キー押下時に現在の修飾キーフラグから modifiers Vec を構築
```

これはシンプルで確実に動く。rdev は KeyRelease も送ってくるので、押下/解放の追跡は可能。

### 2.3 Focusイベントの実装

**現状**: `focus.rs` は空スタブ。

**方針**: Windows UI Automation の `IUIAutomation::AddFocusChangedEventHandler` を使う。

ただし注意点がある:
- UI Automation はCOMベースで、Rustから使うのは面倒（`windows` crateで可能だが冗長）
- **代替案**: フォーカス追跡の代わりに、「クリック後にキー入力が続いた」パターンを使う

**現実的な実装（Phase 1）**:

```rust
// analyzer.rs の analyze_events 内

// Click の直後（500ms以内）にKey イベントが続くパターンを TextInput として検出
// Focusイベントなしでも、これで80%のケースはカバーできる
RecordingEvent::Click { t, x, y, .. } => {
    // 既存のクリック処理の後に...
    // 後続のキーイベントをチェック
    let mut has_keys = false;
    let mut end_time = *t;
    for k in (i+1)..events.len() {
        match &events[k] {
            RecordingEvent::Key { t: kt, .. } if *kt - *t <= 500 || *kt - end_time <= 300 => {
                has_keys = true;
                end_time = *kt;
            }
            RecordingEvent::Click { .. } | RecordingEvent::Focus { .. } => break,
            _ => {}
        }
    }
    if has_keys {
        // TextInput セグメントを生成（クリック位置を中心にズーム）
    }
}
```

**Phase 2（後回し）**: UI Automation による正確なフォーカス追跡。`windows::UI::UIAutomation` の `IUIAutomation` を使えば、フォーカスされた要素の `BoundingRectangle` が取れる。実装コストが高いので、Phase 1で十分に機能することを確認してからやる。

---

## 3. 自動ズームのトリガーとロジック

### 3.1 ズームイン条件

Screen Studio や ScreenArc の挙動を参考にすると、ズームインのトリガーは実質 **クリック** のみ。他はおまけ。

| トリガー | ズーム倍率 | 遷移 | 備考 |
|---------|-----------|------|------|
| 左クリック | 2.0x | SpringIn | 最も重要。操作の95%はクリックで始まる |
| テキスト入力 | 入力領域にフィット（最大2.5x） | SpringIn | Click→Key パターンで検出 |
| スクロール | 1.3x（軽いズーム） | Smooth | スクロール中は少し寄る |
| ラピッドクリック（3+回/200ms） | 1.8x | SpringIn | ダブルクリック選択等 |
| ドラッグ | 1.5x（ドラッグ開始点） | Smooth | 新規追加。ドラッグ検出が必要 |

### 3.2 ズームアウト条件（最大の改善ポイント）

**現状の問題**: Idle セグメントに依存しているが、mouse_move が10ms毎なのでアイドルが検出されない。

**修正方針**: 複数の条件でズームアウトをトリガーする。

```rust
// zoom_planner.rs

enum ZoomOutTrigger {
    IdleTimeout,         // 操作なし（mouse_move除外で計測）
    LargeMovement,       // カーソルが画面の30%以上移動
    ContextSwitch,       // 異なるウィンドウへの移動（将来的にFocusイベントで）
    SegmentEnd,          // テキスト入力やスクロールの終了後
    MaxZoomDuration,     // 同じズームが5秒以上続いた場合
}
```

**具体的なロジック**:

```rust
fn should_zoom_out(
    current_zoom: f64,
    last_significant_event_ms: u64,
    now_ms: u64,
    cursor_x: f64,
    cursor_y: f64,
    viewport_center_x: f64,
    viewport_center_y: f64,
    screen_w: f64,
    screen_h: f64,
) -> bool {
    // 1. すでにズームアウト済み
    if current_zoom <= 1.05 {
        return false;
    }

    // 2. アイドルタイムアウト（mouse_move以外のイベントで計測）
    let idle_ms = now_ms - last_significant_event_ms;
    if idle_ms >= 1500 {
        return true;
    }

    // 3. カーソルが現在のビューポートから大きく離れた
    let dx = (cursor_x - viewport_center_x).abs() / screen_w;
    let dy = (cursor_y - viewport_center_y).abs() / screen_h;
    if (dx * dx + dy * dy).sqrt() > 0.3 {
        return true;
    }

    // 4. 同じズームが5秒以上続いた（長時間ズームは退屈）
    // → zoom_planner側で最後のキーフレームからの経過時間をチェック

    false
}
```

### 3.3 キーフレーム間のタイミング

- **最小間隔**: 300ms（現状と同じ。これより短いとバタつく）
- **ズームイン遷移**: 400ms（スプリングの `is_settled` で実質的に決まる）
- **ズームアウト遷移**: 600ms（ズームアウトはゆっくりのほうが自然）
- **カット判定**: 画面の50%以上離れた移動 → スプリングではなくカット遷移

---

## 4. カーソル追従とビューポート管理

### 4.1 問題: カーソルがビューポート外に出る

**現状**: ビューポートはキーフレーム適用時のみ移動する。キーフレーム間ではスプリング物理が惰性で動くだけ。カーソルが別の場所に移動してもビューポートは追従しない。

**修正**: **フレーム毎にカーソル位置をチェックし、ビューポートがカーソルを追従する**。

```rust
// compositor.rs の compose_frame 内

// カーソルがビューポートの「安全領域」（中央80%）の外に出たら追従
fn check_cursor_follow(
    viewport: &mut AnimatedViewport,
    cursor_x: f64,
    cursor_y: f64,
    screen_w: f64,
    screen_h: f64,
) {
    let vp = viewport.current_viewport(screen_w, screen_h);
    let margin_x = vp.width * 0.1;  // 左右10%マージン
    let margin_y = vp.height * 0.1;  // 上下10%マージン

    let safe_left = vp.x + margin_x;
    let safe_right = vp.x + vp.width - margin_x;
    let safe_top = vp.y + margin_y;
    let safe_bottom = vp.y + vp.height - margin_y;

    let mut need_follow = false;
    let mut new_x = viewport.center_x.target;
    let mut new_y = viewport.center_y.target;

    if cursor_x < safe_left {
        new_x = cursor_x + vp.width / 2.0 - margin_x;
        need_follow = true;
    } else if cursor_x > safe_right {
        new_x = cursor_x - vp.width / 2.0 + margin_x;
        need_follow = true;
    }

    if cursor_y < safe_top {
        new_y = cursor_y + vp.height / 2.0 - margin_y;
        need_follow = true;
    } else if cursor_y > safe_bottom {
        new_y = cursor_y - vp.height / 2.0 + margin_y;
        need_follow = true;
    }

    if need_follow {
        // スクリーン境界にクランプ
        let half_w = vp.width / 2.0;
        let half_h = vp.height / 2.0;
        new_x = new_x.clamp(half_w, screen_w - half_w);
        new_y = new_y.clamp(half_h, screen_h - half_h);

        viewport.center_x.set_target(new_x);
        viewport.center_y.set_target(new_y);
    }
}
```

**ポイント**: これだけで「カーソルがビューポートから消える」問題の90%は解決する。スプリング物理がビューポートの動きを滑らかにしてくれるので、カクつかない。

### 4.2 カーソルスムージング

現在の CursorSmoother は全ポジションをバッチ処理しているが、ビューポート追従と統合するため **フレームループ内でインクリメンタルに処理する**。

```rust
// compositor.rs 内のスプリングベース・カーソルスムーザー

struct InlineCursorSmoother {
    spring_x: SpringAnimation,
    spring_y: SpringAnimation,
}

impl InlineCursorSmoother {
    fn update(&mut self, raw_x: f64, raw_y: f64, dt: f64) -> (f64, f64) {
        self.spring_x.set_target(raw_x);
        self.spring_y.set_target(raw_y);
        (self.spring_x.update(dt), self.spring_y.update(dt))
    }
}
```

カーソルスムージングはスプリング物理で十分。Catmull-Rom スプラインは見た目は良いが、「未来のポイント」が必要で実装が複雑になる。スプリングならtargetを設定してupdateするだけ。

---

## 5. スプリング物理の改善

### 5.1 現状の問題

```rust
// 現在の spring.rs（明示的オイラー法）
self.velocity += acceleration * dt;
self.position += self.velocity * dt;
```

これは**明示的オイラー法**。dt が大きい（30fps = 33ms）とエネルギーが発散する。

### 5.2 修正: 半陰的オイラー法（Symplectic Euler）

```rust
pub fn update(&mut self, dt: f64) -> f64 {
    let displacement = self.position - self.target;
    let spring_force = -self.tension * displacement;
    let damping_force = -self.friction * self.velocity;
    let acceleration = (spring_force + damping_force) / self.mass;

    // 半陰的オイラー: velocity を先に更新し、新しい velocity で position を更新
    self.velocity += acceleration * dt;
    self.position += self.velocity * dt;  // ← 更新済み velocity を使う

    self.position
}
```

**実は現在のコードもすでに半陰的オイラー法になっている**（velocity を先に更新し、その velocity で position を更新）。しかし安定性の問題がある場合は、dtが大きすぎる可能性がある。

**本当の修正**: dtをサブステップに分割する。

```rust
pub fn update(&mut self, dt: f64) -> f64 {
    // dtが大きい場合はサブステップに分割
    let max_step = 1.0 / 120.0;  // 最大8.3ms
    let mut remaining = dt;

    while remaining > 0.0 {
        let step = remaining.min(max_step);
        let displacement = self.position - self.target;
        let spring_force = -self.tension * displacement;
        let damping_force = -self.friction * self.velocity;
        let acceleration = (spring_force + damping_force) / self.mass;

        self.velocity += acceleration * step;
        self.position += self.velocity * step;
        remaining -= step;
    }

    self.position
}
```

**サブステップは最もシンプルで確実な安定化手法**。ゲーム業界でも広く使われている手法で、react-spring も内部で似たことをやっている。Velocity Verlet やRK4 は精度は高いが、この用途ではオーバーキル。

### 5.3 スプリングパラメータのプリセット

```rust
pub struct SpringPreset {
    pub tension: f64,
    pub friction: f64,
    pub mass: f64,
}

impl SpringPreset {
    /// ズーム遷移用（やや硬め、素早く安定）
    pub fn zoom() -> Self {
        Self { tension: 170.0, friction: 26.0, mass: 1.0 }
    }

    /// カーソルスムージング用（柔らかめ、追従性重視）
    pub fn cursor() -> Self {
        Self { tension: 300.0, friction: 30.0, mass: 1.0 }
    }

    /// ビューポート追従用（カーソルよりやや遅い）
    pub fn viewport_follow() -> Self {
        Self { tension: 120.0, friction: 20.0, mass: 1.0 }
    }

    /// ズームアウト用（ゆったり）
    pub fn zoom_out() -> Self {
        Self { tension: 100.0, friction: 22.0, mass: 1.0 }
    }
}
```

パラメータは後から調整する。まずはこの値で動かしてみて、目視で確認して微調整するのが一番速い。

---

## 6. ズームアウト改善の詳細

### 6.1 Idle 検出の修正

**根本原因**: analyzer.rs の idle 検出が全イベント（mouse_move含む）の時間ギャップに依存している。

**修正**: mouse_move を除外した「significant events」のみで idle を判定する。

```rust
fn analyze_events(events: &[RecordingEvent]) -> Vec<Segment> {
    // ...

    // mouse_move 以外のイベントのみでアイドル判定
    let significant_events: Vec<&RecordingEvent> = events.iter()
        .filter(|e| !matches!(e, RecordingEvent::MouseMove { .. }))
        .collect();

    // significant_events 間のギャップでアイドル検出
    for window in significant_events.windows(2) {
        let t1 = event_timestamp(window[0]);
        let t2 = event_timestamp(window[1]);
        if t2 - t1 >= IDLE_THRESHOLD_MS {
            segments.push(Segment {
                segment_type: SegmentType::Idle,
                start_ms: t1,
                end_ms: t2,
                focus_point: None,
            });
        }
    }

    // ...
}
```

### 6.2 zoom_planner でのズームアウト生成

```rust
// 全セグメントを処理した後、暗黙のズームアウトを追加
fn add_implicit_zoom_outs(plan: &mut Vec<ZoomKeyframe>, screen_w: f64, screen_h: f64) {
    let max_zoom_duration_ms = 5000;
    let mut additions = Vec::new();

    for i in 0..plan.len() {
        let current = &plan[i];
        let next_time = if i + 1 < plan.len() {
            plan[i + 1].time_ms
        } else {
            current.time_ms + max_zoom_duration_ms + 1  // 最後のキーフレームの後
        };

        // ズームインが長時間続いていたらズームアウトを挿入
        if current.zoom_level > 1.2 && next_time - current.time_ms > max_zoom_duration_ms {
            additions.push(ZoomKeyframe {
                time_ms: current.time_ms + max_zoom_duration_ms,
                target_x: screen_w / 2.0,
                target_y: screen_h / 2.0,
                zoom_level: 1.0,
                transition: TransitionType::SpringOut,
            });
        }
    }

    plan.extend(additions);
    plan.sort_by_key(|kf| kf.time_ms);
}
```

---

## 7. エフェクト一覧

### 7.1 クリックリング（既存・改善）

**現状**: 動作している。線形アニメーション。

**改善**:
- 線形 `progress` ではなく、イージング関数を適用
- フェードアウトをスムーズにする

```rust
fn eased_progress(linear: f64) -> f64 {
    // ease-out cubic
    1.0 - (1.0 - linear).powi(3)
}
```

### 7.2 キーバッジ（既存・改善）

**現状**: 修飾キー+特殊キーのみ表示。modifiersが常に空なので実質特殊キーのみ。

**改善**:
1. modifiers の修正（上述）で Ctrl+C 等が表示される
2. 連続したキー入力（1秒以内）はバッジを更新するだけ（新しいバッジを生成しない）
3. フェードイン/フェードアウトアニメーション

### 7.3 カーソルカスタマイズ（既存・改善）

**現状**: シンプルな三角形カーソル。

**改善（Phase 2）**:
- macOS風のカーソル画像を使用（PNGアセット）
- 現時点の三角形でも十分機能する。見た目の改善は優先度低

### 7.4 ドラッグ検出（新規）

**現状**: ドラッグイベントなし。

**実装**:
```rust
// preprocessor.rs

// Click(ButtonPress) → MouseMove(距離>閾値) → ButtonRelease パターンでドラッグを検出
// events.jsonl には ButtonRelease が記録されないため、
// 「Click後にmouse_moveが一定距離以上続く」パターンで近似する

pub fn detect_drags(events: &[RecordingEvent]) -> Vec<DragEvent> {
    let mut drags = Vec::new();
    for (i, event) in events.iter().enumerate() {
        if let RecordingEvent::Click { t, x, y, btn } = event {
            if btn == "left" {
                // 次のClickまでの間にマウスが50px以上移動していたらドラッグ
                let mut max_dist = 0.0f64;
                let mut end_time = *t;
                let mut end_x = *x;
                let mut end_y = *y;
                for j in (i+1)..events.len() {
                    match &events[j] {
                        RecordingEvent::MouseMove { t: mt, x: mx, y: my } => {
                            let dist = ((mx - x).powi(2) + (my - y).powi(2)).sqrt();
                            if dist > max_dist {
                                max_dist = dist;
                                end_time = *mt;
                                end_x = *mx;
                                end_y = *my;
                            }
                        }
                        RecordingEvent::Click { .. } => break,
                        _ => {}
                    }
                }
                if max_dist > 50.0 {
                    drags.push(DragEvent {
                        start_ms: *t,
                        end_ms: end_time,
                        start_x: *x, start_y: *y,
                        end_x, end_y,
                    });
                }
            }
        }
    }
    drags
}
```

### 7.5 ドラッグ中のズームアウト（新規）

ドラッグ操作中（ウィンドウのリサイズ、テキスト選択等）はズームアウトして全体を見せる。

### 7.6 背景・角丸・シャドウ（既存）

現状の実装で問題なし。そのまま使う。

---

## 8. パフォーマンス考慮

### 8.1 現状のボトルネック

1. **Lanczos3 リサイズ**: `crop_and_scale` で毎フレーム Lanczos3 を使用。これは重い。
2. **ピクセル単位のループ**: `draw_drop_shadow`、`apply_rounded_corners` がすべてピクセル単位のネストループ。
3. **BMP保存**: PNG比で10倍高速だが、ディスクI/Oはまだボトルネック。

### 8.2 改善策

| 項目 | 現状 | 改善 | 効果 |
|------|------|------|------|
| リサイズフィルタ | Lanczos3 | `Triangle`（バイリニア） | 2-3倍高速。ズーム後のスケーリングなので画質差はほぼ目立たない |
| シャドウ | 毎フレーム計算 | 背景+シャドウをキャッシュ（ズーム変更時のみ再計算） | 大幅改善 |
| 角丸 | 毎フレーム計算 | 角丸マスクをキャッシュ | 改善 |
| BMP保存 | 非圧縮BMP | そのまま（十分速い） | - |
| フレーム処理 | シングルスレッド | `rayon::par_iter` で並列化 | 2-4倍高速（Phase 2） |

**Phase 1**: リサイズフィルタ変更 + 背景キャッシュ。これだけで体感速度はかなり改善する。

**Phase 2**: rayon による並列フレーム処理。ただし compositor の状態（ビューポートのスプリング物理）が前フレームに依存するため、単純な並列化はできない。

```
// 並列化の戦略（Phase 2）
// 1. zoom_planner でキーフレームを生成
// 2. 各フレームのビューポート状態を事前計算（シーケンシャル）
// 3. フレーム合成（crop + effect描画）を並列化（各フレームは独立）
```

---

## 9. 実装計画

### Phase 1: 「動くものを直す」（1-2日）

1. **modifier追跡の修正** (`events.rs`)
   - ビットフラグでCtrl/Shift/Alt/Win状態を管理
   - 工数: 小

2. **Preprocessor追加** (`preprocessor.rs`)
   - mouse_move間引き
   - ドラッグ検出
   - 工数: 小

3. **Idle検出修正** (`analyzer.rs`)
   - mouse_moveを除外したsignificant eventsでアイドル判定
   - Click→Key パターンによるTextInput検出
   - 工数: 小

4. **スプリング物理のサブステップ化** (`spring.rs`)
   - `update()` にサブステップ分割を追加
   - 工数: 最小（数行の変更）

5. **カーソル追従の追加** (`compositor.rs`)
   - `compose_frame()` 内でカーソル位置チェック→ビューポート追従
   - 工数: 中

6. **ズームアウト条件の強化** (`zoom_planner.rs`)
   - ラージムーブメント検出
   - 最大ズーム継続時間
   - 工数: 小

### Phase 2: 「見た目を良くする」（追加1-2日）

7. クリックリングのイージング
8. キーバッジのフェードアニメーション
9. リサイズフィルタの変更（Lanczos3 → Triangle）
10. 背景キャッシュ

### Phase 3: 「磨く」（将来）

11. UI Automation によるフォーカス追跡
12. rayon による並列フレーム処理
13. カーソル画像のカスタマイズ
14. ドラッグ中のズームアウトアニメーション

---

## 10. まとめ

この提案の核心は3つ:

1. **mouse_moveを除外してアイドル検出する** → ズームアウトが正常に動く
2. **フレーム毎にカーソル追従チェック** → カーソルがビューポートから消えない
3. **スプリング物理をサブステップ化** → 安定したアニメーション

この3つを直すだけで、現状の「ズームインしたまま戻らない、カーソルが消える」問題は解決する。残りは磨きの問題。

**全部作り直す必要はない**。今あるコードの骨格は正しい。壊れている部分だけを的確に直す。それが一番速くて確実な道だ。
