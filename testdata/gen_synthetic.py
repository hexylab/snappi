#!/usr/bin/env python3
"""合成データの評価シナリオ（expect.json に `"synthetic": true`）を testdata/recordings/ に生成する。

実録画と同じ形式（meta.json / frame_timestamps.txt / events.jsonl / ui_events.jsonl /
window_rects.jsonl / changes.jsonl）で、frames は含まない。expect.json もここで生成する。

    python3 testdata/gen_synthetic.py
"""

import json
import random
from pathlib import Path

OUT_DIR = Path(__file__).resolve().parent / "recordings"
FPS = 30
MOVE_INTERVAL_MS = 16


class Rec:
    def __init__(self, width=1920, height=1080, start_pos=(1500, 900)):
        self.width = width
        self.height = height
        self.events = []
        self.ui_events = []
        self.window_rects = []
        # 画面の変化領域。キャプチャ領域基準
        self.changes = []
        self.pos = start_pos
        self.rng = random.Random(28)

    def ev(self, **kw):
        self.events.append(kw)

    def move(self, t0, to, dur=500):
        """t0 から dur ms かけて to へカーソルを動かす（ease-in-out）。終了時刻を返す。"""
        (x0, y0), (x1, y1) = self.pos, to
        steps = max(1, dur // MOVE_INTERVAL_MS)
        for i in range(1, steps + 1):
            p = i / steps
            e = p * p * (3 - 2 * p)
            self.ev(type="mouse_move", t=t0 + i * MOVE_INTERVAL_MS,
                    x=round(x0 + (x1 - x0) * e), y=round(y0 + (y1 - y0) * e))
        self.pos = to
        return t0 + steps * MOVE_INTERVAL_MS

    def click(self, t, pos=None, hold=80):
        """クリックとリリース。pos を指定するとそこへ移動してからクリックする。終了時刻を返す。"""
        if pos is not None and pos != self.pos:
            t = self.move(t, pos) + 120
        x, y = self.pos
        self.ev(type="click", t=t, btn="left", x=x, y=y)
        self.ev(type="click_release", t=t + hold, btn="left", x=x, y=y)
        return t + hold

    def drag(self, t, to, dur=1200):
        x, y = self.pos
        self.ev(type="click", t=t, btn="left", x=x, y=y)
        end = self.move(t + 100, to, dur)
        self.ev(type="click_release", t=end + 60, btn="left", x=to[0], y=to[1])
        return end + 60

    def type(self, t0, n, interval=150, key="Character"):
        """n 回キーを押す（間隔に揺らぎを付ける）。終了時刻を返す。"""
        t = t0
        for i in range(n):
            k = "Space" if i % 6 == 5 else key
            self.ev(type="key", t=t, key=k, modifiers=[])
            t += interval + self.rng.randint(-40, 40)
        return t

    def key(self, t, key, modifiers=()):
        self.ev(type="key", t=t, key=key, modifiers=list(modifiers))
        return t

    def typed_line(self, t0, t1, x, y, char_w=24, interval=200):
        """t0〜t1 の入力で、(x, y) から右へ伸びていく入力行の変化領域を interval ごとに記録する"""
        t = t0 + interval
        while t <= t1 + interval:
            n = (t - t0) // interval
            self.changes.append({"t": t, "rects": [[x, y - 16, x + char_w * (n + 1), y + 16, 2 * (n + 1)]]})
            t += interval

    def scroll(self, t, dy=-1.0):
        x, y = self.pos
        self.ev(type="scroll", t=t, x=x, y=y, dx=0.0, dy=dy)

    def write(self, name, duration_ms, description, checks, mode="display", window_rect=None):
        d = OUT_DIR / name
        d.mkdir(parents=True, exist_ok=True)
        # Snappi の開始・停止ホットキー（R5 で無視されるべき操作）
        self.key(150, "Character", ["Ctrl", "Shift"])
        self.key(duration_ms - 300, "Character", ["Ctrl", "Shift"])
        meta = {
            "version": 2,
            "id": f"synthetic-{name}",
            "screen_width": self.width,
            "screen_height": self.height,
            "fps": FPS,
            "start_time": "2026-10-13T12:00:00+09:00",
            "duration_ms": duration_ms,
            "has_audio": False,
            "recording_dir": "",
            "recording_mode": mode,
            "window_title": "Synthetic" if mode == "window" else None,
            "window_initial_rect": window_rect,
            "frame_count": duration_ms * FPS // 1000,
        }
        write_text(d / "meta.json", json.dumps(meta, indent=2, ensure_ascii=False) + "\n")
        # フレームは等間隔に取り込んだものとする
        count = meta["frame_count"]
        write_text(d / "frame_timestamps.txt", "".join(f"{i * duration_ms // count}\n" for i in range(count)))
        write_jsonl(d / "events.jsonl", sorted(self.events, key=lambda e: e["t"]))
        for fname, rows in (
            ("ui_events.jsonl", self.ui_events),
            ("window_rects.jsonl", self.window_rects),
            ("changes.jsonl", self.changes),
        ):
            if rows:
                write_jsonl(d / fname, sorted(rows, key=lambda e: e["t"]))
            elif (d / fname).exists():
                (d / fname).unlink()
        expect = {"description": description, "synthetic": True, "checks": checks}
        write_text(d / "expect.json", json.dumps(expect, indent=2, ensure_ascii=False) + "\n")


def write_text(path, text):
    # Windows でも UTF-8・LF で書く
    path.write_text(text, encoding="utf-8", newline="\n")


def write_jsonl(path, rows):
    write_text(path, "".join(json.dumps(r, ensure_ascii=False) + "\n" for r in rows))


def window_mode():
    # 1280x800 のウィンドウを録画。イベントは画面座標で記録される
    left, top = 200, 100
    r = Rec(width=1280, height=800, start_pos=(1000, 600))
    r.window_rects.append({"t": 0, "rect": [left, top, left + 1280, top + 800]})
    t = r.click(1500, (500, 400))
    t = r.type(t + 300, 15)
    # タイトルバーをドラッグしてウィンドウを (300, 200) 動かす
    r.move(t + 500, (500, 115))
    t0 = t + 1200
    r.ev(type="click", t=t0, btn="left", x=500, y=115)
    steps = 15
    for i in range(1, steps + 1):
        dx, dy = 300 * i // steps, 200 * i // steps
        r.ev(type="mouse_move", t=t0 + i * 100, x=500 + dx, y=115 + dy)
        r.window_rects.append({"t": t0 + i * 100, "rect": [left + dx, top + dy, left + dx + 1280, top + dy + 800]})
    r.pos = (800, 315)
    r.ev(type="click_release", t=t0 + steps * 100 + 50, btn="left", x=800, y=315)
    left, top = left + 300, top + 200
    # 移動後のウィンドウ内 (400, 200) をクリックして入力
    t = r.click(t0 + steps * 100 + 800, (left + 400, top + 200))
    end = r.type(t + 300, 15)
    # 入力行の変化（キャプチャ領域基準。1回目は (300, 300)、2回目は (400, 200) から右へ）
    r.typed_line(1800, 1800 + 15 * 150, 300, 300)
    r.typed_line(t + 300, end, 400, 200)
    r.write("window-mode", 20000, "ウィンドウモードでの録画（録画中にウィンドウを移動）", [
        {"kind": "visible", "t_ms": t, "x": 400, "y": 200},
        {"kind": "zoomed_in", "from_ms": t + 500, "to_ms": end, "min_ratio": 0.9},
        {"kind": "framed", "from_ms": t + 500, "to_ms": end, "rect": [400, 184, 760, 216], "min_ratio": 0.9},
        {"kind": "zoomed_out", "from_ms": end + 4000, "to_ms": 18000},
    ], mode="window", window_rect=[200, 100, 1480, 900])


if __name__ == "__main__":
    window_mode()
    print(f"generated {OUT_DIR / 'window-mode'}")
