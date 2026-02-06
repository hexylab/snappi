import { createSignal, Show } from "solid-js";
import { exportRecording } from "../lib/commands";
import type { ExportFormat, QualityPreset } from "../lib/types";
import ExportButtons from "../components/ExportButtons";

interface Props {
  recordingId: string | null;
  onClose: () => void;
  onRedo: () => void;
}

export default function Preview(props: Props) {
  const [exporting, setExporting] = createSignal(false);
  const [exportedPath, setExportedPath] = createSignal<string | null>(null);
  const [quality, setQuality] = createSignal<QualityPreset>("Social");
  const [error, setError] = createSignal<string | null>(null);

  const handleExport = async (format: ExportFormat) => {
    if (!props.recordingId) return;
    setExporting(true);
    setError(null);
    try {
      const path = await exportRecording(props.recordingId, format, quality());
      setExportedPath(path);
    } catch (e) {
      setError(String(e));
    }
    setExporting(false);
  };

  return (
    <div class="flex flex-col h-screen">
      <header class="flex items-center justify-between px-6 py-4 border-b border-slate-700/50">
        <h2 class="text-lg font-semibold text-white">Preview</h2>
        <button onClick={props.onClose} class="p-2 rounded-lg hover:bg-slate-800 transition-colors text-slate-400">
          <svg class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M6 18L18 6M6 6l12 12" />
          </svg>
        </button>
      </header>

      <div class="flex-1 flex items-center justify-center p-6">
        <div class="w-full max-w-3xl">
          <div class="aspect-video bg-slate-800 rounded-2xl overflow-hidden border border-slate-700/50 flex items-center justify-center">
            <Show when={props.recordingId} fallback={<p class="text-slate-500">No recording selected</p>}>
              <div class="text-center">
                <svg class="w-16 h-16 mx-auto mb-3 text-slate-600" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                  <polygon points="5 3 19 12 5 21 5 3" fill="currentColor" />
                </svg>
                <p class="text-slate-400 text-sm">Recording ready for export</p>
                <p class="text-slate-600 text-xs mt-1">ID: {props.recordingId}</p>
              </div>
            </Show>
          </div>
        </div>
      </div>

      <div class="px-6 py-4 border-t border-slate-700/50 space-y-4">
        <div class="flex items-center gap-4">
          <label class="text-sm text-slate-400">Quality:</label>
          <select
            value={quality()}
            onChange={(e) => setQuality(e.target.value as QualityPreset)}
            class="bg-slate-800 border border-slate-700 rounded-lg px-3 py-1.5 text-sm text-slate-200 focus:outline-none focus:ring-2 focus:ring-purple-500"
          >
            <option value="Social">Social (1080p / 30fps)</option>
            <option value="HighQuality">High Quality (Original / 60fps)</option>
            <option value="Lightweight">Lightweight (720p / 24fps)</option>
          </select>
        </div>

        <ExportButtons onExport={handleExport} exporting={exporting()} />

        <Show when={error()}>
          <p class="text-red-400 text-sm">{error()}</p>
        </Show>

        <Show when={exportedPath()}>
          <p class="text-green-400 text-sm">Exported to: {exportedPath()}</p>
        </Show>

        <div class="flex gap-3">
          <button onClick={props.onRedo} class="flex-1 py-2 px-4 rounded-lg border border-slate-700 text-slate-400 hover:bg-slate-800 hover:text-slate-200 transition-colors text-sm">
            Redo
          </button>
          <button onClick={props.onClose} class="flex-1 py-2 px-4 rounded-lg border border-slate-700 text-slate-400 hover:bg-slate-800 hover:text-slate-200 transition-colors text-sm">
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
