import { Show } from "solid-js";
import type { ZoomSegment } from "../lib/zoomSegments";

interface Props {
  segment: ZoomSegment;
  index: number;
  totalCount: number;
  onChange: (id: number, updates: Partial<ZoomSegment>) => void;
  onDelete: (id: number) => void;
  onClose: () => void;
  onNavigate: (direction: -1 | 1) => void;
  editMode: "position" | null;
  onToggleEditMode: (mode: "position" | null) => void;
}

function formatTime(ms: number): string {
  const totalSec = ms / 1000;
  const min = Math.floor(totalSec / 60);
  const sec = totalSec % 60;
  return `${min}:${sec.toFixed(1).padStart(4, "0")}`;
}

export default function SegmentEditor(props: Props) {
  return (
    <div class="bg-zinc-800/90 border border-zinc-600 rounded-lg p-3 text-xs">
      {/* ヘッダー: ナビゲーション + クローズ */}
      <div class="flex items-center justify-between mb-3">
        <div class="flex items-center gap-1">
          {/* 前へ */}
          <button
            class="p-1 rounded hover:bg-zinc-700 disabled:opacity-30 disabled:cursor-not-allowed"
            disabled={props.index <= 0}
            onClick={() => props.onNavigate(-1)}
            title="前のズーム区間"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <polyline points="15 18 9 12 15 6" />
            </svg>
          </button>

          <span class="text-purple-300 font-medium px-1">
            ズーム区間 #{props.index + 1}
          </span>
          <span class="text-zinc-400">
            ({formatTime(props.segment.startMs)} - {formatTime(props.segment.endMs)})
          </span>

          {/* 次へ */}
          <button
            class="p-1 rounded hover:bg-zinc-700 disabled:opacity-30 disabled:cursor-not-allowed"
            disabled={props.index >= props.totalCount - 1}
            onClick={() => props.onNavigate(1)}
            title="次のズーム区間"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <polyline points="9 18 15 12 9 6" />
            </svg>
          </button>
        </div>

        <button
          class="p-1 rounded hover:bg-zinc-700 text-zinc-400 hover:text-zinc-200"
          onClick={props.onClose}
          title="閉じる"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <line x1="18" y1="6" x2="6" y2="18" />
            <line x1="6" y1="6" x2="18" y2="18" />
          </svg>
        </button>
      </div>

      {/* ズーム倍率スライダー */}
      <div class="mb-3">
        <div class="flex items-center justify-between mb-1">
          <label class="text-zinc-400">ズーム倍率</label>
          <span class="text-purple-300 font-mono">{props.segment.zoomLevel.toFixed(1)}x</span>
        </div>
        <input
          type="range"
          min="1.2"
          max="5.0"
          step="0.1"
          value={props.segment.zoomLevel}
          onInput={(e) =>
            props.onChange(props.segment.id, {
              zoomLevel: parseFloat(e.currentTarget.value),
            })
          }
          class="w-full h-1.5 bg-zinc-700 rounded-full appearance-none cursor-pointer accent-purple-500"
        />
      </div>

      {/* 中心位置 */}
      <div class="mb-3">
        <div class="flex items-center justify-between">
          <label class="text-zinc-400">中心位置</label>
          <span class="text-zinc-300 font-mono text-[10px]">
            ({Math.round(props.segment.centerX)}, {Math.round(props.segment.centerY)})
          </span>
        </div>
        <button
          class={`mt-1 w-full px-2 py-1 rounded text-[10px] transition-colors ${
            props.editMode === "position"
              ? "bg-purple-600 text-white"
              : "bg-zinc-700 text-zinc-300 hover:bg-zinc-600"
          }`}
          onClick={() =>
            props.onToggleEditMode(
              props.editMode === "position" ? null : "position"
            )
          }
        >
          {props.editMode === "position"
            ? "映像をクリックしてください..."
            : "映像をクリックして設定"}
        </button>
      </div>

      {/* フッター: 削除 + カウンター */}
      <div class="flex items-center justify-between border-t border-zinc-700 pt-2">
        <button
          class="px-2 py-1 rounded text-[10px] bg-red-900/30 text-red-400 hover:bg-red-900/50 transition-colors"
          onClick={() => props.onDelete(props.segment.id)}
        >
          このズーム区間を削除
        </button>
        <span class="text-zinc-500 text-[10px]">
          {props.index + 1} / {props.totalCount}
        </span>
      </div>
    </div>
  );
}
