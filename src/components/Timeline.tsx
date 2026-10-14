import { createMemo, For, Show } from "solid-js";
import type { TimelineEvent } from "../lib/types";
import type { ZoomSegment } from "../lib/generated/ZoomSegment";
import { type EditState, formatTime, resizeSegment, resizeTrim } from "../lib/segments";
import { formatClock } from "../lib/format";

interface Props {
  durationMs: number;
  timeMs: number;
  segments: ZoomSegment[];
  trim: EditState["trim"];
  /**
   * false: シークバー（再生位置と前後のカットだけ）。
   * true: 目盛り・ズーム・操作のトラックを持つタイムライン
   */
  detailed: boolean;
  selectedId: string | null;
  /** 操作トラックに出すイベント（クリック・キー入力） */
  events: TimelineEvent[];
  /** zoom が null の区間の倍率（表示用） */
  defaultZoom: number;
  onSeek: (ms: number) => void;
  onSelect: (id: string | null) => void;
  /** ズームトラックの空いている所をダブルクリックした */
  onAddAt: (ms: number) => void;
  /** ドラッグによる編集の開始・途中・終了（Undo はドラッグ 1 回を 1 件にまとめる） */
  onEditBegin: () => void;
  onEditPreview: (patch: Pick<EditState, "segments" | "trim">) => void;
  onEditEnd: () => void;
}

/** これ未満のマウス移動はクリックとみなす (px) */
const DRAG_THRESHOLD_PX = 3;
const MARKER_BUCKETS = 1000;

const MARKER_STYLE: Record<string, { color: string; y1: number; y2: number }> = {
  click: { color: "var(--color-on-surface-dim)", y1: 1, y2: 7 },
  key: { color: "var(--color-eyebrow)", y1: 9, y2: 15 },
};

export default function Timeline(props: Props) {
  let tracksRef!: HTMLDivElement;

  const pct = (ms: number) => (props.durationMs > 0 ? (ms / props.durationMs) * 100 : 0);
  const clientXToMs = (clientX: number) => {
    const rect = tracksRef.getBoundingClientRect();
    const r = Math.max(0, Math.min(1, (clientX - rect.left) / rect.width));
    return r * props.durationMs;
  };

  /** 時間軸の目盛り。幅に対して 8〜14 本程度になる間隔を選ぶ */
  const ticks = createMemo(() => {
    const steps = [500, 1000, 2000, 5000, 10000, 15000, 30000, 60000, 120000];
    const step = steps.find((s) => props.durationMs / s <= 14) ?? 300000;
    const out: { t: number; label: string }[] = [];
    for (let t = 0; t <= props.durationMs; t += step) {
      out.push({ t, label: step % 1000 === 0 ? formatClock(t) : formatTime(t) });
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

  /** クリック・ドラッグで再生位置を動かす */
  const scrub = (e: MouseEvent, deselect: boolean) => {
    if (e.button !== 0) return;
    if (deselect) props.onSelect(null);
    props.onSeek(clientXToMs(e.clientX));
    track(e, { onMove: (ev) => props.onSeek(clientXToMs(ev.clientX)) });
  };

  const selectSegment = (e: MouseEvent, seg: ZoomSegment) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    props.onSelect(seg.id);
    props.onSeek(clientXToMs(e.clientX));
  };

  const dragSegmentEdge = (e: MouseEvent, seg: ZoomSegment, edge: "start" | "end") => {
    const base = props.segments;
    const { trim } = props;
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
    const base = props.trim;
    track(e, {
      edit: true,
      onMove: (ev) => {
        const trim = resizeTrim(base, edge, clientXToMs(ev.clientX), props.durationMs);
        props.onEditPreview({ segments: props.segments, trim });
        props.onSeek(edge === "start" ? trim.start_ms : trim.end_ms);
      },
    });
  };

  /** 目盛りの行の高さ (px)。カット・再生位置は目盛りの下から描く */
  const topOffset = () => (props.detailed ? "20px" : "0px");

  const zoomLabel = (seg: ZoomSegment) => `${(seg.zoom ?? props.defaultZoom).toFixed(1)}×`;

  return (
    <div class="flex select-none text-[10.5px] text-on-surface-dim">
      <Show when={props.detailed}>
        {/* トラック名 */}
        <div class="flex w-10 flex-shrink-0 flex-col font-semibold">
          <div class="h-5" />
          <div class="flex h-9 items-center">ズーム</div>
          <div class="flex h-5 items-center" title="グレーの線: クリック／茶色の線: キー入力">
            操作
          </div>
        </div>
      </Show>

      <div ref={tracksRef} class="relative min-w-0 flex-1">
        <Show
          when={props.detailed}
          fallback={
            /* シークバー */
            <div
              class="h-7 cursor-pointer rounded-sm border border-outline bg-surface-dim"
              onMouseDown={(e) => scrub(e, false)}
            />
          }
        >
          {/* 目盛り */}
          <div class="relative h-5 cursor-pointer" onMouseDown={(e) => scrub(e, false)}>
            <For each={ticks()}>
              {(tick) => (
                <div class="pointer-events-none absolute bottom-0 top-0" style={{ left: `${pct(tick.t)}%` }}>
                  <div class="h-1.5 w-px bg-select-outline" />
                  <span class="tabular absolute top-1 -translate-x-1/2 whitespace-nowrap text-[10px]">{tick.label}</span>
                </div>
              )}
            </For>
          </div>

          {/* ズーム区間 */}
          <div
            class="relative h-9 cursor-pointer rounded-sm border border-outline bg-surface-dim"
            onMouseDown={(e) => scrub(e, true)}
            onDblClick={(e) => props.onAddAt(clientXToMs(e.clientX))}
          >
            <For each={props.segments}>
              {(seg) => {
                const selected = () => props.selectedId === seg.id;
                return (
                  <div
                    class="absolute bottom-1 top-1 cursor-pointer overflow-hidden rounded-[8px] border transition-colors"
                    classList={{
                      "z-10 border-primary-deep-outline bg-primary-container shadow-[0_0_0_1px_var(--color-primary-deep-outline)]":
                        selected(),
                      "border-option-outline bg-surface hover:bg-primary-container": !selected(),
                    }}
                    style={{
                      left: `${pct(seg.start_ms)}%`,
                      width: `${pct(seg.end_ms - seg.start_ms)}%`,
                      "min-width": "8px",
                    }}
                    title="クリックで選択・端をドラッグで長さを変更"
                    onMouseDown={(e) => selectSegment(e, seg)}
                    onDblClick={(e) => e.stopPropagation()}
                  >
                    <span class="pointer-events-none absolute inset-0 flex items-center justify-center whitespace-nowrap font-semibold text-on-primary-container">
                      🔍 {zoomLabel(seg)}
                    </span>
                    <div
                      class="absolute bottom-0 left-0 top-0 w-2 cursor-ew-resize hover:bg-primary-deep/25"
                      onMouseDown={(e) => dragSegmentEdge(e, seg, "start")}
                    />
                    <div
                      class="absolute bottom-0 right-0 top-0 w-2 cursor-ew-resize hover:bg-primary-deep/25"
                      onMouseDown={(e) => dragSegmentEdge(e, seg, "end")}
                    />
                  </div>
                );
              }}
            </For>
          </div>

          {/* 操作（参考表示） */}
          <div class="relative mt-1 h-4 cursor-pointer" onMouseDown={(e) => scrub(e, false)}>
            <svg class="pointer-events-none absolute inset-0 h-full w-full" viewBox="0 0 1000 16" preserveAspectRatio="none">
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
        </Show>

        {/* カットした外側 */}
        <div
          class="pointer-events-none absolute bottom-0 left-0 rounded-l-sm bg-on-surface/15"
          style={{ top: topOffset(), width: `${pct(props.trim.start_ms)}%` }}
        />
        <div
          class="pointer-events-none absolute bottom-0 right-0 rounded-r-sm bg-on-surface/15"
          style={{ top: topOffset(), width: `${100 - pct(props.trim.end_ms)}%` }}
        />

        {/* カットのつまみ（掴みやすいよう、見た目より広く当たり判定を取る） */}
        <div
          class="group absolute bottom-0 z-20 -ml-4 w-5 cursor-ew-resize"
          style={{ top: topOffset(), left: `${pct(props.trim.start_ms)}%` }}
          onMouseDown={(e) => dragTrim(e, "start")}
          title={`ここから書き出す（${formatTime(props.trim.start_ms)}）。ドラッグで前をカット`}
        >
          <div class="absolute inset-y-0 right-0 flex w-2.5 items-center justify-center rounded-l-[6px] bg-primary-deep group-hover:bg-primary-strong">
            <div class="h-3 w-px bg-on-primary/80" />
          </div>
        </div>
        <div
          class="group absolute bottom-0 z-20 -ml-1 w-5 cursor-ew-resize"
          style={{ top: topOffset(), left: `${pct(props.trim.end_ms)}%` }}
          onMouseDown={(e) => dragTrim(e, "end")}
          title={`ここまで書き出す（${formatTime(props.trim.end_ms)}）。ドラッグで後ろをカット`}
        >
          <div class="absolute inset-y-0 left-0 flex w-2.5 items-center justify-center rounded-r-[6px] bg-primary-deep group-hover:bg-primary-strong">
            <div class="h-3 w-px bg-on-primary/80" />
          </div>
        </div>

        {/* 再生位置 */}
        <div
          class="pointer-events-none absolute bottom-0 z-30 w-0.5 -translate-x-1/2 bg-on-surface"
          style={{ top: topOffset(), left: `${pct(props.timeMs)}%` }}
        >
          <div class="absolute -left-[4px] -top-1 h-2.5 w-2.5 rounded-full bg-on-surface" />
        </div>
      </div>
    </div>
  );
}
