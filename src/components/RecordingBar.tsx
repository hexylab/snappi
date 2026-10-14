import { Match, Switch } from "solid-js";
import { formatClock } from "../lib/format";

export type BarPhase = "recording" | "paused" | "finishing";

interface Props {
  phase: BarPhase;
  elapsedMs: number;
  onPauseToggle: () => void;
  onStop: () => void;
}

/**
 * 録画中の表示。ウィンドウ全体がこのバーになる（`set_recording_bar`。録画には映らない）。
 * 空いている所をドラッグするとバーを動かせる。
 */
export default function RecordingBar(props: Props) {
  return (
    <div
      data-tauri-drag-region
      class="flex h-screen items-center gap-2 border border-outline bg-surface px-2.5 select-none"
    >
      <span
        data-tauri-drag-region
        class="cursor-grab px-1 text-on-surface-dim"
        title="ドラッグで移動"
        aria-hidden="true"
      >
        ⠿
      </span>

      <Switch>
        <Match when={props.phase === "recording"}>
          <span data-tauri-drag-region class="status-pill is-live tabular" role="status">
            録画中 {formatClock(props.elapsedMs)}
          </span>
        </Match>
        <Match when={props.phase === "paused"}>
          <span data-tauri-drag-region class="status-pill tabular" role="status">
            ⏸ 一時停止中 {formatClock(props.elapsedMs)}
          </span>
        </Match>
        <Match when={props.phase === "finishing"}>
          <span data-tauri-drag-region class="status-pill" role="status">
            ⏳ 保存しています…
          </span>
        </Match>
      </Switch>

      <span data-tauri-drag-region class="flex-1 self-stretch" />

      <button
        class="btn btn-secondary btn-tight"
        onClick={props.onPauseToggle}
        disabled={props.phase === "finishing"}
      >
        {props.phase === "paused" ? "▶️ 再開" : "⏸ 一時停止"}
      </button>
      <button class="btn btn-danger btn-tight" onClick={props.onStop} disabled={props.phase === "finishing"}>
        ⏹ 停止
      </button>
    </div>
  );
}
