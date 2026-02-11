import { createSignal, For, Show, onCleanup } from "solid-js";
import type { ZoomSegment } from "../lib/zoomSegments";

interface ContextMenu {
  x: number;
  y: number;
  segmentId: number | null;
  /** 隣接セグメントペア（統合用） */
  mergePair?: [number, number];
}

interface Props {
  segments: ZoomSegment[];
  durationMs: number;
  currentTimeMs: number;
  selectedSegmentId: number | null;
  onSelectSegment: (id: number | null) => void;
  onUpdateSegment: (id: number, updates: Partial<ZoomSegment>) => void;
  onAddSegment: (timeMs: number) => void;
  onDeleteSegment: (id: number) => void;
  onMergeSegments: (id1: number, id2: number) => void;
  onSegmentResizeEnd?: (id: number) => void;
}

export default function ZoomTrack(props: Props) {
  let trackRef: HTMLDivElement | undefined;
  const [dragState, setDragState] = createSignal<{
    segmentId: number;
    edge: "start" | "end";
    initialMs: number;
  } | null>(null);
  const [contextMenu, setContextMenu] = createSignal<ContextMenu | null>(null);

  const msToPercent = (ms: number) => {
    if (props.durationMs <= 0) return 0;
    return (ms / props.durationMs) * 100;
  };

  const pxToMs = (clientX: number): number => {
    if (!trackRef) return 0;
    const rect = trackRef.getBoundingClientRect();
    const pct = Math.max(0, Math.min(1, (clientX - rect.left) / rect.width));
    return Math.round(pct * props.durationMs);
  };

  // トラック空白クリック → 新セグメント追加
  const handleTrackClick = (e: MouseEvent) => {
    if (dragState()) return;
    const timeMs = pxToMs(e.clientX);
    // 既存セグメント上のクリックは無視（セグメント側で処理）
    const clickedSeg = props.segments.find(
      s => timeMs >= s.startMs && timeMs <= s.endMs
    );
    if (!clickedSeg) {
      props.onAddSegment(timeMs);
    }
  };

  // セグメントクリック → 選択
  const handleSegmentClick = (e: MouseEvent, id: number) => {
    e.stopPropagation();
    if (dragState()) return;
    props.onSelectSegment(id);
  };

  // ドラッグ開始（リサイズハンドル）
  const handleDragStart = (e: MouseEvent, segmentId: number, edge: "start" | "end") => {
    e.stopPropagation();
    e.preventDefault();
    const seg = props.segments.find(s => s.id === segmentId);
    if (!seg) return;
    setDragState({
      segmentId,
      edge,
      initialMs: edge === "start" ? seg.startMs : seg.endMs,
    });
  };

  // ドラッグ移動/終了（隣接セグメントとの重なり防止付き）
  const handleMouseMove = (e: MouseEvent) => {
    const ds = dragState();
    if (!ds) return;
    const timeMs = Math.max(0, Math.min(props.durationMs, pxToMs(e.clientX)));
    const seg = props.segments.find(s => s.id === ds.segmentId);
    if (!seg) return;

    const sorted = [...props.segments].sort((a, b) => a.startMs - b.startMs);
    const idx = sorted.findIndex(s => s.id === ds.segmentId);

    if (ds.edge === "start") {
      // 前のセグメントのendMsより前には行かない
      const prevEnd = idx > 0 ? sorted[idx - 1].endMs : 0;
      const newStart = Math.max(prevEnd, Math.min(timeMs, seg.endMs - 100));
      props.onUpdateSegment(ds.segmentId, { startMs: newStart });
    } else {
      // 次のセグメントのstartMsより後には行かない
      const nextStart = idx < sorted.length - 1 ? sorted[idx + 1].startMs : props.durationMs;
      const newEnd = Math.min(nextStart, Math.max(timeMs, seg.startMs + 100));
      props.onUpdateSegment(ds.segmentId, { endMs: newEnd });
    }
  };

  // dragStateの変化に応じてglobalイベントを管理
  const startDrag = (e: MouseEvent, segmentId: number, edge: "start" | "end") => {
    handleDragStart(e, segmentId, edge);

    const onMove = (ev: MouseEvent) => handleMouseMove(ev);
    const onUp = () => {
      setDragState(null);
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      // ドラッグ終了時にBBox再計算を通知（クロージャでsegmentIdをキャプチャ）
      props.onSegmentResizeEnd?.(segmentId);
    };

    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };

  // 右クリック → コンテキストメニュー
  const handleContextMenu = (e: MouseEvent, segmentId: number) => {
    e.preventDefault();
    e.stopPropagation();

    // 隣接セグメントの統合チェック
    const sorted = [...props.segments].sort((a, b) => a.startMs - b.startMs);
    const idx = sorted.findIndex(s => s.id === segmentId);
    let mergePair: [number, number] | undefined;
    if (idx > 0) {
      const prev = sorted[idx - 1];
      const curr = sorted[idx];
      // 近接（500ms以内）なら統合提案
      if (curr.startMs - prev.endMs < 500) {
        mergePair = [prev.id, curr.id];
      }
    }
    if (!mergePair && idx < sorted.length - 1) {
      const curr = sorted[idx];
      const next = sorted[idx + 1];
      if (next.startMs - curr.endMs < 500) {
        mergePair = [curr.id, next.id];
      }
    }

    setContextMenu({
      x: e.clientX,
      y: e.clientY,
      segmentId,
      mergePair,
    });
  };

  // コンテキストメニュー外クリックで閉じる
  const closeContextMenu = () => setContextMenu(null);

  // グローバルクリックでメニューを閉じる
  const handleGlobalClick = () => {
    if (contextMenu()) closeContextMenu();
  };

  // mount/unmount
  if (typeof window !== "undefined") {
    window.addEventListener("click", handleGlobalClick);
    onCleanup(() => {
      window.removeEventListener("click", handleGlobalClick);
    });
  }

  return (
    <div class="relative w-full select-none" style={{ height: "28px" }}>
      {/* トラック背景 */}
      <div
        ref={trackRef}
        class="absolute inset-0 cursor-pointer rounded"
        style={{ background: "rgba(255,255,255,0.03)" }}
        onClick={handleTrackClick}
      >
        {/* セグメントブロック */}
        <For each={props.segments}>
          {(seg) => {
            const isSelected = () => props.selectedSegmentId === seg.id;
            const leftPct = () => msToPercent(seg.startMs);
            const widthPct = () => msToPercent(seg.endMs - seg.startMs);

            return (
              <div
                class={`absolute top-0 bottom-0 rounded cursor-pointer transition-colors ${
                  isSelected()
                    ? "bg-purple-500/40 border-2 border-purple-400"
                    : "bg-purple-500/25 border border-purple-500/50 hover:bg-purple-500/35"
                }`}
                style={{
                  left: `${leftPct()}%`,
                  width: `${widthPct()}%`,
                  "min-width": "4px",
                }}
                onClick={(e) => handleSegmentClick(e, seg.id)}
                onContextMenu={(e) => handleContextMenu(e, seg.id)}
              >
                {/* ズーム倍率ラベル */}
                <Show when={widthPct() > 3}>
                  <span class="absolute inset-0 flex items-center justify-center text-[10px] text-purple-200/80 pointer-events-none select-none">
                    {seg.zoomLevel.toFixed(1)}x
                  </span>
                </Show>

                {/* 左リサイズハンドル */}
                <div
                  class="absolute left-0 top-0 bottom-0 w-[6px] cursor-ew-resize z-10 hover:bg-purple-400/40"
                  onMouseDown={(e) => startDrag(e, seg.id, "start")}
                />

                {/* 右リサイズハンドル */}
                <div
                  class="absolute right-0 top-0 bottom-0 w-[6px] cursor-ew-resize z-10 hover:bg-purple-400/40"
                  onMouseDown={(e) => startDrag(e, seg.id, "end")}
                />
              </div>
            );
          }}
        </For>

        {/* 再生位置インジケーター */}
        <div
          class="absolute top-0 bottom-0 w-px bg-white/60 pointer-events-none z-20"
          style={{ left: `${msToPercent(props.currentTimeMs)}%` }}
        />
      </div>

      {/* コンテキストメニュー */}
      <Show when={contextMenu()}>
        {(menu) => (
          <div
            class="fixed bg-zinc-800 border border-zinc-600 rounded shadow-lg py-1 z-50 text-xs"
            style={{
              left: `${menu().x}px`,
              top: `${menu().y}px`,
            }}
            onClick={(e) => e.stopPropagation()}
          >
            <button
              class="w-full text-left px-3 py-1.5 hover:bg-zinc-700 text-red-400"
              onClick={() => {
                if (menu().segmentId != null) {
                  props.onDeleteSegment(menu().segmentId!);
                }
                closeContextMenu();
              }}
            >
              このズーム区間を削除
            </button>
            <Show when={menu().mergePair}>
              {(pair) => (
                <button
                  class="w-full text-left px-3 py-1.5 hover:bg-zinc-700 text-purple-300"
                  onClick={() => {
                    props.onMergeSegments(pair()[0], pair()[1]);
                    closeContextMenu();
                  }}
                >
                  隣接区間と統合
                </button>
              )}
            </Show>
          </div>
        )}
      </Show>
    </div>
  );
}
