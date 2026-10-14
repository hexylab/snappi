# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 言語ルール

このプロジェクトでは**日本語**でコミュニケーションを行うこと。コミットメッセージ、コメント、ユーザーへの応答はすべて日本語で記述する。

## プロジェクト概要

Snappi: Windows向けScreen Studio代替の画面録画アプリ。Tauri v2 + SolidJS + Rust構成。
「録画→停止→エクスポート」の3ステップで、自動エフェクト付きの動画を生成する。
設計書: `docs/screen-recorder-architecture-v2.md`

## ビルド・開発コマンド

```bash
# 開発（Vite + Tauri同時起動）
npm run tauri dev

# 本番ビルド（フロントエンド埋め込み + インストーラー生成）
# ※ cargo build --release ではフロントエンドが埋め込まれないため必ず tauri build を使う
npx tauri build

# フロントエンドのみビルド
npm run build

# Rustのみビルド確認
cargo build --manifest-path src-tauri/Cargo.toml
```

### カメラの評価 (Issue #28, #29)

カメラ（ズーム・パン）の仕様は `docs/camera-spec-v6.md`（Issue #39。編集者の考え方に基づく「主役」と「ショット」）。評価用データは `testdata/recordings/`。

```bash
cd src-tauri
# メトリクス M1〜M7 と expect.json の判定を表示
cargo run --bin snappi-eval -- ../testdata/recordings/vscode-typing --metrics
# frames がある録画に、ビューポート・カーソル・イベント・区間を描き込んだ動画を出力（ffmpeg が必要。重いので --release 推奨）
cargo run --release --bin snappi-eval -- <recording_dir> --debug-video out.mp4
# 全シナリオに仕様を適用（cargo test にも含まれる）
cargo test --test camera_eval -- --nocapture
# 全シナリオのメトリクスを Markdown の表で出力（仕様書 §11 の更新用）
cargo run -q --bin snappi-eval -- ../testdata/recordings --table
# frames から画面の変化領域 changes.jsonl を作り直す
cargo run --release --bin snappi-eval -- <recording_dir> --changes
# 録画の実効フレームレートを計測（実画面を 3 秒キャプチャ）
cargo test --lib capture_rate -- --ignored --nocapture
```

- 実録画を評価シナリオに取り込むときは `python testdata/import_recording.py latest <シナリオ名>`
  （frames/audio を除いてコピーし、`--draft-expect` で expect.json の下書きを作る）。手順は `testdata/recordings/README.md`。

- 評価は `eval::compute_track`（既定設定のズーム強さで `camera::auto_segments` → `camera::camera_track`）。
- 主役は `camera/subject.rs`（操作の直後の画面の変化 = `changes.jsonl`）、ショットは `camera/segments.rs`。
  マウスカーソルの位置は主役に使わない。アプリ向けの分岐は持たない。
- `expect.json` の `framed`（主役が枠内）・`within`（枠がウィンドウ内）には、録画フレームから人が読み取った
  「編集者ならこう映す」を書く（エンジンの出力から書き写さない）。
- 仕様パラメータは `camera/spec.rs`、メトリクスは `eval/metrics.rs`。値を変えたら仕様書も更新する。
- `src-tauri/Cargo.toml` に `default-run = "snappi"` があるため、`cargo run` は従来どおりアプリを起動する。

### 個人情報・秘密情報のチェック

`.githooks/` の pre-commit / pre-push が、コミットのメールアドレス（GitHub の noreply 以外は拒否）と
変更内容（`gitleaks`、ルールは `.gitleaks.toml`）を確かめる。CI でも同じチェックを行う。詳細は `.githooks/README.md`。

- 準備: `winget install gitleaks` と `npm install`（`core.hooksPath` を設定する）
- 個人に固有の文字列は `.gitleaks.toml` に書かず、コミットされない `.git/gitleaks.local.toml` に書く
- 例示用のパスは `C:\Users\...\` のように書く

**ビルド時の注意**: Windows環境で `RC.EXE` が見つからないエラーが出た場合、Windows SDK のパスを通す必要がある:
```bash
set "PATH=C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64;%PATH%"
```

## アーキテクチャ

### 2パス方式

録画フェーズ（データ収集のみ）とレンダリングフェーズ（エフェクト適用）を分離:

```
[録画] Screen Capture → frames/*.png
       Mouse/Key Events → events.jsonl
       Audio → audio.wav
       Metadata → meta.json, dimensions.txt

       停止時に frames の差分 → changes.jsonl（画面の変化領域）

[エクスポート] project.json の ZoomSegment 列とトリム（無ければ camera::auto_segments で自動生成・project::auto_trim）
              → camera::camera_track（出力フレームごとの CameraState）
              → Compositor（CameraState のビューポートで切り抜き・カーソル等を合成）→ FFmpeg → MP4/GIF/WebM
```

### UI (Issue #1, #30)

- 見た目は `DESIGN.md`（Hexyl Light）に従う。トークンは `src/styles.css` の `@theme` / `:root` にだけ書き、
  画面からは `var(--…)` か Tailwind のユーティリティ（`bg-surface` など）で読む。アイコンは絵文字、ライトテーマのみ。
- 方針: **自動の結果で完成形に近いこと**を優先し、通常は「録画を開始 → 停止 → 書き出す」だけで済むようにする。
  細かい編集はプレビューの「細かく編集」スイッチ（状態は localStorage）をオンにしたときだけ出す。
- 録画中はメインウィンドウを小さな録画バーにする（`set_recording_bar`・`window_bar.rs`。
  `WDA_EXCLUDEFROMCAPTURE` で録画に映らない）。録画の対象（画面全体／ウィンドウ）と音声（PCの音・マイク）はホームで選ぶ。
- 設定はホームから開くダイアログ（`SettingsDialog.tsx`）。変更はすぐ保存（`lib/settingsStore.ts`）。
- プレビュー: 開くと `load_project`（無ければ自動生成して保存）→ 完成形を自動再生。編集はデバウンスして `save_project`。
  - 通常: 再生とシークバー（両端のつまみで前後をカット）、「動画を書き出す」（設定の形式・画質）、書き出し後に「動画を開く」「フォルダを開く」
  - 細かく編集: タイムライン（ズーム・操作のトラック）、ズームの追加・削除・伸縮、この録画のズームの強さ・自動に戻す
    （`auto_project`）、Undo/Redo、フレーム送り、形式・画質の選択
- `src/lib/previewRenderer.ts` が書き出しと同じ合成（背景・影・角丸・カーソル・クリックリング・キー表示）を
  canvas に描く。スタイル・時系列・カーソル画像は `get_preview_data`（`export/preview.rs`）が
  書き出しと同じ計算（`OutputStyle`・`EffectTimeline`・`Compositor` のスプライト）から返す。
  カメラは `get_camera_track`。合成を変えたら両方を揃えること。

カメラエンジンは `src-tauri/src/camera/`:
- `subject.rs`: 操作ごとの主役の矩形（仕様 S1〜S2）。純粋関数
- `segments.rs`: 主役からショット（`zoom: Some`・`focus: Fixed` の区間）を作る（S3〜S5, R3〜R5）
- `track.rs`: 出力 fps で決定的にシミュレーションしてカメラトラックを求める（F6。`Auto` の区間は v5 の F1〜F4）
- `config.rs`: `ZoomStrength`（設定「ズーム強さ」）から `CameraConfig` を作る
- `spec.rs`: 仕様パラメータ。`engine/` には合成（`compositor.rs`）・バネ・カーソルスムージングだけが残る

### スレッドモデル

録画中は3スレッドが並行動作。`Arc<AtomicBool>`でpause/stopを制御:
- **capture**: Windows GDI BitBltで画面キャプチャ → JPEG保存（変換・保存はワーカースレッド。開発ビルドでも 30fps 以上を保つため `[profile.dev]` は opt-level 1）
- **events**: rdevでグローバルマウス/キーフック → JSONL
- **audio**: cpal で PC の音（WASAPI loopback）とマイクを別々の WAV に録り、停止時に `audio.wav` にまとめる（両方なら FFmpeg でミックス）

### IPC構造

```
SolidJS (src/lib/commands.ts)
  → invoke("command_name")
  → Tauri IPC
  → Rust (src-tauri/src/commands.rs の #[tauri::command])
  → AppState (Mutex<RecordingState>, Mutex<Option<RecordingSession>>)
```

### フロントエンドのページ管理

ルーターは使わず `createSignal<"home"|"preview">` でページ切り替え（`App.tsx`）。録画中は `RecordingBar` だけを出す。設定は `home` 上のダイアログ。

## 重要な技術的制約

- **Tauri v2の`emit()`**: `use tauri::Emitter` トレイトが必要（`Manager`だけでは不足）
- **tauri.conf.json**: `plugins.global-shortcut` は `null` にする（オブジェクトを渡すとパニック）
- **FFmpegコマンド構築**: すべての `-i` 入力を先に指定し、出力コーデックオプション(`-c:v`, `-crf`等)は後に配置する。入力間に出力オプションを置くとデコーダーとして解釈される
- **audio.wav**: 音声キャプチャが失敗してもファイルが空で作成される場合がある。FFmpegに渡す前に `len() > 44`（WAVヘッダーサイズ超）でバリデーションする
- **画面キャプチャ**: `scap` crateはAPI非互換のため使用せず、`windows` crateのGDI BitBltを直接使用
- **バンドルリソース**: `tauri.conf.json`の`bundle.resources`で`ffmpeg/*`を指定しているため、`src-tauri/ffmpeg/`に最低1ファイル必要

## 録画データの保存先

```
%USERPROFILE%\Videos\Snappi\recordings\{uuid}\
├── frames/frame_00000000.jpg, ...   # 旧録画は .png
├── frame_timestamps.txt  # 各フレームを取り込んだ瞬間の録画時刻 (ms)。1行1フレーム
├── events.jsonl          # マウス/キー入力 + cursor_shape イベント（画面座標）
├── ui_events.jsonl       # UI Automation のフォーカス要素矩形（画面座標）
├── window_events.jsonl   # 前面ウィンドウとその矩形の変化（枠をウィンドウ内に収めるのに使う）
├── changes.jsonl         # 画面の変化領域（停止時に frames から作る。`changes.rs`）
├── window_rects.jsonl    # ウィンドウ録画時のみ。矩形が変化したら {"t","rect":[l,t,r,b]}
├── audio.wav
├── project.json          # プレビューの編集内容（区間・トリム。`project::Project`）。書き出しはこれを使う
└── meta.json             # frame_count を含む（dimensions.txt / frame_count.txt は停止時に統合・削除）
```

- タイムスタンプ `t` はすべて共有の `RecordingClock`（一時停止中は進まない）による録画開始からの ms。
- `cursor_shape` イベント: `{"type":"cursor_shape","t":..,"shape":"ibeam"}`。shape は
  `arrow` / `ibeam` / `hand` / `wait` / `size_ns` / `size_we` / `size_nwse` / `size_nesw` / `size_all` / `other`。
- 録画データは必ず `recording_data::RecordingData` 経由で読む。イベント座標はここで
  **キャプチャ領域基準（フレーム左上が原点）** に変換される（ウィンドウ録画は `window_rects.jsonl` の
  各時刻の位置、範囲録画は範囲の左上を使用）。`frame_timestamps.txt` が無い旧録画は等間隔とみなす。
- エクスポートはプリセットの fps で等間隔に出力し、出力時刻 t には「t 以前で最新の元フレーム」を使う。

## 主要な型定義

Rust側の型は `src-tauri/src/config/mod.rs` に集約。TypeScript側は以下の2系統:
- `src/lib/types.ts`: 手動同期（従来、段階的に下記へ移行予定）
- `src/lib/generated/`: ts-rs による自動生成 (Issue #11)
  - 再生成: `cd src-tauri && cargo test --features ts-export --lib`
  - 対象構造体に `#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]` と
    `#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]` を付与
  - 通常ビルドでは ts-rs 依存は無効化されるため実行時コストゼロ

`RecordingState`: `Idle | Recording | Paused | Processing`
`ZoomStrength`: `Off | Subtle(1.5x) | Standard(1.8x, 既定) | Strong(2.2x)`（`EffectsSettings.zoom_strength`）
`ZoomSegment`: `{ id, start_ms, end_ms, zoom: Option<f64>, focus: Auto | Fixed { x, y } }`（`camera::segments`）
`CameraTrack`: `{ fps, frames: Vec<CameraState { center_x, center_y, zoom }> }`（`camera`）
`Project`: `{ version, segments, trim: { start_ms, end_ms }, zoom_strength: Option }`（`project.rs`、project.json。自動生成時のトリムは `auto_trim`）
`ExportFormat`: `Mp4 | Gif | WebM`
`QualityPreset`: `Social(1080p/30fps) | HighQuality(元解像度/60fps) | Lightweight(720p/24fps)`
