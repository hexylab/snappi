# Snappi エフェクトエンジン設計提案

**Author:** Yuki (Design Engineer)
**Date:** 2026-02-07
**Status:** Draft

---

## 0. 設計思想: なぜこの提案が必要なのか

ユーザーがデスクトップ操作を録画して「停止」を押す。そこから先は、Snappiの仕事だ。
出来上がった動画を見た人が「この操作、よくわかった」と感じること。それが唯一の成功基準。

Screen Studioが実現した「録画するだけで映画的な動画ができる」という体験。
それをWindowsでも実現する。しかも、妥協なく。

現在のSnappiには以下の根本的な課題がある:

1. **イベント記録の欠陥**: Focusイベントが記録されず、mouse_moveが10ms毎で溢れ、modifiersが常に空
2. **ズームロジックの未熟さ**: ズームアウト条件が不十分、カーソル追従なし、キーフレーム適用時のみ移動
3. **物理エンジンの不安定性**: オイラー法によるスプリング物理が発散する可能性
4. **視聴体験の不足**: モーションブラーなし、カーソルの視覚品質が低い、ビューポート管理が受動的

この提案は、これらすべてを根本から設計し直す。

---

## 1. 全体アーキテクチャ

### 1.1 パイプライン構成

```
録画フェーズ                          エクスポートフェーズ
============                          ==================

[Screen Capture]                      [Event Loader]
      |                                     |
[Event Recorder] ──→ events.jsonl ──→ [Event Preprocessor]
  - mouse_move (サンプリング済み)            |
  - click                              [Semantic Analyzer]
  - key (modifiers付き)                     |
  - scroll                            [Zoom Planner]
  - focus (UI Automation)                   |
  - window_switch                      [Timeline Builder]
                                            |
                                       [Frame Compositor]
                                        ├── Viewport (Spring)
                                        ├── Cursor Renderer
                                        ├── Click Effect
                                        ├── Key Badge
                                        ├── Motion Blur
                                        └── Background + Shadow
                                            |
                                       [FFmpeg Encoder]
```

### 1.2 新しいモジュール構成

```
src-tauri/src/engine/
  mod.rs
  pipeline.rs           -- パイプライン全体のオーケストレーション
  event_preprocessor.rs -- イベント前処理（ノイズ除去、補間、意味付け）
  semantic_analyzer.rs  -- セマンティック分析（旧analyzer.rsの進化版）
  zoom_planner.rs       -- ズーム計画（大幅改修）
  timeline.rs           -- タイムライン管理（キーフレーム補間）
  viewport_tracker.rs   -- ビューポート追従（カーソル追従含む）
  spring.rs             -- スプリング物理（Semi-implicit Euler法）
  cursor_smoother.rs    -- カーソルスムージング（改修）
  compositor.rs         -- フレーム合成（改修）
  motion_blur.rs        -- モーションブラー
  effects/
    mod.rs
    background.rs
    click_ring.rs
    cursor.rs
    key_badge.rs
    viewport.rs
    inset_shadow.rs     -- NEW: 画面内側の影（奥行き感）
```

---

## 2. イベント記録の改善案

### 2.1 現状の問題

| 問題 | 原因 | 影響 |
|------|------|------|
| Focusイベント未記録 | `focus.rs`がスタブのまま | テキスト入力検出が完全に機能しない |
| mouse_move過多 | 10msサンプリング | アイドル検出(500ms閾値)がほぼ発動しない |
| modifiers常に空 | rdevのKeyPressでmodifier状態を追跡していない | キーバッジが表示されない |

### 2.2 改善設計

#### 2.2.1 mouse_moveサンプリング戦略

**現状**: 10ms毎に記録（100Hz） → 1分で6000イベント
**提案**: 距離ベースサンプリング + 時間ベース間引き

```
方針:
- 最小間隔: 33ms（30Hz相当）
- ただし移動距離が閾値（5px）未満の場合はスキップ
- 停止検出: 直前イベントから200ms以上経過 + 移動なし → 1回だけ「停止位置」を記録
- これにより:
  - 高速移動時: 30Hzで十分な軌跡データ
  - 微動・静止時: ほぼ記録しない
  - アイドル検出が正しく機能する
```

#### 2.2.2 Focusイベントの実装

Windows UI Automationを使った実装:

```
方針:
- IUIAutomation::AddFocusChangedEventHandler で
  フォーカス変更を監視
- 記録するデータ:
  {
    type: "focus",
    t: u64,
    el: String,         // "Edit", "TextBox", "ComboBox" 等
    name: String,       // コントロール名
    rect: [f64; 4],     // バウンディングボックス [x, y, right, bottom]
    pid: u32,           // プロセスID（ウィンドウ識別用）
    window_title: String // ウィンドウタイトル
  }
- テキスト入力コントロール（Edit, TextBox, RichEdit等）への
  フォーカスを特に重要視
```

#### 2.2.3 Modifier状態追跡

```
方針:
- グローバルな modifier_state: HashSet<String> を管理
- KeyPress時: modifier キーなら state に追加
- KeyRelease時: modifier キーなら state から除去
- 非modifier キーのKeyPress時: 現在の modifier_state をコピーして記録
- 対象modifier: Control, Shift, Alt, Meta(Win)
```

#### 2.2.4 新しいイベントタイプの追加

```rust
// 新規追加
WindowSwitch {
    t: u64,
    from_pid: u32,
    to_pid: u32,
    window_title: String,
    rect: [f64; 4],
}

// 既存のFocusを拡張
Focus {
    t: u64,
    el: String,
    name: String,
    rect: [f64; 4],
    pid: u32,
    window_title: String,
}
```

---

## 3. セマンティック分析（Semantic Analyzer）

### 3.1 現状のanalyzerの問題

現在の`analyzer.rs`は以下の問題を抱えている:

1. **mouse_moveの洪水**: 10ms毎のmouse_moveイベントが大量にあり、イベント間の時間差が常に短いためアイドル検出の`IDLE_THRESHOLD_MS: 500`がほぼ発動しない
2. **テキスト入力検出の前提条件不成立**: Focus→Key の流れを前提としているが、Focusが記録されない
3. **セグメント粒度が粗い**: Click, TextInput, Scroll, Idle, RapidAction の5種類のみ

### 3.2 新しいセグメントタイプ

```rust
pub enum SegmentType {
    // ユーザーの意図を表現するセグメント
    Click,              // 単発クリック（UIボタン押下等）
    DoubleClick,        // ダブルクリック（ファイル開く等）
    DragOperation,      // ドラッグ操作（範囲選択、ウィンドウ移動等）
    TextInput,          // テキスト入力（フォーカスされたフィールドへの入力）
    KeyboardShortcut,   // キーボードショートカット（Ctrl+C等）
    Scroll,             // スクロール操作
    WindowSwitch,       // ウィンドウ切り替え
    Navigation,         // 画面内ナビゲーション（メニュー操作等）
    Idle,               // 操作なし（考え中、読んでいる等）
    RapidAction,        // 高速連続操作（連打等）
}
```

### 3.3 イベント前処理パイプライン

```
Raw Events
    |
[1. Mouse Move Decimation]  -- 近接イベント除去、停止検出
    |
[2. Click Classification]   -- single/double/drag 判定
    |
[3. Key Sequence Analysis]  -- modifier追跡、ショートカット検出
    |
[4. Focus Correlation]      -- Focus + Key → TextInput判定
    |
[5. Idle Detection]         -- mouse_move除去後の真のアイドル検出
    |
[6. Segment Merging]        -- 短すぎるセグメントの統合
    |
Semantic Segments
```

### 3.4 各前処理ステップの詳細

#### Step 1: Mouse Move Decimation

```
入力: 全イベント列
処理:
  - mouse_move以外はそのまま通過
  - mouse_moveは以下の条件でフィルタ:
    - 直前のmouse_moveから33ms未満 → スキップ
    - 直前のmouse_moveからの移動距離が3px未満 → スキップ
    - ただし、直前のmouse_moveから200ms以上経過したら必ず記録（停止位置）
出力: 間引き済みイベント列
```

#### Step 2: Click Classification

```
入力: 間引き済みイベント列
処理:
  - Click → 300ms以内に次のClick → DoubleClick に統合
  - Click → mouse_move（距離>20px）→ ButtonRelease → DragOperation
  - 200ms以内に3回以上Click → RapidAction
出力: クリック分類済みイベント列
```

#### Step 3: Idle Detection（mouse_move除去後）

```
入力: 分類済みイベント列
処理:
  - 「意味のあるイベント」のみの時間差を計算
    意味のあるイベント = Click, Key, Scroll, Focus, WindowSwitch
  - 時間差 > 800ms → Idle セグメント挿入
    （従来の500msから引き上げ。人間の操作間隔を考慮）
  - Idle の長さに応じたサブ分類:
    - 800ms-2000ms: 短い思考時間（ズーム維持）
    - 2000ms-5000ms: 中程度の間（ゆっくりズームアウト）
    - 5000ms+: 長い間（完全にズームアウト）
```

---

## 4. 自動ズーム設計

### 4.1 設計原則

Screen Studioの研究から導き出した原則:

> **「ズームは視聴者の注意を導く"カメラワーク"である」**

映画のカメラワーク理論を適用する:
- **ドリーイン（ズームイン）**: 「ここを見て」という意図。詳細を見せたい時
- **ドリーアウト（ズームアウト）**: 「全体を見て」という意図。文脈を示す時
- **パン（横移動）**: 「次はこっち」という意図。注意の移動
- **カット**: 「場面転換」という意図。大きく位置が変わる時

### 4.2 ズームトリガー条件

#### いつズームインするか

| トリガー | ズームレベル | 理由 |
|----------|-------------|------|
| Click（単発） | 2.0x | クリック先のUI要素を見せる |
| DoubleClick | 2.2x | ダブルクリックはより意図的な操作 |
| TextInput開始 | 2.5x（rect-fit） | 入力フィールドを画面いっぱいに |
| KeyboardShortcut | 1.5x（現位置維持） | ショートカットは画面変化が主。浅めに |
| Scroll | 1.3x（カーソル位置） | スクロール内容を見せる |
| RapidAction | 1.8x | 連続操作エリアに集中 |
| DragOperation | 1.5x→操作範囲fit | ドラッグ範囲を追従 |

#### いつズームアウトするか

**これが現在の最大の弱点**。現在のコードではIdleセグメントの`idle_timeout_ms`でのみズームアウトする。

新しいズームアウト条件:

```
1. 長いIdle（>2000ms）:
   → 段階的にズームアウト（急に引くのではなく2秒かけて1.0xへ）

2. WindowSwitch:
   → 即座にズームアウト → 新ウィンドウ位置へパン → 必要ならズームイン
   （Screen Studioの「カット」に相当）

3. 大距離移動（画面の40%以上）:
   → 一旦ズームアウト → パン → ズームイン
   （直接パンすると動きが速すぎて視聴者がついてこれない）

4. Scroll中のズーム:
   → Scroll開始時に少しズームアウト（1.3x）
   → Scroll終了後500ms で次のアクションに応じてズーム

5. 操作の意味が変わった時:
   → 例: TextInput → Click（別の場所） → 現在のズームを解除して新位置へ
```

### 4.3 ズームレベル計算

#### rect-fit ズーム（テキスト入力等）

```
zoom = min(screen_width / (rect_width * 1.4),
           screen_height / (rect_height * 1.4))
zoom = clamp(zoom, 1.2, max_zoom)
```

- 1.4倍のパディングは「入力フィールドの周囲のUIコンテキストも見せる」ため
- 最低1.2xは保証（ズームしてるのがわからないレベルは意味がない）

#### 距離ベースのズーム調整

```
直前のズーム位置からの距離が大きいほど、一旦浅いズームにしてから深くする:

distance = sqrt((new_x - old_x)^2 + (new_y - old_y)^2)
screen_diagonal = sqrt(screen_w^2 + screen_h^2)
normalized_distance = distance / screen_diagonal

if normalized_distance > 0.3:
    // 遠い場所へ → 一旦ズームアウトしてからズームイン
    insert zoom_out keyframe (t - 200ms, zoom: 1.0)
    insert zoom_in keyframe  (t + 100ms, target zoom)
```

### 4.4 ズームキーフレーム生成アルゴリズム

```
for each segment:
    match segment.type:
        Click | DoubleClick:
            if distance_from_current > 0.3 * screen_diagonal:
                // 大距離移動: ズームアウト → パン → ズームイン
                push(ZoomOut at t-200ms)
                push(ZoomIn at t+100ms, target=click_pos, zoom=segment_zoom)
            else:
                // 近距離: 直接パン+ズーム
                push(ZoomIn at t, target=click_pos, zoom=segment_zoom)

        TextInput:
            push(ZoomIn at t, target=rect_center, zoom=rect_fit_zoom)
            // 入力中はズーム維持（end_msまで追加キーフレームなし）

        Idle(duration):
            if duration > 5000ms:
                push(ZoomOut at t+500ms, zoom=1.0)  // 完全ズームアウト
            elif duration > 2000ms:
                push(ZoomOut at t+500ms, zoom=1.2)  // 少しだけ引く
            // 短いIdleはズーム維持

        WindowSwitch:
            push(Cut at t, target=new_window_center, zoom=1.0)
            // ウィンドウ切替はカットが自然

        Scroll:
            push(Smooth at t, zoom=max(current_zoom, 1.3))
            // スクロール中は浅めズーム維持
```

---

## 5. ビューポート管理とカーソル追従

### 5.1 現状の問題

現在の`compositor.rs`は**キーフレーム適用時にのみ**ビューポートのターゲットを更新する。
つまり、ズームイン中にカーソルがビューポート外に移動しても追従しない。

### 5.2 新しいビューポート管理: Continuous Cursor Tracking

```
フレーム毎の処理:

1. キーフレーム適用（既存）
2. カーソル追従チェック（NEW）:
   if zoom > 1.0 and cursor_visible:
       viewport_rect = current_viewport()
       // カーソルがビューポート内のどこにいるか
       margin = viewport_rect.size * 0.15  // 15%のマージン

       if cursor.x < viewport_rect.left + margin:
           // カーソルが左端に近い → ビューポートを左に追従
           new_center_x = cursor.x + viewport_rect.width/2 - margin
       elif cursor.x > viewport_rect.right - margin:
           // カーソルが右端に近い → ビューポートを右に追従
           new_center_x = cursor.x - viewport_rect.width/2 + margin

       // Y軸も同様

       viewport.set_target(new_center_x, new_center_y, current_zoom)

3. ビューポートのスプリング更新
4. ビューポートのクランプ（画面外に出ないように）
```

### 5.3 追従の速度制御

カーソル追従は、キーフレームのズーム遷移とは**異なるスプリングパラメータ**を使う:

```
ズーム遷移: tension=170, friction=26  // しっかりとした動き
カーソル追従: tension=120, friction=20  // 柔らかい追従（遅れ感が自然）
```

これにより:
- ズームイン/アウトは「カメラが動いている」感覚
- カーソル追従は「カメラがゆるやかにカーソルを追いかけている」感覚

Screen Studioのあの「気持ちいい遅れ感」はここで生まれる。

---

## 6. スプリング物理の設計

### 6.1 現状の問題

現在の`spring.rs`は**オイラー法（Euler method）**を使用:

```rust
// 現在のコード (spring.rs:25-33)
self.velocity += acceleration * dt;
self.position += self.velocity * dt;
```

オイラー法の問題:
- dtが大きいとエネルギーが増加し発散する
- dtが小さくても数値的に不安定になり得る
- 特にstiffness（tension）が高い時に問題

### 6.2 Semi-implicit Euler法への移行

```rust
// 改善案: Semi-implicit Euler (Symplectic Euler)
// velocity を先に更新し、更新後の velocity で position を更新
pub fn update(&mut self, dt: f64) -> f64 {
    let displacement = self.position - self.target;
    let spring_force = -self.tension * displacement;
    let damping_force = -self.friction * self.velocity;
    let acceleration = (spring_force + damping_force) / self.mass;

    // Key difference: velocity first, then position with NEW velocity
    self.velocity += acceleration * dt;
    self.position += self.velocity * dt;  // ← uses updated velocity

    self.position
}
```

実は現在のコードも計算順序は同じだが、**サブステッピング**を追加することでさらに安定化する:

```rust
pub fn update(&mut self, dt: f64) -> f64 {
    // サブステッピング: dtを小さなステップに分割
    let sub_steps = ((dt * self.tension.sqrt()).ceil() as usize).max(1).min(8);
    let sub_dt = dt / sub_steps as f64;

    for _ in 0..sub_steps {
        let displacement = self.position - self.target;
        let spring_force = -self.tension * displacement;
        let damping_force = -self.friction * self.velocity;
        let acceleration = (spring_force + damping_force) / self.mass;

        self.velocity += acceleration * sub_dt;
        self.position += self.velocity * sub_dt;
    }

    self.position
}
```

### 6.3 スプリングプリセット

Screen Studioの4段階（Slow, Mellow, Quick, Rapid）に相当するプリセット:

```rust
pub enum SpringPreset {
    Slow,     // ゆっくり、優雅。プレゼンテーション向け
    Mellow,   // 落ち着いた動き。チュートリアル向け（デフォルト）
    Quick,    // キビキビ。テック系デモ向け
    Rapid,    // 素早い。上級者向けコンテンツ
}

impl SpringPreset {
    pub fn params(&self) -> (f64, f64, f64) {
        // (tension, friction, mass)
        match self {
            // 臨界減衰に近い（オーバーシュートなし）
            SpringPreset::Slow    => (80.0,  22.0, 1.0),
            // わずかにアンダーダンプ（微小なオーバーシュート）
            SpringPreset::Mellow  => (170.0, 26.0, 1.0),
            // しっかりアンダーダンプ（見える程度のオーバーシュート）
            SpringPreset::Quick   => (280.0, 24.0, 1.0),
            // 高スティフネス（スナッピーだが制御された動き）
            SpringPreset::Rapid   => (400.0, 30.0, 1.0),
        }
    }

    /// 臨界減衰比 (damping ratio)
    /// < 1.0: アンダーダンプ（オーバーシュートあり）
    /// = 1.0: 臨界減衰（最速で収束、オーバーシュートなし）
    /// > 1.0: オーバーダンプ（ゆっくり収束）
    pub fn damping_ratio(&self) -> f64 {
        let (tension, friction, mass) = self.params();
        friction / (2.0 * (tension * mass).sqrt())
    }
}
```

各プリセットの減衰比:
- **Slow**: 0.78 (やや臨界減衰寄り、ほぼオーバーシュートなし)
- **Mellow**: 1.0 (臨界減衰、オーバーシュートなし) ← デフォルト
- **Quick**: 0.72 (アンダーダンプ、軽いバウンス)
- **Rapid**: 0.75 (高速だが制御された動き)

### 6.4 コンテキスト別のスプリングパラメータ

```
ビューポートズーム:   preset設定に従う
ビューポートパン:     zoom用のtensionの0.7倍、frictionの0.8倍
カーソルスムージング: tension=120, friction=18（柔らかく遅れる）
クリックリング:       tension=300, friction=25（素早く広がる）
ズームアウト:         zoom用のtensionの0.5倍（ゆっくり引く）
```

ズームインは「注目」なので素早く。ズームアウトは「解放」なのでゆっくり。
この非対称性が、映画的なカメラワークの「気持ちよさ」を生む。

---

## 7. モーションブラー

### 7.1 Screen Studioの実装分析

Screen Studioは3つの対象に個別にモーションブラーを適用:
1. **カーソル移動**: カーソルの移動方向にブラー
2. **画面ズーム**: ズーム中の画面にラジアルブラー
3. **画面パン**: パン中の画面に方向ブラー

### 7.2 Snappiでの実装方針

パフォーマンスを考慮し、**フレーム間差分ベースのモーションブラー**を採用:

```
方針:
1. 現フレームと前フレームのビューポート位置/ズームの差分を計算
2. 差分が閾値を超えている場合にのみモーションブラーを適用
3. ブラー強度 = 差分の大きさに比例

実装:
- パン時: 移動方向に沿った1Dガウシアンブラー
  strength = pan_speed * blur_multiplier (0.0-1.0, ユーザー調整可)
  max_blur_pixels = 8

- ズーム時: 画面中心からのラジアルブラー（簡易実装）
  zoom_delta = abs(current_zoom - prev_zoom)
  if zoom_delta > 0.01:
      apply radial blur from viewport center
      strength = zoom_delta * 15.0

- カーソル: カーソル画像自体に移動方向のスミアを適用
  cursor_speed = sqrt(dx^2 + dy^2)
  if cursor_speed > 5.0:
      smear in movement direction
      length = min(cursor_speed * 0.3, 12.0) pixels
```

### 7.3 パフォーマンス最適化

モーションブラーは計算コストが高いため:

```
- ブラーが不要なフレーム（静止時）はスキップ
- ブラーカーネルサイズの上限を設定（max 8px）
- 1D分離可能フィルタを使用（2Dブラーを2回の1Dブラーに分解）
- 品質設定: Off / Subtle / Cinematic
  - Off: ブラーなし
  - Subtle: max 4px, カーソルブラーのみ
  - Cinematic: max 8px, 全対象
```

---

## 8. エフェクト一覧と品質目標

### 8.1 カーソルレンダリング

**現状**: 手描きの三角形カーソル（`draw_cursor`関数、12px固定）
**目標**: システムカーソルに近い高品質レンダリング

```
改善案:
1. カーソル画像をプリレンダリング（32x32 RGBA、アンチエイリアス付き）
2. サイズ: 1.0x-2.0xのスケール設定（デフォルト1.2x）
3. ズームレベルに応じた自動スケーリング:
   - zoom > 2.0x の時、カーソルを少し大きくして見やすくする
   - cursor_display_size = base_size * (1.0 + (zoom - 1.0) * 0.15)
4. カーソル非表示: 1.5秒以上静止時にフェードアウト（opacity 0.3まで）
   Screen Studioと同様の「自動非表示」機能
```

### 8.2 クリックリングエフェクト

**現状**: 拡大する円リング（400ms, max radius 30px）
**目標**: 滑らかで美しい拡散エフェクト

```
改善案:
1. 二重リング: 内リング（素早く）+ 外リング（遅れて）
   - inner: duration 300ms, max_radius 20px, stroke 2px
   - outer: duration 500ms, max_radius 35px, stroke 1.5px, delay 50ms
2. イージング: 線形ではなくease-out cubic
   progress_eased = 1.0 - (1.0 - progress)^3
3. 色: アクセントカラー設定可能（デフォルト: #3B82F6, 70% opacity）
4. 左クリック/右クリックで色を変える:
   - 左: アクセントカラー
   - 右: 赤系（#EF4444）で右クリックを視覚的に区別
```

### 8.3 キーバッジ

**現状**: 黒い矩形のみ（テキスト描画なし）
**目標**: macOS風の美しいキーバッジ

```
改善案:
1. フォントレンダリング: ab_glyph or rusttype crateでテキスト描画
2. デザイン:
   - 背景: rgba(20, 20, 20, 0.85) with backdrop-blur感
   - 角丸: 8px
   - パディング: 8px 14px
   - テキスト: 白、14px、San Francisco風
   - 位置: 画面下部中央、出力画面の下端から32px上
3. アニメーション:
   - 表示: 下から8pxスライドイン + フェードイン (200ms, ease-out)
   - 消去: フェードアウト (300ms, ease-in)
   - 連続キー入力時: バッジ内テキストのみ更新（位置は維持）
4. 表示ロジック:
   - modifier + key の組み合わせ: 常に表示 (例: "Ctrl + C")
   - 特殊キー（Enter, Esc, Tab等）: 常に表示
   - 通常の文字キー: 非表示（テキスト入力の邪魔になるため）
```

### 8.4 背景とフレーム

**現状**: グラデーション背景 + ドロップシャドウ + 角丸
**目標**: Screen Studioに匹敵する美しいフレーミング

```
改善案:
1. 背景プリセット:
   - Ocean: #667eea → #764ba2 (135deg)
   - Sunset: #f093fb → #f5576c (135deg)
   - Forest: #11998e → #38ef7d (135deg)
   - Midnight: #2c3e50 → #3498db (135deg)
   - Clean White: #f5f7fa → #c3cfe2 (135deg)
   - Transparent: 背景なし（角丸のみ）
   - Custom: ユーザー指定

2. ドロップシャドウの改善:
   - 現在: 単純な距離ベースの減衰
   - 改善: ガウシアンブラー近似
     shadow_alpha = base_alpha * exp(-dist^2 / (2 * sigma^2))
     sigma = blur_radius / 3.0

3. インセットシャドウ（NEW）:
   - 画面の内側上端に薄い影を追加
   - 「画面が少し凹んでいる」感じ → 奥行き感
   - height: 4px, alpha: 0.15

4. 角丸の改善:
   - アンチエイリアシング: 境界ピクセルのalpha をサブピクセル計算
   - 現在の実装は角丸の境界がギザギザ
```

### 8.5 画面内影（Inset Shadow）

```
位置: 録画画面の内側、上端と左右端
パラメータ:
  - 上辺: height=6px, color=rgba(0,0,0,0.12)
  - 左右辺: width=3px, color=rgba(0,0,0,0.06)
  - 下辺: なし（シャドウと重なるため）
効果: 画面が「はめ込まれている」感じ。プロフェッショナルな質感
```

---

## 9. 視聴者にとって「わかりやすい」動画にするための工夫

### 9.1 研究に基づく原則

ユーザビリティ研究と映像制作理論から:

1. **「次に何が起きるか」を予告する**: ズームインが「ここで何か起きますよ」のサイン
2. **一度に一つのことに集中**: 二つの操作を同時にズームで追わない
3. **視線の動きを最小化**: 画面の端から端へ急に飛ばない
4. **緩急をつける**: 全部同じ速度だと注意が散漫になる
5. **文脈を失わない**: ズームしすぎて「今画面のどこにいるか」がわからなくならないように

### 9.2 具体的な実装

#### 9.2.1 「予告ズーム」パターン

```
通常のクリックズーム:
  t=0ms: Click発生 → ズームイン開始

改善版「予告ズーム」:
  ズーム遷移自体が完了するまでの時間をイージングで制御し、
  クリック後の視覚的変化（UIの反応）と同時にズームが完了するようにする:

  t=0ms: Click発生 → スプリングアニメーション開始
  t=~200ms: ズーム遷移の80%が完了（スプリングの自然な動き）
  t=~400ms: ズームが安定（ユーザーがクリック結果を確認する頃）
```

#### 9.2.2 「文脈維持」ズーム

```
max_zoomの制限:
- 画面の30%以上が常に見えるようにする
- zoom_level <= screen_area / (screen_area * 0.30) = 3.33x
- 推奨最大: 3.0x（デフォルト設定のまま）

ズーム遷移中の中間状態:
- 2.5x → 1.0x への遷移時、1.5x付近で0.3秒程度「タメ」を入れる
  これにより視聴者が「今からズームアウトする」ことを認識できる
  → timeline.rsで実装（中間キーフレーム自動挿入）
```

#### 9.2.3 「リズム」の制御

```
連続操作時のズーム抑制:
- 500ms以内に3回以上のClickがある場合:
  → 1回目のClickでズームイン
  → 2回目以降はビューポート追従のみ（再ズームしない）
  → 最後のClickから800ms後にズーム状態を再評価

- テキスト入力中:
  → ズームレベル固定（入力中の画面の揺れは最悪）
  → 入力終了（Focusが別の場所に移る or 2秒間キー入力なし）まで維持
```

#### 9.2.4 大距離移動時の「ブリッジ」

```
画面の40%以上離れた位置へ移動する場合:

パターンA: ズームアウト→パン→ズームイン
  t=0ms: ズームアウト開始（current → 1.0x）
  t=300ms: パン開始（新位置へ）
  t=600ms: ズームイン開始（1.0x → target）
  合計: 約900ms

パターンB: カット（即座に切り替え）
  - 画面の60%以上離れている場合
  - または WindowSwitch の場合
  - snap_to で即座に遷移（スプリングなし）
  → 映画の「カット」と同じ。視聴者は場面転換として認識する

選択基準:
  distance > 0.6 * screen_diagonal → カット
  distance > 0.4 * screen_diagonal → ブリッジ
  それ以外 → 直接パン
```

---

## 10. ユーザー設定インターフェース

### 10.1 設定項目

```
Effects
  ├── Auto Zoom
  │     ├── Enabled: bool (default: true)
  │     ├── Speed: SpringPreset (Slow/Mellow/Quick/Rapid, default: Mellow)
  │     ├── Zoom Level: f64 (1.5-3.0, default: 2.0)
  │     ├── Text Input Zoom: f64 (1.5-3.5, default: 2.5)
  │     └── Max Zoom: f64 (2.0-4.0, default: 3.0)
  │
  ├── Cursor
  │     ├── Smoothing: bool (default: true)
  │     ├── Size: f64 (0.8-2.0, default: 1.2)
  │     ├── Auto-hide: bool (default: true)
  │     └── Auto-hide delay: u64 ms (default: 1500)
  │
  ├── Click Effect
  │     ├── Enabled: bool (default: true)
  │     ├── Color: [u8; 4] (default: #3B82F6B3)
  │     └── Style: ClickStyle (Ring/Dot/Pulse, default: Ring)
  │
  ├── Key Badge
  │     ├── Enabled: bool (default: true)
  │     ├── Show modifiers only: bool (default: true)
  │     └── Duration: u64 ms (default: 1500)
  │
  ├── Motion Blur
  │     ├── Quality: MotionBlurQuality (Off/Subtle/Cinematic, default: Subtle)
  │     └── Strength: f64 (0.0-1.0, default: 0.5)
  │
  └── Background
        ├── Preset: BackgroundPreset (Ocean/Sunset/.../Custom)
        ├── Border Radius: u32 (0-24, default: 12)
        ├── Shadow: bool (default: true)
        └── Inset Shadow: bool (default: true)
```

---

## 11. 品質チェックリスト

リリース前に以下をすべて確認:

- [ ] ズームイン中にカーソルがビューポート外に出ない
- [ ] 連続クリック時に画面がガタガタしない（ズーム抑制が機能）
- [ ] テキスト入力中にズームレベルが変わらない
- [ ] WindowSwitch時にカットが自然に見える
- [ ] Idle後のズームアウトが「ぬるっ」と動く（急に引かない）
- [ ] モーションブラーが自然に見える（不自然なアーティファクトなし）
- [ ] クリックリングのイージングが気持ちいい
- [ ] キーバッジが読める（フォントレンダリングが正しい）
- [ ] 背景のグラデーションが美しい
- [ ] 角丸にギザギザがない（アンチエイリアス）
- [ ] 5分の録画でエクスポートが3分以内に完了する
- [ ] Spring物理が発散しない（高tension設定でも安定）

---

## 12. 実装優先順位

### Phase 1: 基盤修正（最優先）
1. イベント記録の修正（mouse_move間引き、modifier追跡、Focus実装）
2. Spring物理のサブステッピング追加
3. ビューポートのカーソル追従実装

### Phase 2: ズームロジック改善
4. セマンティック分析の実装
5. ズームアウト条件の改善（Idle段階的、WindowSwitch、大距離移動）
6. ズームキーフレーム生成アルゴリズムの改修

### Phase 3: 視覚品質向上
7. クリックリングの改善（二重リング、イージング）
8. カーソルレンダリングの改善（プリレンダリング画像）
9. 背景・角丸のアンチエイリアス改善
10. キーバッジのフォントレンダリング

### Phase 4: 仕上げ
11. モーションブラーの実装
12. インセットシャドウの追加
13. スプリングプリセット（Slow/Mellow/Quick/Rapid）のUI化

---

## 13. 参考資料

- [Screen Studio Auto Zoom Guide](https://screen.studio/guide/auto-zoom)
- [Screen Studio Animations & Motion](https://screen.studio/guide/animations-motion)
- [Screen Studio Slower Zoom Presets Discussion](https://hub.screen.studio/p/slower-zoom-presets)
- [Tella Auto Zoom Features](https://www.tella.com/)
- [FocuSee Auto Zoom and Cursor Animation](https://focusee.imobie.com/features/auto-zoom-and-cursor-animation.htm)
- [CANVID Cursor Movement Smoothing](https://www.canvid.com/features/cursor-movement-smoothing)
- [Rapidemo Cursor Guide](https://getrapidemo.com/guides/editing/cursor)
- [Josh Comeau: A Friendly Introduction to Spring Physics](https://www.joshwcomeau.com/animation/a-friendly-introduction-to-spring-physics/)
- [Maxime Heckel: The Physics Behind Spring Animations](https://blog.maximeheckel.com/posts/the-physics-behind-spring-animations/)
- [Effortless UI Spring Animations: A Two-Parameter Approach](https://www.kvin.me/posts/effortless-ui-spring-animations)
- [Android Spring Animation Developer Guide](https://developer.android.com/develop/ui/views/animations/spring-animation)
- [Apple: interpolatingSpring Documentation](https://developer.apple.com/documentation/swiftui/animation/interpolatingspring(mass:stiffness:damping:initialvelocity:))
- [SwiftUI Spring Animations Guide](https://github.com/GetStream/swiftui-spring-animations)
- [Figma: Prototype Easing and Spring Animations](https://help.figma.com/hc/en-us/articles/360051748654-Prototype-easing-and-spring-animations)
- [Fiveable: Cinematography Camera Movements](https://fiveable.me/cinematography/unit-5)
- [Camera Movements in Film - Journalism University](https://journalism.university/electronic-media/the-art-of-camera-movements-in-film/)
- [NVIDIA GPU Gems 3: Motion Blur as Post-Processing](https://developer.nvidia.com/gpugems/gpugems3/part-iv-image-effects/chapter-27-motion-blur-post-processing-effect)
- [Pixel Motion Blur Explained](https://focusee.imobie.com/edit-video/pixel-motion-blur.htm)

---

*この設計は「ユーザーが操作を録画するだけで、プロが編集したような動画ができる」という体験を実現するためのものです。妥協は、しません。*
