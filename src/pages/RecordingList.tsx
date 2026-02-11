import { createSignal, onMount, For, Show } from "solid-js";
import { getRecordingsList, deleteRecording, getSettings, saveSettings, listWindows } from "../lib/commands";
import type { RecordingInfo, RecordingState, RecordingMode, AppSettings, WindowInfo } from "../lib/types";
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
  const [settings, setSettingsState] = createSignal<AppSettings | null>(null);
  const [windows, setWindows] = createSignal<WindowInfo[]>([]);

  const currentMode = () => settings()?.recording.recording_mode ?? { type: "Display" as const };

  onMount(async () => {
    await loadRecordings();
    try {
      const s = await getSettings();
      setSettingsState(s);
      if (s.recording.recording_mode.type === "Window") {
        const wins = await listWindows();
        setWindows(wins);
      }
    } catch (e) {
      console.error("Failed to load settings:", e);
    }
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

  const changeMode = async (mode: RecordingMode) => {
    const s = settings();
    if (!s) return;
    const updated = { ...s, recording: { ...s.recording, recording_mode: mode } };
    setSettingsState(updated);
    try {
      await saveSettings(updated);
    } catch (e) {
      console.error("Failed to save mode:", e);
    }
  };

  const refreshWindows = async () => {
    try {
      const wins = await listWindows();
      setWindows(wins);
    } catch (e) {
      console.error("Failed to list windows:", e);
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
                  thumbnailPath={rec.thumbnail_path}
                  onClick={() => props.onOpenPreview(rec.id)}
                  onDelete={() => handleDelete(rec.id)}
                />
              )}
            </For>
          </div>
        </Show>
      </div>

      <div class="px-6 py-4 border-t border-slate-700/50 space-y-3">
        {/* モード選択 */}
        <div class="flex items-center gap-1 bg-slate-800 rounded-lg p-1">
          <button
            onClick={() => changeMode({ type: "Display" })}
            class={`flex-1 flex items-center justify-center gap-1.5 py-2 px-3 rounded-md text-sm font-medium transition-all ${
              currentMode().type === "Display"
                ? "bg-slate-700 text-white shadow-sm"
                : "text-slate-400 hover:text-slate-200"
            }`}
          >
            <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <rect x="2" y="3" width="20" height="14" rx="2" />
              <path d="M8 21h8M12 17v4" />
            </svg>
            画面全体
          </button>
          <button
            onClick={async () => {
              await changeMode({ type: "Window", hwnd: 0, title: "", rect: [0, 0, 0, 0] });
              await refreshWindows();
            }}
            class={`flex-1 flex items-center justify-center gap-1.5 py-2 px-3 rounded-md text-sm font-medium transition-all ${
              currentMode().type === "Window"
                ? "bg-slate-700 text-white shadow-sm"
                : "text-slate-400 hover:text-slate-200"
            }`}
          >
            <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <rect x="3" y="3" width="18" height="18" rx="2" />
              <path d="M3 9h18" />
              <path d="M9 3v6" />
            </svg>
            ウィンドウ
          </button>
          <button
            onClick={() => changeMode({ type: "Area", x: 0, y: 0, width: 1920, height: 1080 })}
            class={`flex-1 flex items-center justify-center gap-1.5 py-2 px-3 rounded-md text-sm font-medium transition-all ${
              currentMode().type === "Area"
                ? "bg-slate-700 text-white shadow-sm"
                : "text-slate-400 hover:text-slate-200"
            }`}
          >
            <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M6 2L2 2 2 6" /><path d="M18 2L22 2 22 6" />
              <path d="M6 22L2 22 2 18" /><path d="M18 22L22 22 22 18" />
            </svg>
            範囲指定
          </button>
        </div>

        {/* ウィンドウ選択ドロップダウン */}
        <Show when={currentMode().type === "Window"}>
          <div class="flex items-center gap-2">
            <select
              onChange={(e) => {
                const idx = parseInt(e.target.value);
                const win = windows()[idx];
                if (win) {
                  changeMode({ type: "Window", hwnd: win.hwnd, title: win.title, rect: win.rect });
                }
              }}
              class="flex-1 bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200"
            >
              <option value="">
                {(currentMode() as { title?: string }).title || "ウィンドウを選択..."}
              </option>
              <For each={windows()}>
                {(win, i) => <option value={i()}>{win.title}</option>}
              </For>
            </select>
            <button
              onClick={refreshWindows}
              class="p-2 rounded-lg bg-slate-800 border border-slate-700 hover:bg-slate-700 transition-colors text-slate-400 hover:text-slate-200"
              title="ウィンドウ一覧を更新"
            >
              <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M1 4v6h6M23 20v-6h-6" />
                <path d="M20.49 9A9 9 0 0 0 5.64 5.64L1 10m22 4l-4.64 4.36A9 9 0 0 1 3.51 15" />
              </svg>
            </button>
          </div>
        </Show>

        <button
          onClick={props.onStartRecording}
          disabled={props.recordingState !== "Idle"}
          class="w-full py-3 px-4 rounded-xl font-medium transition-all bg-gradient-to-r from-purple-500 to-blue-500 hover:from-purple-600 hover:to-blue-600 text-white shadow-lg shadow-purple-500/20 disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2"
        >
          <div class="w-3 h-3 rounded-full bg-red-400" />
          録画開始
          <span class="text-xs opacity-70 ml-1">(Ctrl+Shift+R)</span>
        </button>
      </div>
    </div>
  );
}
