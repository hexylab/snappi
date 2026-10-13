import type { ZoomSegment } from "./generated/ZoomSegment";
import type { Trim } from "./generated/Trim";

/**
 * プレビューで直せる内容（project.json の区間とトリム）。Undo/Redo の単位。
 * ズームは自動で完成形に近くなる前提なので、編集は「区間を消す・端を詰める・前後を切る」だけにしている。
 */
export interface EditState {
  segments: ZoomSegment[];
  trim: Trim;
}

/** 区間の最小の長さ (ms) */
export const MIN_SEGMENT_MS = 300;
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
