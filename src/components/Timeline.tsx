import { createSignal, createMemo, Show, For, onMount } from "solid-js";
import { getZoomKeyframes, getRecordingScenes, getRecordingEvents, applySceneEdits } from "../lib/commands";
import type { ZoomKeyframe, SceneInfo, TimelineEvent, SceneEditOp } from "../lib/types";

interface Props {
  recordingId: string;
  durationMs: number;
  screenWidth: number;
  screenHeight: number;
  currentTimeMs?: number;
  onSeekToTime?: (timeMs: number) => void;
  onKeyframesChange?: (keyframes: ZoomKeyframe[]) => void;
}

const GRAPH_HEIGHT = 80;
const GRAPH_PADDING_TOP = 8;
const GRAPH_PADDING_BOTTOM = 4;
const EVENT_LANE_HEIGHT = 32;
const STATE_BAND_HEIGHT = 18;
const SCENE_BAND_HEIGHT = 12;

const EVENT_LANE_Y = GRAPH_HEIGHT;
const STATE_BAND_Y = GRAPH_HEIGHT + EVENT_LANE_HEIGHT;
const SCENE_BAND_Y = STATE_BAND_Y + STATE_BAND_HEIGHT;

type ZoomState = "fullscreen" | "window" | "workarea";

interface StateSegment {
  start_ms: number;
  end_ms: number;
  state: ZoomState;
  window_title: string | null;
  zone_id: number; // scene index or -1 for fullscreen
  zone_label: string; // e.g. "WorkArea#1", "全画面"
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
  const [keyframes, setKeyframes] = createSignal<ZoomKeyframe[]>([]);
  const [scenes, setScenes] = createSignal<SceneInfo[]>([]);
  const [events, setEvents] = createSignal<TimelineEvent[]>([]);
  const [loading, setLoading] = createSignal(false);
  const [hoveredKfIdx, setHoveredKfIdx] = createSignal<number | null>(null);
  const [hoveredSceneIdx, setHoveredSceneIdx] = createSignal<number | null>(null);
  const [hoveredEventIdx, setHoveredEventIdx] = createSignal<number | null>(null);
  const [tooltipPos, setTooltipPos] = createSignal<{ x: number; y: number } | null>(null);
  const [pendingEdits, setPendingEdits] = createSignal<SceneEditOp[]>([]);
  const [hoveredBoundaryIdx, setHoveredBoundaryIdx] = createSignal<number | null>(null);
  const [splitPreviewTime, setSplitPreviewTime] = createSignal<number | null>(null);
  const [ctrlHeld, setCtrlHeld] = createSignal(false);
  const [editApplying, setEditApplying] = createSignal(false);
  let containerRef: HTMLDivElement | undefined;

  onMount(async () => {
    setLoading(true);
    try {
      const [kfs, scns, evts] = await Promise.all([
        getZoomKeyframes(props.recordingId),
        getRecordingScenes(props.recordingId),
        getRecordingEvents(props.recordingId),
      ]);
      setKeyframes(kfs);
      setScenes(scns);
      setEvents(evts);
      props.onKeyframesChange?.(kfs);
    } catch (e) {
      console.error("Failed to load timeline data:", e);
    }
    setLoading(false);

    const onKeyDown = (e: KeyboardEvent) => { if (e.key === "Control") setCtrlHeld(true); };
    const onKeyUp = (e: KeyboardEvent) => { if (e.key === "Control") { setCtrlHeld(false); setSplitPreviewTime(null); } };
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
  });

  const applyEdit = async (newEdit: SceneEditOp) => {
    if (editApplying()) return;
    setEditApplying(true);
    try {
      const allEdits = [...pendingEdits(), newEdit];
      const result = await applySceneEdits(props.recordingId, allEdits);
      setPendingEdits(allEdits);
      setScenes(result.scenes);
      setKeyframes(result.keyframes);
      props.onKeyframesChange?.(result.keyframes);
    } catch (e) {
      console.error("Failed to apply scene edit:", e);
    }
    setEditApplying(false);
  };

  const resetEdits = async () => {
    if (editApplying()) return;
    setEditApplying(true);
    try {
      const [kfs, scns] = await Promise.all([
        getZoomKeyframes(props.recordingId),
        getRecordingScenes(props.recordingId),
      ]);
      setPendingEdits([]);
      setScenes(scns);
      setKeyframes(kfs);
      props.onKeyframesChange?.(kfs);
    } catch (e) {
      console.error("Failed to reset edits:", e);
    }
    setEditApplying(false);
  };

  const maxZoom = createMemo(() => {
    const kfs = keyframes();
    if (kfs.length === 0) return 2.0;
    return Math.max(2.0, ...kfs.map((kf) => kf.zoom_level)) + 0.2;
  });

  const timeToX = (ms: number, width: number) =>
    props.durationMs > 0 ? (ms / props.durationMs) * width : 0;

  const zoomToY = (zoom: number) => {
    const usable = GRAPH_HEIGHT - GRAPH_PADDING_TOP - GRAPH_PADDING_BOTTOM;
    const norm = (zoom - 1.0) / (maxZoom() - 1.0);
    return GRAPH_HEIGHT - GRAPH_PADDING_BOTTOM - norm * usable;
  };

  const eventDotY = (eventType: string) => {
    const config = EVENT_TYPE_CONFIG[eventType];
    if (!config) return EVENT_LANE_Y + 4;
    const laneH = (EVENT_LANE_HEIGHT - 8) / LANE_COUNT;
    return EVENT_LANE_Y + 4 + config.lane * laneH + laneH / 2;
  };

  // Deduplicate events on the same pixel to prevent SVG overload
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

  // Build SVG polyline path for zoom level graph
  const zoomPath = createMemo(() => {
    const kfs = keyframes();
    if (kfs.length === 0) return "";
    const W = 1000;
    const points = kfs.map((kf) => {
      const x = timeToX(kf.time_ms, W);
      const y = zoomToY(kf.zoom_level);
      return `${x},${y}`;
    });
    const lastKf = kfs[kfs.length - 1];
    points.push(`${W},${zoomToY(lastKf.zoom_level)}`);
    return `M${points.join(" L")}`;
  });

  // Fill area under the zoom curve
  const zoomFill = createMemo(() => {
    const kfs = keyframes();
    if (kfs.length === 0) return "";
    const W = 1000;
    const points = kfs.map((kf) => {
      const x = timeToX(kf.time_ms, W);
      const y = zoomToY(kf.zoom_level);
      return `${x},${y}`;
    });
    const lastKf = kfs[kfs.length - 1];
    points.push(`${W},${zoomToY(lastKf.zoom_level)}`);
    const baseline = zoomToY(1.0);
    return `M${points.join(" L")} L${W},${baseline} L${timeToX(kfs[0].time_ms, W)},${baseline} Z`;
  });

  // Detect center coordinate changes (where camera pans)
  const panMarkers = createMemo(() => {
    const kfs = keyframes();
    if (kfs.length < 2) return [];
    const markers: { time_ms: number; from: { x: number; y: number }; to: { x: number; y: number }; idx: number }[] = [];
    for (let i = 1; i < kfs.length; i++) {
      const prev = kfs[i - 1];
      const curr = kfs[i];
      const dx = curr.target_x - prev.target_x;
      const dy = curr.target_y - prev.target_y;
      const dist = Math.sqrt(dx * dx + dy * dy);
      if (dist > 30) {
        markers.push({
          time_ms: curr.time_ms,
          from: { x: prev.target_x, y: prev.target_y },
          to: { x: curr.target_x, y: curr.target_y },
          idx: i,
        });
      }
    }
    return markers;
  });

  // Classify zoom state for each time segment using keyframes and scenes.
  // Each zoom point change (different scene) creates a new WorkArea segment.
  const stateSegments = createMemo((): StateSegment[] => {
    const kfs = keyframes();
    const scns = scenes();
    if (kfs.length === 0) return [];

    const segments: StateSegment[] = [];

    // Find the scene whose time range best matches the keyframe
    const getSceneAt = (timeMs: number): SceneInfo | null => {
      for (const s of scns) {
        if (timeMs >= s.start_ms && timeMs <= s.end_ms) return s;
      }
      // Anticipation: keyframe may be placed before scene starts.
      // Find the next scene that starts after this keyframe.
      let nextScene: SceneInfo | null = null;
      for (const s of scns) {
        if (s.start_ms > timeMs) { nextScene = s; break; }
      }
      // Also find the previous scene
      let prevScene: SceneInfo | null = null;
      for (const s of scns) {
        if (s.end_ms <= timeMs) prevScene = s;
        else break;
      }
      // Prefer next scene (anticipation target) if within 3s
      if (nextScene && nextScene.start_ms - timeMs < 3000) return nextScene;
      return prevScene;
    };

    for (let i = 0; i < kfs.length; i++) {
      const kf = kfs[i];
      const nextKf = i + 1 < kfs.length ? kfs[i + 1] : null;
      const end_ms = nextKf ? nextKf.time_ms : props.durationMs;

      const scene = getSceneAt(kf.time_ms);
      const sceneId = scene?.id ?? -1;

      let state: ZoomState;
      if (kf.transition === "SpringOut") {
        state = "fullscreen";
      } else if (scene?.window_rect) {
        const wr = scene.window_rect;
        const winZoomW = props.screenWidth / (wr.width * 1.1);
        const winZoomH = props.screenHeight / (wr.height * 1.1);
        const winZoom = Math.min(winZoomW, winZoomH);
        if (kf.zoom_level <= winZoom * 1.2) {
          state = "window";
        } else {
          state = "workarea";
        }
      } else {
        state = "workarea";
      }

      const windowTitle = scene?.window_title ?? null;
      const zoneId = state === "fullscreen" ? -1 : sceneId;

      // Only merge if same state AND same zone (same scene target)
      const prev = segments.length > 0 ? segments[segments.length - 1] : null;
      if (prev && prev.state === state && prev.zone_id === zoneId) {
        prev.end_ms = end_ms;
      } else {
        segments.push({
          start_ms: kf.time_ms,
          end_ms,
          state,
          window_title: windowTitle,
          zone_id: zoneId,
          zone_label: "", // filled below
        });
      }
    }

    // Assign sequential WorkArea numbers
    let waCounter = 0;
    for (const seg of segments) {
      if (seg.state === "fullscreen") {
        seg.zone_label = "全画面";
      } else if (seg.state === "window") {
        waCounter++;
        seg.zone_label = seg.window_title
          ? `Window#${waCounter}`
          : `Window#${waCounter}`;
      } else {
        waCounter++;
        seg.zone_label = `WorkArea#${waCounter}`;
      }
    }

    return segments;
  });

  // Scene hover info with event breakdown
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
      case "window": return "rgba(96,165,250,0.6)";
      case "workarea": return "rgba(168,85,247,0.6)";
    }
  };

  const stateLabel = (state: ZoomState) => {
    switch (state) {
      case "fullscreen": return "全画面";
      case "window": return "Window";
      case "workarea": return "WorkArea";
    }
  };

  const formatTime = (ms: number) => {
    const s = Math.floor(ms / 1000);
    const m = Math.floor(s / 60);
    const rem = s % 60;
    const frac = Math.floor((ms % 1000) / 100);
    return `${m}:${rem.toString().padStart(2, "0")}.${frac}`;
  };

  const truncTitle = (title: string | null, max: number = 20) => {
    if (!title) return "";
    return title.length > max ? title.slice(0, max) + "..." : title;
  };

  const getZoomAtTime = (timeMs: number) => {
    const kfs = keyframes();
    if (kfs.length === 0) return 1.0;
    let current = kfs[0].zoom_level;
    for (const kf of kfs) {
      if (kf.time_ms <= timeMs) current = kf.zoom_level;
      else break;
    }
    return current;
  };

  const getTargetAtTime = (timeMs: number) => {
    const kfs = keyframes();
    if (kfs.length === 0) return null;
    let current = kfs[0];
    for (const kf of kfs) {
      if (kf.time_ms <= timeMs) current = kf;
      else break;
    }
    return { x: current.target_x, y: current.target_y };
  };

  const handleClick = (e: MouseEvent) => {
    if (!containerRef) return;
    const rect = containerRef.getBoundingClientRect();
    const pct = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
    const timeMs = Math.round(pct * props.durationMs);
    props.onSeekToTime?.(timeMs);
  };

  const yLabels = createMemo(() => {
    const max = maxZoom();
    const labels: number[] = [1.0];
    let step = 0.5;
    if (max > 3) step = 1.0;
    for (let z = 1.0 + step; z <= max; z += step) {
      labels.push(Math.round(z * 10) / 10);
    }
    return labels;
  });

  const TOTAL_HEIGHT = GRAPH_HEIGHT + EVENT_LANE_HEIGHT + STATE_BAND_HEIGHT + SCENE_BAND_HEIGHT;

  return (
    <div class="space-y-1">
      <Show when={!loading()} fallback={<div class="text-xs text-slate-500">...</div>}>
        <Show when={keyframes().length > 0} fallback={
          <p class="text-xs text-slate-500">キーフレームがありません</p>
        }>
          {/* Main visualization area */}
          <div class="relative" style={{ height: `${TOTAL_HEIGHT + 20}px` }}>
            {/* Y-axis labels */}
            <div class="absolute left-0 top-0 w-8 pointer-events-none" style={{ height: `${GRAPH_HEIGHT}px` }}>
              <span class="absolute text-[9px] font-mono text-slate-500" style={{ top: `${zoomToY(maxZoom()) - 5}px` }}>
                {maxZoom().toFixed(1)}x
              </span>
              <span class="absolute text-[9px] font-mono text-slate-500" style={{ top: `${zoomToY(1.0) - 5}px` }}>
                1.0x
              </span>
            </div>

            {/* Graph + event lane + state band + scene band */}
            <div
              ref={containerRef}
              class="ml-8 relative cursor-crosshair"
              style={{ height: `${TOTAL_HEIGHT}px` }}
              onClick={handleClick}
            >
              <svg
                class="absolute inset-0 w-full"
                viewBox={`0 0 1000 ${TOTAL_HEIGHT}`}
                preserveAspectRatio="none"
                style={{ height: `${TOTAL_HEIGHT}px` }}
              >
                {/* Horizontal grid lines */}
                <For each={yLabels()}>
                  {(z) => (
                    <line
                      x1="0" y1={zoomToY(z)}
                      x2="1000" y2={zoomToY(z)}
                      stroke="rgba(100,116,139,0.2)" stroke-width="1"
                      vector-effect="non-scaling-stroke"
                    />
                  )}
                </For>

                {/* 1.0x baseline */}
                <line
                  x1="0" y1={zoomToY(1.0)}
                  x2="1000" y2={zoomToY(1.0)}
                  stroke="rgba(100,116,139,0.4)" stroke-width="1"
                  stroke-dasharray="4 4"
                  vector-effect="non-scaling-stroke"
                />

                {/* Zoom fill area */}
                <path d={zoomFill()} fill="url(#zoomGrad)" opacity="0.3" />

                {/* Zoom line */}
                <path
                  d={zoomPath()}
                  fill="none"
                  stroke="rgba(168,85,247,0.9)"
                  stroke-width="2"
                  vector-effect="non-scaling-stroke"
                />

                {/* Gradient definition */}
                <defs>
                  <linearGradient id="zoomGrad" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0%" stop-color="rgba(168,85,247,0.6)" />
                    <stop offset="100%" stop-color="rgba(168,85,247,0.05)" />
                  </linearGradient>
                </defs>

                {/* Keyframe dots */}
                <For each={keyframes()}>
                  {(kf, i) => (
                    <circle
                      cx={timeToX(kf.time_ms, 1000)}
                      cy={zoomToY(kf.zoom_level)}
                      r={hoveredKfIdx() === i() ? 5 : 3.5}
                      fill={
                        kf.transition === "SpringIn" ? "rgba(96,165,250,1)"
                        : kf.transition === "SpringOut" ? "rgba(74,222,128,1)"
                        : "rgba(250,204,21,1)"
                      }
                      stroke="white" stroke-width="1"
                      vector-effect="non-scaling-stroke"
                      class="cursor-pointer"
                      onMouseEnter={() => setHoveredKfIdx(i())}
                      onMouseLeave={() => setHoveredKfIdx(null)}
                      onClick={(e) => {
                        e.stopPropagation();
                        props.onSeekToTime?.(kf.time_ms);
                      }}
                    />
                  )}
                </For>

                {/* Pan markers (vertical lines where center moves) */}
                <For each={panMarkers()}>
                  {(m) => (
                    <line
                      x1={timeToX(m.time_ms, 1000)}
                      y1={0}
                      x2={timeToX(m.time_ms, 1000)}
                      y2={GRAPH_HEIGHT}
                      stroke="rgba(251,146,60,0.5)"
                      stroke-width="1"
                      stroke-dasharray="3 3"
                      vector-effect="non-scaling-stroke"
                    />
                  )}
                </For>

                {/* === Event dot lane === */}
                <rect
                  x="0" y={EVENT_LANE_Y}
                  width="1000" height={EVENT_LANE_HEIGHT}
                  fill="rgba(15,23,42,0.3)"
                />
                {/* Lane separators */}
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
                        onMouseMove={(e) => {
                          if (ctrlHeld() && containerRef) {
                            const rect = containerRef.getBoundingClientRect();
                            const pct = (e.clientX - rect.left) / rect.width;
                            const t = Math.round(pct * props.durationMs);
                            if (t > scene.start_ms + 200 && t < scene.end_ms - 200) {
                              setSplitPreviewTime(t);
                            } else {
                              setSplitPreviewTime(null);
                            }
                          } else {
                            setSplitPreviewTime(null);
                          }
                        }}
                        onClick={(e) => {
                          e.stopPropagation();
                          if (ctrlHeld()) {
                            const spt = splitPreviewTime();
                            if (spt !== null) {
                              applyEdit({ type: "Split", scene_id: scene.id, split_time_ms: spt });
                              setSplitPreviewTime(null);
                            }
                          } else {
                            props.onSeekToTime?.(scene.start_ms);
                          }
                        }}
                      />
                    );
                  }}
                </For>

                {/* Merge indicators at scene boundaries */}
                <For each={scenes()}>
                  {(scene, i) => {
                    const nextScene = () => scenes()[i() + 1];
                    return (
                      <Show when={nextScene()}>
                        {(next) => {
                          const bx = () => timeToX(next().start_ms, 1000);
                          const isHovered = () => hoveredBoundaryIdx() === i();
                          return (
                            <g
                              class="cursor-pointer"
                              onMouseEnter={() => setHoveredBoundaryIdx(i())}
                              onMouseLeave={() => setHoveredBoundaryIdx(null)}
                              onClick={(e) => {
                                e.stopPropagation();
                                applyEdit({ type: "Merge", scene_id: scene.id });
                              }}
                            >
                              {/* Hit area */}
                              <rect
                                x={bx() - 8}
                                y={SCENE_BAND_Y - 2}
                                width={16}
                                height={SCENE_BAND_HEIGHT + 4}
                                fill="transparent"
                              />
                              {/* Boundary line */}
                              <line
                                x1={bx()} y1={SCENE_BAND_Y}
                                x2={bx()} y2={SCENE_BAND_Y + SCENE_BAND_HEIGHT}
                                stroke={isHovered() ? "rgba(250,204,21,0.9)" : "rgba(100,116,139,0.4)"}
                                stroke-width={isHovered() ? 2 : 1}
                                vector-effect="non-scaling-stroke"
                              />
                              {/* Merge icon (arrows pointing inward) on hover */}
                              <Show when={isHovered()}>
                                <text
                                  x={bx()}
                                  y={SCENE_BAND_Y - 2}
                                  text-anchor="middle"
                                  fill="rgba(250,204,21,1)"
                                  font-size="8"
                                  class="pointer-events-none select-none"
                                >
                                  {"merge"}
                                </text>
                              </Show>
                            </g>
                          );
                        }}
                      </Show>
                    );
                  }}
                </For>

                {/* Split preview line (Ctrl+hover) */}
                <Show when={splitPreviewTime() !== null}>
                  <line
                    x1={timeToX(splitPreviewTime()!, 1000)}
                    y1={SCENE_BAND_Y}
                    x2={timeToX(splitPreviewTime()!, 1000)}
                    y2={SCENE_BAND_Y + SCENE_BAND_HEIGHT}
                    stroke="rgba(250,204,21,0.8)"
                    stroke-width="2"
                    stroke-dasharray="3 2"
                    vector-effect="non-scaling-stroke"
                    class="pointer-events-none"
                  />
                </Show>

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
            <div class="ml-8 flex justify-between text-[9px] text-slate-500 font-mono mt-0.5">
              <span>0:00</span>
              <span>{formatTime(Math.round(props.durationMs / 4))}</span>
              <span>{formatTime(Math.round(props.durationMs / 2))}</span>
              <span>{formatTime(Math.round(props.durationMs * 3 / 4))}</span>
              <span>{formatTime(props.durationMs)}</span>
            </div>
          </div>

          {/* Tooltip (fixed position, outside SVG) */}
          <Show when={tooltipPos() && (hoveredSceneIdx() !== null || hoveredEventIdx() !== null)}>
            <div
              class="fixed z-50 bg-slate-900 border border-slate-600 rounded-lg px-3 py-2 text-[11px] font-mono text-slate-300 shadow-xl pointer-events-none"
              style={{
                left: `${tooltipPos()!.x + 12}px`,
                top: `${tooltipPos()!.y - 10}px`,
              }}
            >
              {/* Scene tooltip */}
              <Show when={hoveredSceneIdx() !== null && hoveredSceneInfo()}>
                {(info) => (
                  <div class="space-y-1">
                    <div class="text-slate-200 font-medium">Scene #{info().id}</div>
                    <div>
                      {formatTime(info().start_ms)} - {formatTime(info().end_ms)}
                      <span class="text-slate-500 ml-1">({(info().duration_ms / 1000).toFixed(1)}s)</span>
                    </div>
                    <div>zoom: <span class="text-purple-300">{info().zoom_level.toFixed(2)}x</span></div>
                    <div class="text-slate-500">{info().event_count} activity points</div>
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

              {/* Event tooltip */}
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
                    <Show when={seg().state === "window" && seg().window_title}>
                      <span class="text-blue-300 truncate max-w-[180px]">{seg().window_title}</span>
                    </Show>
                  </span>
                )}
              </Show>
              <span>
                zoom: <span class="text-purple-300 font-medium">{getZoomAtTime(props.currentTimeMs!).toFixed(2)}x</span>
              </span>
              <Show when={getTargetAtTime(props.currentTimeMs!)}>
                {(target) => (
                  <span>
                    center: <span class="text-orange-300 font-medium">({Math.round(target().x)}, {Math.round(target().y)})</span>
                  </span>
                )}
              </Show>
              <Show when={pendingEdits().length > 0}>
                <span class="text-yellow-400">{pendingEdits().length} edits</span>
                <button
                  class="px-1.5 py-0.5 bg-slate-700 hover:bg-slate-600 text-slate-300 rounded text-[10px]"
                  onClick={resetEdits}
                  disabled={editApplying()}
                >
                  {editApplying() ? "..." : "リセット"}
                </button>
              </Show>
              <span class="text-slate-600 ml-auto">{keyframes().length} kf / {scenes().length} scenes / {events().length} events</span>
            </div>
          </Show>

          {/* Hovered keyframe tooltip */}
          <Show when={hoveredKfIdx() !== null}>
            {(_) => {
              const kf = () => keyframes()[hoveredKfIdx()!];
              return (
                <Show when={kf()}>
                  {(k) => (
                    <div class="bg-slate-800 border border-slate-600 rounded px-3 py-1.5 text-[11px] font-mono text-slate-300 flex items-center gap-3">
                      <span class="text-slate-500">{formatTime(k().time_ms)}</span>
                      <span>zoom <span class="text-purple-300">{k().zoom_level.toFixed(2)}x</span></span>
                      <span>center <span class="text-orange-300">({Math.round(k().target_x)}, {Math.round(k().target_y)})</span></span>
                      <span class="text-slate-500">
                        {k().transition === "SpringIn" ? "Spring In"
                         : k().transition === "SpringOut" ? "Spring Out"
                         : "Smooth"}
                      </span>
                    </div>
                  )}
                </Show>
              );
            }}
          </Show>

          {/* Legend */}
          <div class="flex items-center gap-3 text-[10px] text-slate-500 flex-wrap">
            <span class="flex items-center gap-1">
              <span class="inline-block w-3 h-2 rounded-sm" style={{ background: "rgba(100,116,139,0.5)" }} />
              全画面
            </span>
            <span class="flex items-center gap-1">
              <span class="inline-block w-3 h-2 rounded-sm" style={{ background: "rgba(96,165,250,0.6)" }} />
              Window
            </span>
            <span class="flex items-center gap-1">
              <span class="inline-block w-3 h-2 rounded-sm" style={{ background: "rgba(168,85,247,0.6)" }} />
              WorkArea
            </span>
            <Show when={panMarkers().length > 0}>
              <span class="flex items-center gap-1">
                <span class="inline-block w-3 border-t border-dashed border-orange-400" />
                中心移動 ({panMarkers().length})
              </span>
            </Show>
            <span class="text-slate-600">|</span>
            <For each={Object.entries(EVENT_TYPE_CONFIG).filter(([k]) => k !== "window_focus")}>
              {([, config]) => (
                <span class="flex items-center gap-1">
                  <span
                    class="inline-block w-2 h-2 rounded-full"
                    style={{ background: config.color }}
                  />
                  {config.label}
                </span>
              )}
            </For>
            <span class="flex items-center gap-1">
              <span class="inline-block w-3 h-2 rounded-sm" style={{ background: "rgba(139,92,246,0.4)" }} />
              Scene
            </span>
            <span class="text-slate-600">|</span>
            <span class="text-slate-500">境界クリック: merge / Ctrl+クリック: split</span>
          </div>
        </Show>
      </Show>
    </div>
  );
}
