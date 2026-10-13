import { createEffect, on, onCleanup, onMount, Show } from "solid-js";
import type { CameraTrack } from "../lib/generated/CameraTrack";
import type { Trim } from "../lib/generated/Trim";
import { cameraAt, type PreviewRenderer } from "../lib/previewRenderer";

interface Props {
  renderer: PreviewRenderer;
  /** カメラトラック（get_camera_track）。読み込み中は null で全体を表示する */
  track: CameraTrack | null;
  timeMs: number;
  playing: boolean;
  trim: Trim;
  /** audio.wav の URL。音声が無ければ null */
  audioSrc: string | null;
  onTimeChange: (timeMs: number) => void;
  /** トリムの終わりまで再生した */
  onEnded: () => void;
}

/** 音声と映像のずれがこれを超えたら音声の再生位置を合わせ直す (ms) */
const AUDIO_RESYNC_MS = 120;
/** 再生中に先読みする長さ (ms) */
const PREFETCH_MS = 1500;

/**
 * 完成形のプレビュー。書き出しと同じ合成（PreviewRenderer）を canvas に描き、音声を同期再生する。
 * 再生中は映像の時計（performance.now）を基準にし、音声はずれたときだけ合わせ直す。
 */
export default function VideoPlayer(props: Props) {
  let canvas!: HTMLCanvasElement;
  let audio: HTMLAudioElement | undefined;
  let ctx: CanvasRenderingContext2D | null = null;
  let raf: number | undefined;
  let drawQueued = false;
  /** 再生の基準: performance.now() が clockStart のとき時刻 timeAtStart */
  let clockStart = 0;
  let timeAtStart = 0;

  const draw = (t: number) => {
    if (!ctx) return;
    const { renderer } = props;
    const camera = cameraAt(props.track, t, renderer.data.width, renderer.data.height);
    renderer.draw(ctx, t, camera);
  };

  const requestDraw = () => {
    if (drawQueued) return;
    drawQueued = true;
    requestAnimationFrame(() => {
      drawQueued = false;
      draw(props.timeMs);
    });
  };

  const tick = () => {
    const t = timeAtStart + (performance.now() - clockStart);
    if (t >= props.trim.end_ms) {
      props.onTimeChange(props.trim.end_ms);
      props.onEnded();
      return;
    }
    props.renderer.prefetch(t, PREFETCH_MS);
    if (audio && !audio.paused && Math.abs(audio.currentTime * 1000 - t) > AUDIO_RESYNC_MS) {
      audio.currentTime = t / 1000;
    }
    draw(t);
    props.onTimeChange(t);
    raf = requestAnimationFrame(tick);
  };

  const stop = () => {
    if (raf !== undefined) cancelAnimationFrame(raf);
    raf = undefined;
    audio?.pause();
  };

  onMount(() => {
    ctx = canvas.getContext("2d");
  });

  // 画質を変えるとレンダラーが替わる
  createEffect(() => {
    const renderer = props.renderer;
    renderer.onFrameLoaded = () => {
      if (!props.playing) requestDraw();
    };
    requestDraw();
    onCleanup(() => {
      renderer.onFrameLoaded = null;
    });
  });

  createEffect(
    on(
      () => props.playing,
      (playing) => {
        stop();
        if (!playing) return;
        clockStart = performance.now();
        timeAtStart = props.timeMs;
        if (audio) {
          audio.currentTime = props.timeMs / 1000;
          // 自動再生が拒否されても映像だけは再生を続ける
          audio.play().catch((e) => console.warn("audio playback failed:", e));
        }
        raf = requestAnimationFrame(tick);
      },
    ),
  );

  // 停止中は時刻・カメラの変化で描き直す
  createEffect(() => {
    const t = props.timeMs;
    void props.track;
    if (props.playing) return;
    props.renderer.prefetch(t, 300);
    requestDraw();
  });

  onCleanup(stop);

  return (
    <div class="relative w-full h-full flex items-center justify-center">
      <canvas
        ref={canvas}
        width={props.renderer.canvasWidth}
        height={props.renderer.canvasHeight}
        class="w-full h-full object-contain"
      />
      <Show when={props.audioSrc}>
        {(src) => <audio ref={audio} src={src()} preload="auto" />}
      </Show>
    </div>
  );
}
