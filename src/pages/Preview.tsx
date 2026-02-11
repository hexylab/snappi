import { createSignal, onMount, onCleanup, Show } from "solid-js";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { exportRecording, exportWithKeyframes, getRecordingsList } from "../lib/commands";
import type { ExportFormat, ExportProgress, QualityPreset, RecordingInfo, ZoomKeyframe } from "../lib/types";
import ExportButtons from "../components/ExportButtons";
import Timeline from "../components/Timeline";
import VideoPlayer from "../components/VideoPlayer";

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
  const [recordingInfo, setRecordingInfo] = createSignal<RecordingInfo | null>(null);
  const [exportProgress, setExportProgress] = createSignal<ExportProgress | null>(null);
  const [showTimeline, setShowTimeline] = createSignal(true);
  const [editedKeyframes, setEditedKeyframes] = createSignal<ZoomKeyframe[] | null>(null);
  const [seekTimeMs, setSeekTimeMs] = createSignal<number | undefined>(undefined);
  const [currentTimeMs, setCurrentTimeMs] = createSignal(0);
  const [showZoomOverlay, setShowZoomOverlay] = createSignal(true);
  const [showZoomPreview, setShowZoomPreview] = createSignal(false);

  let unlistenProgress: UnlistenFn | undefined;
  let unlistenComplete: UnlistenFn | undefined;
  let unlistenError: UnlistenFn | undefined;

  onMount(async () => {
    if (props.recordingId) {
      try {
        const recordings = await getRecordingsList();
        const rec = recordings.find((r) => r.id === props.recordingId);
        if (rec) setRecordingInfo(rec);
      } catch (e) {
        console.error("Failed to load recording info:", e);
      }
    }

    unlistenProgress = await listen<ExportProgress>("export-progress", (event) => {
      setExportProgress(event.payload);
    });
    unlistenComplete = await listen<{ output_path: string }>("export-complete", (event) => {
      setExporting(false);
      setExportProgress(null);
      setExportedPath(event.payload.output_path);
    });
    unlistenError = await listen<{ message: string }>("export-error", (event) => {
      setExporting(false);
      setExportProgress(null);
      setError(event.payload.message);
    });
  });

  onCleanup(() => {
    unlistenProgress?.();
    unlistenComplete?.();
    unlistenError?.();
  });

  const handleExport = async (format: ExportFormat) => {
    if (!props.recordingId) return;
    setExporting(true);
    setError(null);
    setExportedPath(null);
    setExportProgress({ stage: "starting", progress: 0, output_path: null });
    try {
      const kfs = editedKeyframes();
      if (kfs) {
        await exportWithKeyframes(props.recordingId, kfs, format, quality());
      } else {
        await exportRecording(props.recordingId, format, quality());
      }
    } catch (e) {
      setError(String(e));
      setExporting(false);
      setExportProgress(null);
    }
  };

  const progressLabel = () => {
    const p = exportProgress();
    if (!p) return "";
    switch (p.stage) {
      case "composing": return `エフェクト合成中... ${Math.round(p.progress * 100)}%`;
      case "encoding": return "エンコード中...";
      case "complete": return "完了";
      default: return "準備中...";
    }
  };

  return (
    <div class="flex flex-col h-screen">
      <header class="flex items-center justify-between px-6 py-3 border-b border-slate-700/50">
        <h2 class="text-lg font-semibold text-white">プレビュー</h2>
        <button onClick={props.onClose} class="p-2 rounded-lg hover:bg-slate-800 transition-colors text-slate-400">
          <svg class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M6 18L18 6M6 6l12 12" />
          </svg>
        </button>
      </header>

      <div class="flex-1 overflow-y-auto">
        <div class="p-6 space-y-4 max-w-4xl mx-auto">
          {/* Video Player */}
          <Show
            when={recordingInfo()}
            fallback={
              <div class="aspect-video bg-slate-800 rounded-xl border border-slate-700/50 flex items-center justify-center">
                <Show when={props.recordingId} fallback={<p class="text-slate-500">録画が選択されていません</p>}>
                  <div class="text-center">
                    <svg class="w-12 h-12 mx-auto mb-2 text-slate-600 animate-pulse" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                      <polygon points="5 3 19 12 5 21 5 3" fill="currentColor" />
                    </svg>
                    <p class="text-slate-500 text-sm">読み込み中...</p>
                  </div>
                </Show>
              </div>
            }
          >
            {(rec) => (
              <VideoPlayer
                recordingDir={rec().recording_dir}
                frameCount={rec().frame_count}
                durationMs={rec().duration_ms}
                keyframes={editedKeyframes() ?? undefined}
                seekToTimeMs={seekTimeMs()}
                onTimeChange={(t) => setCurrentTimeMs(t)}
                screenWidth={rec().screen_width}
                screenHeight={rec().screen_height}
                showZoomOverlay={showZoomOverlay()}
                showZoomPreview={showZoomPreview()}
              />
            )}
          </Show>

          {/* Timeline section */}
          <Show when={recordingInfo() && recordingInfo()!.duration_ms > 0}>
            <div class="border border-slate-700/50 rounded-lg overflow-hidden">
              {/* Header with toggle buttons */}
              <div class="flex items-center justify-between px-4 py-2 bg-slate-800/50">
                <button
                  onClick={() => setShowTimeline(!showTimeline())}
                  class="flex items-center gap-2 text-sm text-slate-400 hover:text-slate-200 transition-colors"
                >
                  <svg
                    class={`w-3 h-3 transition-transform ${showTimeline() ? "rotate-90" : ""}`}
                    viewBox="0 0 24 24"
                    fill="currentColor"
                  >
                    <path d="M8 5l8 7-8 7z" />
                  </svg>
                  <span>ズームタイムライン</span>
                </button>

                <div class="flex items-center gap-1.5">
                  <button
                    onClick={() => setShowZoomPreview(!showZoomPreview())}
                    class={`text-[11px] px-2 py-0.5 rounded transition-colors ${
                      showZoomPreview()
                        ? "bg-blue-500/20 text-blue-300 hover:bg-blue-500/30"
                        : "bg-slate-700/50 text-slate-500 hover:bg-slate-700"
                    }`}
                  >
                    {showZoomPreview() ? "ズームプレビュー ON" : "ズームプレビュー OFF"}
                  </button>
                  <button
                    onClick={() => setShowZoomOverlay(!showZoomOverlay())}
                    class={`text-[11px] px-2 py-0.5 rounded transition-colors ${
                      showZoomOverlay()
                        ? "bg-orange-500/20 text-orange-300 hover:bg-orange-500/30"
                        : "bg-slate-700/50 text-slate-500 hover:bg-slate-700"
                    }`}
                  >
                    {showZoomOverlay() ? "中心表示 ON" : "中心表示 OFF"}
                  </button>
                </div>
              </div>

              <Show when={showTimeline()}>
                <div class="px-4 py-3 border-t border-slate-700/30">
                  <Timeline
                    recordingId={props.recordingId!}
                    durationMs={recordingInfo()!.duration_ms}
                    screenWidth={recordingInfo()!.screen_width}
                    screenHeight={recordingInfo()!.screen_height}
                    currentTimeMs={currentTimeMs()}
                    onSeekToTime={(ms) => setSeekTimeMs(ms)}
                    onKeyframesChange={(kfs) => setEditedKeyframes(kfs)}
                  />
                </div>
              </Show>
            </div>
          </Show>

          {/* Export section */}
          <div class="space-y-3">
            <div class="flex items-center gap-4">
              <label class="text-sm text-slate-400">品質:</label>
              <select
                value={quality()}
                onChange={(e) => setQuality(e.target.value as QualityPreset)}
                class="bg-slate-800 border border-slate-700 rounded-lg px-3 py-1.5 text-sm text-slate-200 focus:outline-none focus:ring-2 focus:ring-purple-500"
              >
                <option value="Social">Social (1080p / 30fps)</option>
                <option value="HighQuality">High Quality (元解像度 / 60fps)</option>
                <option value="Lightweight">Lightweight (720p / 24fps)</option>
              </select>
            </div>

            <ExportButtons onExport={handleExport} exporting={exporting()} />

            <Show when={exporting() && exportProgress()}>
              <div class="space-y-2">
                <div class="flex items-center justify-between text-sm">
                  <span class="text-slate-300">{progressLabel()}</span>
                  <span class="text-slate-500 font-mono text-xs">
                    {Math.round((exportProgress()?.progress ?? 0) * 100)}%
                  </span>
                </div>
                <div class="w-full bg-slate-700/50 rounded-full h-2 overflow-hidden">
                  <div
                    class="h-full bg-gradient-to-r from-purple-500 to-blue-500 rounded-full transition-all duration-300"
                    style={{ width: `${Math.round((exportProgress()?.progress ?? 0) * 100)}%` }}
                  />
                </div>
              </div>
            </Show>

            <Show when={error()}>
              <p class="text-red-400 text-sm">{error()}</p>
            </Show>

            <Show when={exportedPath()}>
              <p class="text-green-400 text-sm">エクスポート完了: {exportedPath()}</p>
            </Show>
          </div>

          {/* Bottom actions */}
          <div class="flex gap-3 pb-2">
            <button onClick={props.onRedo} class="flex-1 py-2 px-4 rounded-lg border border-slate-700 text-slate-400 hover:bg-slate-800 hover:text-slate-200 transition-colors text-sm">
              再録画
            </button>
            <button onClick={props.onClose} class="flex-1 py-2 px-4 rounded-lg border border-slate-700 text-slate-400 hover:bg-slate-800 hover:text-slate-200 transition-colors text-sm">
              閉じる
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
