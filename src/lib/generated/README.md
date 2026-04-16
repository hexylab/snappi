# Generated TypeScript Types

このディレクトリ内のファイルは `cargo test --features ts-export --manifest-path src-tauri/Cargo.toml --lib` 実行時に ts-rs によって自動生成されます。

**手動で編集しないでください** - 変更は再生成で失われます。

## 再生成

```bash
cd src-tauri
cargo test --features ts-export --lib
```

## 対象型

`src-tauri/src/config/mod.rs` の下記に `#[cfg_attr(feature = "ts-export", ts(export))]` が
付与された構造体が対象:

- RecordingMeta, RecordingInfo, RecordingMode, WindowInfo, TimelineEvent
- ExportProgress, ExportFormat, QualityPreset, RecordingState

## 既存の `src/lib/types.ts` との関係

既存の手書き型定義は `src/lib/types.ts` に残っています。段階的に `generated/` へ
移行する方針で、当面は両方が共存します。新規型の追加時は `generated/` を優先してください。
