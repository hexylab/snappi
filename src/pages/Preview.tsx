import { createSignal, createEffect, createMemo, onMount, onCleanup, Show } from "solid-js";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { exportRecording, getAutoSegments, getCameraTrack, getRecordingsList } from "../lib/commands";
import type { ExportFormat, ExportProgress, QualityPreset, RecordingInfo } from "../lib/types";
import type { CameraTrack } from "../lib/generated/CameraTrack";
import {
  type ZoomSegment,
  fromBackendSegment,
  toBackendSegment,
  DEFAULT_ZOOM,
  DEFAULT_SEGMENT_DURATION_MS,
  generateSegmentId,
} from "../lib/zoomSegments";
import ExportButtons from "../components/ExportButtons";
import SegmentEditor from "../components/SegmentEditor";
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
  const [showTimeline, setShowTimeline] = createSignal(false);
  const [seekTimeMs, setSeekTimeMs] = createSignal<number | undefined>(undefined);
  const [currentTimeMs, setCurrentTimeMs] = createSignal(0);
  const [showZoomOverlay, setShowZoomOverlay] = createSignal(true);
  const [showZoomPreview, setShowZoomPreview] = createSignal(false);
  const [editMode, setEditMode] = createSignal<"position" | null>(null);

  // --- セグメントベースのstate ---
  const [segments, setSegments] = createSignal<ZoomSegment[]>([]);
  /** 自動生成した区間を読み込めたか。読み込めていなければ書き出し時にバックエンドで自動生成させる */
  const [segmentsLoaded, setSegmentsLoaded] = createSignal(false);
  const [selectedSegmentId, setSelectedSegmentId] = createSignal<number | null>(null);
  /** ズームプレビュー用のカメラトラック（区間を変えるたびにバックエンドで再計算） */
  const [cameraTrack, setCameraTrack] = createSignal<CameraTrack | null>(null);

  // 選択中のセグメント
  const selectedSegment = createMemo(() => {
    const id = selectedSegmentId();
    if (id === null) return null;
    return segments().find(s => s.id === id) ?? null;
  });

  // 選択中セグメントのindex
  const selectedSegmentIndex = createMemo(() => {
    const id = selectedSegmentId();
    if (id === null) return -1;
    return segments().findIndex(s => s.id === id);
  });

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

      // 自動生成したズーム区間を編集用の区間に変換
      try {
        const rec = recordingInfo();
        const segs = await getAutoSegments(props.recordingId);
        setSegments(segs.map((s) => fromBackendSegment(s, rec?.screen_width ?? 1920, rec?.screen_height ?? 1080)));
        setSegmentsLoaded(true);
      } catch (e) {
        console.error("Failed to load zoom segments:", e);
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
    const rec = recordingInfo();
    if (!rec) return;
    setExporting(true);
    setError(null);
    setExportedPath(null);
    setExportProgress({ stage: "starting", progress: 0, output_path: null });
    try {
      const segs = segmentsLoaded() ? segments().map(toBackendSegment) : null;
      await exportRecording(props.recordingId, format, quality(), segs);
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

  // --- ズームプレビュー: 区間が変わったらカメラトラックを再計算 ---
  let trackRequest = 0;
  createEffect(() => {
    const segs = segments().map(toBackendSegment);
    if (!showZoomPreview() || !props.recordingId) return;
    const request = ++trackRequest;
    getCameraTrack(props.recordingId, segs)
      .then((track) => {
        if (request === trackRequest) setCameraTrack(track);
      })
      .catch((e) => console.error("get_camera_track failed:", e));
  });

  // --- セグメント選択時の自動シーク ---
  createEffect(() => {
    const seg = selectedSegment();
    if (seg) {
      setSeekTimeMs(seg.startMs);
      if (!showZoomOverlay()) setShowZoomOverlay(true);
    }
  });

  // --- セグメント操作関数 ---

  const updateSegment = (id: number, updates: Partial<ZoomSegment>) => {
    setSegments(prev => prev.map(s => s.id === id ? { ...s, ...updates } : s));
  };

  const addSegment = (timeMs: number) => {
    const rec = recordingInfo();
    const durationMs = rec?.duration_ms ?? timeMs + DEFAULT_SEGMENT_DURATION_MS;

    // 既存セグメントとの重なりチェック → 空きスロットに収める
    const sorted = [...segments()].sort((a, b) => a.startMs - b.startMs);
    let slotStart = timeMs;
    let slotEnd = durationMs;

    // timeMs以降で最初に来るセグメント = 右側の壁
    const nextSeg = sorted.find(s => s.startMs > timeMs);
    if (nextSeg) slotEnd = nextSeg.startMs;

    // timeMsを含む or 直前のセグメント = 左側の壁
    const prevSeg = [...sorted].reverse().find(s => s.endMs <= timeMs);
    const overlapping = sorted.find(s => s.startMs <= timeMs && s.endMs > timeMs);
    if (overlapping) {
      // クリック位置が既存セグメント内 → その直後に配置
      slotStart = overlapping.endMs;
      if (slotStart >= slotEnd) return; // 空きなし
    } else if (prevSeg) {
      slotStart = Math.max(timeMs, prevSeg.endMs);
    }

    const endMs = Math.min(slotStart + DEFAULT_SEGMENT_DURATION_MS, slotEnd);
    if (endMs - slotStart < 100) return; // 最小幅未満なら追加しない

    // 中心は自動追従、倍率はズーム強さの既定値（手動で変更するまで）
    const centerX = rec ? rec.screen_width / 2 : 960;
    const centerY = rec ? rec.screen_height / 2 : 540;
    const zoomLevel = DEFAULT_ZOOM;

    const newSeg: ZoomSegment = {
      id: generateSegmentId(),
      startMs: slotStart,
      endMs,
      zoomLevel,
      centerX,
      centerY,
    };
    setSegments(prev => [...prev, newSeg].sort((a, b) => a.startMs - b.startMs));
    setSelectedSegmentId(newSeg.id);
  };

  const deleteSegment = (id: number) => {
    setSegments(prev => prev.filter(s => s.id !== id));
    if (selectedSegmentId() === id) {
      setSelectedSegmentId(null);
      setEditMode(null);
    }
  };

  const mergeSegments = (id1: number, id2: number) => {
    const s1 = segments().find(s => s.id === id1);
    const s2 = segments().find(s => s.id === id2);
    if (!s1 || !s2) return;

    // 手動で設定した中心・倍率はそちらを優先し、どちらも自動なら自動のまま
    const centerSrc = s1.manualCenter ? s1 : s2.manualCenter ? s2 : s1;
    const zoomSrc = s1.manualZoom ? s1 : s2.manualZoom ? s2 : s1;
    const merged: ZoomSegment = {
      id: s1.id,
      startMs: Math.min(s1.startMs, s2.startMs),
      endMs: Math.max(s1.endMs, s2.endMs),
      zoomLevel: zoomSrc.zoomLevel,
      centerX: centerSrc.centerX,
      centerY: centerSrc.centerY,
      manualCenter: s1.manualCenter || s2.manualCenter,
      manualZoom: s1.manualZoom || s2.manualZoom,
    };
    setSegments(prev =>
      prev.filter(s => s.id !== id1 && s.id !== id2).concat(merged).sort((a, b) => a.startMs - b.startMs)
    );
  };

  const handleVideoClick = (screenX: number, screenY: number) => {
    const id = selectedSegmentId();
    if (id === null) return;
    // 中心位置を手動設定したのでフラグを立てる
    updateSegment(id, { centerX: screenX, centerY: screenY, manualCenter: true });
    setEditMode(null);
  };

  const navigateSegment = (dir: -1 | 1) => {
    const segs = segments();
    const idx = selectedSegmentIndex();
    const newIdx = idx + dir;
    if (newIdx >= 0 && newIdx < segs.length) {
      setSelectedSegmentId(segs[newIdx].id);
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

      {/* メインコンテンツ: 動画エディタ風レイアウト
          - Video Player: 中央寄せで max-w-7xl（視聴体験のため大きすぎない）
          - Timeline: ウィンドウ幅いっぱいに広げて編集しやすく */}
      <div class="flex-1 overflow-y-auto">
        <div class="pt-4 pb-3 space-y-3 w-full">
          {/* Video Player: 中央寄せエリア */}
          <div class="px-6 max-w-7xl mx-auto w-full">
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
                cameraTrack={cameraTrack()}
                segments={segments()}
                selectedSegment={selectedSegment()}
                seekToTimeMs={seekTimeMs()}
                onTimeChange={(t) => setCurrentTimeMs(t)}
                screenWidth={rec().screen_width}
                screenHeight={rec().screen_height}
                showZoomOverlay={showZoomOverlay()}
                showZoomPreview={showZoomPreview()}
                editMode={editMode()}
                onVideoClick={handleVideoClick}
              />
            )}
          </Show>
          </div>{/* /Video Player 中央寄せエリア */}

          {/* Timeline section: ウィンドウ幅いっぱいで編集しやすく（動画エディタ風） */}
          <Show when={recordingInfo() && recordingInfo()!.duration_ms > 0}>
            <div class="mx-2 border border-slate-700/50 rounded-lg overflow-hidden">
              {/* Header with toggle */}
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
                  <span>詳細タイムライン</span>
                </button>

                <Show when={showTimeline()}>
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
                </Show>
              </div>

              <Show when={showTimeline()}>
                <div class="px-4 py-3 border-t border-slate-700/30 space-y-3">
                  <Timeline
                    recordingId={props.recordingId!}
                    durationMs={recordingInfo()!.duration_ms}
                    screenWidth={recordingInfo()!.screen_width}
                    screenHeight={recordingInfo()!.screen_height}
                    currentTimeMs={currentTimeMs()}
                    onSeekToTime={(ms) => setSeekTimeMs(ms)}
                    segments={segments()}
                    selectedSegmentId={selectedSegmentId()}
                    onSelectSegment={(id) => setSelectedSegmentId(id)}
                    onAddSegment={addSegment}
                    onDeleteSegment={deleteSegment}
                    onUpdateSegment={updateSegment}
                    onMergeSegments={mergeSegments}
                  />

                  {/* SegmentEditor: セグメント選択時のみ表示 */}
                  <Show when={selectedSegment() && selectedSegmentIndex() >= 0}>
                    <SegmentEditor
                      segment={selectedSegment()!}
                      index={selectedSegmentIndex()}
                      totalCount={segments().length}
                      onChange={updateSegment}
                      onDelete={deleteSegment}
                      onClose={() => { setSelectedSegmentId(null); setEditMode(null); }}
                      onNavigate={navigateSegment}
                      editMode={editMode()}
                      onToggleEditMode={setEditMode}
                    />
                  </Show>
                </div>
              </Show>
            </div>
          </Show>
        </div>
      </div>

      {/* 固定フッター: エクスポート + アクション（ウィンドウ幅いっぱい） */}
      <footer class="flex-shrink-0 border-t border-slate-700/50 bg-slate-900/95 backdrop-blur-sm px-6 py-3">
        <div class="max-w-7xl mx-auto space-y-2">
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
              <button onClick={props.onRedo} class="py-1.5 px-3 rounded-lg border border-slate-700 text-slate-400 hover:bg-slate-800 hover:text-slate-200 transition-colors text-sm">
                再録画
              </button>
              <button onClick={props.onClose} class="py-1.5 px-3 rounded-lg border border-slate-700 text-slate-400 hover:bg-slate-800 hover:text-slate-200 transition-colors text-sm">
                閉じる
              </button>
            </div>
          </div>

          {/* 進捗バー */}
          <Show when={exporting() && exportProgress()}>
            <div class="flex items-center gap-3">
              <span class="text-slate-300 text-sm whitespace-nowrap">{progressLabel()}</span>
              <div class="flex-1 bg-slate-700/50 rounded-full h-2 overflow-hidden">
                <div
                  class="h-full bg-gradient-to-r from-purple-500 to-blue-500 rounded-full transition-all duration-300"
                  style={{ width: `${Math.round((exportProgress()?.progress ?? 0) * 100)}%` }}
                />
              </div>
              <span class="text-slate-500 font-mono text-xs">
                {Math.round((exportProgress()?.progress ?? 0) * 100)}%
              </span>
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
