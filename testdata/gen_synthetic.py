#!/usr/bin/env python3
"""評価シナリオの合成録画データを testdata/recordings/ に生成する (Issue #28)。

Windows 実機で録画したデータに置き換えるまでの暫定データ。実録画と同じ形式
（meta.json / events.jsonl / ui_events.jsonl / window_rects.jsonl）で、frames は含まない。
各シナリオの expect.json もここで生成する。

    python3 testdata/gen_synthetic.py [シナリオ名 ...]

シナリオ名を省略すると全シナリオを生成する。実録画に置き換えたシナリオは名前を指定せずに
実行すると上書きされるので、置き換え後はこのスクリプトの SCENARIOS から外すこと。
"""

import json
import random
import sys
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
            "monitor_scale": 1.0,
            "recording_dir": "",
            "recording_mode": mode,
            "window_title": "Synthetic" if mode == "window" else None,
            "window_initial_rect": window_rect,
            "frame_count": duration_ms * FPS // 1000,
        }
        (d / "meta.json").write_text(json.dumps(meta, indent=2, ensure_ascii=False) + "\n")
        write_jsonl(d / "events.jsonl", sorted(self.events, key=lambda e: e["t"]))
        for fname, rows in (("ui_events.jsonl", self.ui_events), ("window_rects.jsonl", self.window_rects)):
            if rows:
                write_jsonl(d / fname, sorted(rows, key=lambda e: e["t"]))
            elif (d / fname).exists():
                (d / fname).unlink()
        expect = {"description": description, "synthetic": True, "checks": checks}
        (d / "expect.json").write_text(json.dumps(expect, indent=2, ensure_ascii=False) + "\n")


def write_jsonl(path, rows):
    path.write_text("".join(json.dumps(r, ensure_ascii=False) + "\n" for r in rows))


def vscode_typing():
    r = Rec()
    r.click(1500, (700, 400))
    r.ev(type="cursor_shape", t=1900, shape="ibeam")
    end = r.type(2500, 45)
    r.key(end + 300, "Return")
    end = r.type(end + 600, 20)
    r.key(end + 800, "Character", ["Ctrl"])  # Ctrl+S
    r.move(12000, (1200, 600), 600)
    r.ev(type="cursor_shape", t=12300, shape="arrow")
    r.write("vscode-typing", 20000, "エディタでクリックしてからコードを入力し、保存する", [
        {"kind": "visible", "t_ms": 2400, "x": 700, "y": 400},
        {"kind": "zoomed_in", "from_ms": 3000, "to_ms": 9500, "min_ratio": 0.9},
        {"kind": "zoomed_out", "from_ms": 15000, "to_ms": 18000},
        {"kind": "max_switches", "max": 2},
    ])


def terminal():
    r = Rec(start_pos=(960, 700))
    end = r.type(2000, 25)
    r.key(end + 200, "Return")
    end = r.type(end + 2000, 20)
    r.key(end + 200, "Return")
    r.write("terminal", 18000, "クリックせずにターミナルでコマンドを入力する", [
        {"kind": "zoomed_in", "from_ms": 3000, "to_ms": 10000, "min_ratio": 0.9},
        {"kind": "zoomed_out", "from_ms": 14500, "to_ms": 16000},
        {"kind": "max_switches", "max": 2},
    ])


def browser_form():
    r = Rec()
    t = 1500
    fields = [(800, 300), (800, 400), (800, 500), (800, 600)]
    for i, pos in enumerate(fields):
        t = r.click(t, pos)
        r.ui_events.append({"type": "ui_focus", "t": t, "control": "Edit", "name": f"field{i}",
                            "rect": [pos[0] - 150, pos[1] - 18, pos[0] + 150, pos[1] + 18]})
        t = r.type(t + 300, 10) + 600
    end = r.click(t, (900, 700))
    r.write("browser-form", 25000, "ブラウザのフォームで複数の入力欄を順にクリックして入力する", [
        {"kind": "visible", "t_ms": 2000, "x": 800, "y": 300},
        {"kind": "visible", "t_ms": end, "x": 900, "y": 700},
        {"kind": "max_switches", "max": 2},
        {"kind": "zoomed_out", "from_ms": end + 4000, "to_ms": 23000},
    ])


def menu_navigation():
    r = Rec()
    t = r.click(1500, (40, 15))
    r.ui_events.append({"type": "ui_menu_open", "t": t + 50, "control": "Menu", "name": "File",
                        "rect": [0, 30, 300, 400]})
    t = r.click(t + 700, (80, 200))
    r.ui_events.append({"type": "ui_menu_close", "t": t + 30, "control": "Menu", "name": "File"})
    r.ui_events.append({"type": "ui_dialog_open", "t": t + 300, "control": "Window", "name": "Save As",
                        "rect": [560, 240, 1360, 840]})
    t = r.click(t + 900, (700, 400))
    t = r.type(t + 300, 12)
    end = r.click(t + 500, (1250, 800))
    r.ui_events.append({"type": "ui_dialog_close", "t": end + 50, "control": "Window", "name": "Save As"})
    r.write("menu-navigation", 20000, "アプリのメニューとダイアログを操作する", [
        {"kind": "visible", "t_ms": 1800, "x": 40, "y": 15},
        {"kind": "max_switches", "max": 2},
        {"kind": "zoomed_out", "from_ms": end + 4000, "to_ms": 18000},
    ])


def far_jumps():
    r = Rec(start_pos=(960, 540))
    corners = [(150, 120), (1770, 960)]
    t = 2000
    times = []
    for i in range(6):
        t = r.click(t, corners[i % 2])
        times.append((t, corners[i % 2]))
        t += 1500
    checks = [{"kind": "visible", "t_ms": ct, "x": x, "y": y} for ct, (x, y) in times]
    checks.append({"kind": "zoomed_out", "from_ms": times[-1][0] + 4000, "to_ms": 20000})
    r.write("far-jumps", 22000, "画面の左上と右下を交互にクリックする", checks)


def drag_scroll():
    r = Rec()
    r.move(1500, (300, 500))
    end = r.drag(2200, (1400, 500))
    r.move(end + 1500, (960, 540))
    t = end + 2500
    scroll_start = t
    for i in range(40):
        r.scroll(t)
        t += 200 + (i % 3) * 20
    r.write("drag-scroll", 24000, "ドラッグ＆ドロップと長いスクロール", [
        {"kind": "visible", "t_ms": end, "x": 1400, "y": 500},
        {"kind": "zoomed_in", "from_ms": scroll_start + 500, "to_ms": t, "min_ratio": 0.9},
        {"kind": "zoomed_out", "from_ms": t + 4000, "to_ms": 22000},
    ])


def idle_gaps():
    r = Rec()
    t = r.click(1500, (600, 350))
    end1 = r.type(t + 300, 25)
    t = r.click(end1 + 10000, (1300, 750))
    end2 = r.type(t + 300, 25)
    r.write("idle-gaps", 30000, "操作 → 10秒放置 → 操作。放置中は全体表示に戻る", [
        {"kind": "zoomed_in", "from_ms": 2500, "to_ms": end1, "min_ratio": 0.9},
        {"kind": "zoomed_out", "from_ms": end1 + 3500, "to_ms": end1 + 8500},
        {"kind": "zoomed_in", "from_ms": t + 500, "to_ms": end2, "min_ratio": 0.9},
        {"kind": "zoomed_out", "from_ms": end2 + 4000, "to_ms": 28000},
        {"kind": "max_switches", "max": 4},
    ])


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
    r.write("window-mode", 20000, "ウィンドウモードでの録画（録画中にウィンドウを移動）", [
        {"kind": "visible", "t_ms": t, "x": 400, "y": 200},
        {"kind": "zoomed_in", "from_ms": t + 500, "to_ms": end, "min_ratio": 0.9},
        {"kind": "zoomed_out", "from_ms": end + 4000, "to_ms": 18000},
    ], mode="window", window_rect=[200, 100, 1480, 900])


SCENARIOS = {
    "vscode-typing": vscode_typing,
    "terminal": terminal,
    "browser-form": browser_form,
    "menu-navigation": menu_navigation,
    "far-jumps": far_jumps,
    "drag-scroll": drag_scroll,
    "idle-gaps": idle_gaps,
    "window-mode": window_mode,
}

if __name__ == "__main__":
    names = sys.argv[1:] or list(SCENARIOS)
    for name in names:
        SCENARIOS[name]()
        print(f"generated {OUT_DIR / name}")
