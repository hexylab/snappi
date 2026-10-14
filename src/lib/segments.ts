import type { ZoomSegment } from "./generated/ZoomSegment";
import type { Trim } from "./generated/Trim";
import type { ZoomStrength } from "./generated/ZoomStrength";

/**
 * プレビューで直せる内容（project.json の区間・トリム・この録画のズーム強さ）。Undo/Redo の単位。
 * ズームは自動で完成形に近くなる前提なので、編集は「前後を切る」（通常モード）と
 * 「区間の追加・削除・端の伸縮・強さの変更・自動に戻す」（詳細モード）だけにしている。
 */
export interface EditState {
  segments: ZoomSegment[];
  trim: Trim;
  /** null なら設定の値 */
  zoom_strength: ZoomStrength | null;
}

/** 区間の最小の長さ (ms) */
export const MIN_SEGMENT_MS = 300;
/** 「ズームを追加」で作る区間の長さ (ms)。前後が詰まっていれば入る分だけにする */
export const NEW_SEGMENT_MS = 2000;
/** トリム後に残す最小の長さ (ms) */
export const MIN_TRIM_MS = 500;

const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));

export function sortSegments(segments: ZoomSegment[]): ZoomSegment[] {
  return [...segments].sort((a, b) => a.start_ms - b.start_ms);
}

/** 区間の端を ms に動かす（前後の区間に重ならず、最小の長さを保つ範囲で） */
export function resizeSegment(
  segments: ZoomSegment[],
  id: string,
  edge: "start" | "end",
  ms: number,
  durationMs: number,
): ZoomSegment[] {
  const sorted = sortSegments(segments);
  const idx = sorted.findIndex((s) => s.id === id);
  const lo = idx > 0 ? sorted[idx - 1].end_ms : 0;
  const hi = idx >= 0 && idx < sorted.length - 1 ? sorted[idx + 1].start_ms : durationMs;
  return segments.map((s) => {
    if (s.id !== id) return s;
    if (edge === "start") {
      return { ...s, start_ms: Math.round(clamp(ms, lo, s.end_ms - MIN_SEGMENT_MS)) };
    }
    return { ...s, end_ms: Math.round(clamp(ms, s.start_ms + MIN_SEGMENT_MS, hi)) };
  });
}

/**
 * ms から始まる新しい区間（寄る場所は自動）。区間の中や、前後に最小の長さが取れない位置なら null。
 */
export function newSegmentAt(segments: ZoomSegment[], ms: number, durationMs: number): ZoomSegment | null {
  const sorted = sortSegments(segments);
  if (sorted.some((s) => ms >= s.start_ms && ms < s.end_ms)) return null;
  const next = sorted.find((s) => s.start_ms > ms);
  const start = Math.round(clamp(ms, 0, durationMs));
  const end = Math.round(Math.min(start + NEW_SEGMENT_MS, next ? next.start_ms : durationMs));
  if (end - start < MIN_SEGMENT_MS) return null;
  return { id: `manual-${Date.now()}`, start_ms: start, end_ms: end, zoom: null, focus: { type: "Auto" } };
}

/** トリムの端を ms に動かす（最小の長さを保つ） */
export function resizeTrim(trim: Trim, edge: "start" | "end", ms: number, durationMs: number): Trim {
  if (edge === "start") {
    return { ...trim, start_ms: Math.round(clamp(ms, 0, trim.end_ms - MIN_TRIM_MS)) };
  }
  return { ...trim, end_ms: Math.round(clamp(ms, trim.start_ms + MIN_TRIM_MS, durationMs)) };
}

export function formatTime(ms: number): string {
  const totalSec = Math.max(0, ms) / 1000;
  const min = Math.floor(totalSec / 60);
  const sec = totalSec - min * 60;
  return `${min}:${sec.toFixed(1).padStart(4, "0")}`;
}
