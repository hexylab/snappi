import { createSignal, createEffect, createMemo, onCleanup, For, Show } from "solid-js";
import { convertFileSrc } from "@tauri-apps/api/core";
import type { ZoomKeyframe } from "../lib/types";
import type { ZoomSegment } from "../lib/zoomSegments";
import { computeViewportAt } from "../lib/springMath";

interface Props {
  recordingDir: string;
  frameCount: number;
  durationMs: number;
  keyframes?: ZoomKeyframe[];
  segments: ZoomSegment[];
  selectedSegment?: ZoomSegment | null;
  seekToTimeMs?: number;
  onTimeChange?: (timeMs: number) => void;
  screenWidth?: number;
  screenHeight?: number;
  showZoomOverlay?: boolean;
  showZoomPreview?: boolean;
  editMode?: "position" | null;
  onVideoClick?: (screenX: number, screenY: number) => void;
}

export default function VideoPlayer(props: Props) {
  const [playing, setPlaying] = createSignal(false);
  const [currentFrame, setCurrentFrame] = createSignal(1);
  const [seeking, setSeeking] = createSignal(false);
  const [loaded, setLoaded] = createSignal(false);

  let playbackPos = 0;
  let animationRef: number | undefined;
  let lastTimestamp: number | undefined;
  let seekBarRef: HTMLDivElement | undefined;

  const msPerFrame = () =>
    props.frameCount > 0 ? props.durationMs / props.frameCount : 33.33;

  const currentTimeMs = () => (currentFrame() - 1) * msPerFrame();

  const frameToPercent = (frame: number) =>
    props.frameCount > 1 ? ((frame - 1) / (props.frameCount - 1)) * 100 : 0;

  const framePath = (frameNum: number) => {
    const padded = String(frameNum).padStart(8, "0");
    return convertFileSrc(
      `${props.recordingDir}\\frames\\frame_${padded}.png`
    );
  };

  const formatTime = (ms: number) => {
    const totalSec = Math.floor(ms / 1000);
    const min = Math.floor(totalSec / 60);
    const sec = totalSec % 60;
    const frac = Math.floor((ms % 1000) / 100);
    return `${min}:${sec.toString().padStart(2, "0")}.${frac}`;
  };

  // ズーム区間をセグメントから直接計算
  const zoomRegions = createMemo(() =>
    props.segments.map(seg => ({
      startPct: props.durationMs > 0 ? (seg.startMs / props.durationMs) * 100 : 0,
      endPct: props.durationMs > 0 ? (seg.endMs / props.durationMs) * 100 : 0,
      zoomLevel: seg.zoomLevel,
      id: seg.id,
    }))
  );

  // 現在再生位置のズームレベル
  const currentZoomLevel = () => {
    const t = currentTimeMs();
    for (const seg of props.segments) {
      if (t >= seg.startMs && t <= seg.endMs) return seg.zoomLevel;
    }
    return 1.0;
  };

  // Compute animated viewport at current time (Spring physics simulation)
  const viewport = createMemo(() => {
    if (
      !props.showZoomPreview ||
      !props.keyframes?.length ||
      !props.screenWidth ||
      !props.screenHeight
    )
      return null;
    return computeViewportAt(
      props.keyframes,
      currentTimeMs(),
      props.screenWidth,
      props.screenHeight,
    );
  });

  // Playback loop
  const tick = (timestamp: number) => {
    if (!playing()) return;

    if (lastTimestamp !== undefined) {
      const delta = timestamp - lastTimestamp;
      const framesToAdvance = delta / msPerFrame();
      playbackPos += framesToAdvance;

      if (playbackPos >= props.frameCount - 1) {
        playbackPos = props.frameCount - 1;
        setCurrentFrame(props.frameCount);
        setPlaying(false);
        lastTimestamp = undefined;
        return;
      }

      setCurrentFrame(Math.floor(playbackPos) + 1);
    }

    lastTimestamp = timestamp;
    animationRef = requestAnimationFrame(tick);
  };

  createEffect(() => {
    if (playing()) {
      playbackPos = currentFrame() - 1;
      lastTimestamp = undefined;
      animationRef = requestAnimationFrame(tick);
    } else {
      if (animationRef) {
        cancelAnimationFrame(animationRef);
        animationRef = undefined;
      }
      lastTimestamp = undefined;
    }
  });

  createEffect(() => {
    const t = currentTimeMs();
    props.onTimeChange?.(t);
  });

  createEffect(() => {
    const seekMs = props.seekToTimeMs;
    if (seekMs !== undefined && seekMs >= 0 && !playing()) {
      const frame = Math.max(
        1,
        Math.min(
          props.frameCount,
          Math.round(seekMs / msPerFrame()) + 1
        )
      );
      playbackPos = frame - 1;
      setCurrentFrame(frame);
    }
  });

  onCleanup(() => {
    if (animationRef) cancelAnimationFrame(animationRef);
  });

  const togglePlay = () => {
    if (currentFrame() >= props.frameCount) {
      setCurrentFrame(1);
      playbackPos = 0;
    }
    setPlaying(!playing());
  };

  const seekTo = (e: MouseEvent) => {
    if (!seekBarRef) return;
    const rect = seekBarRef.getBoundingClientRect();
    const pct = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
    const frame = Math.max(
      1,
      Math.min(props.frameCount, Math.round(pct * (props.frameCount - 1)) + 1)
    );
    playbackPos = frame - 1;
    setCurrentFrame(frame);
  };

  const onSeekStart = (e: MouseEvent) => {
    setSeeking(true);
    setPlaying(false);
    seekTo(e);

    const onMove = (ev: MouseEvent) => seekTo(ev);
    const onUp = () => {
      setSeeking(false);
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };

  const stepFrame = (delta: number) => {
    setPlaying(false);
    const next = Math.max(1, Math.min(props.frameCount, currentFrame() + delta));
    playbackPos = next - 1;
    setCurrentFrame(next);
  };

  const handleVideoAreaClick = (e: MouseEvent) => {
    if (props.editMode !== "position" || !props.onVideoClick) return;
    if (!props.screenWidth || !props.screenHeight) return;

    const container = (e.currentTarget as HTMLElement);
    const rect = container.getBoundingClientRect();
    const containerAspect = rect.width / rect.height;
    const videoAspect = props.screenWidth / props.screenHeight;

    let renderW: number, renderH: number, offsetX: number, offsetY: number;
    if (containerAspect > videoAspect) {
      renderH = rect.height;
      renderW = renderH * videoAspect;
      offsetX = (rect.width - renderW) / 2;
      offsetY = 0;
    } else {
      renderW = rect.width;
      renderH = renderW / videoAspect;
      offsetX = 0;
      offsetY = (rect.height - renderH) / 2;
    }

    const localX = e.clientX - rect.left - offsetX;
    const localY = e.clientY - rect.top - offsetY;

    if (localX < 0 || localX > renderW || localY < 0 || localY > renderH) return;

    const screenX = (localX / renderW) * props.screenWidth;
    const screenY = (localY / renderH) * props.screenHeight;

    e.stopPropagation();
    props.onVideoClick(screenX, screenY);
  };

  // Preload first frame
  createEffect(() => {
    if (props.frameCount > 0) {
      const img = new Image();
      img.onload = () => setLoaded(true);
      img.src = framePath(1);
    }
  });

  return (
    <div class="space-y-2">
      {/* Video display area */}
      <div
        class={`aspect-video bg-slate-800 rounded-xl overflow-hidden border border-slate-700/50 flex items-center justify-center relative ${
          props.editMode === "position" ? "cursor-crosshair" : ""
        }`}
        onClick={handleVideoAreaClick}
      >
        {/* editMode overlay */}
        <Show when={props.editMode === "position"}>
          <div class="absolute inset-0 z-20 flex items-center justify-center pointer-events-none">
            <div class="bg-black/40 text-purple-300 text-sm px-4 py-2 rounded-lg backdrop-blur-sm">
              映像をクリックしてズーム中心を設定
            </div>
          </div>
        </Show>

        <Show
          when={loaded() && props.frameCount > 0}
          fallback={
            <div class="text-center">
              <svg
                class="w-12 h-12 mx-auto mb-2 text-slate-600 animate-pulse"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.5"
              >
                <polygon points="5 3 19 12 5 21 5 3" fill="currentColor" />
              </svg>
              <p class="text-slate-500 text-sm">
                {props.frameCount === 0
                  ? "フレームがありません"
                  : "読み込み中..."}
              </p>
            </div>
          }
        >
          <img
            src={framePath(currentFrame())}
            alt={`Frame ${currentFrame()}`}
            class="w-full h-full object-contain"
            draggable={false}
            style={
              viewport()
                ? {
                    "transform-origin": "0 0",
                    transform: `scale(${viewport()!.zoom}) translate(${-(viewport()!.x / props.screenWidth!) * 100}%, ${-(viewport()!.y / props.screenHeight!) * 100}%)`,
                  }
                : {}
            }
          />

          {/* Zoom level indicator when preview is active */}
          <Show when={viewport() && viewport()!.zoom > 1.01}>
            <div class="absolute top-2 right-2 text-[11px] font-mono text-white/80 bg-black/50 rounded px-1.5 py-0.5 pointer-events-none">
              {viewport()!.zoom.toFixed(1)}x
            </div>
          </Show>

          {/* 選択中セグメントの中心位置を紫crosshairで表示 */}
          <Show when={!props.showZoomPreview && props.showZoomOverlay && props.screenWidth && props.screenHeight && props.selectedSegment}>
            {(_) => {
              const seg = () => props.selectedSegment!;
              const pctX = () => (seg().centerX / props.screenWidth!) * 100;
              const pctY = () => (seg().centerY / props.screenHeight!) * 100;
              return (
                <>
                  <div
                    class="absolute pointer-events-none"
                    style={{
                      left: `${pctX()}%`,
                      top: `${pctY()}%`,
                      transform: "translate(-50%, -50%)",
                    }}
                  >
                    <div
                      class="rounded-full border-2 border-purple-400/80"
                      style={{
                        width: "20px",
                        height: "20px",
                        "box-shadow": "0 0 8px rgba(168,85,247,0.5)",
                      }}
                    />
                    <div class="absolute top-1/2 left-[-12px] w-[8px] h-px bg-purple-400/70" />
                    <div class="absolute top-1/2 right-[-12px] w-[8px] h-px bg-purple-400/70" />
                    <div class="absolute left-1/2 top-[-12px] h-[8px] w-px bg-purple-400/70" />
                    <div class="absolute left-1/2 bottom-[-12px] h-[8px] w-px bg-purple-400/70" />
                  </div>
                  <div
                    class="absolute pointer-events-none text-[10px] font-mono rounded px-1 text-purple-300 bg-black/60"
                    style={{
                      left: `calc(${pctX()}% + 16px)`,
                      top: `calc(${pctY()}% - 8px)`,
                    }}
                  >
                    {seg().zoomLevel.toFixed(1)}x
                  </div>
                </>
              );
            }}
          </Show>
        </Show>
      </div>

      {/* Controls */}
      <div class="space-y-1.5">
        {/* Seekbar with zoom region highlights */}
        <div class="relative group">
          <div
            ref={seekBarRef}
            class="relative h-3 bg-slate-700/60 rounded-full cursor-pointer overflow-visible"
            onMouseDown={onSeekStart}
          >
            {/* ズーム区間ハイライト */}
            <For each={zoomRegions()}>
              {(region) => (
                <div
                  class="absolute inset-y-0 rounded-full bg-purple-500/25 pointer-events-none"
                  style={{
                    left: `${region.startPct}%`,
                    width: `${region.endPct - region.startPct}%`,
                  }}
                />
              )}
            </For>

            {/* Progress fill */}
            <div
              class="absolute inset-y-0 left-0 bg-gradient-to-r from-purple-500 to-blue-500 rounded-full"
              style={{ width: `${frameToPercent(currentFrame())}%` }}
            />

            {/* Seek thumb */}
            <div
              class="absolute top-1/2 -translate-y-1/2 w-3.5 h-3.5 bg-white rounded-full shadow-md border-2 border-purple-500 transition-transform group-hover:scale-110"
              style={{ left: `calc(${frameToPercent(currentFrame())}% - 7px)` }}
            />
          </div>
        </div>

        {/* Playback controls */}
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-1">
            <button
              onClick={() => stepFrame(-1)}
              class="p-1.5 rounded-md hover:bg-slate-700/60 text-slate-400 hover:text-white transition-colors"
              title="1フレーム戻る"
            >
              <svg class="w-4 h-4" viewBox="0 0 24 24" fill="currentColor">
                <path d="M6 6h2v12H6zm3.5 6l8.5 6V6z" />
              </svg>
            </button>

            <button
              onClick={togglePlay}
              class="p-2 rounded-lg bg-slate-700/60 hover:bg-slate-600 text-white transition-colors"
              title={playing() ? "一時停止" : "再生"}
            >
              <Show
                when={playing()}
                fallback={
                  <svg class="w-5 h-5" viewBox="0 0 24 24" fill="currentColor">
                    <polygon points="5 3 19 12 5 21 5 3" />
                  </svg>
                }
              >
                <svg class="w-5 h-5" viewBox="0 0 24 24" fill="currentColor">
                  <rect x="6" y="4" width="4" height="16" />
                  <rect x="14" y="4" width="4" height="16" />
                </svg>
              </Show>
            </button>

            <button
              onClick={() => stepFrame(1)}
              class="p-1.5 rounded-md hover:bg-slate-700/60 text-slate-400 hover:text-white transition-colors"
              title="1フレーム進む"
            >
              <svg class="w-4 h-4" viewBox="0 0 24 24" fill="currentColor">
                <path d="M6 18l8.5-6L6 6v12zM16 6v12h2V6h-2z" />
              </svg>
            </button>
          </div>

          <div class="flex items-center gap-2 text-xs font-mono text-slate-400">
            <span>
              <span class="text-slate-200">{formatTime(currentTimeMs())}</span>
              <span class="mx-1">/</span>
              <span>{formatTime(props.durationMs)}</span>
            </span>
            <Show when={currentZoomLevel() > 1.05}>
              <span class="text-purple-300 text-[10px]">
                ズーム {currentZoomLevel().toFixed(1)}x
              </span>
            </Show>
            <span class="text-slate-700 text-[10px]">
              F{currentFrame()}/{props.frameCount}
            </span>
          </div>
        </div>
      </div>
    </div>
  );
}
