import { createSignal, createEffect, createMemo, onMount, onCleanup, Show } from "solid-js";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { exportRecording, exportWithKeyframes, getRecordingsList, getZoomKeyframes, computeActivityCenter } from "../lib/commands";
import type { ExportFormat, ExportProgress, QualityPreset, RecordingInfo } from "../lib/types";
import {
  type ZoomSegment,
  keyframesToSegments,
  segmentsToKeyframes,
  DEFAULT_ZOOM,
  DEFAULT_SEGMENT_DURATION_MS,
  ZOOM_THRESHOLD,
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
  const [selectedSegmentId, setSelectedSegmentId] = createSignal<number | null>(null);

  // セグメントからKF配列を派生（プレビュー・エクスポート用）
  const derivedKeyframes = createMemo(() => {
    const rec = recordingInfo();
    if (!rec) return [];
    return segmentsToKeyframes(segments(), rec.screen_width, rec.screen_height);
  });

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

      // KFをプリフェッチしてセグメントに変換
      try {
        const kfs = await getZoomKeyframes(props.recordingId);
        setSegments(keyframesToSegments(kfs));
      } catch (e) {
        console.error("Failed to preload keyframes:", e);
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
      const kfs = segmentsToKeyframes(segments(), rec.screen_width, rec.screen_height);
      if (kfs.length > 1) {
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

  // リサイズ終了時にBBox再計算 → center/zoomを更新
  const handleSegmentResizeEnd = async (id: number) => {
    const seg = segments().find(s => s.id === id);
    if (!seg || !props.recordingId) return;
    try {
      const result = await computeActivityCenter(
        props.recordingId, Math.round(seg.startMs), Math.round(seg.endMs),
      );
      const updates: Partial<ZoomSegment> = {
        centerX: result.center_x,
        centerY: result.center_y,
        zoomLevel: Math.max(result.zoom_level, 1.0),
      };
      setSegments(prev => prev.map(s => s.id === id ? { ...s, ...updates } : s));
    } catch (e) { console.error("compute_activity_center failed (resize):", e); }
  };

  const addSegment = async (timeMs: number) => {
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

    let centerX = rec ? rec.screen_width / 2 : 960;
    let centerY = rec ? rec.screen_height / 2 : 540;
    let zoomLevel = DEFAULT_ZOOM;

    // Rust側でBBox再計算 → 正しいcenter取得
    if (props.recordingId) {
      try {
        const result = await computeActivityCenter(
          props.recordingId, Math.round(slotStart), Math.round(endMs),
        );
        centerX = result.center_x;
        centerY = result.center_y;
        if (result.zoom_level > ZOOM_THRESHOLD) zoomLevel = result.zoom_level;
      } catch (e) { console.error("compute_activity_center failed (add):", e); }
    }

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

  const mergeSegments = async (id1: number, id2: number) => {
    const s1 = segments().find(s => s.id === id1);
    const s2 = segments().find(s => s.id === id2);
    if (!s1 || !s2) return;

    const startMs = Math.min(s1.startMs, s2.startMs);
    const endMs = Math.max(s1.endMs, s2.endMs);

    let centerX = (s1.centerX + s2.centerX) / 2;
    let centerY = (s1.centerY + s2.centerY) / 2;
    let zoomLevel = Math.min(s1.zoomLevel, s2.zoomLevel);

    // Rust側でBBox再計算 → 正しいcenter/zoom取得
    if (props.recordingId) {
      try {
        const result = await computeActivityCenter(
          props.recordingId, Math.round(startMs), Math.round(endMs),
        );
        centerX = result.center_x;
        centerY = result.center_y;
        zoomLevel = Math.max(result.zoom_level, 1.0);
      } catch (e) { console.error("compute_activity_center failed (merge):", e); }
    }

    const merged: ZoomSegment = {
      id: s1.id,
      startMs,
      endMs,
      zoomLevel,
      centerX,
      centerY,
    };
    setSegments(prev =>
      prev.filter(s => s.id !== id1 && s.id !== id2).concat(merged).sort((a, b) => a.startMs - b.startMs)
    );
  };

  const handleVideoClick = (screenX: number, screenY: number) => {
    const id = selectedSegmentId();
    if (id === null) return;
    updateSegment(id, { centerX: screenX, centerY: screenY });
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

      {/* メインコンテンツ */}
      <div class="flex-1 overflow-y-auto">
        <div class="px-6 pt-4 pb-3 space-y-3 max-w-4xl mx-auto w-full">
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
                keyframes={derivedKeyframes()}
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

          {/* Timeline section */}
          <Show when={recordingInfo() && recordingInfo()!.duration_ms > 0}>
            <div class="border border-slate-700/50 rounded-lg overflow-hidden">
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
                    onSegmentResizeEnd={handleSegmentResizeEnd}
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

      {/* 固定フッター: エクスポート + アクション */}
      <footer class="flex-shrink-0 border-t border-slate-700/50 bg-slate-900/95 backdrop-blur-sm px-6 py-3">
        <div class="max-w-4xl mx-auto space-y-2">
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
