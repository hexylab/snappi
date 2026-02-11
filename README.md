<div align="center">

# Snappi

**Windows向け 自動エフェクト付き画面録画アプリ**

録画して、停止して、書き出す。たった3ステップで、プロ品質のスクリーンキャストが完成します。

[![Release](https://img.shields.io/github/v/release/hexylab/snappi?style=flat-square&color=8b5cf6)](https://github.com/hexylab/snappi/releases/latest)
[![License](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)
[![Windows](https://img.shields.io/badge/platform-Windows%2010%2F11-0078D4?style=flat-square&logo=windows)](https://github.com/hexylab/snappi/releases/latest)
[![Tauri](https://img.shields.io/badge/Tauri-v2-FFC131?style=flat-square&logo=tauri&logoColor=white)](https://v2.tauri.app)

[**ダウンロード**](https://github.com/hexylab/snappi/releases/latest) ·
[不具合報告](https://github.com/hexylab/snappi/issues)

</div>

---

## どんなアプリ？

Snappi は、macOS の [Screen Studio](https://www.screen.studio/) にインスパイアされた Windows 向けの画面録画ツールです。

普通に画面を録画するだけで、マウス操作に追従する**自動ズーム＆パン**エフェクトがかかった、見やすいスクリーンキャストを書き出せます。チュートリアル動画、バグ報告、デモ動画の作成に最適です。

### 主な特徴

- **自動ズーム＆パン** — マウス操作を解析し、注目すべきポイントに自動でカメラが寄る
- **シーン自動分割** — 操作の流れを賢くシーンに分割し、自然な映像遷移を実現
- **Spring物理アニメーション** — 機械的でないなめらかなカメラワーク
- **タイムライン編集** — 自動生成されたズーム区間を手動で微調整可能
- **フルスクリーン / ウィンドウ選択** — 録画範囲を柔軟に指定
- **複数フォーマット対応** — MP4 / GIF / WebM で書き出し
- **システム音声録音** — WASAPI loopback による高品質な音声キャプチャ
- **軽量＆ネイティブ** — Tauri v2 + Rust でメモリ使用量が小さく高速

## インストール

[**Releases ページ**](https://github.com/hexylab/snappi/releases/latest)からインストーラーをダウンロードしてください。

| ファイル | 形式 | 備考 |
|---------|------|------|
| `snappi_x.x.x_x64-setup.exe` | NSIS | インストーラー（推奨） |
| `snappi_x.x.x_x64_en-US.msi` | MSI | グループポリシー配布向け |

> **動作要件**: Windows 10 / 11 (x64)

## 使い方

```
1. 録画モードを選択（フルスクリーン or ウィンドウ）
2. 録画開始 → 通常通り操作する
3. 録画停止 → プレビュー画面で確認・ズーム区間を調整
4. 書き出し → MP4 / GIF / WebM を選んでエクスポート
```

ズーム区間はタイムライン上でドラッグして範囲を変えたり、倍率を調整したり、新しい区間を追加・削除できます。

## 技術スタック

| レイヤー | 技術 |
|---------|------|
| フレームワーク | [Tauri v2](https://v2.tauri.app) |
| フロントエンド | [SolidJS](https://www.solidjs.com/) + [TailwindCSS v4](https://tailwindcss.com/) |
| バックエンド | Rust |
| 画面キャプチャ | Windows GDI / PrintWindow API |
| 音声キャプチャ | [cpal](https://github.com/RustAudio/cpal) (WASAPI loopback) |
| エンコード | FFmpeg |
| 物理演算 | 自前の Spring アニメーションエンジン |

## ビルド（開発者向け）

### 前提条件

- [Node.js](https://nodejs.org/) >= 18
- [Rust](https://rustup.rs/) >= 1.75
- [Tauri v2 CLI](https://v2.tauri.app/start/prerequisites/)
- FFmpeg バイナリ（`src-tauri/ffmpeg/ffmpeg.exe` に配置）

### 開発サーバー

```bash
npm install
npm run tauri dev
```

### リリースビルド

```bash
npx tauri build
```

`src-tauri/target/release/bundle/` にインストーラーが生成されます。

## ライセンス

[MIT](LICENSE)

