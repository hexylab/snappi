import type { ExportFormat, QualityPreset, ZoomStrength } from "./types";

const WEEKDAYS = ["日", "月", "火", "水", "木", "金", "土"];

/** 録画日時: 10月14日（火）15:32 */
export function formatDate(dateStr: string): string {
  const d = new Date(dateStr);
  if (Number.isNaN(d.getTime())) return dateStr;
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  return `${d.getMonth() + 1}月${d.getDate()}日（${WEEKDAYS[d.getDay()]}）${hh}:${mm}`;
}

/** 長さ: 0:42、12:05 */
export function formatClock(ms: number): string {
  const total = Math.floor(Math.max(0, ms) / 1000);
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}

export const FORMAT_OPTIONS: { value: ExportFormat; label: string; desc: string }[] = [
  { value: "Mp4", label: "MP4", desc: "いちばん扱いやすい動画" },
  { value: "Gif", label: "GIF", desc: "チャットや資料に貼る（音なし）" },
  { value: "WebM", label: "WebM", desc: "Web ページ向け" },
];

/** spec は狭い場所（プレビューのヘッダー）で使う短い表記 */
export const QUALITY_OPTIONS: { value: QualityPreset; label: string; spec: string; desc: string }[] = [
  { value: "Social", label: "標準", spec: "1080p", desc: "1080p・30fps" },
  { value: "HighQuality", label: "高画質", spec: "元の解像度", desc: "元の解像度・60fps" },
  { value: "Lightweight", label: "軽量", spec: "720p", desc: "720p・24fps" },
];

export const STRENGTH_OPTIONS: { value: ZoomStrength; label: string; desc: string }[] = [
  { value: "Off", label: "なし", desc: "ズームしない" },
  { value: "Subtle", label: "控えめ", desc: "1.5倍" },
  { value: "Standard", label: "標準", desc: "1.8倍" },
  { value: "Strong", label: "強め", desc: "2.2倍" },
];

/** ファイル名だけを取り出す */
export function baseName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}
