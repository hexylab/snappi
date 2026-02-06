import { createSignal, onMount, For, Show } from "solid-js";
import { getRecordingsList, deleteRecording } from "../lib/commands";
import type { RecordingInfo, RecordingState } from "../lib/types";
import ThumbnailCard from "../components/ThumbnailCard";

interface Props {
  onStartRecording: () => void;
  onOpenSettings: () => void;
  onOpenPreview: (id: string) => void;
  recordingState: RecordingState;
}

export default function RecordingList(props: Props) {
  const [recordings, setRecordings] = createSignal<RecordingInfo[]>([]);
  const [loading, setLoading] = createSignal(true);

  onMount(async () => {
    await loadRecordings();
  });

  const loadRecordings = async () => {
    try {
      const list = await getRecordingsList();
      setRecordings(list);
    } catch (e) {
      console.error("Failed to load recordings:", e);
    }
    setLoading(false);
  };

  const handleDelete = async (id: string) => {
    try {
      await deleteRecording(id);
      await loadRecordings();
    } catch (e) {
      console.error("Failed to delete recording:", e);
    }
  };

  const formatDate = (dateStr: string) => {
    try {
      const d = new Date(dateStr);
      return `${d.getMonth() + 1}/${d.getDate()} ${d.getHours()}:${String(d.getMinutes()).padStart(2, "0")}`;
    } catch {
      return dateStr;
    }
  };

  const formatDuration = (ms: number) => {
    const s = Math.floor(ms / 1000);
    const m = Math.floor(s / 60);
    const sec = s % 60;
    if (m > 0) return `${m}m ${sec}s`;
    return `${sec}s`;
  };

  return (
    <div class="flex flex-col h-screen">
      <header class="flex items-center justify-between px-6 py-4 border-b border-slate-700/50">
        <div class="flex items-center gap-3">
          <div class="w-8 h-8 rounded-lg bg-gradient-to-br from-purple-500 to-blue-500 flex items-center justify-center">
            <svg class="w-5 h-5 text-white" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <circle cx="12" cy="12" r="10" />
              <circle cx="12" cy="12" r="4" fill="currentColor" />
            </svg>
          </div>
          <h1 class="text-lg font-semibold text-white">Snappi</h1>
        </div>
        <button
          onClick={props.onOpenSettings}
          class="p-2 rounded-lg hover:bg-slate-800 transition-colors text-slate-400 hover:text-slate-200"
        >
          <svg class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M12.22 2h-.44a2 2 0 00-2 2v.18a2 2 0 01-1 1.73l-.43.25a2 2 0 01-2 0l-.15-.08a2 2 0 00-2.73.73l-.22.38a2 2 0 00.73 2.73l.15.1a2 2 0 011 1.72v.51a2 2 0 01-1 1.74l-.15.09a2 2 0 00-.73 2.73l.22.38a2 2 0 002.73.73l.15-.08a2 2 0 012 0l.43.25a2 2 0 011 1.73V20a2 2 0 002 2h.44a2 2 0 002-2v-.18a2 2 0 011-1.73l.43-.25a2 2 0 012 0l.15.08a2 2 0 002.73-.73l.22-.39a2 2 0 00-.73-2.73l-.15-.08a2 2 0 01-1-1.74v-.5a2 2 0 011-1.74l.15-.09a2 2 0 00.73-2.73l-.22-.38a2 2 0 00-2.73-.73l-.15.08a2 2 0 01-2 0l-.43-.25a2 2 0 01-1-1.73V4a2 2 0 00-2-2z" />
            <circle cx="12" cy="12" r="3" />
          </svg>
        </button>
      </header>

      <div class="flex-1 overflow-y-auto px-6 py-4">
        <Show when={loading()}>
          <div class="flex items-center justify-center h-64 text-slate-500">Loading...</div>
        </Show>

        <Show when={!loading() && recordings().length === 0}>
          <div class="flex flex-col items-center justify-center h-64 text-slate-500">
            <svg class="w-16 h-16 mb-4 text-slate-600" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
              <path d="M15 10l4.553-2.276A1 1 0 0121 8.618v6.764a1 1 0 01-1.447.894L15 14M5 18h8a2 2 0 002-2V8a2 2 0 00-2-2H5a2 2 0 00-2 2v8a2 2 0 002 2z" />
            </svg>
            <p class="text-sm">No recordings yet</p>
            <p class="text-xs text-slate-600 mt-1">Press Ctrl+Shift+R to start recording</p>
          </div>
        </Show>

        <Show when={!loading() && recordings().length > 0}>
          <div class="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-4 gap-4">
            <For each={recordings()}>
              {(rec) => (
                <ThumbnailCard
                  date={formatDate(rec.date)}
                  duration={formatDuration(rec.duration_ms)}
                  onClick={() => props.onOpenPreview(rec.id)}
                  onDelete={() => handleDelete(rec.id)}
                />
              )}
            </For>
          </div>
        </Show>
      </div>

      <div class="px-6 py-4 border-t border-slate-700/50">
        <button
          onClick={props.onStartRecording}
          disabled={props.recordingState !== "Idle"}
          class="w-full py-3 px-4 rounded-xl font-medium transition-all bg-gradient-to-r from-purple-500 to-blue-500 hover:from-purple-600 hover:to-blue-600 text-white shadow-lg shadow-purple-500/20 disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2"
        >
          <div class="w-3 h-3 rounded-full bg-red-400" />
          Start Recording
          <span class="text-xs opacity-70 ml-1">(Ctrl+Shift+R)</span>
        </button>
      </div>
    </div>
  );
}
