import { createSignal, Show } from "solid-js";
import { convertFileSrc } from "@tauri-apps/api/core";
import { formatClock, formatDate } from "../lib/format";
import type { RecordingInfo } from "../lib/types";

interface Props {
  recording: RecordingInfo;
  onOpen: () => void;
  onDelete: () => void;
}

/** 録画の一覧の1件。クリックでプレビュー、🗑️ はカードの上で確認してから削除する */
export default function RecordingTile(props: Props) {
  const [confirming, setConfirming] = createSignal(false);
  const thumbnail = () => (props.recording.thumbnail_path ? convertFileSrc(props.recording.thumbnail_path) : null);

  return (
    <div class="group relative overflow-hidden rounded-md border border-outline bg-surface">
      <button class="block w-full cursor-pointer text-left" onClick={props.onOpen} aria-label={`${formatDate(props.recording.date)} の録画を開く`}>
        <div class="flex aspect-video items-center justify-center overflow-hidden bg-surface-dim">
          <Show when={thumbnail()} fallback={<span class="text-2xl">🎞️</span>}>
            {(src) => <img src={src()} alt="" class="h-full w-full object-cover" />}
          </Show>
        </div>
        <div class="flex items-baseline justify-between gap-2 px-2.5 py-1.5 transition-colors group-hover:bg-surface-dim">
          <span class="text-label-sm truncate">{formatDate(props.recording.date)}</span>
          <span class="text-caption tabular text-on-surface-dim">{formatClock(props.recording.duration_ms)}</span>
        </div>
      </button>

      <button
        class="btn-icon absolute right-1.5 top-1.5 bg-surface opacity-60 hover:opacity-100 group-hover:opacity-100 group-focus-within:opacity-100 focus-visible:opacity-100"
        aria-label="この録画を削除"
        title="削除"
        onClick={() => setConfirming(true)}
      >
        🗑️
      </button>

      <Show when={confirming()}>
        <div
          class="absolute inset-0 flex flex-col items-center justify-center gap-3 bg-surface/95 p-3 text-center"
          onKeyDown={(e) => e.key === "Escape" && setConfirming(false)}
        >
          <span class="text-label-sm">この録画を削除しますか？<br />元に戻せません。</span>
          <div class="flex gap-2">
            <button class="btn btn-danger btn-tight" onClick={props.onDelete}>
              削除する
            </button>
            <button
              class="btn btn-secondary btn-tight"
              onClick={() => setConfirming(false)}
              ref={(el) => queueMicrotask(() => el.focus())}
            >
              やめる
            </button>
          </div>
        </div>
      </Show>
    </div>
  );
}
