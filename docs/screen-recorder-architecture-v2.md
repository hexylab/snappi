# Screen Studio-Level Screen Recorder — アーキテクチャ設計書 v2

## 1. プロジェクト概要

Windows向けのScreen Studioクラスの画面録画アプリケーション。
**コンセプト：「録画して、停止して、エクスポート」の3ステップで完成する。**

ユーザーは動画編集者ではなく、アプリの説明ページやREADMEに貼るデモ動画を
手早く作りたいエンジニア/プロダクト担当者。
編集作業なしで、Screen Studioレベルの見栄えの良い動画が自動生成されることがゴール。

### ユーザーフロー（最短パス）

```
1. システムトレイのアイコン or ショートカットキー（Ctrl+Shift+R）で録画開始
2. アプリを操作する（デモしたい操作を普通に行う）
3. 同じショートカットキーで録画停止
4. 自動でエフェクト付きプレビューが表示される
5. 「Export MP4」ボタンを押す → 完成
```

### 設計原則

1. **Zero-Config で美しい出力** — デフォルト設定のまま何も触らずに高品質な動画が出る
2. **録画中のオーバーヘッド最小** — 録画はデータ収集に徹し、重い処理は後で行う
3. **設定は隠す、しかし用意はする** — 上級者向けの調整は「設定」に控えめに置く

---

## 2. アーキテクチャ全体像

### 2パス方式：録画（データ収集）→ 自動レンダリング

```
[録画フェーズ]                    [自動レンダリングフェーズ]
                                  
  Screen Capture ──→ video.h264     ┌─────────────────────────┐
  Mouse Events   ──→ events.jsonl   │  Auto Effect Engine     │
  Key Events     ──→ events.jsonl   │                         │
  UI Focus       ──→ events.jsonl   │  1. Zoom Plan生成       │──→ preview.mp4
  Audio          ──→ audio.wav      │  2. Cursor Smoothing    │──→ output.mp4
  Metadata       ──→ meta.json      │  3. Frame Composition   │──→ output.gif
                                    │  4. Encode              │──→ output.webm
                                    └─────────────────────────┘
```

---

## 3. 技術スタック

| レイヤー | 技術 | 用途 |
|---------|------|------|
| フレームワーク | Tauri v2 | デスクトップアプリ基盤 |
| フロントエンド | SolidJS + TailwindCSS | 最小限のUI |
| バックエンド | Rust | 録画・レンダリング全般 |
| 画面キャプチャ | `scap` crate | Desktop Duplication API |
| 入力イベント | `rdev` crate | グローバルマウス/キーフック |
| UI Focus検出 | `windows` crate (UIA) | 入力フォーム自動検出 |
| 映像処理 | `image` crate + custom compositor | フレーム合成 |
| エンコード | FFmpeg (bundled) | 最終出力 |
| 音声 | `cpal` crate | システム音声・マイク |

---

## 4. 録画フェーズ詳細

### 4.1 並行データ収集

録画中は4つのスレッドが独立してデータを収集する。
エフェクト処理は一切行わない。

```
┌─ Recording Session ─────────────────────────────────────┐
│                                                          │
│  Thread 1: Screen Capture                                │
│    scap / Desktop Duplication API → 60fps                │
│    H.264 CRF 0（ほぼロスレス）で中間保存                  │
│    NVENCがあればGPUエンコード                             │
│                                                          │
│  Thread 2: Input Event Stream                            │
│    rdev グローバルフック                                  │
│    マウス座標は10ms間隔でサンプリング                     │
│    クリック・スクロール・キー押下をタイムスタンプ付き記録  │
│                                                          │
│  Thread 3: UI Focus Tracker                              │
│    IUIAutomation::AddFocusChangedEventHandler            │
│    フォーカス要素のタイプ・BoundingRect・名前を記録       │
│                                                          │
│  Thread 4: Audio Capture                                 │
│    WASAPI loopback → システム音声                        │
│    WASAPI capture → マイク（オプション）                  │
│                                                          │
└──────────────────────────────────────────────────────────┘
```

### 4.2 イベントログ形式（events.jsonl）

1行1イベントのJSONLフォーマット（追記が高速）。

```jsonl
{"t":0,"type":"mouse_move","x":512,"y":384}
{"t":10,"type":"mouse_move","x":515,"y":386}
{"t":1250,"type":"click","btn":"left","x":800,"y":600}
{"t":1250,"type":"key","key":"Enter","mod":[]}
{"t":1300,"type":"key","key":"Tab","mod":[]}
{"t":2000,"type":"focus","el":"Edit","name":"Search Box","rect":[400,300,600,330]}
{"t":5500,"type":"click","btn":"left","x":200,"y":400}
{"t":8000,"type":"scroll","x":500,"y":300,"dx":0,"dy":-120}
```

### 4.3 メタデータ（meta.json）

```json
{
  "version": 1,
  "screen_width": 1920,
  "screen_height": 1080,
  "fps": 60,
  "start_time": "2026-02-06T15:30:00+09:00",
  "duration_ms": 12500,
  "has_audio": true,
  "monitor_scale": 1.5
}
```

---

## 5. 自動エフェクトエンジン（核心部分）

録画停止後、events.jsonl を解析し、完全自動でエフェクトプラン（ZoomPlan）を生成する。
ユーザーの操作は不要。

### 5.1 処理パイプライン

```
events.jsonl + meta.json
        │
        ▼
┌─ Step 1: Event Analysis ──────────────────────────────┐
│  - イベントストリームをセグメントに分割                 │
│  - 各セグメントに「意図」を推定                        │
│    (クリック操作 / テキスト入力 / スクロール閲覧 / 待機) │
└────────────────────────┬──────────────────────────────┘
                         │
                         ▼
┌─ Step 2: Zoom Plan Generation ────────────────────────┐
│  - セグメントごとにズームターゲットと倍率を決定         │
│  - 不要なズームの間引き（短すぎる操作は無視）          │
│  - アイドル区間でのズームアウト挿入                     │
│  → ZoomPlan（タイムライン上のズーム指示リスト）         │
└────────────────────────┬──────────────────────────────┘
                         │
                         ▼
┌─ Step 3: Cursor Path Smoothing ───────────────────────┐
│  - 生のマウス軌跡にスプリングフィルタ適用              │
│  - 微細な振動（手ブレ）を除去                          │
│  - クリック前後の急な移動をイージングで滑らかに          │
│  → SmoothedCursorPath                                  │
└────────────────────────┬──────────────────────────────┘
                         │
                         ▼
┌─ Step 4: Frame-by-Frame Composition ──────────────────┐
│  フレームごとに以下を順に適用：                        │
│  (a) ビューポート計算（ZoomPlanからスプリング補間）     │
│  (b) フレームのクロップ＆スケール                      │
│  (c) カーソル描画（スムージング済み座標 + カスタム画像）│
│  (d) クリックリングエフェクト                          │
│  (e) キー表示オーバーレイ                              │
│  (f) 角丸・影・背景の合成                              │
│  → FFmpegへパイプ出力                                  │
└───────────────────────────────────────────────────────┘
```

### 5.2 Step 1: Event Analysis — セグメント分割

イベントストリームを「操作のかたまり」に分割する。

```rust
#[derive(Debug)]
enum SegmentType {
    Click,          // クリック操作（ボタン押下、メニュー選択など）
    TextInput,      // テキスト入力中（フォーカス + キー入力の連続）
    Scroll,         // スクロール閲覧
    Idle,           // 操作なし（500ms以上イベントなし）
    RapidAction,    // 短時間の連続クリック（間引き対象）
}

struct Segment {
    segment_type: SegmentType,
    start_ms: u64,
    end_ms: u64,
    // このセグメント中の注目ポイント（ズーム先）
    focus_point: Option<FocusPoint>,
}

struct FocusPoint {
    x: f64,
    y: f64,
    // ズーム対象の領域（入力フォームなど）
    region: Option<Rect>,
}
```

**分割ルール：**

| 条件 | セグメントタイプ | 説明 |
|------|-----------------|------|
| click イベント + 前後200ms以内に次のclickなし | `Click` | 単独クリック → ズーム対象 |
| click が200ms以内に3回以上連続 | `RapidAction` | ダブル/トリプルクリック → まとめて1ズーム |
| focus(Edit/TextInput) + 500ms以内にkey連続 | `TextInput` | テキスト入力 → フォーム領域にズーム |
| scroll が連続 | `Scroll` | スクロール閲覧 → ズームアウト気味 |
| 500ms以上イベントなし | `Idle` | 待機 → ズームアウトして全体表示 |

### 5.3 Step 2: Zoom Plan Generation

セグメントリストからズームの指示リストを生成する。

```rust
struct ZoomKeyframe {
    time_ms: u64,
    target_x: f64,       // ズーム中心X（画面座標）
    target_y: f64,       // ズーム中心Y
    zoom_level: f64,     // 1.0 = 等倍, 2.0 = 2倍拡大
    transition: TransitionType,
}

enum TransitionType {
    SpringIn,     // バネ的にズームイン（クリック時）
    SpringOut,    // バネ的にズームアウト（アイドル時）
    Smooth,       // 滑らかに移動（連続操作間）
    Cut,          // 即座に切り替え（画面の大きく離れた場所への移動）
}
```

**ズームプラン生成ルール：**

```rust
fn generate_zoom_plan(segments: &[Segment], meta: &Metadata) -> Vec<ZoomKeyframe> {
    let mut plan = Vec::new();

    for (i, seg) in segments.iter().enumerate() {
        match seg.segment_type {
            SegmentType::Click => {
                if let Some(fp) = &seg.focus_point {
                    // クリック位置にズームイン
                    plan.push(ZoomKeyframe {
                        time_ms: seg.start_ms,
                        target_x: fp.x,
                        target_y: fp.y,
                        zoom_level: 2.0,
                        transition: TransitionType::SpringIn,
                    });
                }
            }

            SegmentType::TextInput => {
                if let Some(fp) = &seg.focus_point {
                    if let Some(rect) = &fp.region {
                        // 入力フォーム領域にフィットするようにズーム
                        let zoom = calc_zoom_to_fit(rect, meta, 0.3); // 余白30%
                        plan.push(ZoomKeyframe {
                            time_ms: seg.start_ms,
                            target_x: rect.center_x(),
                            target_y: rect.center_y(),
                            zoom_level: zoom.min(3.0), // 最大3倍に制限
                            transition: TransitionType::SpringIn,
                        });
                    }
                }
            }

            SegmentType::Scroll => {
                // スクロール中はやや引きで全体を見せる
                plan.push(ZoomKeyframe {
                    time_ms: seg.start_ms,
                    target_x: meta.screen_width as f64 / 2.0,
                    target_y: meta.screen_height as f64 / 2.0,
                    zoom_level: 1.2,
                    transition: TransitionType::Smooth,
                });
            }

            SegmentType::Idle => {
                // アイドル → ズームアウトして全体表示
                if current_zoom > 1.0 {
                    plan.push(ZoomKeyframe {
                        time_ms: seg.start_ms + 300, // 少し遅延
                        target_x: meta.screen_width as f64 / 2.0,
                        target_y: meta.screen_height as f64 / 2.0,
                        zoom_level: 1.0,
                        transition: TransitionType::SpringOut,
                    });
                }
            }

            SegmentType::RapidAction => {
                // 連続クリック → 最初のクリック位置に1回だけズーム
                if let Some(fp) = &seg.focus_point {
                    plan.push(ZoomKeyframe {
                        time_ms: seg.start_ms,
                        target_x: fp.x,
                        target_y: fp.y,
                        zoom_level: 1.8,
                        transition: TransitionType::SpringIn,
                    });
                }
            }
        }
    }

    // 後処理：近すぎるズームキーフレームを間引く
    deduplicate_keyframes(&mut plan, MIN_ZOOM_INTERVAL_MS);

    // 後処理：画面の遠い場所へのジャンプはCutに変更
    detect_cuts(&mut plan, meta, CUT_DISTANCE_THRESHOLD);

    plan
}
```

**間引きルール（自然さのために重要）：**

```rust
fn deduplicate_keyframes(plan: &mut Vec<ZoomKeyframe>, min_interval_ms: u64) {
    // 300ms以内に複数のズームキーフレームがある場合
    // → 最後のもの以外を削除（ユーザーの「最終目的地」を優先）

    // 同じズームレベルへの連続遷移は削除
    // → zoom 2.0 → zoom 2.0 は無意味

    // ズームイン直後（200ms以内）のズームアウトは削除
    // → 一瞬だけズームしてすぐ戻るのは目障り
}

fn detect_cuts(plan: &mut Vec<ZoomKeyframe>, meta: &Metadata, threshold: f64) {
    // 前のキーフレームから画面幅の50%以上離れた場所へ移動する場合
    // → スプリングアニメーションだと長距離をグニャっと移動して不自然
    // → Cutに変更して一瞬でズームアウト→ズームインし直す
}
```

### 5.4 Step 3: Cursor Path Smoothing

生のマウス軌跡は手ブレで微妙にガタつく。スプリングフィルタで滑らかにする。

```rust
struct CursorSmoother {
    spring_x: SpringAnimation,
    spring_y: SpringAnimation,
}

impl CursorSmoother {
    fn new() -> Self {
        Self {
            // カーソルスムージング用のパラメータ
            // tension高め = 追従性が良い、friction高め = 振動しない
            spring_x: SpringAnimation::new(300.0, 30.0, 1.0),
            spring_y: SpringAnimation::new(300.0, 30.0, 1.0),
        }
    }

    fn smooth(&mut self, raw_positions: &[(u64, f64, f64)]) -> Vec<(u64, f64, f64)> {
        let mut result = Vec::new();
        let dt = 1.0 / 60.0; // 60fps

        for &(t, raw_x, raw_y) in raw_positions {
            self.spring_x.target = raw_x;
            self.spring_y.target = raw_y;
            let smooth_x = self.spring_x.update(dt);
            let smooth_y = self.spring_y.update(dt);
            result.push((t, smooth_x, smooth_y));
        }
        result
    }
}
```

### 5.5 Step 4: Frame Composition

1フレームずつ最終映像を合成する。

```rust
fn compose_frame(
    raw_frame: &RgbaImage,      // 元の画面キャプチャ
    frame_time_ms: u64,
    viewport: &AnimatedViewport, // スプリング補間されたビューポート
    cursor: &CursorState,       // スムージング済みカーソル位置
    click_effects: &[ClickEffect],
    key_overlay: Option<&KeyOverlay>,
    style: &OutputStyle,        // 背景・角丸・影の設定
) -> RgbaImage {

    // --- (a) ビューポート領域のクロップ＆スケール ---
    let vp = viewport.current();
    let cropped = crop_and_scale(
        raw_frame,
        vp.x, vp.y, vp.width, vp.height,
        style.output_width, style.output_height,
    );

    // --- (b) カーソル描画 ---
    let cursor_screen_pos = viewport.to_output_coords(cursor.x, cursor.y);
    draw_custom_cursor(&mut cropped, cursor_screen_pos, &style.cursor);

    // --- (c) クリックリングエフェクト ---
    for effect in click_effects {
        if effect.is_active(frame_time_ms) {
            let pos = viewport.to_output_coords(effect.x, effect.y);
            let progress = effect.progress(frame_time_ms);
            draw_click_ring(
                &mut cropped,
                pos,
                progress,
                &style.click_ring,
            );
        }
    }

    // --- (d) キーボードショートカット表示 ---
    if let Some(overlay) = key_overlay {
        if overlay.is_visible(frame_time_ms) {
            draw_key_badge(&mut cropped, overlay, &style.key_badge);
        }
    }

    // --- (e) 角丸マスク ---
    if style.border_radius > 0 {
        apply_rounded_corners(&mut cropped, style.border_radius);
    }

    // --- (f) 影 + 背景合成 ---
    let mut canvas = create_background(
        style.canvas_width,
        style.canvas_height,
        &style.background,
    );
    let shadow = generate_drop_shadow(&cropped, &style.shadow);
    composite(&mut canvas, &shadow, style.content_offset);
    composite(&mut canvas, &cropped, style.content_offset);

    canvas
}
```

### 5.6 スプリングアニメーション

すべてのアニメーション（ズーム、カーソル、エフェクト）に使う共通エンジン。

```rust
struct SpringAnimation {
    position: f64,
    velocity: f64,
    target: f64,
    tension: f64,      // バネの強さ（大きいほどキビキビ）
    friction: f64,      // 減衰（大きいほど振動しにくい）
    mass: f64,          // 質量
}

impl SpringAnimation {
    fn new(tension: f64, friction: f64, mass: f64) -> Self {
        Self {
            position: 0.0,
            velocity: 0.0,
            target: 0.0,
            tension,
            friction,
            mass,
        }
    }

    fn update(&mut self, dt: f64) -> f64 {
        let displacement = self.position - self.target;
        let spring_force = -self.tension * displacement;
        let damping_force = -self.friction * self.velocity;
        let acceleration = (spring_force + damping_force) / self.mass;

        self.velocity += acceleration * dt;
        self.position += self.velocity * dt;
        self.position
    }

    fn is_settled(&self) -> bool {
        (self.position - self.target).abs() < 0.5
            && self.velocity.abs() < 0.1
    }
}

/// ビューポート全体（X,Y,Zoom）をスプリングで管理
struct AnimatedViewport {
    center_x: SpringAnimation,
    center_y: SpringAnimation,
    zoom: SpringAnimation,
}

impl AnimatedViewport {
    fn apply_keyframe(&mut self, kf: &ZoomKeyframe) {
        match kf.transition {
            TransitionType::SpringIn => {
                // 通常のスプリング遷移
                self.center_x.target = kf.target_x;
                self.center_y.target = kf.target_y;
                self.zoom.target = kf.zoom_level;
            }
            TransitionType::Cut => {
                // 即座に移動（スプリングをスキップ）
                self.center_x.position = kf.target_x;
                self.center_x.target = kf.target_x;
                self.center_x.velocity = 0.0;
                self.center_y.position = kf.target_y;
                self.center_y.target = kf.target_y;
                self.center_y.velocity = 0.0;
                self.zoom.target = kf.zoom_level;
            }
            // ...
        }
    }
}
```

---

## 6. デフォルトスタイル設定（Zero-Config）

ユーザーが何も設定しなくても適用されるデフォルト値。
Screen Studioの「いい感じ」を再現するプリセット。

```rust
struct OutputStyle {
    // --- 出力サイズ ---
    output_width: u32,          // 1920 (元の解像度を維持)
    output_height: u32,         // 1080
    canvas_width: u32,          // 2048 (背景込みのサイズ)
    canvas_height: u32,         // 1152

    // --- 背景 ---
    background: Background,     // グラデーション (淡い紫〜青)
    content_offset: (u32, u32), // 中央配置のオフセット

    // --- 角丸・影 ---
    border_radius: u32,         // 12px
    shadow: ShadowConfig {
        blur: 40.0,
        spread: 0.0,
        offset_y: 10.0,
        color: [0, 0, 0, 80],  // 半透明の黒
    },

    // --- カーソル ---
    cursor: CursorConfig {
        style: CursorStyle::System, // システムカーソルをそのまま使用
        size_multiplier: 1.2,       // 少し拡大して視認性UP
        smoothing: true,
    },

    // --- クリックリング ---
    click_ring: ClickRingConfig {
        enabled: true,
        max_radius: 30.0,
        duration_ms: 400,
        color: [59, 130, 246, 180], // 青系半透明
        stroke_width: 2.5,
    },

    // --- キーボード表示 ---
    key_badge: KeyBadgeConfig {
        enabled: true,
        // Ctrl, Shift, Alt 等の修飾キーとの組み合わせのみ表示
        // 通常のタイピングは表示しない（うるさくなるため）
        show_only_with_modifiers: true,
        position: BadgePosition::BottomCenter,
        duration_ms: 1500,
        style: BadgeStyle::Rounded,  // 角丸の小さなバッジ
    },

    // --- ズーム ---
    zoom: ZoomConfig {
        auto_zoom_enabled: true,
        default_zoom_level: 2.0,         // クリック時のデフォルト倍率
        text_input_zoom_level: 2.5,      // テキスト入力時
        max_zoom: 3.0,
        idle_timeout_ms: 1500,           // この時間操作なし→ズームアウト
        min_segment_duration_ms: 300,    // これより短い操作は無視
        spring_tension: 170.0,
        spring_friction: 26.0,
    },
}

enum Background {
    Gradient {
        from: [u8; 3],     // [139, 92, 246]  紫
        to: [u8; 3],       // [59, 130, 246]   青
        angle: f64,         // 135度
    },
    Solid([u8; 3]),
    Image(PathBuf),
    Transparent,            // PNG出力用
}
```

---

## 7. UI設計（最小限）

### 7.1 画面構成

編集UIは作らない。代わりに3つの画面のみ。

```
┌─ メイン画面（トレイアイコンクリック時）────────────────────┐
│                                                            │
│  ┌──────────────────────────────────────────────────────┐  │
│  │                                                      │  │
│  │              過去の録画一覧                            │  │
│  │              (サムネイル + 日時 + 長さ)                │  │
│  │                                                      │  │
│  │  ┌──────┐  ┌──────┐  ┌──────┐  ┌──────┐            │  │
│  │  │ 2/6  │  │ 2/5  │  │ 2/3  │  │ 2/1  │            │  │
│  │  │12:30 │  │10:15 │  │16:45 │  │09:00 │            │  │
│  │  │ 15s  │  │ 32s  │  │ 8s   │  │ 45s  │            │  │
│  │  └──────┘  └──────┘  └──────┘  └──────┘            │  │
│  │                                                      │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                            │
│  ┌──────────────────┐  ┌────────┐                         │
│  │  ● 録画開始       │  │  設定  │                         │
│  │  (Ctrl+Shift+R)  │  │  ⚙    │                         │
│  └──────────────────┘  └────────┘                         │
│                                                            │
└────────────────────────────────────────────────────────────┘

┌─ プレビュー画面（録画停止後に自動表示）────────────────────┐
│                                                            │
│  ┌──────────────────────────────────────────────────────┐  │
│  │                                                      │  │
│  │           エフェクト適用済みプレビュー再生             │  │
│  │           (自動生成された結果をそのまま表示)           │  │
│  │                                                      │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                            │
│  ▶ 再生  ⏸ 一時停止                          00:15 / 00:15│
│                                                            │
│  ┌───────────────┐  ┌──────────┐  ┌──────────┐           │
│  │ Export MP4 📥  │  │ GIF 📥   │  │ WebM 📥  │           │
│  └───────────────┘  └──────────┘  └──────────┘           │
│                                                            │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ Quality: [Social ▾]    Resolution: [1080p ▾]         │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                            │
│  ┌──────────┐  ┌──────────┐                               │
│  │ やり直す  │  │ 閉じる    │                               │
│  └──────────┘  └──────────┘                               │
└────────────────────────────────────────────────────────────┘

┌─ 設定画面 ─────────────────────────────────────────────────┐
│                                                            │
│  録画                                                      │
│    ショートカットキー: [Ctrl+Shift+R]                      │
│    FPS: [60 ▾]                                             │
│    音声を録音: [✓ システム音声] [□ マイク]                  │
│                                                            │
│  スタイル                                                   │
│    背景: [グラデーション ▾] 🟣→🔵                          │
│    角丸: [12px ▾]                                          │
│    影:   [✓ 有効]                                          │
│                                                            │
│  エフェクト                                                 │
│    自動ズーム: [✓ 有効]                                    │
│    ズーム倍率: [2.0x ▾]                                    │
│    クリックリング: [✓ 有効]                                │
│    キーボード表示: [✓ 修飾キー付きのみ]                    │
│    カーソルスムージング: [✓ 有効]                           │
│                                                            │
│  出力                                                      │
│    デフォルト形式: [MP4 ▾]                                 │
│    デフォルト品質: [Social (1080p/30fps) ▾]                │
│    保存先: [~/Videos/ScreenRecorder ▾]                     │
│                                                            │
└────────────────────────────────────────────────────────────┘
```

### 7.2 システムトレイ

```
  [アイコン] 右クリックメニュー
    ├── 録画開始 (Ctrl+Shift+R)
    ├── ──────────
    ├── 最近の録画 →
    │     ├── 2/6 12:30 (15s)
    │     ├── 2/5 10:15 (32s)
    │     └── もっと見る...
    ├── ──────────
    ├── 設定
    └── 終了
```

### 7.3 録画中の表示

```
  画面の端に小さなフローティングバー（常に最前面）
  ┌─────────────────────────┐
  │  ● REC  00:15  ⏸ ⏹    │
  └─────────────────────────┘
  ※ ドラッグで移動可能、録画には映らない
```

---

## 8. エクスポート

### 8.1 品質プリセット

| プリセット名 | 解像度 | FPS | CRF | 想定用途 |
|-------------|--------|-----|-----|---------|
| Social | 1080p | 30 | 23 | X, LinkedIn, README |
| High Quality | 元解像度 | 60 | 18 | YouTube, プレゼン |
| Lightweight | 720p | 24 | 30 | Slack, ドキュメント |

### 8.2 形式別FFmpegパイプライン

```rust
enum ExportFormat {
    Mp4 { preset: QualityPreset },
    Gif { max_width: u32, fps: u32 },
    WebM { preset: QualityPreset },
}

fn build_ffmpeg_command(format: &ExportFormat, input: &str, output: &str) -> Command {
    match format {
        ExportFormat::Mp4 { preset } => {
            // ffmpeg -framerate 60 -i pipe:0
            //   -c:v libx264 -crf {crf} -preset medium
            //   -pix_fmt yuv420p -movflags +faststart
            //   output.mp4
        }
        ExportFormat::Gif { max_width, fps } => {
            // Pass 1: パレット生成
            // Pass 2: パレット適用でGIF出力
            // → 高品質GIFにはこの2パスが必要
        }
        ExportFormat::WebM { preset } => {
            // ffmpeg -i pipe:0
            //   -c:v libvpx-vp9 -crf {crf} -b:v 0
            //   output.webm
        }
    }
}
```

---

## 9. ディレクトリ構造

```
screen-recorder/
├── src-tauri/
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs                # Tauri エントリポイント
│   │   ├── tray.rs                # システムトレイ
│   │   ├── shortcuts.rs           # グローバルショートカット
│   │   ├── commands.rs            # Tauri IPC コマンド
│   │   │
│   │   ├── recording/             # === 録画フェーズ ===
│   │   │   ├── mod.rs
│   │   │   ├── session.rs         # 録画セッション管理
│   │   │   ├── capture.rs         # 画面キャプチャ (scap)
│   │   │   ├── events.rs          # マウス/キーボードフック
│   │   │   ├── focus.rs           # UI Automation
│   │   │   └── audio.rs           # 音声録音
│   │   │
│   │   ├── engine/                # === 自動エフェクトエンジン ===
│   │   │   ├── mod.rs
│   │   │   ├── analyzer.rs        # Step 1: イベント解析・セグメント分割
│   │   │   ├── zoom_planner.rs    # Step 2: ズームプラン生成
│   │   │   ├── cursor_smoother.rs # Step 3: カーソルスムージング
│   │   │   ├── compositor.rs      # Step 4: フレーム合成メインループ
│   │   │   ├── spring.rs          # スプリングアニメーション
│   │   │   └── effects/
│   │   │       ├── mod.rs
│   │   │       ├── click_ring.rs  # クリックリング描画
│   │   │       ├── key_badge.rs   # キーボードバッジ描画
│   │   │       ├── cursor.rs      # カスタムカーソル描画
│   │   │       ├── background.rs  # 背景・角丸・影
│   │   │       └── viewport.rs    # ビューポート（ズーム領域）管理
│   │   │
│   │   ├── export/                # === エクスポート ===
│   │   │   ├── mod.rs
│   │   │   ├── encoder.rs         # FFmpegラッパー
│   │   │   └── presets.rs         # 品質プリセット
│   │   │
│   │   └── config/                # === 設定 ===
│   │       ├── mod.rs
│   │       └── defaults.rs        # デフォルトスタイル定義
│   │
│   ├── ffmpeg/                    # バンドルするFFmpegバイナリ
│   └── tauri.conf.json
│
├── src/                           # フロントエンド (SolidJS)
│   ├── App.tsx
│   ├── pages/
│   │   ├── RecordingList.tsx      # メイン画面（録画一覧）
│   │   ├── Preview.tsx            # プレビュー＆エクスポート
│   │   └── Settings.tsx           # 設定
│   ├── components/
│   │   ├── VideoPlayer.tsx        # プレビュー再生
│   │   ├── ExportButtons.tsx      # エクスポートボタン群
│   │   ├── RecordingBar.tsx       # 録画中フローティングバー
│   │   └── ThumbnailCard.tsx      # 録画サムネイル
│   └── lib/
│       ├── commands.ts            # Tauri IPC
│       └── types.ts
│
├── package.json
└── README.md
```

---

## 10. 開発フェーズ

### Phase 1: 録画基盤（2週間）
- [ ] Tauri v2 + SolidJS プロジェクトセットアップ
- [ ] システムトレイ + グローバルショートカット (Ctrl+Shift+R)
- [ ] scap による画面キャプチャ → H.264中間保存
- [ ] rdev によるマウス/キーイベント → events.jsonl
- [ ] 録画中フローティングバー
- [ ] FFmpegで素のMP4エクスポート（エフェクトなし）
- **ここでまず動くものができる**

### Phase 2: 自動エフェクトエンジン（3週間）★最重要
- [ ] イベント解析 + セグメント分割
- [ ] ズームプラン自動生成
- [ ] スプリングアニメーション実装
- [ ] ビューポート（ズーム領域）のフレーム単位補間
- [ ] カーソルスムージング
- [ ] フレームのクロップ＆スケール
- [ ] クリックリングエフェクト
- [ ] 角丸 + 影 + 背景合成
- **ここで「自動でいい感じの動画」が出るようになる**

### Phase 3: 仕上げ（2週間）
- [ ] UI Automation によるフォーカス検出 + テキスト入力ズーム
- [ ] キーボードショートカット表示
- [ ] 音声録音 + 映像との同期
- [ ] GIF / WebM エクスポート
- [ ] プレビュー再生UI
- [ ] 品質プリセット選択UI
- [ ] 設定画面
- [ ] 録画一覧画面

### Phase 4: 品質向上（継続的）
- [ ] ズーム間引きロジックの調整（自然さのチューニング）
- [ ] スプリングパラメータの最適化
- [ ] GPU活用（NVENC、GPUベースのフレーム合成）
- [ ] 複数モニター対応
- [ ] ウィンドウ単位の録画

---

## 11. 参考プロジェクト

| プロジェクト | 参考ポイント |
|------------|------------|
| [Cap](https://github.com/CapSoftware/Cap) | Tauri+Rust構成全体、scap crate、ズームエフェクト |
| [scap](https://github.com/CapSoftware/scap) | クロスプラットフォーム画面キャプチャ |
| [screen-demo](https://github.com/njraladdin/screen-demo) | Tauri製Screen Studio代替 |
| [Rapidemo](https://getrapidemo.com) | 2パス方式の設計思想 |
| [rusty-duplication](https://github.com/DiscreteTom/rusty-duplication) | Desktop Duplication APIのRustラッパー |

---

## 12. リスクと対策

| リスク | 対策 |
|-------|------|
| ズームが頻繁すぎて目が疲れる | 間引きルールの閾値調整、min_segment_duration |
| ズームの自動判定が的外れ | フォールバック: クリック位置のみベース |
| 中間H.264のデコードが遅い | GPUデコード(DXVA2)活用 |
| FFmpegバンドルでアプリサイズ増大 | 必要最小限のビルド (~15MB) |
| UI Automationが取得不可なアプリ | フォーカス検出なしでもクリックベースで動作 |
| 長時間録画のディスク消費 | 中間圧縮 + 録画上限設定(デフォルト5分) |
