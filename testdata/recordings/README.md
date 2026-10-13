# カメラ評価用の録画データ (Issue #28)

各ディレクトリが1つの評価シナリオ。仕様とシナリオの一覧は `docs/camera-spec-v5.md` を参照。

- 録画ディレクトリと同じ形式（`meta.json` / `events.jsonl` / `ui_events.jsonl` / `window_rects.jsonl` / `frame_timestamps.txt`）。
  **`frames/` と `audio.wav` はコミットしない**（`--debug-video` で目視確認するときは手元の録画を使う）。
- `expect.json` にシナリオ固有の期待値を書く。
- 現在のデータは `../gen_synthetic.py` で生成した合成データ（`expect.json` の `"synthetic": true`）。
  Windows 実機で録画したデータに置き換えたら、`"synthetic"` を外し、`gen_synthetic.py` の `SCENARIOS` からそのシナリオを削除する。
