import type { ZoomSegment as BackendZoomSegment } from "./generated/ZoomSegment";

/**
 * 編集 UI 用のズーム区間。バックエンドの `ZoomSegment`（src/lib/generated）と相互に変換する。
 * - manualCenter: true なら固定位置（focus: Fixed）、false なら注目点を自動追従（focus: Auto）
 * - manualZoom: true なら区間の倍率、false ならズーム強さの既定倍率（zoom: null）
 */
export interface ZoomSegment {
  id: number;
  startMs: number;
  endMs: number;
  zoomLevel: number;
  centerX: number;
  centerY: number;
  /** ユーザーが centerX/centerY を手動で設定したか（固定位置）。false なら自動追従 */
  manualCenter?: boolean;
  /** ユーザーが zoomLevel を手動で変更したか。false ならズーム強さの既定倍率 */
  manualZoom?: boolean;
}

/** ズーム強さ「標準」の倍率（src-tauri/src/camera/spec.rs の ZOOM_STANDARD） */
export const DEFAULT_ZOOM = 1.8;
export const DEFAULT_SEGMENT_DURATION_MS = 3000;

let nextId = 1;
export function generateSegmentId(): number {
  return nextId++;
}

/** バックエンドの区間 → 編集用の区間。自動追従の区間は中心を画面中央として表示する */
export function fromBackendSegment(
  seg: BackendZoomSegment,
  screenWidth: number,
  screenHeight: number,
): ZoomSegment {
  const fixed = seg.focus.type === "Fixed" ? seg.focus : null;
  return {
    id: generateSegmentId(),
    startMs: seg.start_ms,
    endMs: seg.end_ms,
    zoomLevel: seg.zoom ?? DEFAULT_ZOOM,
    centerX: fixed ? fixed.x : screenWidth / 2,
    centerY: fixed ? fixed.y : screenHeight / 2,
    manualCenter: fixed !== null,
    manualZoom: seg.zoom !== null,
  };
}

/** 編集用の区間 → バックエンドの区間 */
export function toBackendSegment(seg: ZoomSegment): BackendZoomSegment {
  return {
    id: `ui-${seg.id}`,
    start_ms: Math.round(seg.startMs),
    end_ms: Math.round(seg.endMs),
    zoom: seg.manualZoom ? seg.zoomLevel : null,
    focus: seg.manualCenter
      ? { type: "Fixed", x: seg.centerX, y: seg.centerY }
      : { type: "Auto" },
  };
}
