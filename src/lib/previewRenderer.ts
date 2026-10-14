/**
 * プレビューの合成。書き出し（src-tauri/src/engine/compositor.rs）と同じ順序・
 * 同じパラメータ（get_preview_data の PreviewStyle）で canvas に描く:
 * 背景と影 → カメラのビューポートで切り抜いたフレーム → カーソル → クリックリング → キー表示、
 * をすべて角丸の内側に描く。モーションブラーは描かない。
 */
import { convertFileSrc } from "@tauri-apps/api/core";
import type { CameraState } from "./generated/CameraState";
import type { CameraTrack } from "./generated/CameraTrack";
import type { CursorKind } from "./generated/CursorKind";
import type { PreviewData } from "./generated/PreviewData";
import type { PreviewStyle } from "./generated/PreviewStyle";

/** 読み込んだフレーム画像を保持する数 */
const FRAME_CACHE_SIZE = 120;

/** 最後の要素が `t` 以下になる位置（`times` は昇順）。どれも `t` より大きければ -1 */
function lastIndexAtOrBefore(len: number, timeAt: (i: number) => number, t: number): number {
  let lo = 0;
  let hi = len;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (timeAt(mid) <= t) lo = mid + 1;
    else hi = mid;
  }
  return lo - 1;
}

/** カメラトラックのフレーム k の時刻（Rust の output_frame_time と同じ丸め） */
function trackTimeMs(k: number, fps: number): number {
  return Math.floor((k * 1000 + Math.floor(fps / 2)) / fps);
}

/** 時刻 t 以前で最新のカメラ状態（Rust の CameraTrack::state_at と同じ） */
export function cameraAt(track: CameraTrack | null, t: number, width: number, height: number): CameraState {
  if (!track || track.frames.length === 0) return { center_x: width / 2, center_y: height / 2, zoom: 1 };
  const last = track.frames.length - 1;
  let k = Math.min(last, Math.max(0, Math.floor((t * track.fps) / 1000)));
  while (k < last && trackTimeMs(k + 1, track.fps) <= t) k++;
  while (k > 0 && trackTimeMs(k, track.fps) > t) k--;
  return track.frames[k];
}

const rgba = (c: number[], alpha = c[3] / 255) => `rgba(${c[0]},${c[1]},${c[2]},${alpha})`;

/** 背景（グラデーション・単色）と影を描いた canvas。毎フレーム同じなので 1 度だけ作る */
function createBackground(style: PreviewStyle): HTMLCanvasElement {
  const w = style.canvas_width;
  const h = style.canvas_height;
  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext("2d")!;
  const img = ctx.createImageData(w, h);
  const px = img.data;

  // 背景（background.rs と同じ式）。Transparent は書き出しで黒になるので黒
  const bg = style.background;
  if (bg.type === "Gradient") {
    const rad = (bg.angle * Math.PI) / 180;
    const cos = Math.cos(rad);
    const sin = Math.sin(rad);
    const maxDist = (w * Math.abs(cos) + h * Math.abs(sin)) / 2;
    for (let y = 0; y < h; y++) {
      for (let x = 0; x < w; x++) {
        const dist = (x - w / 2) * cos + (y - h / 2) * sin;
        const t = Math.min(1, Math.max(0, (dist / maxDist + 1) / 2));
        const i = (y * w + x) * 4;
        px[i] = bg.from[0] * (1 - t) + bg.to[0] * t;
        px[i + 1] = bg.from[1] * (1 - t) + bg.to[1] * t;
        px[i + 2] = bg.from[2] * (1 - t) + bg.to[2] * t;
        px[i + 3] = 255;
      }
    }
  } else {
    const c = bg.type === "Solid" ? bg.color : [0, 0, 0];
    for (let i = 0; i < px.length; i += 4) {
      px[i] = c[0];
      px[i + 1] = c[1];
      px[i + 2] = c[2];
      px[i + 3] = 255;
    }
  }

  // 影（compositor.rs の draw_drop_shadow と同じく、角丸矩形からの距離で二乗減衰）
  const blur = style.shadow_blur;
  const color = style.shadow_color;
  if (blur > 0 && color[3] > 0) {
    const ox = (w - style.output_width) / 2;
    const oy = (h - style.output_height) / 2;
    const r = style.border_radius;
    const left = ox;
    const right = ox + style.output_width;
    const top = oy + style.shadow_offset_y;
    const bottom = top + style.output_height;
    const spread = Math.ceil(blur);
    const y0 = Math.max(0, Math.floor(oy) - spread);
    const y1 = Math.min(h - 1, Math.ceil(oy + style.output_height + style.shadow_offset_y) + spread);
    const x0 = Math.max(0, Math.floor(ox) - spread);
    const x1 = Math.min(w - 1, Math.ceil(right) + spread);
    for (let y = y0; y <= y1; y++) {
      for (let x = x0; x <= x1; x++) {
        const pxx = x + 0.5;
        const pyy = y + 0.5;
        const dx = pxx < left + r ? left + r - pxx : pxx > right - r ? pxx - (right - r) : 0;
        const dy = pyy < top + r ? top + r - pyy : pyy > bottom - r ? pyy - (bottom - r) : 0;
        const dist = dx > 0 && dy > 0 ? Math.max(0, Math.hypot(dx, dy) - r) : Math.max(0, Math.max(dx, dy) - r);
        if (dist <= 0 || dist > blur) continue;
        const t = 1 - dist / blur;
        const a = (t * t * color[3]) / 255;
        const i = (y * w + x) * 4;
        px[i] = color[0] * a + px[i] * (1 - a);
        px[i + 1] = color[1] * a + px[i + 1] * (1 - a);
        px[i + 2] = color[2] * a + px[i + 2] * (1 - a);
      }
    }
  }

  ctx.putImageData(img, 0, 0);
  return canvas;
}

function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error(`failed to load ${src}`));
    img.src = src;
  });
}

interface Sprite {
  image: HTMLImageElement;
  hotspotX: number;
  hotspotY: number;
}

export class PreviewRenderer {
  readonly data: PreviewData;
  private readonly recordingDir: string;
  private background: HTMLCanvasElement;
  private sprites = new Map<CursorKind, Sprite>();
  /** フレーム番号 → 画像（読み込み中は null）。Map の順序を LRU に使う */
  private frames = new Map<number, HTMLImageElement | null>();
  /** 最後に描けたフレーム（目的のフレームが読み込み中のとき代わりに描く） */
  private lastFrame: HTMLImageElement | null = null;
  private keyEncoder = new TextEncoder();
  /** フレーム画像の読み込みが終わったときに呼ばれる（再描画用） */
  onFrameLoaded: ((index: number) => void) | null = null;

  constructor(data: PreviewData, recordingDir: string) {
    this.data = data;
    this.recordingDir = recordingDir;
    this.background = createBackground(data.style);
  }

  /** カーソル画像を読み込む */
  async init(): Promise<void> {
    await Promise.all(
      this.data.sprites.map(async (s) => {
        const image = await loadImage(s.data_url);
        this.sprites.set(s.kind, { image, hotspotX: s.hotspot_x, hotspotY: s.hotspot_y });
      }),
    );
  }

  get canvasWidth() {
    return this.data.style.canvas_width;
  }

  get canvasHeight() {
    return this.data.style.canvas_height;
  }

  /** 出力画像（角丸の内側）の canvas 上の矩形 */
  get outputRect() {
    const s = this.data.style;
    return {
      x: Math.floor((s.canvas_width - s.output_width) / 2),
      y: Math.floor((s.canvas_height - s.output_height) / 2),
      width: s.output_width,
      height: s.output_height,
    };
  }

  /** 時刻 t 以前で最新のフレーム番号（Rust の frame_index_at と同じ） */
  frameIndexAt(t: number): number {
    const times = this.data.frame_times;
    return Math.max(0, lastIndexAtOrBefore(times.length, (i) => times[i], t));
  }

  private frameUrl(index: number): string {
    const padded = String(index).padStart(8, "0");
    return convertFileSrc(`${this.recordingDir}\\frames\\frame_${padded}.jpg`);
  }

  /** フレームを読み込み済みなら返す。未読み込みなら読み込みを始めて null */
  private frame(index: number): HTMLImageElement | null {
    if (this.frames.has(index)) {
      const img = this.frames.get(index)!;
      // LRU: 使ったものを末尾へ
      this.frames.delete(index);
      this.frames.set(index, img);
      return img;
    }
    this.frames.set(index, null);
    const img = new Image();
    img.src = this.frameUrl(index);
    img
      .decode()
      .then(() => {
        if (!this.frames.has(index)) return;
        this.frames.set(index, img);
        this.onFrameLoaded?.(index);
      })
      .catch(() => this.frames.delete(index));
    while (this.frames.size > FRAME_CACHE_SIZE) {
      const oldest = this.frames.keys().next().value as number;
      this.frames.delete(oldest);
    }
    return null;
  }

  /** 時刻 t から先 aheadMs 分のフレームを先読みする */
  prefetch(t: number, aheadMs: number): void {
    const first = this.frameIndexAt(t);
    const last = this.frameIndexAt(t + aheadMs);
    for (let i = first; i <= last && i - first < FRAME_CACHE_SIZE / 2; i++) {
      if (!this.frames.has(i)) this.frame(i);
    }
  }

  private cursorAt(t: number): { x: number; y: number } | null {
    const pts = this.data.cursor;
    if (pts.length === 0) return null;
    const i = lastIndexAtOrBefore(pts.length, (k) => pts[k].t, t);
    if (i < 0) return { x: pts[0].x, y: pts[0].y };
    if (i >= pts.length - 1 || pts[i].t === t) return { x: pts[i].x, y: pts[i].y };
    // 次の点との間を線形補間（encoder.rs の find_cursor_at_time と同じ）
    const a = pts[i];
    const b = pts[i + 1];
    const r = b.t > a.t ? (t - a.t) / (b.t - a.t) : 0;
    return { x: a.x + (b.x - a.x) * r, y: a.y + (b.y - a.y) * r };
  }

  private cursorKindAt(t: number): CursorKind {
    const shapes = this.data.cursor_shapes;
    const i = lastIndexAtOrBefore(shapes.length, (k) => shapes[k].t, t);
    return i < 0 ? "arrow" : shapes[i].kind;
  }

  /** 時刻 t の完成形を ctx（canvasWidth x canvasHeight）に描く */
  draw(ctx: CanvasRenderingContext2D, t: number, cam: CameraState): void {
    const { data } = this;
    const style = data.style;
    const out = this.outputRect;

    const index = this.frameIndexAt(t);
    const image = this.frame(index) ?? this.lastFrame;
    if (image) this.lastFrame = image;

    ctx.clearRect(0, 0, this.canvasWidth, this.canvasHeight);
    ctx.drawImage(this.background, 0, 0);

    ctx.save();
    ctx.beginPath();
    ctx.roundRect(out.x, out.y, out.width, out.height, style.border_radius);
    ctx.clip();
    ctx.fillStyle = "#000";
    ctx.fillRect(out.x, out.y, out.width, out.height);

    const vw = data.width / cam.zoom;
    const vh = data.height / cam.zoom;
    const vx = cam.center_x - vw / 2;
    const vy = cam.center_y - vh / 2;
    // キャプチャ領域の座標 → canvas 座標
    const toCanvas = (x: number, y: number) => ({
      x: out.x + ((x - vx) / vw) * out.width,
      y: out.y + ((y - vy) / vh) * out.height,
    });

    if (image) {
      ctx.imageSmoothingQuality = "high";
      ctx.drawImage(image, vx, vy, vw, vh, out.x, out.y, out.width, out.height);
    }

    // カーソル（倍率に比例して大きくする）
    const cursor = this.cursorAt(t);
    const sprite = this.sprites.get(this.cursorKindAt(t)) ?? this.sprites.get("arrow");
    if (cursor && sprite) {
      const p = toCanvas(cursor.x, cursor.y);
      const scale = (style.cursor_width * cam.zoom) / sprite.image.width;
      ctx.drawImage(
        sprite.image,
        p.x - sprite.hotspotX * scale,
        p.y - sprite.hotspotY * scale,
        sprite.image.width * scale,
        sprite.image.height * scale,
      );
    }

    // クリックリング（ease-out cubic で広がりながら消える）
    const dur = style.click_ring_duration_ms;
    const color = style.click_ring_color;
    const firstClick = lastIndexAtOrBefore(data.clicks.length, (k) => data.clicks[k].t, t - dur - 1) + 1;
    for (let k = Math.max(0, firstClick); k < data.clicks.length && data.clicks[k].t <= t; k++) {
      const click = data.clicks[k];
      const progress = 1 - Math.pow(1 - (t - click.t) / dur, 3);
      const radius = style.click_ring_max_radius * cam.zoom * progress;
      const alpha = ((1 - progress) * color[3]) / 255;
      const p = toCanvas(click.x, click.y);
      ctx.beginPath();
      ctx.arc(p.x, p.y, radius, 0, Math.PI * 2);
      ctx.fillStyle = rgba(color, alpha * 0.15);
      ctx.fill();
      ctx.lineWidth = style.click_ring_stroke_width * cam.zoom * 2;
      ctx.strokeStyle = rgba(color, alpha);
      ctx.stroke();
    }

    // キー表示（表示中のうち最新のもの）
    const keyIdx = lastIndexAtOrBefore(data.keys.length, (k) => data.keys[k].t, t);
    const key = keyIdx >= 0 ? data.keys[keyIdx] : null;
    if (key && t <= key.t + style.key_badge_duration_ms) {
      const bytes = this.keyEncoder.encode(key.keys).length;
      const bw = Math.min(bytes * style.key_badge_char_width + style.key_badge_padding, out.width);
      const bx = out.x + Math.floor((out.width - bw) / 2);
      const by = out.y + out.height - style.key_badge_height - style.key_badge_margin_bottom;
      ctx.beginPath();
      ctx.roundRect(bx, by, bw, style.key_badge_height, style.key_badge_radius);
      ctx.fillStyle = rgba(style.key_badge_color);
      ctx.fill();
    }

    ctx.restore();
  }
}
