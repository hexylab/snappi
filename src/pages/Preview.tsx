import { createEffect, createSignal, For, on, onCleanup, onMount, Show, untrack } from "solid-js";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  autoProject,
  exportRecording,
  getCameraTrack,
  getPreviewData,
  getRecordingEvents,
  getRecordingsList,
  loadProject,
  saveProject,
  openFile,
  showInFolder,
} from "../lib/commands";
import { baseName, formatClock, formatDate, FORMAT_OPTIONS, QUALITY_OPTIONS, STRENGTH_OPTIONS } from "../lib/format";
import type { ExportFormat } from "../lib/generated/ExportFormat";
import type { ExportProgress } from "../lib/generated/ExportProgress";
import type { QualityPreset } from "../lib/generated/QualityPreset";
import type { RecordingInfo } from "../lib/generated/RecordingInfo";
import type { TimelineEvent } from "../lib/generated/TimelineEvent";
import type { CameraTrack } from "../lib/generated/CameraTrack";
import type { Project } from "../lib/generated/Project";
import type { ZoomStrength } from "../lib/generated/ZoomStrength";
import { createHistory } from "../lib/history";
import { PreviewRenderer } from "../lib/previewRenderer";
import { type EditState, formatTime, newSegmentAt, sortSegments } from "../lib/segments";
import { createStoredFlag, settingsStore } from "../lib/settingsStore";
import Timeline from "../components/Timeline";
import VideoPlayer from "../components/VideoPlayer";
import { ToggleSwitch } from "../components/ui";

interface Props {
  recordingId: string;
  onBack: () => void;
}

/** 編集してから自動保存するまでの待ち時間 (ms) */
const SAVE_DEBOUNCE_MS = 500;
/** 区間を編集してからカメラトラックを求め直すまでの待ち時間 (ms) */
const TRACK_DEBOUNCE_MS = 150;

/** ズーム強さごとの倍率（src-tauri/src/camera/spec.rs）。Off でも手動の区間は標準の倍率で寄る */
const STRENGTH_ZOOM: Record<ZoomStrength, number> = { Off: 1.8, Subtle: 1.5, Standard: 1.8, Strong: 2.2 };

/**
 * プレビュー。開くと完成形を自動再生し、「書き出す」1つで書き出せる。
 * 通常モードは再生と前後のカットだけ。「詳細」をオンにするとズームの編集と形式・画質の選択が出る。
 */
export default function Preview(props: Props) {
  const { settings } = settingsStore;
  const [details, setDetails] = createStoredFlag("snappi.preview.details", false);

  const [recordingInfo, setRecordingInfo] = createSignal<RecordingInfo | null>(null);
  const [project, setProject] = createSignal<Project | null>(null);
  const [events, setEvents] = createSignal<TimelineEvent[]>([]);
  const [renderer, setRenderer] = createSignal<PreviewRenderer | null>(null);
  const [cameraTrack, setCameraTrack] = createSignal<CameraTrack | null>(null);

  const [timeMs, setTimeMs] = createSignal(0);
  const [playing, setPlaying] = createSignal(false);
  const [selectedId, setSelectedId] = createSignal<string | null>(null);
  const [saveState, setSaveState] = createSignal<"saved" | "saving" | "error">("saved");
  const [regenerating, setRegenerating] = createSignal(false);

  // 書き出し（詳細モードで選んだ形式・画質。通常モードでは設定の値を使う）
  const [pickedFormat, setPickedFormat] = createSignal<ExportFormat | null>(null);
  const [pickedQuality, setPickedQuality] = createSignal<QualityPreset | null>(null);
  const format = (): ExportFormat =>
    (details() && pickedFormat()) || settings()?.output.default_format || "Mp4";
  const quality = (): QualityPreset =>
    (details() && pickedQuality()) || settings()?.output.default_quality || "Social";

  const [exportProgress, setExportProgress] = createSignal<ExportProgress | null>(null);
  const [exportedPath, setExportedPath] = createSignal<string | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  const exporting = () => exportProgress() !== null;

  const history = createHistory<EditState>({ segments: [], trim: { start_ms: 0, end_ms: 0 }, zoom_strength: null });
  const segments = () => history.present().segments;
  const trim = () => history.present().trim;
  const durationMs = () => recordingInfo()?.duration_ms ?? 0;
  /** この録画のズーム強さ（未指定なら設定の値） */
  const strength = (): ZoomStrength => history.present().zoom_strength ?? settings()?.effects.zoom_strength ?? "Standard";
  const defaultZoom = () => STRENGTH_ZOOM[strength()];

  // --- 読み込み ---

  onMount(async () => {
    const id = props.recordingId;
    try {
      const [recordings, loaded] = await Promise.all([
        getRecordingsList(),
        loadProject(id),
        settings() ? Promise.resolve() : settingsStore.load(),
      ]);
      setRecordingInfo(recordings.find((r) => r.id === id) ?? null);
      setProject(loaded);
      const state: EditState = {
        segments: sortSegments(loaded.segments),
        trim: loaded.trim,
        zoom_strength: loaded.zoom_strength,
      };
      lastSaved = state;
      history.reset(state);
      setTimeMs(loaded.trim.start_ms);
    } catch (e) {
      setError(`録画を読み込めませんでした: ${e}`);
    }
    getRecordingEvents(id).then(setEvents).catch((e) => console.error("get_recording_events failed:", e));
  });

  // 画質に合わせて合成用データを読み込む（出力サイズでカーソル等の相対的な大きさが変わる）
  let previewRequest = 0;
  createEffect(() => {
    const rec = recordingInfo();
    const q = quality();
    if (!rec) return;
    const request = ++previewRequest;
    getPreviewData(props.recordingId, q)
      .then(async (data) => {
        const r = new PreviewRenderer(data, rec.recording_dir);
        await r.init();
        if (request === previewRequest) setRenderer(r);
      })
      .catch((e) => setError(`プレビューを準備できませんでした: ${e}`));
  });

  // 区間・強さを変えたらカメラトラックを求め直す
  let trackRequest = 0;
  let trackTimer: number | undefined;
  createEffect(() => {
    const segs = segments();
    const zs = history.present().zoom_strength;
    if (!project()) return;
    clearTimeout(trackTimer);
    const request = ++trackRequest;
    trackTimer = window.setTimeout(
      () => {
        getCameraTrack(props.recordingId, segs, zs)
          .then((track) => {
            if (request === trackRequest) setCameraTrack(track);
          })
          .catch((e) => console.error("get_camera_track failed:", e));
      },
      untrack(cameraTrack) ? TRACK_DEBOUNCE_MS : 0,
    );
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
    const base = project();
    const state = history.present();
    if (!base || state === lastSaved) return;
    setSaveState("saving");
    try {
      await saveProject(props.recordingId, {
        ...base,
        segments: state.segments,
        trim: state.trim,
        zoom_strength: state.zoom_strength,
      });
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
        setExportProgress(null);
        setExportedPath(event.payload.output_path);
      }),
      listen<{ message: string }>("export-error", (event) => {
        setExportProgress(null);
        setError(`書き出せませんでした: ${event.payload.message}`);
      }),
    ]);
  });

  onCleanup(() => {
    unlisteners.forEach((u) => u());
    clearTimeout(trackTimer);
    void saveNow();
  });

  const handleExport = async () => {
    setPlaying(false);
    setError(null);
    setExportedPath(null);
    setExportProgress({ stage: "starting", progress: 0, output_path: null });
    try {
      // 書き出しは project.json を読むので、編集内容を先に保存する
      await saveNow();
      if (saveState() === "error") throw new Error("編集内容を保存できませんでした");
      await exportRecording(props.recordingId, format(), quality());
    } catch (e) {
      setError(`書き出せませんでした: ${e}`);
      setExportProgress(null);
    }
  };

  const progressPercent = () => Math.round((exportProgress()?.progress ?? 0) * 100);
  const progressLabel = () => {
    switch (exportProgress()?.stage) {
      case "composing":
        return `動画を作成しています… ${progressPercent()}%`;
      case "encoding":
        return "動画ファイルに保存しています…";
      default:
        return "書き出しを準備しています…";
    }
  };

  const handleBack = async () => {
    setPlaying(false);
    await saveNow();
    props.onBack();
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

  // --- 編集（詳細モード） ---

  const deleteSelected = () => {
    const id = selectedId();
    if (!id) return;
    history.commit({ ...history.present(), segments: segments().filter((s) => s.id !== id) });
    setSelectedId(null);
  };

  const addSegmentAt = (ms: number) => {
    const seg = newSegmentAt(segments(), ms, durationMs());
    if (!seg) return;
    history.commit({ ...history.present(), segments: sortSegments([...segments(), seg]) });
    setSelectedId(seg.id);
  };
  const canAddAtPlayhead = () => newSegmentAt(segments(), timeMs(), durationMs()) !== null;

  /** 自動のズームを作り直す。keepTrim なら今のカットは残す */
  const regenerate = async (zs: ZoomStrength, keepTrim: boolean) => {
    setRegenerating(true);
    try {
      const auto = await autoProject(props.recordingId, zs);
      history.commit({
        segments: sortSegments(auto.segments),
        trim: keepTrim ? trim() : auto.trim,
        zoom_strength: zs,
      });
      setSelectedId(null);
    } catch (e) {
      setError(`自動のズームを作れませんでした: ${e}`);
    } finally {
      setRegenerating(false);
    }
  };

  createEffect(() => {
    if (!details()) setSelectedId(null);
  });

  // --- キーボード ---

  const handleKeyDown = (e: KeyboardEvent) => {
    const target = e.target as HTMLElement | null;
    if (target && ["INPUT", "SELECT", "TEXTAREA"].includes(target.tagName)) return;
    if (document.querySelector("dialog[open]")) return;
    const ctrl = e.ctrlKey || e.metaKey;
    if (e.key === " ") {
      e.preventDefault();
      (document.activeElement as HTMLElement | null)?.blur();
      togglePlay();
    } else if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      e.preventDefault();
      step(e.key === "ArrowLeft" ? -1 : 1, e.shiftKey);
    } else if ((e.key === "Delete" || e.key === "Backspace") && details()) {
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

  const formatLabel = () => FORMAT_OPTIONS.find((o) => o.value === format())?.label ?? "";
  const qualityLabel = () => `${QUALITY_OPTIONS.find((o) => o.value === quality())?.label ?? ""}画質`;

  const audioSrc = () => {
    const r = renderer();
    const rec = recordingInfo();
    return r?.data.has_audio && rec ? convertFileSrc(`${rec.recording_dir}\\audio.wav`) : null;
  };

  /** 文字として描く（絵文字の色付きアイコンにしない） */
  const glyph = (c: string) => `${c}︎`;

  return (
    <div class="flex h-full flex-col">
      <header class="app-header">
        <button
          class="btn-icon flex-shrink-0"
          onClick={handleBack}
          disabled={exporting()}
          title={exporting() ? "書き出しが終わるまでお待ちください" : undefined}
        >
          ← これまでの録画
        </button>
        <h1 class="text-title min-w-0 truncate" classList={{ "max-[800px]:hidden": details() }}>
          {recordingInfo() ? `${formatDate(recordingInfo()!.date)} の録画` : ""}
        </h1>
        <span class="flex-1" />

        <Show when={saveState() !== "saved"}>
          <span class="status-pill" classList={{ "is-error": saveState() === "error" }} role="status">
            {saveState() === "error" ? "⚠️ 保存できませんでした" : "保存中…"}
          </span>
        </Show>

        <label
          class="flex flex-shrink-0 cursor-pointer items-center gap-2 text-label-sm"
          title="ズームの調整と、書き出す形式・画質の選択ができるようになります"
        >
          細かく編集
          <ToggleSwitch label="細かく編集" checked={details()} onChange={setDetails} />
        </label>

        <Show when={details()}>
          <select
            class="select flex-shrink-0"
            aria-label="形式"
            title="形式"
            disabled={exporting()}
            onChange={(e) => setPickedFormat(e.currentTarget.value as ExportFormat)}
          >
            <For each={FORMAT_OPTIONS}>
              {(o) => (
                <option value={o.value} selected={format() === o.value}>
                  {o.label}
                </option>
              )}
            </For>
          </select>
          <select
            class="select flex-shrink-0"
            aria-label="画質"
            title="画質"
            disabled={exporting()}
            onChange={(e) => setPickedQuality(e.currentTarget.value as QualityPreset)}
          >
            <For each={QUALITY_OPTIONS}>
              {(o) => (
                <option value={o.value} selected={quality() === o.value}>
                  {o.label}（{o.spec}）
                </option>
              )}
            </For>
          </select>
        </Show>

        <Show when={!details()}>
          <span
            class="flex-shrink-0 text-caption text-on-surface-dim"
            title="形式と画質は、設定または「細かく編集」で変えられます"
          >
            {formatLabel()}・{qualityLabel()}
          </span>
        </Show>
        <button class="btn btn-primary flex-shrink-0" onClick={handleExport} disabled={!project() || exporting()}>
          {exporting() ? "書き出し中…" : "動画を書き出す"}
        </button>
      </header>

      {/* お知らせ */}
      <Show when={error() || exporting() || exportedPath()}>
        <div class="flex flex-col gap-2 px-4 pt-3">
          <Show when={error()}>
            <div class="error-bar" role="alert">
              <span class="flex-1">{error()}</span>
              <button class="btn-icon" aria-label="閉じる" onClick={() => setError(null)}>
                ✕
              </button>
            </div>
          </Show>
          <Show when={exporting()}>
            <div class="notice-bar" role="status">
              <span class="flex-1">⏳ {progressLabel()}</span>
              <div class="meter-track">
                <div class="meter-fill" style={{ width: `${progressPercent()}%` }} />
              </div>
            </div>
          </Show>
          <Show when={exportedPath()}>
            {(path) => (
              <div class="notice-bar" role="status">
                <span class="min-w-0 flex-1 truncate">
                  ✅ 書き出しました：<span class="font-semibold">{baseName(path())}</span>
                </span>
                <button class="btn btn-secondary btn-tight" onClick={() => openFile(path()).catch((e) => setError(String(e)))}>
                  ▶️ 動画を開く
                </button>
                <button class="btn btn-secondary btn-tight" onClick={() => void showInFolder(path())}>
                  📂 フォルダを開く
                </button>
                <button class="btn-icon" aria-label="閉じる" onClick={() => setExportedPath(null)}>
                  ✕
                </button>
              </div>
            )}
          </Show>
        </div>
      </Show>

      {/* 完成形のプレビュー */}
      <main class="min-h-0 flex-1 px-4 py-3">
        <div class="card h-full overflow-hidden p-2">
          <Show
            when={renderer()}
            fallback={<div class="empty-placeholder flex h-full items-center justify-center">準備しています…</div>}
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
      </main>

      {/* 再生とカット・編集 */}
      <section class="card mx-4 mb-4 flex flex-col gap-2 px-3 py-2.5">
        <div class="flex items-center gap-2">
          <Show when={details()}>
            <button class="btn-icon" onClick={() => step(-1, false)} title="1コマ戻る（← キー。Shift+← で1秒）">
              ◀ 1コマ
            </button>
          </Show>
          <button class="btn-play" onClick={togglePlay} title="再生／一時停止（Space キー）" aria-label={playing() ? "一時停止" : "再生"}>
            {playing() ? glyph("⏸") : glyph("▶")}
          </button>
          <Show when={details()}>
            <button class="btn-icon" onClick={() => step(1, false)} title="1コマ進む（→ キー。Shift+→ で1秒）">
              1コマ ▶
            </button>
          </Show>
          <span class="tabular text-label-sm">
            {details() ? formatTime(timeMs()) : formatClock(timeMs())}
            <span class="text-on-surface-dim"> / {formatClock(durationMs())}</span>
          </span>
          <span class="flex-1" />
          <span class="text-caption text-on-surface-dim">
            書き出す長さ <span class="tabular font-semibold text-on-surface">{formatClock(trim().end_ms - trim().start_ms)}</span>
          </span>
        </div>

        <Show when={durationMs() > 0}>
          <Timeline
            durationMs={durationMs()}
            timeMs={timeMs()}
            segments={segments()}
            trim={trim()}
            detailed={details()}
            selectedId={selectedId()}
            events={events()}
            defaultZoom={defaultZoom()}
            onSeek={seek}
            onSelect={setSelectedId}
            onAddAt={addSegmentAt}
            onEditBegin={() => history.begin()}
            onEditPreview={(patch) => history.preview({ ...history.present(), ...patch })}
            onEditEnd={() => history.end()}
          />
        </Show>

        <Show when={!details()}>
          <p class="text-caption text-on-surface-dim">
            両端のオレンジのつまみをドラッグすると、動画の最初と最後をカットできます。
          </p>
        </Show>

        <Show when={details()}>
          <div class="flex flex-wrap items-center gap-x-4 gap-y-1 text-caption text-on-surface-dim">
            <span class="flex-1">
              ズームする区間はクリックで選択、端をドラッグで長さを変更、空いている所をダブルクリックで追加できます。
            </span>
            <span class="flex items-center gap-1">
              <span class="inline-block h-3 w-0.5 bg-on-surface-dim" /> クリック
            </span>
            <span class="flex items-center gap-1">
              <span class="inline-block h-3 w-0.5 bg-eyebrow" /> キー入力
            </span>
          </div>
          <div class="flex flex-wrap items-center gap-1.5 border-t border-outline pt-2">
            <button
              class="btn-icon"
              onClick={() => history.undo()}
              disabled={!history.canUndo()}
              title="元に戻す（Ctrl+Z）"
              aria-label="元に戻す"
            >
              ↩️
            </button>
            <button
              class="btn-icon"
              onClick={() => history.redo()}
              disabled={!history.canRedo()}
              title="やり直す（Ctrl+Shift+Z）"
              aria-label="やり直す"
            >
              ↪️
            </button>
            <span class="mx-1 h-5 w-px bg-outline" />
            <button
              class="btn-compact"
              onClick={() => addSegmentAt(timeMs())}
              disabled={!canAddAtPlayhead()}
              title="再生位置からズームする区間を追加（タイムラインの空いている所をダブルクリックでも追加）"
            >
              ➕ ズームを追加
            </button>
            <button class="btn-compact" onClick={deleteSelected} disabled={!selectedId()} title="選んだズームを削除（Delete キー）">
              🗑️ ズームを削除
            </button>
            <span class="flex-1" />
            <label class="flex items-center gap-2 text-label-sm">
              ズームの強さ
              <select
                class="select"
                title="変えると、ズームする区間を自動で作り直します（元に戻すこともできます）"
                disabled={regenerating()}
                onChange={(e) => void regenerate(e.currentTarget.value as ZoomStrength, true)}
              >
                <For each={STRENGTH_OPTIONS}>
                  {(o) => (
                    <option value={o.value} selected={strength() === o.value}>
                      {o.label}
                    </option>
                  )}
                </For>
              </select>
            </label>
            <button
              class="btn-compact"
              onClick={() => void regenerate(strength(), false)}
              disabled={regenerating()}
              title="ズームとカットを、録画直後の自動の結果に戻します（元に戻すこともできます）"
            >
              🔄 自動の結果に戻す
            </button>
          </div>
        </Show>
      </section>
    </div>
  );
}
