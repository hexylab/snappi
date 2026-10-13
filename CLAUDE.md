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

### カメラの評価 (Issue #28)

カメラ（ズーム・パン）の仕様は `docs/camera-spec-v5.md`。評価用データは `testdata/recordings/`。

```bash
cd src-tauri
# メトリクス M1〜M7 と expect.json の判定を表示
cargo run --bin snappi-eval -- ../testdata/recordings/vscode-typing --metrics
# frames がある録画に、ビューポート・カーソル・イベント・区間を描き込んだ動画を出力（ffmpeg が必要。重いので --release 推奨）
cargo run --release --bin snappi-eval -- <recording_dir> --debug-video out.mp4
# 全シナリオに仕様を適用（新エンジン #29 完成までは ignore）
cargo test --test camera_eval -- --ignored --nocapture
```

- 評価するエンジンは `eval::compute_track` で切り替える（現在は旧エンジン `legacy_camera_track`）。
- 仕様パラメータは `camera/spec.rs`、メトリクスは `eval/metrics.rs`。値を変えたら仕様書も更新する。
- `src-tauri/Cargo.toml` に `default-run = "snappi"` があるため、`cargo run` は従来どおりアプリを起動する。

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

[エクスポート] Analyzer → Segments → ZoomPlanner → Keyframes
              → Compositor(Spring物理) → FFmpeg → MP4/GIF/WebM
```

### スレッドモデル

録画中は3スレッドが並行動作。`Arc<AtomicBool>`でpause/stopを制御:
- **capture**: Windows GDI BitBltで画面キャプチャ → PNG保存
- **events**: rdevでグローバルマウス/キーフック → JSONL
- **audio**: cpal WASAPI loopbackでシステム音声 → WAV

### IPC構造

```
SolidJS (src/lib/commands.ts)
  → invoke("command_name")
  → Tauri IPC
  → Rust (src-tauri/src/commands.rs の #[tauri::command])
  → AppState (Mutex<RecordingState>, Mutex<Option<RecordingSession>>)
```

### フロントエンドのページ管理

ルーターは使わず `createSignal<"list"|"preview"|"settings">` でページ切り替え（`App.tsx`）。

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
├── window_events.jsonl   # 前面ウィンドウの変化（ズーム計算には未使用）
├── window_rects.jsonl    # ウィンドウ録画時のみ。矩形が変化したら {"t","rect":[l,t,r,b]}
├── audio.wav
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
`ExportFormat`: `Mp4 | Gif | WebM`
`QualityPreset`: `Social(1080p/30fps) | HighQuality(元解像度/60fps) | Lightweight(720p/24fps)`
