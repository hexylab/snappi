import type { ZoomKeyframe } from "./types";

export interface ZoomSegment {
  id: number;
  startMs: number;
  endMs: number;
  zoomLevel: number;
  centerX: number;
  centerY: number;
}

export const ZOOM_THRESHOLD = 1.05;
export const DEFAULT_ZOOM = 2.0;
export const DEFAULT_SEGMENT_DURATION_MS = 3000;

let nextId = 1;
export function generateSegmentId(): number {
  return nextId++;
}

/** center座標が大きく変化したら別シーンとみなす閾値 (px) */
const CENTER_JUMP_THRESHOLD = 200;

/**
 * KF配列 → セグメント配列
 * zoom_level > ZOOM_THRESHOLD の連続区間をグループ化。
 * Smooth遷移でzoomが下がらなくてもcenter座標が大きくジャンプした場合は
 * 別セグメントとして分割する（zoom_plannerのclose-scene Smooth遷移対応）。
 */
export function keyframesToSegments(kfs: ZoomKeyframe[]): ZoomSegment[] {
  if (!kfs.length) return [];

  const sorted = [...kfs].sort((a, b) => a.time_ms - b.time_ms);
  const segments: ZoomSegment[] = [];

  let segStart: ZoomKeyframe | null = null;
  let maxZoom = 1.0;
  let centerX = 0;
  let centerY = 0;

  const flushSegment = (endMs: number) => {
    if (!segStart) return;
    segments.push({
      id: generateSegmentId(),
      startMs: segStart.time_ms,
      endMs,
      zoomLevel: maxZoom,
      centerX,
      centerY,
    });
    segStart = null;
    maxZoom = 1.0;
  };

  for (let i = 0; i < sorted.length; i++) {
    const kf = sorted[i];

    if (kf.zoom_level > ZOOM_THRESHOLD) {
      if (!segStart) {
        segStart = kf;
        maxZoom = kf.zoom_level;
        centerX = kf.target_x;
        centerY = kf.target_y;
      } else {
        // center座標が大きくジャンプした場合は別セグメントとして分割
        const dx = kf.target_x - centerX;
        const dy = kf.target_y - centerY;
        const dist = Math.sqrt(dx * dx + dy * dy);
        if (dist > CENTER_JUMP_THRESHOLD) {
          // 現在のセグメントをこのKFの時刻で閉じる
          flushSegment(kf.time_ms);
          // 新しいセグメントを開始
          segStart = kf;
          maxZoom = kf.zoom_level;
          centerX = kf.target_x;
          centerY = kf.target_y;
        } else {
          if (kf.zoom_level > maxZoom) {
            maxZoom = kf.zoom_level;
          }
        }
      }
    } else {
      if (segStart) {
        flushSegment(kf.time_ms);
      }
    }
  }

  // 最後まで zoomed だった場合
  if (segStart) {
    const lastKf = sorted[sorted.length - 1];
    flushSegment(lastKf.time_ms + DEFAULT_SEGMENT_DURATION_MS);
  }

  return segments;
}

/**
 * セグメント配列 → KF配列
 * 各セグメントの前後にSpringIn/Out KFを生成
 */
export function segmentsToKeyframes(
  segments: ZoomSegment[],
  screenWidth: number,
  screenHeight: number,
): ZoomKeyframe[] {
  const kfs: ZoomKeyframe[] = [];
  const sorted = [...segments].sort((a, b) => a.startMs - b.startMs);

  // t=0 の Overview KF
  kfs.push({
    time_ms: 0,
    target_x: screenWidth / 2,
    target_y: screenHeight / 2,
    zoom_level: 1.0,
    transition: "Smooth",
  });

  for (const seg of sorted) {
    // SpringIn: ズーム開始
    kfs.push({
      time_ms: seg.startMs,
      target_x: seg.centerX,
      target_y: seg.centerY,
      zoom_level: seg.zoomLevel,
      transition: "SpringIn",
    });

    // SpringOut: ズーム終了 → 全体表示に戻る
    kfs.push({
      time_ms: seg.endMs,
      target_x: screenWidth / 2,
      target_y: screenHeight / 2,
      zoom_level: 1.0,
      transition: "SpringOut",
    });
  }

  // time_msでソート
  kfs.sort((a, b) => a.time_ms - b.time_ms);

  return kfs;
}

/**
 * 指定時刻がどのセグメント内にあるか検索
 */
export function findSegmentAtTime(
  segments: ZoomSegment[],
  timeMs: number,
): ZoomSegment | undefined {
  return segments.find(s => timeMs >= s.startMs && timeMs <= s.endMs);
}
