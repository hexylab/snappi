import { createEffect, createSignal, on, onCleanup, onMount, Show, untrack } from "solid-js";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  exportRecording,
  getCameraTrack,
  getPreviewData,
  getRecordingEvents,
  getRecordingsList,
  getSettings,
  loadProject,
  saveProject,
} from "../lib/commands";
import type { ExportFormat, ExportProgress, QualityPreset, RecordingInfo, TimelineEvent } from "../lib/types";
import type { CameraTrack } from "../lib/generated/CameraTrack";
import type { Project } from "../lib/generated/Project";
import type { ZoomStrength } from "../lib/generated/ZoomStrength";
import { createHistory } from "../lib/history";
import { PreviewRenderer } from "../lib/previewRenderer";
import { type EditState, formatTime, sortSegments } from "../lib/segments";
import ExportButtons from "../components/ExportButtons";
import Timeline from "../components/Timeline";
import VideoPlayer from "../components/VideoPlayer";

interface Props {
  recordingId: string | null;
  onClose: () => void;
  onRedo: () => void;
}

/** 編集してから自動保存するまでの待ち時間 (ms) */
const SAVE_DEBOUNCE_MS = 500;
/** 区間を編集してからカメラトラックを求め直すまでの待ち時間 (ms) */
const TRACK_DEBOUNCE_MS = 150;

/** ズーム強さごとの倍率（src-tauri/src/camera/spec.rs）。Off でも手動の区間は標準の倍率で寄る */
const STRENGTH_ZOOM: Record<ZoomStrength, number> = { Off: 1.8, Subtle: 1.5, Standard: 1.8, Strong: 2.2 };

export default function Preview(props: Props) {
  const [recordingInfo, setRecordingInfo] = createSignal<RecordingInfo | null>(null);
  const [project, setProject] = createSignal<Project | null>(null);
  const [settingsStrength, setSettingsStrength] = createSignal<ZoomStrength>("Standard");
  const [events, setEvents] = createSignal<TimelineEvent[]>([]);
  const [quality, setQuality] = createSignal<QualityPreset>("Social");
  const [renderer, setRenderer] = createSignal<PreviewRenderer | null>(null);
  const [cameraTrack, setCameraTrack] = createSignal<CameraTrack | null>(null);

  const [timeMs, setTimeMs] = createSignal(0);
  const [playing, setPlaying] = createSignal(false);
  const [selectedId, setSelectedId] = createSignal<string | null>(null);
  const [saveState, setSaveState] = createSignal<"saved" | "saving" | "error">("saved");

  const [exporting, setExporting] = createSignal(false);
  const [exportedPath, setExportedPath] = createSignal<string | null>(null);
  const [exportProgress, setExportProgress] = createSignal<ExportProgress | null>(null);
  const [error, setError] = createSignal<string | null>(null);

  const history = createHistory<EditState>({ segments: [], trim: { start_ms: 0, end_ms: 0 } });
  const segments = () => history.present().segments;
  const trim = () => history.present().trim;
  const durationMs = () => recordingInfo()?.duration_ms ?? 0;
  const defaultZoom = () => STRENGTH_ZOOM[project()?.zoom_strength ?? settingsStrength()];


  // --- 読み込み ---

  onMount(async () => {
    const id = props.recordingId;
    if (!id) return;
    try {
      const [recordings, loaded, settings] = await Promise.all([
        getRecordingsList(),
        loadProject(id),
        getSettings(),
      ]);
      setRecordingInfo(recordings.find((r) => r.id === id) ?? null);
      setSettingsStrength(settings.effects.zoom_strength);
      setQuality(settings.output.default_quality);
      setProject(loaded);
      const state = { segments: sortSegments(loaded.segments), trim: loaded.trim };
      lastSaved = state;
      history.reset(state);
      setTimeMs(loaded.trim.start_ms);
    } catch (e) {
      setError(`読み込みに失敗しました: ${e}`);
    }
    getRecordingEvents(id).then(setEvents).catch((e) => console.error("get_recording_events failed:", e));
  });

  // 画質プリセットに合わせて合成用データを読み込む（出力サイズでカーソル等の相対的な大きさが変わる）
  let previewRequest = 0;
  createEffect(() => {
    const id = props.recordingId;
    const rec = recordingInfo();
    const q = quality();
    if (!id || !rec) return;
    const request = ++previewRequest;
    getPreviewData(id, q)
      .then(async (data) => {
        const r = new PreviewRenderer(data, rec.recording_dir);
        await r.init();
        if (request === previewRequest) setRenderer(r);
      })
      .catch((e) => setError(`プレビューの準備に失敗しました: ${e}`));
  });

  // 区間を編集したらカメラトラックを求め直す
  let trackRequest = 0;
  let trackTimer: number | undefined;
  createEffect(() => {
    const segs = segments();
    const id = props.recordingId;
    if (!id || !project()) return;
    clearTimeout(trackTimer);
    const request = ++trackRequest;
    trackTimer = window.setTimeout(() => {
      getCameraTrack(id, segs)
        .then((track) => {
          if (request === trackRequest) setCameraTrack(track);
        })
        .catch((e) => console.error("get_camera_track failed:", e));
    }, untrack(cameraTrack) ? TRACK_DEBOUNCE_MS : 0);
  });

  // 準備ができたら操作なしで完成形を再生する
  let autoplayed = false;
  createEffect(() => {
    if (autoplayed || !renderer() || !cameraTrack()) return;
    autoplayed = true;
    setTimeMs(trim().start_ms);
    setPlaying(true);
  });

  // --- 自動保存 ---

  let lastSaved: EditState | null = null;
  let saveTimer: number | undefined;

  const saveNow = async () => {
    clearTimeout(saveTimer);
    saveTimer = undefined;
    const id = props.recordingId;
    const base = project();
    const state = history.present();
    if (!id || !base || state === lastSaved) return;
    setSaveState("saving");
    try {
      await saveProject(id, { ...base, segments: state.segments, trim: state.trim });
      lastSaved = state;
      setSaveState(history.present() === state ? "saved" : "saving");
    } catch (e) {
      console.error("save_project failed:", e);
      setSaveState("error");
    }
  };

  createEffect(
    on(
      history.present,
      (state) => {
        if (state === lastSaved || !project()) return;
        setSaveState("saving");
        clearTimeout(saveTimer);
        saveTimer = window.setTimeout(saveNow, SAVE_DEBOUNCE_MS);
      },
      { defer: true },
    ),
  );

  // --- 書き出し ---

  let unlisteners: UnlistenFn[] = [];
  onMount(async () => {
    unlisteners = await Promise.all([
      listen<ExportProgress>("export-progress", (event) => setExportProgress(event.payload)),
      listen<{ output_path: string }>("export-complete", (event) => {
        setExporting(false);
        setExportProgress(null);
        setExportedPath(event.payload.output_path);
      }),
      listen<{ message: string }>("export-error", (event) => {
        setExporting(false);
        setExportProgress(null);
        setError(event.payload.message);
      }),
    ]);
  });

  onCleanup(() => {
    unlisteners.forEach((u) => u());
    clearTimeout(trackTimer);
    void saveNow();
  });

  const handleExport = async (format: ExportFormat) => {
    const id = props.recordingId;
    if (!id) return;
    setPlaying(false);
    setExporting(true);
    setError(null);
    setExportedPath(null);
    setExportProgress({ stage: "starting", progress: 0, output_path: null });
    try {
      // 書き出しは project.json を読むので、編集内容を先に保存する
      await saveNow();
      if (saveState() === "error") throw new Error("編集内容を保存できませんでした");
      await exportRecording(id, format, quality());
    } catch (e) {
      setError(String(e));
      setExporting(false);
      setExportProgress(null);
    }
  };

  const handleClose = async () => {
    setPlaying(false);
    await saveNow();
    props.onClose();
  };

  const progressLabel = () => {
    const p = exportProgress();
    if (!p) return "";
    switch (p.stage) {
      case "composing":
        return `エフェクト合成中... ${Math.round(p.progress * 100)}%`;
      case "encoding":
        return "エンコード中...";
      case "complete":
        return "完了";
      default:
        return "準備中...";
    }
  };

  // --- 再生 ---

  const seek = (ms: number) => {
    setPlaying(false);
    setTimeMs(Math.max(0, Math.min(durationMs(), ms)));
  };

  const togglePlay = () => {
    if (playing()) {
      setPlaying(false);
      return;
    }
    const t = timeMs();
    if (t < trim().start_ms || t >= trim().end_ms - 50) setTimeMs(trim().start_ms);
    setPlaying(true);
  };

  /** 1 フレーム（元の録画のフレーム）単位、または 1 秒単位で動かす */
  const step = (dir: -1 | 1, bySecond: boolean) => {
    const r = renderer();
    if (bySecond || !r) {
      seek(timeMs() + dir * 1000);
      return;
    }
    const times = r.data.frame_times;
    const idx = Math.max(0, Math.min(times.length - 1, r.frameIndexAt(timeMs()) + dir));
    seek(times[idx] ?? timeMs());
  };

  // --- 編集 ---

  const deleteSelected = () => {
    const id = selectedId();
    if (!id) return;
    history.commit({ ...history.present(), segments: segments().filter((s) => s.id !== id) });
    setSelectedId(null);
  };

  // --- キーボード ---

  const handleKeyDown = (e: KeyboardEvent) => {
    const target = e.target as HTMLElement | null;
    if (target && ["INPUT", "SELECT", "TEXTAREA"].includes(target.tagName)) return;
    const ctrl = e.ctrlKey || e.metaKey;
    if (e.key === " ") {
      e.preventDefault();
      (document.activeElement as HTMLElement | null)?.blur();
      togglePlay();
    } else if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      e.preventDefault();
      step(e.key === "ArrowLeft" ? -1 : 1, e.shiftKey);
    } else if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      deleteSelected();
    } else if (ctrl && e.key.toLowerCase() === "z") {
      e.preventDefault();
      if (e.shiftKey) history.redo();
      else history.undo();
    } else if (ctrl && e.key.toLowerCase() === "y") {
      e.preventDefault();
      history.redo();
    } else if (e.key === "Escape") {
      setSelectedId(null);
    }
  };
  window.addEventListener("keydown", handleKeyDown);
  onCleanup(() => window.removeEventListener("keydown", handleKeyDown));

  const audioSrc = () => {
    const r = renderer();
    const rec = recordingInfo();
    return r?.data.has_audio && rec ? convertFileSrc(`${rec.recording_dir}\\audio.wav`) : null;
  };

  const toolButton =
    "px-2 py-1 rounded-md text-xs text-slate-300 hover:bg-slate-700 disabled:opacity-30 disabled:hover:bg-transparent transition-colors";

  return (
    <div class="flex flex-col h-screen">
      <header class="flex items-center justify-between px-6 py-2.5 border-b border-slate-700/50">
        <div class="flex items-center gap-3">
          <h2 class="text-lg font-semibold text-white">プレビュー</h2>
          <span class="text-[11px] text-slate-500">
            {saveState() === "saving" ? "保存中..." : saveState() === "error" ? "保存に失敗しました" : "編集内容は自動保存されます"}
          </span>
        </div>
        <button onClick={handleClose} class="p-2 rounded-lg hover:bg-slate-800 transition-colors text-slate-400">
          <svg class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M6 18L18 6M6 6l12 12" />
          </svg>
        </button>
      </header>

      {/* 完成形のプレビュー */}
      <div class="flex-1 min-h-0 px-6 pt-3">
        <Show
          when={renderer()}
          fallback={
            <div class="w-full h-full flex items-center justify-center text-slate-500 text-sm">
              {props.recordingId ? "読み込み中..." : "録画が選択されていません"}
            </div>
          }
        >
          {(r) => (
            <VideoPlayer
              renderer={r()}
              track={cameraTrack()}
              timeMs={timeMs()}
              playing={playing()}
              trim={trim()}
              audioSrc={audioSrc()}
              onTimeChange={setTimeMs}
              onEnded={() => setPlaying(false)}
            />
          )}
        </Show>
      </div>

      {/* 再生操作 */}
      <div class="flex items-center gap-2 px-6 py-2">
        <button onClick={() => step(-1, false)} class={toolButton} title="1フレーム戻る (←、Shift+←で1秒)">
          <svg class="w-4 h-4" viewBox="0 0 24 24" fill="currentColor">
            <path d="M6 6h2v12H6zm3.5 6l8.5 6V6z" />
          </svg>
        </button>
        <button
          onClick={togglePlay}
          class="p-2 rounded-lg bg-slate-700/60 hover:bg-slate-600 text-white transition-colors"
          title="再生 / 停止 (Space)"
        >
          <Show
            when={playing()}
            fallback={
              <svg class="w-5 h-5" viewBox="0 0 24 24" fill="currentColor">
                <polygon points="6 4 20 12 6 20 6 4" />
              </svg>
            }
          >
            <svg class="w-5 h-5" viewBox="0 0 24 24" fill="currentColor">
              <rect x="6" y="4" width="4" height="16" />
              <rect x="14" y="4" width="4" height="16" />
            </svg>
          </Show>
        </button>
        <button onClick={() => step(1, false)} class={toolButton} title="1フレーム進む (→、Shift+→で1秒)">
          <svg class="w-4 h-4" viewBox="0 0 24 24" fill="currentColor">
            <path d="M6 18l8.5-6L6 6v12zM16 6v12h2V6h-2z" />
          </svg>
        </button>
        <span class="ml-2 font-mono text-xs text-slate-300">
          {formatTime(timeMs())} <span class="text-slate-500">/ {formatTime(durationMs())}</span>
        </span>
        <span class="text-[11px] text-slate-500">
          書き出す長さ {formatTime(trim().end_ms - trim().start_ms)}
        </span>
      </div>

      {/* タイムラインと区間の設定 */}
      <div class="px-6 pb-3 space-y-2">
        <div class="flex items-center gap-1 border-t border-slate-700/50 pt-2">
          <button class={toolButton} onClick={deleteSelected} disabled={!selectedId()} title="選択中のズーム区間を削除 (Delete)">
            区間を削除
          </button>
          <button class={toolButton} onClick={() => history.undo()} disabled={!history.canUndo()} title="元に戻す (Ctrl+Z)">
            元に戻す
          </button>
          <button class={toolButton} onClick={() => history.redo()} disabled={!history.canRedo()} title="やり直す (Ctrl+Shift+Z)">
            やり直す
          </button>
          <span class="ml-auto text-[10px] text-slate-600">
            ズームは自動です。不要な区間はクリックして削除、端のドラッグで伸縮、両端の黄色いハンドルで前後をカット
          </span>
        </div>

        <Show when={durationMs() > 0}>
          <Timeline
            durationMs={durationMs()}
            timeMs={timeMs()}
            segments={segments()}
            trim={trim()}
            selectedId={selectedId()}
            events={events()}
            defaultZoom={defaultZoom()}
            onSeek={seek}
            onSelect={setSelectedId}
            onEditBegin={() => history.begin()}
            onEditPreview={(state) => history.preview(state)}
            onEditEnd={() => history.end()}
          />
        </Show>

      </div>

      {/* 書き出し */}
      <footer class="flex-shrink-0 border-t border-slate-700/50 bg-slate-900/95 px-6 py-3">
        <div class="space-y-2">
          <div class="flex items-center gap-3">
            <select
              value={quality()}
              onChange={(e) => setQuality(e.target.value as QualityPreset)}
              class="bg-slate-800 border border-slate-700 rounded-lg px-3 py-1.5 text-sm text-slate-200 focus:outline-none focus:ring-2 focus:ring-purple-500"
            >
              <option value="Social">Social (1080p / 30fps)</option>
              <option value="HighQuality">High Quality (元解像度 / 60fps)</option>
              <option value="Lightweight">Lightweight (720p / 24fps)</option>
            </select>

            <ExportButtons onExport={handleExport} exporting={exporting()} />

            <div class="ml-auto flex gap-2">
              <button
                onClick={props.onRedo}
                class="py-1.5 px-3 rounded-lg border border-slate-700 text-slate-400 hover:bg-slate-800 hover:text-slate-200 transition-colors text-sm"
              >
                再録画
              </button>
              <button
                onClick={handleClose}
                class="py-1.5 px-3 rounded-lg border border-slate-700 text-slate-400 hover:bg-slate-800 hover:text-slate-200 transition-colors text-sm"
              >
                閉じる
              </button>
            </div>
          </div>

          <Show when={exporting() && exportProgress()}>
            <div class="flex items-center gap-3">
              <span class="text-slate-300 text-sm whitespace-nowrap">{progressLabel()}</span>
              <div class="flex-1 bg-slate-700/50 rounded-full h-2 overflow-hidden">
                <div
                  class="h-full bg-gradient-to-r from-purple-500 to-blue-500 rounded-full transition-all duration-300"
                  style={{ width: `${Math.round((exportProgress()?.progress ?? 0) * 100)}%` }}
                />
              </div>
              <span class="text-slate-500 font-mono text-xs">{Math.round((exportProgress()?.progress ?? 0) * 100)}%</span>
            </div>
          </Show>

          <Show when={error()}>
            <p class="text-red-400 text-sm">{error()}</p>
          </Show>

          <Show when={exportedPath()}>
            <p class="text-green-400 text-sm">エクスポート完了: {exportedPath()}</p>
          </Show>
        </div>
      </footer>
    </div>
  );
}
