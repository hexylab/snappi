# Generated TypeScript Types

このディレクトリ内のファイルは ts-rs が Rust の型から自動生成する。フロントエンドと受け渡す型はすべてここから import する。

**手動で編集しないこと** - 変更は再生成で失われる。

## 再生成

```bash
cd src-tauri
cargo test --features ts-export --lib
```

## 対象型

`#[cfg_attr(feature = "ts-export", ts(export))]` が付いた型が対象:

- `src-tauri/src/config/mod.rs`: AppSettings, RecordingSettings, StyleSettings, EffectsSettings, OutputSettings,
  RecordingMode, WindowInfo, BackgroundConfig, ExportFormat, QualityPreset, RecordingInfo, RecordingState,
  ExportProgress, TimelineEvent
- `src-tauri/src/camera/`: ZoomSegment, FocusMode, ZoomStrength, CameraState, CameraTrack
- `src-tauri/src/project.rs`: Project, Trim
- `src-tauri/src/export/preview.rs`: PreviewData, PreviewStyle, TimedPoint, CursorShapeChange, KeyBadge, CursorSprite
- `src-tauri/src/engine/compositor.rs`: CursorKind
