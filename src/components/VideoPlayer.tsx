import { createSignal, createEffect, createMemo, onCleanup, For, Show } from "solid-js";
import { convertFileSrc } from "@tauri-apps/api/core";
import type { ZoomKeyframe } from "../lib/types";
import { computeViewportAt } from "../lib/springMath";

interface Props {
  recordingDir: string;
  frameCount: number;
  durationMs: number;
  keyframes?: ZoomKeyframe[];
  seekToTimeMs?: number;
  onTimeChange?: (timeMs: number) => void;
  screenWidth?: number;
  screenHeight?: number;
  showZoomOverlay?: boolean;
  showZoomPreview?: boolean;
}

export default function VideoPlayer(props: Props) {
  const [playing, setPlaying] = createSignal(false);
  const [currentFrame, setCurrentFrame] = createSignal(1);
  const [seeking, setSeeking] = createSignal(false);
  const [loaded, setLoaded] = createSignal(false);

  // Float position for smooth playback (0-indexed)
  let playbackPos = 0;
  let animationRef: number | undefined;
  let lastTimestamp: number | undefined;
  let seekBarRef: HTMLDivElement | undefined;

  const msPerFrame = () =>
    props.frameCount > 0 ? props.durationMs / props.frameCount : 33.33;

  const currentTimeMs = () => (currentFrame() - 1) * msPerFrame();

  const frameToPercent = (frame: number) =>
    props.frameCount > 1 ? ((frame - 1) / (props.frameCount - 1)) * 100 : 0;

  const timeToPercent = (ms: number) =>
    props.durationMs > 0 ? (ms / props.durationMs) * 100 : 0;

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

  // Get current zoom target at the current time
  const currentTarget = () => {
    const kfs = props.keyframes;
    if (!kfs || kfs.length === 0) return null;
    const t = currentTimeMs();
    let current = kfs[0];
    for (const kf of kfs) {
      if (kf.time_ms <= t) current = kf;
      else break;
    }
    return current;
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

  // Playback loop using float accumulator
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
      // Sync playbackPos from current display frame before starting
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

  // Notify parent of time changes
  createEffect(() => {
    const t = currentTimeMs();
    props.onTimeChange?.(t);
  });

  // Seek from parent (Timeline keyframe click etc.)
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
      <div class="aspect-video bg-slate-800 rounded-xl overflow-hidden border border-slate-700/50 flex items-center justify-center relative">
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

          {/* Zoom center overlay (hidden during zoom preview) */}
          <Show when={!props.showZoomPreview && props.showZoomOverlay && props.screenWidth && props.screenHeight && currentTarget()}>
            {(target) => {
              // Clamp center to the achievable range for the current zoom level
              // (mirrors spring.rs current_viewport clamping logic)
              const effectiveCenter = () => {
                const sw = props.screenWidth!;
                const sh = props.screenHeight!;
                const zoom = Math.max(target().zoom_level, 1.0);
                const vpW = sw / zoom;
                const vpH = sh / zoom;
                const minCx = vpW / 2;
                const maxCx = sw - vpW / 2;
                const minCy = vpH / 2;
                const maxCy = sh - vpH / 2;
                return {
                  x: Math.max(minCx, Math.min(maxCx, target().target_x)),
                  y: Math.max(minCy, Math.min(maxCy, target().target_y)),
                };
              };
              const pctX = () => (effectiveCenter().x / props.screenWidth!) * 100;
              const pctY = () => (effectiveCenter().y / props.screenHeight!) * 100;
              return (
                <>
                  {/* Crosshair */}
                  <div
                    class="absolute pointer-events-none"
                    style={{
                      left: `${pctX()}%`,
                      top: `${pctY()}%`,
                      transform: "translate(-50%, -50%)",
                    }}
                  >
                    {/* Center ring */}
                    <div
                      class="rounded-full border-2 border-orange-400/70"
                      style={{
                        width: "20px",
                        height: "20px",
                        "box-shadow": "0 0 6px rgba(251,146,60,0.4)",
                      }}
                    />
                    {/* Crosshair lines */}
                    <div class="absolute top-1/2 left-[-12px] w-[8px] h-px bg-orange-400/60" />
                    <div class="absolute top-1/2 right-[-12px] w-[8px] h-px bg-orange-400/60" />
                    <div class="absolute left-1/2 top-[-12px] h-[8px] w-px bg-orange-400/60" />
                    <div class="absolute left-1/2 bottom-[-12px] h-[8px] w-px bg-orange-400/60" />
                  </div>
                  {/* Zoom level badge */}
                  <div
                    class="absolute pointer-events-none text-[10px] font-mono text-orange-300 bg-black/50 rounded px-1"
                    style={{
                      left: `calc(${pctX()}% + 16px)`,
                      top: `calc(${pctY()}% - 8px)`,
                    }}
                  >
                    {target().zoom_level.toFixed(1)}x
                  </div>
                </>
              );
            }}
          </Show>
        </Show>
      </div>

      {/* Controls */}
      <div class="space-y-1.5">
        {/* Seekbar with keyframe markers */}
        <div class="relative group">
          <div
            ref={seekBarRef}
            class="relative h-3 bg-slate-700/60 rounded-full cursor-pointer overflow-visible"
            onMouseDown={onSeekStart}
          >
            {/* Progress fill */}
            <div
              class="absolute inset-y-0 left-0 bg-gradient-to-r from-purple-500 to-blue-500 rounded-full"
              style={{ width: `${frameToPercent(currentFrame())}%` }}
            />

            {/* Keyframe markers */}
            <Show when={props.keyframes}>
              <For each={props.keyframes}>
                {(kf) => (
                  <div
                    class="absolute top-[-2px] w-1.5 h-[calc(100%+4px)] rounded-full bg-yellow-400/80 pointer-events-none"
                    style={{ left: `${timeToPercent(kf.time_ms)}%` }}
                    title={`${formatTime(kf.time_ms)} - ${kf.zoom_level.toFixed(1)}x`}
                  />
                )}
              </For>
            </Show>

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
            {/* Step back */}
            <button
              onClick={() => stepFrame(-1)}
              class="p-1.5 rounded-md hover:bg-slate-700/60 text-slate-400 hover:text-white transition-colors"
              title="1フレーム戻る"
            >
              <svg class="w-4 h-4" viewBox="0 0 24 24" fill="currentColor">
                <path d="M6 6h2v12H6zm3.5 6l8.5 6V6z" />
              </svg>
            </button>

            {/* Play/Pause */}
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

            {/* Step forward */}
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

          {/* Time display */}
          <div class="text-xs font-mono text-slate-400">
            <span class="text-slate-200">{formatTime(currentTimeMs())}</span>
            <span class="mx-1">/</span>
            <span>{formatTime(props.durationMs)}</span>
            <span class="ml-2 text-slate-600">
              F{currentFrame()}/{props.frameCount}
            </span>
          </div>
        </div>
      </div>
    </div>
  );
}
