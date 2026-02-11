import { createSignal, createMemo, Show, For, onMount } from "solid-js";
import { getRecordingScenes, getRecordingEvents } from "../lib/commands";
import type { SceneInfo, TimelineEvent } from "../lib/types";
import type { ZoomSegment } from "../lib/zoomSegments";
import ZoomTrack from "./ZoomTrack";

interface Props {
  recordingId: string;
  durationMs: number;
  screenWidth: number;
  screenHeight: number;
  currentTimeMs?: number;
  onSeekToTime?: (timeMs: number) => void;
  segments: ZoomSegment[];
  selectedSegmentId: number | null;
  onSelectSegment: (id: number | null) => void;
  onAddSegment: (timeMs: number) => void;
  onDeleteSegment: (id: number) => void;
  onUpdateSegment: (id: number, updates: Partial<ZoomSegment>) => void;
  onMergeSegments: (id1: number, id2: number) => void;
  onSegmentResizeEnd?: (id: number) => void;
}

const EVENT_LANE_HEIGHT = 32;
const STATE_BAND_HEIGHT = 18;
const SCENE_BAND_HEIGHT = 12;

const EVENT_LANE_Y = 0;
const STATE_BAND_Y = EVENT_LANE_HEIGHT;
const SCENE_BAND_Y = STATE_BAND_Y + STATE_BAND_HEIGHT;

type ZoomState = "fullscreen" | "workarea";

interface StateSegment {
  start_ms: number;
  end_ms: number;
  state: ZoomState;
  zone_label: string;
}

const EVENT_TYPE_CONFIG: Record<string, { color: string; lane: number; label: string }> = {
  click:        { color: "rgba(96,165,250,0.9)",  lane: 0, label: "Click" },
  key:          { color: "rgba(74,222,128,0.9)",   lane: 1, label: "Key" },
  scroll:       { color: "rgba(251,146,60,0.9)",   lane: 2, label: "Scroll" },
  focus:        { color: "rgba(168,85,247,0.9)",   lane: 3, label: "Focus" },
  window_focus: { color: "rgba(244,114,182,0.9)",  lane: 3, label: "WinFocus" },
};

const LANE_COUNT = 4;

export default function Timeline(props: Props) {
  const [scenes, setScenes] = createSignal<SceneInfo[]>([]);
  const [events, setEvents] = createSignal<TimelineEvent[]>([]);
  const [loading, setLoading] = createSignal(false);
  const [hoveredSceneIdx, setHoveredSceneIdx] = createSignal<number | null>(null);
  const [hoveredEventIdx, setHoveredEventIdx] = createSignal<number | null>(null);
  const [tooltipPos, setTooltipPos] = createSignal<{ x: number; y: number } | null>(null);
  let containerRef: HTMLDivElement | undefined;

  onMount(async () => {
    setLoading(true);
    try {
      const [scns, evts] = await Promise.all([
        getRecordingScenes(props.recordingId),
        getRecordingEvents(props.recordingId),
      ]);
      setScenes(scns);
      setEvents(evts);
    } catch (e) {
      console.error("Failed to load timeline data:", e);
    }
    setLoading(false);
  });

  const timeToX = (ms: number, width: number) =>
    props.durationMs > 0 ? (ms / props.durationMs) * width : 0;

  const eventDotY = (eventType: string) => {
    const config = EVENT_TYPE_CONFIG[eventType];
    if (!config) return EVENT_LANE_Y + 4;
    const laneH = (EVENT_LANE_HEIGHT - 8) / LANE_COUNT;
    return EVENT_LANE_Y + 4 + config.lane * laneH + laneH / 2;
  };

  const visibleEvents = createMemo(() => {
    const evts = events();
    if (evts.length < 500) return evts;
    const W = 1000;
    const seen = new Set<string>();
    return evts.filter((evt) => {
      const px = Math.round(timeToX(evt.time_ms, W));
      const key = `${evt.event_type}:${px}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });
  });

  // セグメントからステート遷移を計算
  const stateSegments = createMemo((): StateSegment[] => {
    const segs = props.segments;
    if (segs.length === 0) {
      return [{ start_ms: 0, end_ms: props.durationMs, state: "fullscreen", zone_label: "全画面" }];
    }

    const sorted = [...segs].sort((a, b) => a.startMs - b.startMs);
    const result: StateSegment[] = [];
    let cursor = 0;
    let waCounter = 0;

    for (const seg of sorted) {
      if (seg.startMs > cursor) {
        result.push({ start_ms: cursor, end_ms: seg.startMs, state: "fullscreen", zone_label: "全画面" });
      }
      waCounter++;
      result.push({
        start_ms: seg.startMs,
        end_ms: seg.endMs,
        state: "workarea",
        zone_label: `ズーム区間#${waCounter}`,
      });
      cursor = seg.endMs;
    }
    if (cursor < props.durationMs) {
      result.push({ start_ms: cursor, end_ms: props.durationMs, state: "fullscreen", zone_label: "全画面" });
    }
    return result;
  });

  const hoveredSceneInfo = createMemo(() => {
    const idx = hoveredSceneIdx();
    if (idx === null) return null;
    const scene = scenes()[idx];
    if (!scene) return null;

    const sceneEvents = events().filter(
      (e) => e.time_ms >= scene.start_ms && e.time_ms <= scene.end_ms
    );
    const typeCounts: Record<string, number> = {};
    for (const e of sceneEvents) {
      typeCounts[e.event_type] = (typeCounts[e.event_type] || 0) + 1;
    }

    return {
      id: scene.id,
      start_ms: scene.start_ms,
      end_ms: scene.end_ms,
      duration_ms: scene.end_ms - scene.start_ms,
      event_count: scene.event_count,
      zoom_level: scene.zoom_level,
      typeCounts,
    };
  });

  const currentState = createMemo(() => {
    const t = props.currentTimeMs;
    if (t === undefined) return null;
    const segs = stateSegments();
    for (const seg of segs) {
      if (t >= seg.start_ms && t < seg.end_ms) return seg;
    }
    return segs.length > 0 ? segs[segs.length - 1] : null;
  });

  const stateColor = (state: ZoomState) => {
    switch (state) {
      case "fullscreen": return "rgba(100,116,139,0.5)";
      case "workarea": return "rgba(168,85,247,0.6)";
    }
  };

  const formatTime = (ms: number) => {
    const s = Math.floor(ms / 1000);
    const m = Math.floor(s / 60);
    const rem = s % 60;
    const frac = Math.floor((ms % 1000) / 100);
    return `${m}:${rem.toString().padStart(2, "0")}.${frac}`;
  };

  const getZoomAtTime = (timeMs: number) => {
    for (const seg of props.segments) {
      if (timeMs >= seg.startMs && timeMs <= seg.endMs) return seg.zoomLevel;
    }
    return 1.0;
  };

  const handleClick = (e: MouseEvent) => {
    if (!containerRef) return;
    const rect = containerRef.getBoundingClientRect();
    const pct = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
    const timeMs = Math.round(pct * props.durationMs);
    props.onSeekToTime?.(timeMs);
  };

  const TOTAL_HEIGHT = EVENT_LANE_HEIGHT + STATE_BAND_HEIGHT + SCENE_BAND_HEIGHT;

  return (
    <div class="space-y-1">
      <Show when={!loading()} fallback={<div class="text-xs text-slate-500">...</div>}>
        {/* ZoomTrack: 紫ブロック表示 */}
        <div>
          <div class="text-[9px] text-slate-500 mb-0.5">ズーム区間（クリックで追加 / ドラッグでリサイズ）</div>
          <ZoomTrack
            segments={props.segments}
            durationMs={props.durationMs}
            currentTimeMs={props.currentTimeMs ?? 0}
            selectedSegmentId={props.selectedSegmentId}
            onSelectSegment={props.onSelectSegment}
            onUpdateSegment={props.onUpdateSegment}
            onAddSegment={props.onAddSegment}
            onDeleteSegment={props.onDeleteSegment}
            onMergeSegments={props.onMergeSegments}
            onSegmentResizeEnd={props.onSegmentResizeEnd}
          />
        </div>

        {/* Event lane + state band + scene band */}
        <div class="relative" style={{ height: `${TOTAL_HEIGHT + 20}px` }}>
          <div
            ref={containerRef}
            class="relative cursor-crosshair"
            style={{ height: `${TOTAL_HEIGHT}px` }}
            onClick={handleClick}
          >
            <svg
              class="absolute inset-0 w-full"
              viewBox={`0 0 1000 ${TOTAL_HEIGHT}`}
              preserveAspectRatio="none"
              style={{ height: `${TOTAL_HEIGHT}px` }}
            >
              {/* === Event dot lane === */}
              <rect
                x="0" y={EVENT_LANE_Y}
                width="1000" height={EVENT_LANE_HEIGHT}
                fill="rgba(15,23,42,0.3)"
              />
              {[1, 2, 3].map((i) => {
                const laneH = (EVENT_LANE_HEIGHT - 8) / LANE_COUNT;
                return (
                  <line
                    x1="0" y1={EVENT_LANE_Y + 4 + i * laneH}
                    x2="1000" y2={EVENT_LANE_Y + 4 + i * laneH}
                    stroke="rgba(100,116,139,0.1)" stroke-width="1"
                    vector-effect="non-scaling-stroke"
                  />
                );
              })}

              {/* Event dots */}
              <For each={visibleEvents()}>
                {(evt, i) => {
                  const config = () => EVENT_TYPE_CONFIG[evt.event_type];
                  return (
                    <Show when={config()}>
                      <circle
                        cx={timeToX(evt.time_ms, 1000)}
                        cy={eventDotY(evt.event_type)}
                        r={hoveredEventIdx() === i() ? 4 : 2}
                        fill={config()!.color}
                        stroke="rgba(0,0,0,0.3)"
                        stroke-width="0.5"
                        vector-effect="non-scaling-stroke"
                        class="cursor-pointer"
                        onMouseEnter={(e) => {
                          setHoveredEventIdx(i());
                          setTooltipPos({ x: e.clientX, y: e.clientY });
                        }}
                        onMouseLeave={() => {
                          setHoveredEventIdx(null);
                          setTooltipPos(null);
                        }}
                        onClick={(e) => {
                          e.stopPropagation();
                          props.onSeekToTime?.(evt.time_ms);
                        }}
                      />
                    </Show>
                  );
                }}
              </For>

              {/* === State transition band === */}
              <For each={stateSegments()}>
                {(seg) => {
                  const x = () => timeToX(seg.start_ms, 1000);
                  const w = () => timeToX(seg.end_ms, 1000) - x();
                  return (
                    <rect
                      x={x()}
                      y={STATE_BAND_Y + 1}
                      width={Math.max(w(), 1)}
                      height={STATE_BAND_HEIGHT - 2}
                      rx="2"
                      fill={stateColor(seg.state)}
                    />
                  );
                }}
              </For>

              {/* State band labels */}
              <For each={stateSegments()}>
                {(seg) => {
                  const x = () => timeToX(seg.start_ms, 1000);
                  const w = () => timeToX(seg.end_ms, 1000) - x();
                  return (
                    <Show when={w() > 40}>
                      <text
                        x={x() + w() / 2}
                        y={STATE_BAND_Y + STATE_BAND_HEIGHT / 2 + 1}
                        text-anchor="middle"
                        dominant-baseline="central"
                        fill="white"
                        font-size="8"
                        font-family="monospace"
                        vector-effect="non-scaling-stroke"
                        class="pointer-events-none select-none"
                        style={{ "paint-order": "stroke", stroke: "rgba(0,0,0,0.5)", "stroke-width": "3px" }}
                      >
                        {seg.zone_label}
                      </text>
                    </Show>
                  );
                }}
              </For>

              {/* === Scene interval band === */}
              <rect
                x="0" y={SCENE_BAND_Y}
                width="1000" height={SCENE_BAND_HEIGHT}
                fill="rgba(15,23,42,0.2)"
              />
              <For each={scenes()}>
                {(scene, i) => {
                  const x = () => timeToX(scene.start_ms, 1000);
                  const w = () => Math.max(timeToX(scene.end_ms, 1000) - x(), 2);
                  const color = () => i() % 2 === 0
                    ? "rgba(139,92,246,0.4)"
                    : "rgba(59,130,246,0.4)";
                  return (
                    <rect
                      x={x()}
                      y={SCENE_BAND_Y + 1}
                      width={w()}
                      height={SCENE_BAND_HEIGHT - 2}
                      rx="1.5"
                      fill={color()}
                      stroke={hoveredSceneIdx() === i() ? "rgba(255,255,255,0.6)" : "none"}
                      stroke-width="1"
                      vector-effect="non-scaling-stroke"
                      class="cursor-pointer"
                      onMouseEnter={(e) => {
                        setHoveredSceneIdx(i());
                        setTooltipPos({ x: e.clientX, y: e.clientY });
                      }}
                      onMouseLeave={() => {
                        setHoveredSceneIdx(null);
                        setTooltipPos(null);
                      }}
                      onClick={(e) => {
                        e.stopPropagation();
                        props.onSeekToTime?.(scene.start_ms);
                      }}
                    />
                  );
                }}
              </For>

              {/* Scene band labels */}
              <For each={scenes()}>
                {(scene, i) => {
                  const x = () => timeToX(scene.start_ms, 1000);
                  const w = () => timeToX(scene.end_ms, 1000) - x();
                  return (
                    <Show when={w() > 25}>
                      <text
                        x={x() + w() / 2}
                        y={SCENE_BAND_Y + SCENE_BAND_HEIGHT / 2 + 1}
                        text-anchor="middle"
                        dominant-baseline="central"
                        fill="white"
                        font-size="7"
                        font-family="monospace"
                        class="pointer-events-none select-none"
                        style={{ opacity: "0.7" }}
                      >
                        S{scene.id}
                      </text>
                    </Show>
                  );
                }}
              </For>

              {/* Current time playhead */}
              <Show when={props.currentTimeMs !== undefined}>
                <line
                  x1={timeToX(props.currentTimeMs!, 1000)}
                  y1={0}
                  x2={timeToX(props.currentTimeMs!, 1000)}
                  y2={TOTAL_HEIGHT}
                  stroke="rgba(255,255,255,0.7)"
                  stroke-width="1.5"
                  vector-effect="non-scaling-stroke"
                />
              </Show>
            </svg>
          </div>

          {/* Time labels */}
          <div class="flex justify-between text-[9px] text-slate-500 font-mono mt-0.5">
            <span>0:00</span>
            <span>{formatTime(Math.round(props.durationMs / 4))}</span>
            <span>{formatTime(Math.round(props.durationMs / 2))}</span>
            <span>{formatTime(Math.round(props.durationMs * 3 / 4))}</span>
            <span>{formatTime(props.durationMs)}</span>
          </div>
        </div>

        {/* Tooltip */}
        <Show when={tooltipPos() && (hoveredSceneIdx() !== null || hoveredEventIdx() !== null)}>
          <div
            class="fixed z-50 bg-slate-900 border border-slate-600 rounded-lg px-3 py-2 text-[11px] font-mono text-slate-300 shadow-xl pointer-events-none"
            style={{
              left: `${tooltipPos()!.x + 12}px`,
              top: `${tooltipPos()!.y - 10}px`,
            }}
          >
            <Show when={hoveredSceneIdx() !== null && hoveredSceneInfo()}>
              {(info) => (
                <div class="space-y-1">
                  <div class="text-slate-200 font-medium">シーン #{info().id}</div>
                  <div>
                    {formatTime(info().start_ms)} - {formatTime(info().end_ms)}
                    <span class="text-slate-500 ml-1">({(info().duration_ms / 1000).toFixed(1)}s)</span>
                  </div>
                  <div>ズーム: <span class="text-purple-300">{info().zoom_level.toFixed(2)}x</span></div>
                  <div class="text-slate-500">{info().event_count} アクティビティ</div>
                  <div class="flex flex-wrap gap-1.5 mt-1">
                    <For each={Object.entries(info().typeCounts)}>
                      {([type_, count]) => (
                        <span
                          class="px-1.5 py-0.5 rounded text-[10px]"
                          style={{
                            background: EVENT_TYPE_CONFIG[type_]?.color ?? "rgba(100,116,139,0.5)",
                            color: "white",
                          }}
                        >
                          {EVENT_TYPE_CONFIG[type_]?.label ?? type_} x{count}
                        </span>
                      )}
                    </For>
                  </div>
                </div>
              )}
            </Show>

            <Show when={hoveredEventIdx() !== null && hoveredSceneIdx() === null}>
              {(_) => {
                const evt = () => visibleEvents()[hoveredEventIdx()!];
                return (
                  <Show when={evt()}>
                    {(e) => (
                      <div class="space-y-0.5">
                        <span
                          class="inline-block px-1.5 py-0.5 rounded text-[10px] font-medium"
                          style={{
                            background: EVENT_TYPE_CONFIG[e().event_type]?.color ?? "gray",
                            color: "white",
                          }}
                        >
                          {EVENT_TYPE_CONFIG[e().event_type]?.label ?? e().event_type}
                        </span>
                        <div class="text-slate-400">{formatTime(e().time_ms)}</div>
                        <Show when={e().x !== null && e().y !== null}>
                          <div class="text-orange-300">({Math.round(e().x!)}, {Math.round(e().y!)})</div>
                        </Show>
                        <Show when={e().label}>
                          <div class="text-slate-300">{e().label}</div>
                        </Show>
                      </div>
                    )}
                  </Show>
                );
              }}
            </Show>
          </div>
        </Show>

        {/* Current state info bar */}
        <Show when={props.currentTimeMs !== undefined}>
          <div class="flex items-center gap-4 px-2 py-1.5 bg-slate-800/60 rounded text-[11px] font-mono text-slate-400">
            <Show when={currentState()}>
              {(seg) => (
                <span class="flex items-center gap-1.5">
                  <span
                    class="inline-block w-2 h-2 rounded-sm"
                    style={{ background: stateColor(seg().state) }}
                  />
                  <span class="text-slate-200">{seg().zone_label}</span>
                </span>
              )}
            </Show>
            <span>
              ズーム: <span class="text-purple-300 font-medium">{getZoomAtTime(props.currentTimeMs!).toFixed(2)}x</span>
            </span>
          </div>
        </Show>

        {/* Legend */}
        <div class="flex items-center gap-3 text-[10px] text-slate-500 flex-wrap">
          <span class="flex items-center gap-1">
            <span class="inline-block w-3 h-2 rounded-sm" style={{ background: "rgba(100,116,139,0.5)" }} />
            全画面
          </span>
          <span class="flex items-center gap-1">
            <span class="inline-block w-3 h-2 rounded-sm" style={{ background: "rgba(168,85,247,0.6)" }} />
            ズーム区間
          </span>
          <span class="flex items-center gap-1">
            <span class="inline-block w-3 h-2 rounded-sm" style={{ background: "rgba(139,92,246,0.4)" }} />
            シーン
          </span>
          <span class="text-slate-600">|</span>
          <span class="text-slate-600">{props.segments.length} 区間 / {scenes().length} シーン / {events().length} イベント</span>
        </div>
      </Show>
    </div>
  );
}
