import { Show } from "solid-js";
import { convertFileSrc } from "@tauri-apps/api/core";

interface Props {
  date: string;
  duration: string;
  thumbnailPath?: string | null;
  onClick: () => void;
  onDelete: () => void;
}

export default function ThumbnailCard(props: Props) {
  const thumbnailUrl = () => {
    if (props.thumbnailPath) {
      return convertFileSrc(props.thumbnailPath);
    }
    return null;
  };

  return (
    <div class="group relative">
      <button
        onClick={props.onClick}
        class="w-full rounded-xl overflow-hidden bg-slate-800 border border-slate-700/50 hover:border-purple-500/50 transition-all hover:shadow-lg hover:shadow-purple-500/10"
      >
        <div class="aspect-video bg-gradient-to-br from-slate-700 to-slate-800 flex items-center justify-center relative">
          <Show when={thumbnailUrl()} fallback={
            <svg class="w-8 h-8 text-slate-600 group-hover:text-purple-400 transition-colors" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
              <polygon points="5 3 19 12 5 21 5 3" fill="currentColor" />
            </svg>
          }>
            <img
              src={thumbnailUrl()!}
              alt="Recording thumbnail"
              class="w-full h-full object-cover"
            />
            <div class="absolute inset-0 flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity bg-black/30">
              <svg class="w-8 h-8 text-white" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                <polygon points="5 3 19 12 5 21 5 3" fill="currentColor" />
              </svg>
            </div>
          </Show>
        </div>
        <div class="px-3 py-2 text-left">
          <p class="text-sm text-slate-300">{props.date}</p>
          <p class="text-xs text-slate-500">{props.duration}</p>
        </div>
      </button>
      <button
        onClick={(e) => { e.stopPropagation(); props.onDelete(); }}
        class="absolute top-2 right-2 p-1 rounded-lg bg-slate-900/80 opacity-0 group-hover:opacity-100 transition-opacity text-slate-400 hover:text-red-400"
      >
        <svg class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M6 18L18 6M6 6l12 12" />
        </svg>
      </button>
    </div>
  );
}
