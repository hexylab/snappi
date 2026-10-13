import { createMemo, For } from "solid-js";
import type { TimelineEvent } from "../lib/types";
import type { ZoomSegment } from "../lib/generated/ZoomSegment";
import { type EditState, formatTime, resizeSegment, resizeTrim } from "../lib/segments";

interface Props {
  durationMs: number;
  timeMs: number;
  segments: ZoomSegment[];
  trim: EditState["trim"];
  selectedId: string | null;
  /** 操作トラックに出すイベント（クリック・キー入力） */
  events: TimelineEvent[];
  /** zoom が null の区間の倍率（表示用） */
  defaultZoom: number;
  onSeek: (ms: number) => void;
  onSelect: (id: string | null) => void;
  /** ドラッグによる編集の開始・途中・終了（Undo はドラッグ 1 回を 1 件にまとめる） */
  onEditBegin: () => void;
  onEditPreview: (state: EditState) => void;
  onEditEnd: () => void;
}

/** これ未満のマウス移動はクリックとみなす (px) */
const DRAG_THRESHOLD_PX = 3;
const MARKER_BUCKETS = 1000;

const MARKER_STYLE: Record<string, { color: string; y1: number; y2: number }> = {
  click: { color: "rgba(96,165,250,0.9)", y1: 2, y2: 10 },
  key: { color: "rgba(74,222,128,0.8)", y1: 12, y2: 20 },
};

export default function Timeline(props: Props) {
  let tracksRef!: HTMLDivElement;

  const pct = (ms: number) => (props.durationMs > 0 ? (ms / props.durationMs) * 100 : 0);
  const clientXToMs = (clientX: number) => {
    const rect = tracksRef.getBoundingClientRect();
    const r = Math.max(0, Math.min(1, (clientX - rect.left) / rect.width));
    return r * props.durationMs;
  };

  /** 時間軸の目盛り。幅に対して 8〜16 本程度になる間隔を選ぶ */
  const ticks = createMemo(() => {
    const steps = [500, 1000, 2000, 5000, 10000, 15000, 30000, 60000, 120000];
    const step = steps.find((s) => props.durationMs / s <= 14) ?? 300000;
    const out: { t: number; label: string }[] = [];
    for (let t = 0; t <= props.durationMs; t += step) {
      const sec = t / 1000;
      const label = step % 1000 === 0
        ? `${Math.floor(sec / 60)}:${String(Math.floor(sec % 60)).padStart(2, "0")}`
        : formatTime(t);
      out.push({ t, label });
    }
    return out;
  });

  /** 操作マーカー（同じ位置に重なるものは間引く） */
  const markers = createMemo(() => {
    const seen = new Set<string>();
    return props.events.filter((e) => {
      if (!MARKER_STYLE[e.event_type]) return false;
      const key = `${e.event_type}:${Math.round((e.time_ms / Math.max(1, props.durationMs)) * MARKER_BUCKETS)}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });
  });

  /** マウスを押してから離すまでを追う。動かさずに離したら onClick */
  const track = (
    e: MouseEvent,
    handlers: { onMove: (ev: MouseEvent) => void; onClick?: (ev: MouseEvent) => void; edit?: boolean },
  ) => {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    const startX = e.clientX;
    let moved = false;
    if (handlers.edit) props.onEditBegin();
    const move = (ev: MouseEvent) => {
      if (!moved && Math.abs(ev.clientX - startX) < DRAG_THRESHOLD_PX) return;
      moved = true;
      handlers.onMove(ev);
    };
    const up = (ev: MouseEvent) => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      if (handlers.edit) props.onEditEnd();
      if (!moved) handlers.onClick?.(ev);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  /** 目盛り・空いている場所: クリック・ドラッグで再生位置を動かす */
  const scrub = (e: MouseEvent, deselect: boolean) => {
    if (e.button !== 0) return;
    if (deselect) props.onSelect(null);
    props.onSeek(clientXToMs(e.clientX));
    track(e, { onMove: (ev) => props.onSeek(clientXToMs(ev.clientX)) });
  };

  /** 区間をクリック: 選択してその位置へ移動する */
  const selectSegment = (e: MouseEvent, seg: ZoomSegment) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    props.onSelect(seg.id);
    props.onSeek(clientXToMs(e.clientX));
  };

  const dragSegmentEdge = (e: MouseEvent, seg: ZoomSegment, edge: "start" | "end") => {
    const base = props.segments;
    const trim = props.trim;
    props.onSelect(seg.id);
    track(e, {
      edit: true,
      onMove: (ev) =>
        props.onEditPreview({
          segments: resizeSegment(base, seg.id, edge, clientXToMs(ev.clientX), props.durationMs),
          trim,
        }),
    });
  };

  const dragTrim = (e: MouseEvent, edge: "start" | "end") => {
    const segments = props.segments;
    const base = props.trim;
    track(e, {
      edit: true,
      onMove: (ev) => {
        const trim = resizeTrim(base, edge, clientXToMs(ev.clientX), props.durationMs);
        props.onEditPreview({ segments, trim });
        props.onSeek(edge === "start" ? trim.start_ms : trim.end_ms);
      },
    });
  };

  const zoomLabel = (seg: ZoomSegment) => `${(seg.zoom ?? props.defaultZoom).toFixed(1)}x`;

  return (
    <div class="flex select-none text-[10px] text-slate-500">
      {/* トラック名 */}
      <div class="w-14 flex-shrink-0 flex flex-col">
        <div class="h-5" />
        <div class="h-11 flex items-center">ズーム</div>
        <div class="h-6 flex items-center">操作</div>
      </div>

      <div ref={tracksRef} class="relative flex-1 min-w-0">
        {/* 目盛り */}
        <div class="relative h-5 cursor-pointer border-b border-slate-700/60" onMouseDown={(e) => scrub(e, false)}>
          <For each={ticks()}>
            {(tick) => (
              <div class="absolute top-0 bottom-0 pointer-events-none" style={{ left: `${pct(tick.t)}%` }}>
                <div class="w-px h-1.5 bg-slate-600" />
                <span class="absolute top-1.5 -translate-x-1/2 font-mono text-[9px] text-slate-500 whitespace-nowrap">
                  {tick.label}
                </span>
              </div>
            )}
          </For>
        </div>

        {/* ズーム区間トラック */}
        <div
          class="relative h-11 my-0.5 rounded bg-slate-800/60 cursor-pointer"
          onMouseDown={(e) => scrub(e, true)}
        >
          <For each={props.segments}>
            {(seg) => {
              const selected = () => props.selectedId === seg.id;
              return (
                <div
                  class={`zoom-segment absolute top-1 bottom-1 rounded-md cursor-pointer overflow-hidden ${
                    selected()
                      ? "bg-purple-500/50 ring-2 ring-purple-300 z-10"
                      : "bg-purple-500/30 border border-purple-400/50 hover:bg-purple-500/40"
                  }`}
                  style={{ left: `${pct(seg.start_ms)}%`, width: `${pct(seg.end_ms - seg.start_ms)}%`, "min-width": "6px" }}
                  onMouseDown={(e) => selectSegment(e, seg)}
                >
                  <span class="absolute inset-0 flex items-center justify-center text-[10px] text-purple-100 pointer-events-none whitespace-nowrap">
                    {zoomLabel(seg)}
                  </span>
                  <div
                    class="absolute left-0 top-0 bottom-0 w-2 cursor-ew-resize hover:bg-purple-300/50"
                    onMouseDown={(e) => dragSegmentEdge(e, seg, "start")}
                  />
                  <div
                    class="absolute right-0 top-0 bottom-0 w-2 cursor-ew-resize hover:bg-purple-300/50"
                    onMouseDown={(e) => dragSegmentEdge(e, seg, "end")}
                  />
                </div>
              );
            }}
          </For>
        </div>

        {/* 操作トラック（参考表示） */}
        <div class="relative h-6 rounded bg-slate-800/30 cursor-pointer" onMouseDown={(e) => scrub(e, false)}>
          <svg class="absolute inset-0 w-full h-full pointer-events-none" viewBox="0 0 1000 22" preserveAspectRatio="none">
            <For each={markers()}>
              {(m) => {
                const style = MARKER_STYLE[m.event_type];
                const x = (m.time_ms / Math.max(1, props.durationMs)) * 1000;
                return (
                  <line
                    x1={x}
                    x2={x}
                    y1={style.y1}
                    y2={style.y2}
                    stroke={style.color}
                    stroke-width="1.5"
                    vector-effect="non-scaling-stroke"
                  />
                );
              }}
            </For>
          </svg>
        </div>

        {/* トリムの外側を暗くする */}
        <div
          class="absolute top-0 bottom-0 left-0 bg-black/55 pointer-events-none"
          style={{ width: `${pct(props.trim.start_ms)}%` }}
        />
        <div
          class="absolute top-0 bottom-0 right-0 bg-black/55 pointer-events-none"
          style={{ width: `${100 - pct(props.trim.end_ms)}%` }}
        />

        {/* トリムのハンドル */}
        {/* 掴みやすいよう、つまみ (6px) の外側まで当たり判定を広げる */}
        <div
          class="absolute top-0 bottom-0 w-5 -ml-3.5 cursor-ew-resize z-20 group"
          style={{ left: `${pct(props.trim.start_ms)}%` }}
          onMouseDown={(e) => dragTrim(e, "start")}
          title={`書き出しの開始 ${formatTime(props.trim.start_ms)}（ドラッグでトリム）`}
        >
          <div class="absolute inset-y-0 right-1.5 w-1.5 rounded-l bg-amber-400/90 group-hover:bg-amber-300" />
        </div>
        <div
          class="absolute top-0 bottom-0 w-5 -ml-1.5 cursor-ew-resize z-20 group"
          style={{ left: `${pct(props.trim.end_ms)}%` }}
          onMouseDown={(e) => dragTrim(e, "end")}
          title={`書き出しの終了 ${formatTime(props.trim.end_ms)}（ドラッグでトリム）`}
        >
          <div class="absolute inset-y-0 left-1.5 w-1.5 rounded-r bg-amber-400/90 group-hover:bg-amber-300" />
        </div>

        {/* 再生位置 */}
        <div
          class="absolute top-0 bottom-0 w-px bg-white pointer-events-none z-30"
          style={{ left: `${pct(props.timeMs)}%` }}
        >
          <div class="absolute -top-0.5 -left-1 w-2 h-2 rotate-45 bg-white" />
        </div>
      </div>
    </div>
  );
}
