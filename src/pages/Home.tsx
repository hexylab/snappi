import { createSignal, For, onMount, Show } from "solid-js";
import { deleteRecording, getRecordingsList, listWindows } from "../lib/commands";
import { settingsStore } from "../lib/settingsStore";
import type { RecordingInfo, RecordingMode, WindowInfo } from "../lib/types";
import RecordingTile from "../components/RecordingTile";
import SettingsDialog from "../components/SettingsDialog";
import { ToggleSwitch } from "../components/ui";

interface Props {
  onStartRecording: () => void;
  onOpenPreview: (id: string) => void;
  settingsOpen: boolean;
  onSettingsOpenChange: (open: boolean) => void;
  /** 録画の開始・保存の失敗 */
  error: string | null;
  onDismissError: () => void;
}

/** ホーム: 録画を始める（主役）と、これまでの録画 */
export default function Home(props: Props) {
  const { settings, update } = settingsStore;
  const [recordings, setRecordings] = createSignal<RecordingInfo[] | null>(null);
  const [windows, setWindows] = createSignal<WindowInfo[]>([]);
  const [loadError, setLoadError] = createSignal<string | null>(null);

  const mode = () => settings()?.recording.recording_mode ?? ({ type: "Display" } as RecordingMode);
  /** 選んであるウィンドウ（今は無いウィンドウなら null） */
  const selectedWindow = () => {
    const m = mode();
    if (m.type !== "Window") return null;
    return windows().find((w) => w.hwnd === m.hwnd) ?? null;
  };
  const canStart = () => settings() !== null && (mode().type !== "Window" || selectedWindow() !== null);

  const loadRecordings = async () => {
    try {
      setRecordings(await getRecordingsList());
    } catch (e) {
      setRecordings([]);
      setLoadError(`録画の一覧を読み込めませんでした: ${e}`);
    }
  };

  const refreshWindows = async () => {
    try {
      setWindows(await listWindows());
    } catch (e) {
      console.error("list_windows failed:", e);
    }
  };

  onMount(async () => {
    void loadRecordings();
    try {
      const s = settings() ?? (await settingsStore.load());
      if (s.recording.recording_mode.type === "Window") await refreshWindows();
    } catch (e) {
      setLoadError(`設定を読み込めませんでした: ${e}`);
    }
  });

  const setMode = (next: RecordingMode) => update((s) => ({ ...s, recording: { ...s.recording, recording_mode: next } }));

  const chooseWindowMode = async () => {
    await refreshWindows();
    if (mode().type !== "Window") setMode({ type: "Window", hwnd: 0, title: "", rect: [0, 0, 0, 0] });
  };

  const setAudio = (key: "capture_system_audio" | "capture_microphone", on: boolean) =>
    update((s) => ({ ...s, recording: { ...s.recording, [key]: on } }));

  const handleDelete = async (id: string) => {
    try {
      await deleteRecording(id);
      setRecordings((list) => (list ?? []).filter((r) => r.id !== id));
    } catch (e) {
      setLoadError(`録画を削除できませんでした: ${e}`);
    }
  };

  return (
    <div class="flex h-full flex-col">
      <header class="app-header">
        <span class="text-title">🎬 Snappi</span>
        <span class="flex-1" />
        <button class="btn btn-secondary btn-tight" onClick={() => props.onSettingsOpenChange(true)}>
          ⚙️ 設定
        </button>
      </header>

      <main class="flex-1 min-h-0 overflow-y-auto">
        <div class="mx-auto flex max-w-[1080px] flex-col gap-3 p-4">
          <Show when={props.error ?? loadError() ?? settingsStore.saveError()}>
            {(message) => (
              <div class="error-bar" role="alert">
                <span class="flex-1">{message()}</span>
                <button
                  class="btn-icon"
                  aria-label="閉じる"
                  onClick={() => {
                    props.onDismissError();
                    setLoadError(null);
                  }}
                >
                  ✕
                </button>
              </div>
            )}
          </Show>

          {/* 録画を始める */}
          <section class="card">
            <div class="card-head">
              <span class="card-title">新しい録画</span>
            </div>
            <div class="flex flex-col gap-4 px-4 py-4">
              <p class="text-body-sm">
                録画を止めると、操作した場所を自動で拡大した動画ができあがります。
              </p>
              <div class="grid grid-cols-[88px_1fr] items-center gap-x-3 gap-y-3">
                <span class="text-label-sm text-muted">録画する範囲</span>
                <div class="flex flex-wrap items-center gap-3">
                  <div class="segmented" role="radiogroup" aria-label="録画する範囲">
                    <button role="radio" aria-checked={mode().type !== "Window"} onClick={() => setMode({ type: "Display" })}>
                      🖥️ 画面全体
                    </button>
                    <button role="radio" aria-checked={mode().type === "Window"} onClick={chooseWindowMode}>
                      🪟 ウィンドウ
                    </button>
                  </div>
                  <Show when={mode().type === "Window"}>
                    <select
                      class="select min-w-0 max-w-[360px] flex-1"
                      aria-label="録画するウィンドウ"
                      onFocus={refreshWindows}
                      onChange={(e) => {
                        const win = windows().find((w) => String(w.hwnd) === e.currentTarget.value);
                        if (win) setMode({ type: "Window", hwnd: win.hwnd, title: win.title, rect: win.rect });
                      }}
                    >
                      <option value="" disabled selected={!selectedWindow()}>
                        ウィンドウを選んでください
                      </option>
                      <For each={windows()}>
                        {(w) => (
                          <option value={String(w.hwnd)} selected={selectedWindow()?.hwnd === w.hwnd}>
                            {w.title}
                          </option>
                        )}
                      </For>
                    </select>
                  </Show>
                </div>

                <span class="text-label-sm text-muted">音声</span>
                <div class="flex flex-wrap items-center gap-5">
                  <label class="flex cursor-pointer items-center gap-2 text-body-sm font-semibold">
                    <ToggleSwitch
                      label="PCの音を録音"
                      checked={settings()?.recording.capture_system_audio ?? false}
                      onChange={(v) => setAudio("capture_system_audio", v)}
                    />
                    🔊 PCの音
                  </label>
                  <label class="flex cursor-pointer items-center gap-2 text-body-sm font-semibold">
                    <ToggleSwitch
                      label="マイクを録音"
                      checked={settings()?.recording.capture_microphone ?? false}
                      onChange={(v) => setAudio("capture_microphone", v)}
                    />
                    🎙️ マイク
                  </label>
                </div>
              </div>

              <div class="flex flex-wrap items-center gap-3 border-t border-outline pt-4">
                <button class="btn btn-primary btn-large" onClick={props.onStartRecording} disabled={!canStart()}>
                  ⏺ 録画を開始
                </button>
                <div class="text-caption text-on-surface-dim">
                  <Show
                    when={mode().type === "Window" && !selectedWindow()}
                    fallback={<div>録画中は、この画面が画面上部の小さなバーになります（録画には映りません）。</div>}
                  >
                    <div class="font-semibold text-on-surface">録画するウィンドウを選んでください。</div>
                  </Show>
                  <div>Ctrl+Shift+R でも開始・停止できます。</div>
                </div>
              </div>
            </div>
          </section>

          {/* これまでの録画 */}
          <section class="card">
            <div class="card-head">
              <span class="card-title">これまでの録画</span>
              <Show when={recordings()?.length}>
                <span class="card-label">{recordings()!.length}件</span>
              </Show>
            </div>
            <div class="px-4 py-4">
              <Show when={recordings()} fallback={<p class="empty-placeholder">読み込み中…</p>}>
                {(list) => (
                  <Show
                    when={list().length > 0}
                    fallback={
                      <p class="empty-placeholder">
                        まだ録画がありません。録画すると、ここに並びます。
                      </p>
                    }
                  >
                    <div class="grid grid-cols-[repeat(auto-fill,minmax(168px,1fr))] gap-3">
                      <For each={list()}>
                        {(rec) => (
                          <RecordingTile
                            recording={rec}
                            onOpen={() => props.onOpenPreview(rec.id)}
                            onDelete={() => handleDelete(rec.id)}
                          />
                        )}
                      </For>
                    </div>
                  </Show>
                )}
              </Show>
            </div>
          </section>
        </div>
      </main>

      <SettingsDialog open={props.settingsOpen} onClose={() => props.onSettingsOpenChange(false)} />
    </div>
  );
}
