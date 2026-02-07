# Snappi 自動エフェクト仕様書

**Status**: 実装済み（現行仕様）
**Last Updated**: 2026-02-07

---

## 概要

Snappiの自動エフェクトエンジンは、録画中に収集したスクリーンキャプチャ・入力イベント・ウインドウフォーカス情報を元に、エクスポート時に自動でズーム・カーソル・視覚エフェクトを適用する。ユーザーの操作は不要で、録画→停止→エクスポートの3ステップで完成する。

---

## 1. パイプライン全体像

```
┌─────────────────────────────────────────────────────────┐
│  録画フェーズ（3スレッド並行）                             │
│  ├─ capture.rs   : 画面キャプチャ → frame_XXXXXXXX.png   │
│  ├─ events.rs    : 入力イベント   → events.jsonl         │
│  └─ focus.rs     : ウインドウ変化 → window_events.jsonl  │
└──────────────┬──────────────────────────────────────────┘
               │
               v
┌─────────────────────────────────────────────────────────┐
│  前処理 (preprocessor.rs)                                │
│  ├─ マウス移動の間引き（3px未満除去）                      │
│  └─ ドラッグ検出（Click→20px移動→Release）                │
└──────────────┬──────────────────────────────────────────┘
               │
               v
┌─────────────────────────────────────────────────────────┐
│  イベント解析 (analyzer.rs)                               │
│  ├─ フェーズ1: アイドル区間検出（Short/Medium/Long）       │
│  ├─ フェーズ2: アクションセグメント分類                    │
│  └─ ウインドウコンテキスト付与                             │
└──────────────┬──────────────────────────────────────────┘
               │
               v
┌─────────────────────────────────────────────────────────┐
│  ズーム計画 (zoom_planner.rs)                             │
│  ├─ セグメントからキーフレーム生成                         │
│  ├─ 2段階ズーム（ウインドウ→アクション点）                 │
│  ├─ 重複除去・カット検出                                  │
│  └─ セグメント別SpringHint付与                            │
└──────────────┬──────────────────────────────────────────┘
               │
               v
┌─────────────────────────────────────────────────────────┐
│  フレーム合成 (compositor.rs) — 1フレームごと              │
│  ├─ キーフレーム適用 → スプリングアニメーション             │
│  ├─ デッドゾーンカーソル追従                               │
│  ├─ クロップ＆スケール                                    │
│  ├─ カーソル・クリックリング・キーバッジ描画                 │
│  └─ 角丸・影・背景合成                                    │
└──────────────┬──────────────────────────────────────────┘
               │
               v
┌─────────────────────────────────────────────────────────┐
│  エクスポート (encoder.rs)                                │
│  └─ BMP連番 → FFmpeg → MP4 / GIF / WebM                 │
└─────────────────────────────────────────────────────────┘
```

---

## 2. 録画フェーズ

録画中は3つのスレッドが独立してデータを収集する。エフェクト処理は一切行わない。

### 2.1 画面キャプチャ (`recording/capture.rs`)

- Windows GDI `BitBlt` による画面キャプチャ
- 指定FPS（デフォルト30fps）でフレームをPNGとして保存
- 出力: `frame_XXXXXXXX.png`（連番）、`frame_count.txt`、`dimensions.txt`
- 32bit BGRA → RGBA 変換

### 2.2 入力イベント (`recording/events.rs`)

`rdev` クレートによるグローバルフックで以下のイベントを `events.jsonl` に記録:

| イベント | 内容 | サンプリング |
|---------|------|------------|
| `mouse_move` | カーソル座標 (x, y) | 10ms間隔で間引き |
| `click` | ボタン名, 座標 | 全件記録 |
| `click_release` | ボタン名, 座標 | 全件記録 |
| `key` | キー名, 修飾キー配列 | 全件記録 |
| `scroll` | スクロール量 (dx, dy) | 全件記録 |

**修飾キー追跡**: ビットフラグ（Ctrl/Shift/Alt/Meta）でリアルタイム追跡。Keyイベント発行時に現在の修飾キー状態を付与。左右の区別は正規化（LShift/RShift → Shift）。

### 2.3 ウインドウフォーカス (`recording/focus.rs`)

- Windows APIの `GetForegroundWindow()` を100msごとにポーリング
- HWND変更を検知したら `GetWindowTextW()` と `GetWindowRect()` を取得
- 出力: `window_events.jsonl`（events.jsonlとは別ファイル、スレッド間の競合回避）
- 空タイトルウインドウ（デスクトップ等）はスキップ

```jsonl
{"type":"window_focus","t":1250,"title":"Visual Studio Code","rect":[100,50,1820,1030]}
```

### 2.4 メタデータ (`meta.json`)

録画停止時に生成:

```json
{
  "version": 1,
  "id": "uuid",
  "screen_width": 1920,
  "screen_height": 1080,
  "fps": 30,
  "start_time": "2026-02-07T15:30:00+09:00",
  "duration_ms": 12500,
  "has_audio": true,
  "monitor_scale": 1.0,
  "recording_dir": "C:/Users/.../recordings/uuid"
}
```

`has_audio`は `audio.wav` のファイルサイズが44バイト超（WAVヘッダーのみでない）で判定。

---

## 3. 前処理 (`engine/preprocessor.rs`)

エクスポート時に実行。録画データは変更しない（生データ保持）。

### 3.1 マウス移動の間引き

- 移動距離3px未満の `MouseMove` を除去
- ただし、Click/Key/Scrollの前後100msのMouseMoveは保持（精度維持）
- 200ms以上間隔が空いた地点は停止位置として1件保持

### 3.2 ドラッグ検出

- `Click → MouseMove（累積20px以上）→ ClickRelease` パターンを検出
- ClickReleaseがない場合は50px以上の移動で近似
- `DragEvent`（start/end座標・時間）として返却

---

## 4. イベント解析 (`engine/analyzer.rs`)

間引き済みイベント列を意味的なセグメントに分割する。2フェーズで処理。

### 4.1 フェーズ1: アイドル検出

**重要イベント**（Click, Key, Scroll, WindowFocus, ClickRelease）のみを対象に間隔を計測。MouseMoveは無視（10msごとの記録でアイドルが検出できなくなるため）。

| アイドルレベル | 間隔 | ズーム動作 |
|--------------|------|----------|
| `Short` | 800ms〜2秒 | 何もしない（現在のズーム維持） |
| `Medium` | 2秒〜5秒 | 1.2xまで部分ズームアウト |
| `Long` | 5秒以上 | 1.0x（等倍）まで完全ズームアウト |

### 4.2 フェーズ2: アクションセグメント分類

| セグメント | 検出条件 | フォーカスポイント |
|-----------|---------|-----------------|
| `Click` | 単発クリック（前後200ms以内に次のクリックなし） | クリック座標 |
| `TextInput` | Click後500ms以内にKeyが続き、Key同士が300ms以内 | 最初のクリック座標 |
| `Scroll` | Scrollイベントが300ms以内に連続 | 画面中央 |
| `RapidAction` | 200ms以内に3回以上クリック | クリック座標の平均 |

### 4.3 ウインドウコンテキスト

各セグメントに以下を付与:

- `window_rect: Option<Rect>` — セグメント生成時のアクティブウインドウ矩形
- `window_changed: bool` — WindowFocusイベントから500ms以内のアクションか

古い録画（WindowFocusイベントなし）では `window_rect: None`, `window_changed: false` となり、従来通りの単段ズームで動作する（後方互換性）。

---

## 5. ズーム計画 (`engine/zoom_planner.rs`)

セグメント列からカメラ動作のキーフレーム列を生成する。

### 5.1 キーフレーム構造

```rust
struct ZoomKeyframe {
    time_ms: u64,           // キーフレーム発動時刻
    target_x: f64,          // カメラ中心X
    target_y: f64,          // カメラ中心Y
    zoom_level: f64,        // ズーム倍率 (1.0=等倍)
    transition: TransitionType,
    spring_hint: Option<SpringHint>,  // スプリング速度ヒント
}
```

### 5.2 セグメント別ズームルール

| セグメント | ズーム倍率 | トランジション | SpringHint (zoom / pan) |
|-----------|-----------|--------------|------------------------|
| Click | `default_zoom` (例: 2.0x) | SpringIn | 0.12s / 0.15s |
| TextInput | `text_input_zoom` or フィット計算 | SpringIn | 0.20s / 0.15s |
| Scroll | 1.2x | Smooth | 0.28s / 0.15s |
| RapidAction | 1.8x | SpringIn | 0.12s / 0.15s |
| Idle (Medium) | 1.2x | SpringOut | 0.25s / 0.15s |
| Idle (Long) | 1.0x | SpringOut | 0.35s / 0.15s |

### 5.3 2段階ズーム（ウインドウ対応）

`window_changed == true` のセグメントでは2段階のズームを生成:

```
段階1 (アクション400ms前): ウインドウ全体にフィットするズーム
  → TransitionType::WindowZoom
  → SpringHint: zoom=0.25s, pan=0.20s
  → zoom_level = calc_zoom_to_fit(window_rect)  ※最低1.2x, max_zoom上限

段階2 (アクション時刻): アクション点へのズーム
  → 通常のセグメント別トランジション
```

例:
```
t=500ms  ウインドウ切替発生
t=600ms  WindowZoom → ウインドウ全体表示 (1.5x, center=ウインドウ中心)
t=1000ms SpringIn   → クリック位置ズーム (2.0x, center=クリック座標)
```

### 5.4 後処理

**重複除去** (`deduplicate_keyframes`):
- 300ms以内に複数キーフレーム → 後のものを残す
- 同一ズーム倍率（差0.01未満）の連続 → 後のものを除去
- ズームイン直後（200ms以内）のズームアウト → ズームインを除去

**カット検出** (`detect_cuts`):
- 前後キーフレームのパン距離が画面対角50%以上 → `TransitionType::Cut`（スプリングなし、瞬時切替）

---

## 6. スプリングアニメーション (`engine/spring.rs`)

すべてのアニメーション（ズーム、パン、カーソル追従）に使用する共通物理エンジン。

### 6.1 臨界減衰スプリング（解析解）

フレームレート非依存、無条件安定の解析解を使用:

```
damping = (4 * ln(2)) / half_life
position(t+dt) = e^(-d*dt) * (j0 + j1*dt) + target
velocity(t+dt) = e^(-d*dt) * (v - j1*d*dt)
```

パラメータは **half-life**（バネが残距離の50%を進む時間、秒）で指定。直感的で調整しやすい。

### 6.2 half-life定数

| 定数名 | 値 | 95%到達時間 | 用途 |
|--------|-----|-----------|------|
| `VIEWPORT_PAN` | 0.15s | ~0.6s | カーソル追従パン |
| `WINDOW_PAN` | 0.20s | ~0.9s | ウインドウ表示用パン |
| `ZOOM_IN_FAST` | 0.12s | ~0.5s | クリック・RapidAction |
| `ZOOM_IN` | 0.20s | ~0.9s | テキスト入力 |
| `ZOOM_IN_SLOW` | 0.28s | ~1.2s | スクロール |
| `ZOOM_OUT` | 0.25s | ~1.1s | 中程度アイドル |
| `ZOOM_OUT_SLOW` | 0.35s | ~1.5s | 長時間アイドル |
| `WINDOW_ZOOM` | 0.25s | ~1.1s | ウインドウレベルズーム |
| `CURSOR_SMOOTHING` | 0.05s | ~0.2s | カーソルジッター除去 |

### 6.3 AnimatedViewport

3つのスプリング（center_x, center_y, zoom）でビューポートを管理:

- `set_target(x, y, zoom)` — デフォルトhalf-lifeでターゲット設定
- `set_target_with_half_life(x, y, zoom, zoom_hl, pan_hl)` — キーフレーム別の速度で設定
- `snap_to(x, y, zoom)` — 瞬時移動（Cutトランジション用）
- `update(dt)` — dtだけスプリングを進行
- `current_viewport(screen_w, screen_h)` — 現在のクロップ領域を取得

---

## 7. フレーム合成 (`engine/compositor.rs`)

1フレームごとに以下の処理を順に実行する。

### 7.1 カーソル追従（デッドゾーンモデル）

ズーム中（zoom > 1.0x）のみ有効。3層ゾーンモデルでカーソルに滑らかに追従:

```
┌─────────────────────────────────┐
│  Push Zone (>70%): フル追従      │
│  ┌─────────────────────────┐   │
│  │ Soft Zone (30-70%):      │   │
│  │ smoothstep段階的追従      │   │
│  │ ┌───────────────────┐   │   │
│  │ │ Dead Zone (<30%):  │   │   │
│  │ │ 追従なし (安定)     │   │   │
│  │ └───────────────────┘   │   │
│  └─────────────────────────┘   │
└─────────────────────────────────┘
```

- Dead Zone (30%): カーソルが中心近くでは追従しない。テキスト入力時の安定性を確保
- Soft Zone (30-70%): `smoothstep` で追従強度が連続的に増加
- Push Zone (>70%): フル追従。カーソルがビューポート外に出ることを防止

### 7.2 クロップ＆スケール

ビューポート領域を元フレームから切り出し、出力サイズにリサイズ。Triangleフィルタ（バイリニア補間）で高速処理。画面端でのクランプあり。

### 7.3 カーソル描画

**システムカーソル取得** (Windows):
1. `LoadCursorW(None, IDC_ARROW)` → HCURSOR
2. `CopyIcon(cursor)` → HICON
3. `GetIconInfo(icon)` → ICONINFO（hbmColor, hbmMask, xHotspot, yHotspot）
4. `CreateCompatibleDC` + `GetDIBits(hbmColor)` → BGRA ピクセルデータ
5. BGRA→RGBA変換、alpha=0の場合はマスクから透過情報取得
6. GDIオブジェクトのクリーンアップ

取得失敗時はSDF生成のフォールバックカーソル（macOS風矢印、アンチエイリアス付き影）を使用。

描画時はホットスポット座標を基準に配置し、ズーム倍率に応じてスケーリング。

### 7.4 クリックリング

クリック位置に拡大するリングアニメーション:
- 半径: 0 → `max_radius` に拡大（ease-out cubic）
- アルファ: フェードアウト（1.0 → 0.0）
- ストローク: 指定太さの円弧
- 内側: ストロークの15%アルファで薄く塗りつぶし
- 持続時間: 設定値（デフォルト400ms）

### 7.5 キーバッジ

修飾キー付きキー操作や特殊キーを画面下部中央に表示:
- 暗い半透明の角丸矩形（`rgba(0,0,0,200)`）
- フォーマット: `Ctrl+C`, `Shift+Tab`, `Return` 等
- 表示対象:
  - 修飾キー＋通常キーの組み合わせ（Ctrl+C, Alt+F4 等）
  - 特殊キー単体（Return, Tab, Escape, Backspace, Delete, F1-F12, Space, 矢印キー等）
- 表示期間: 設定値（デフォルト1500ms）

### 7.6 角丸

サブピクセルアンチエイリアス付きの角丸マスク:
```
alpha = clamp(1.0 - (distance - radius), 0.0, 1.0)
```

### 7.7 影＆背景

1. **背景生成**: グラデーション / 単色 / 透明（初回のみ生成、以降キャッシュ再利用）
2. **ドロップシャドウ**: 角丸矩形のシャドウ（ブラー付き、オフセットあり）
3. **合成**: 背景 → シャドウ → 映像フレームの順に合成

---

## 8. カーソルスムージング (`engine/cursor_smoother.rs`)

生のマウス軌跡の手ブレを除去するスプリングベースフィルタ:

- 入力: `Vec<(timestamp_ms, x, y)>`
- 各点間で実タイムスタンプからdtを計算（フレームレート非依存）
- half-life: `CURSOR_SMOOTHING` (0.05s) で追従性が高く、微ジッターのみ除去
- 出力: 同サイズの滑らかな座標列

---

## 9. エクスポート (`export/encoder.rs`)

### 9.1 処理フロー

1. `meta.json` 読み込み
2. `events.jsonl` + `window_events.jsonl` 読み込み、タイムスタンプ順にマージ
3. 前処理（マウス間引き）
4. イベント解析 → セグメント列
5. ズーム計画生成 → キーフレーム列（auto_zoom有効時のみ）
6. カーソルスムージング（有効時のみ）
7. クリックエフェクト・キーオーバーレイ抽出
8. Compositor初期化（システムカーソル取得含む）
9. フレームループ:
   - `frame_time_ms = frame_index * (duration_ms / frame_count)`
   - 該当時刻のキーフレームを適用
   - PNGフレーム読み込み → `compose_frame()` → RGBA→RGB変換 → BMP保存
10. FFmpegでBMP連番を動画にエンコード

### 9.2 タイミング計算

フレームタイムスタンプはプリセットFPSではなく **実際の録画時間** に基づく:
```
frame_step_ms = meta.duration_ms / frame_count
actual_fps = frame_count * 1000 / meta.duration_ms
dt = 1.0 / actual_fps
```

### 9.3 出力フォーマット

| フォーマット | コーデック | 音声 | 備考 |
|------------|----------|------|------|
| MP4 | H.264 (libx264), CRF指定, medium preset | AAC | `-movflags +faststart` |
| GIF | palette生成 → paletteuse | なし | 15fps, Lanczosスケーリング |
| WebM | VP9 (libvpx-vp9), CRF指定 | Opus | `-b:v 0` |

音声は `audio.wav` が有効な場合のみ含む。`-shortest` フラグで映像と音声の長さの差を吸収。

---

## 10. 設定項目

### 10.1 エフェクト設定 (`EffectsSettings`)

| 設定 | デフォルト | 説明 |
|------|----------|------|
| `auto_zoom_enabled` | `true` | 自動ズームON/OFF |
| `default_zoom_level` | `2.0` | クリック時のズーム倍率 |
| `text_input_zoom_level` | `2.5` | テキスト入力時のズーム倍率 |
| `max_zoom` | `3.0` | ズーム倍率の上限 |
| `click_ring_enabled` | `true` | クリックリングON/OFF |
| `key_badge_enabled` | `true` | キーバッジON/OFF |
| `cursor_smoothing` | `true` | カーソルスムージングON/OFF |

### 10.2 出力スタイル (`OutputStyle`)

プリセットで定義される出力外観設定:
- キャンバスサイズ、出力サイズ、ボーダー半径
- シャドウ（ブラー、オフセット、色）
- 背景（グラデーション/単色/透明）
- クリックリング（色、持続時間、最大半径、線幅）
- キーバッジ持続時間
- カーソルサイズ倍率

---

## 11. ファイル構成

```
src-tauri/src/
├── recording/                  # 録画フェーズ
│   ├── session.rs              # 録画セッション管理（スレッド起動）
│   ├── capture.rs              # GDI BitBlt画面キャプチャ
│   ├── events.rs               # rdevによる入力イベント収集
│   ├── focus.rs                # GetForegroundWindowポーリング
│   └── audio.rs                # WASAPI音声キャプチャ
│
├── engine/                     # 自動エフェクトエンジン
│   ├── preprocessor.rs         # マウス間引き・ドラッグ検出
│   ├── analyzer.rs             # セマンティックセグメント分割
│   ├── zoom_planner.rs         # ズームキーフレーム生成
│   ├── spring.rs               # 臨界減衰スプリング（解析解）
│   ├── compositor.rs           # フレーム合成メインループ
│   ├── cursor_smoother.rs      # カーソル軌跡スムージング
│   └── effects/
│       ├── background.rs       # 背景・グラデーション生成
│       ├── click_ring.rs       # クリックリング描画
│       ├── cursor.rs           # カーソル描画ユーティリティ
│       ├── key_badge.rs        # キーバッジ描画
│       └── viewport.rs         # ビューポート管理
│
├── export/                     # エクスポート
│   ├── encoder.rs              # FFmpegエンコードオーケストレーター
│   └── presets.rs              # 品質プリセット定義
│
└── config/                     # 設定
    ├── mod.rs                  # RecordingEvent, RecordingMeta, AppSettings
    └── defaults.rs             # デフォルト値定義
```

---

## 12. 録画データ構造

1録画分のディレクトリ:

```
recordings/{uuid}/
├── meta.json                   # 録画メタデータ
├── events.jsonl                # 入力イベント（mouse_move, click, key, scroll等）
├── window_events.jsonl         # ウインドウフォーカス変化
├── frame_00000000.png          # キャプチャフレーム（連番）
├── frame_00000001.png
├── ...
├── frame_count.txt             # 総フレーム数
├── dimensions.txt              # 画面解像度 (例: "1920x1080")
└── audio.wav                   # 音声データ（オプション）
```

---

## 13. 後方互換性

- `window_events.jsonl` がない録画: ウインドウコンテキストなしで動作。全セグメントで `window_rect: None`, `window_changed: false`。単段ズームのみ（従来動作と同じ）。
- `click_release` イベントがない録画: ドラッグ検出は50px移動の近似で対応。
- `spring_hint` がないキーフレーム: デフォルトhalf-lifeでスプリング動作。

---

## 14. 今後の改善候補

- **ドラッグ対応ズーム**: ドラッグ操作をセグメントとして検出し、ドラッグ範囲にフィットするズーム
- **スプリングプリセット**: Slow / Mellow / Quick / Rapid の4段階ユーザー選択
- **不足減衰スプリング**: Quick/Rapidプリセットで微小オーバーシュート（弾力感）
- **双方向カーソルスムージング**: 前方→後方パスの平均で位相遅延ゼロのフィルタ
- **ブリッジパターン**: 中距離移動時に「ズームアウト→パン→ズームイン」
- **モーションブラー**: カーソル、パン、ズームのモーションブラー
- **並列フレーム処理**: rayonによるフレーム合成の並列化
