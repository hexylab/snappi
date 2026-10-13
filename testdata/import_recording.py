#!/usr/bin/env python3
"""Windows 実機で録画したデータを評価シナリオとして testdata/recordings/ に取り込む (Issue #28)。

    python testdata/import_recording.py <録画ディレクトリ | 録画ID | latest> <シナリオ名>

- 録画ディレクトリから小さいファイル（meta.json / events.jsonl / ui_events.jsonl /
  window_rects.jsonl / frame_timestamps.txt）だけをコピーする。frames/ と audio.wav、
  ズーム計算に使わない window_events.jsonl（ウィンドウタイトルを含む）はコピーしない。
- meta.json の recording_dir（ユーザー名を含むローカルパス）は空にする。
- `snappi-eval --draft-expect` で expect.json の下書きを作る（description は既存のものを引き継ぐ）。
- 取り込み後に、コミット前に確認すべき UI 要素名の一覧とメトリクスを表示する。

録画IDだけを指定した場合と latest は %USERPROFILE%\\Videos\\Snappi\\recordings から探す。
"""

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT_DIR = ROOT / "testdata" / "recordings"
MANIFEST = ROOT / "src-tauri" / "Cargo.toml"
RECORDINGS = Path(os.environ.get("USERPROFILE", Path.home())) / "Videos" / "Snappi" / "recordings"
COPY_FILES = ["events.jsonl", "ui_events.jsonl", "window_rects.jsonl", "frame_timestamps.txt"]
# コピー先に残っていると取り込んだ録画と食い違うファイル（合成データの残り等）
DATA_FILES = ["meta.json", *COPY_FILES]


def resolve_source(arg):
    if arg == "latest":
        dirs = [d for d in RECORDINGS.iterdir() if (d / "meta.json").exists()]
        if not dirs:
            sys.exit(f"録画が見つかりません: {RECORDINGS}")
        return max(dirs, key=lambda d: d.stat().st_mtime)
    path = Path(arg)
    if not path.is_dir():
        path = RECORDINGS / arg
    if not (path / "meta.json").exists():
        sys.exit(f"録画ディレクトリではありません: {arg}")
    return path


def snappi_eval(*args):
    cmd = ["cargo", "run", "-q", "--manifest-path", str(MANIFEST), "--bin", "snappi-eval", "--", *args]
    return subprocess.run(cmd, check=True, capture_output=True, text=True, encoding="utf-8").stdout


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    src = resolve_source(sys.argv[1])
    name = sys.argv[2]
    dest = OUT_DIR / name
    dest.mkdir(parents=True, exist_ok=True)

    for fname in DATA_FILES:
        (dest / fname).unlink(missing_ok=True)
    meta = json.loads((src / "meta.json").read_text(encoding="utf-8"))
    meta["recording_dir"] = ""
    (dest / "meta.json").write_text(json.dumps(meta, indent=2, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")
    for fname in COPY_FILES:
        if (src / fname).exists():
            shutil.copyfile(src / fname, dest / fname)
    print(f"{src} -> {dest}")
    print(f"  {meta['screen_width']}x{meta['screen_height']} / {meta['duration_ms'] / 1000:.1f}秒 / "
          f"モード {meta.get('recording_mode') or 'display'}")

    draft = snappi_eval(str(dest), "--draft-expect")
    (dest / "expect.json").write_text(draft, encoding="utf-8", newline="\n")
    print(f"expect.json の下書きを書き出しました（チェック {len(json.loads(draft)['checks'])} 件）")

    ui_events = dest / "ui_events.jsonl"
    if ui_events.exists():
        names = sorted({json.loads(line).get("name", "") for line in ui_events.read_text(encoding="utf-8").splitlines()
                        if line.strip()} - {""})
        if names:
            print("\nui_events.jsonl に含まれる UI 要素名（公開して問題ないか確認すること）:")
            for n in names:
                print(f"  - {n}")

    print()
    print(snappi_eval(str(dest), "--metrics"))


if __name__ == "__main__":
    main()
