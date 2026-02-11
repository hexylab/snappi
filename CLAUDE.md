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
├── frames/frame_00000001.png, ...
├── events.jsonl
├── audio.wav
├── meta.json
├── dimensions.txt
└── frame_count.txt
```

## 主要な型定義

Rust側の型は `src-tauri/src/config/mod.rs` に集約。TypeScript側のミラーは `src/lib/types.ts`。
両者は手動同期のため、Rust側の型を変更したらTypeScript側も必ず更新する。

`RecordingState`: `Idle | Recording | Paused | Processing`
`ExportFormat`: `Mp4 | Gif | WebM`
`QualityPreset`: `Social(1080p/30fps) | HighQuality(元解像度/60fps) | Lightweight(720p/24fps)`
