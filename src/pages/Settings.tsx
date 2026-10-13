import { createSignal, onMount, Show, For } from "solid-js";
import { getSettings, saveSettings, listWindows } from "../lib/commands";
import type { AppSettings, WindowInfo, RecordingMode } from "../lib/types";

interface Props {
  onClose: () => void;
}

function SettingRow(props: { label: string; desc?: string; children: any }) {
  return (
    <div class="space-y-1">
      <div class="flex items-center justify-between">
        <label class="text-sm text-slate-200">{props.label}</label>
        {props.children}
      </div>
      <Show when={props.desc}>
        <p class="text-xs text-slate-500 leading-relaxed">{props.desc}</p>
      </Show>
    </div>
  );
}

export default function Settings(props: Props) {
  const [settings, setSettings] = createSignal<AppSettings | null>(null);
  const [saved, setSaved] = createSignal(false);
  const [windows, setWindows] = createSignal<WindowInfo[]>([]);

  onMount(async () => {
    try {
      const s = await getSettings();
      setSettings(s);
    } catch (e) {
      console.error("Failed to load settings:", e);
    }
  });

  const refreshWindows = async () => {
    try {
      const wins = await listWindows();
      setWindows(wins);
    } catch (e) {
      console.error("Failed to list windows:", e);
    }
  };

  const setRecordingMode = (mode: RecordingMode) => {
    const current = settings();
    if (!current) return;
    setSettings({
      ...current,
      recording: { ...current.recording, recording_mode: mode },
    });
  };

  const updateField = <K extends keyof AppSettings>(section: K, key: string, value: unknown) => {
    const current = settings();
    if (!current) return;
    setSettings({
      ...current,
      [section]: { ...current[section], [key]: value },
    });
  };

  const handleSave = async () => {
    const s = settings();
    if (!s) return;
    try {
      await saveSettings(s);
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      console.error("Failed to save settings:", e);
    }
  };

  const numInput = (cls?: string) =>
    `bg-slate-700 rounded-lg px-3 py-1 text-sm w-24 text-right ${cls || ""}`;

  return (
    <div class="flex flex-col h-screen">
      <header class="flex items-center justify-between px-6 py-4 border-b border-slate-700/50">
        <h2 class="text-lg font-semibold text-white">設定</h2>
        <button onClick={props.onClose} class="p-2 rounded-lg hover:bg-slate-800 transition-colors text-slate-400">
          <svg class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M6 18L18 6M6 6l12 12" />
          </svg>
        </button>
      </header>

      <Show when={settings()} fallback={<div class="flex-1 flex items-center justify-center text-slate-500">読み込み中...</div>}>
        {(s) => (
          <div class="flex-1 overflow-y-auto px-6 py-4 space-y-6">

            {/* ===== 録画 ===== */}
            <section>
              <h3 class="text-sm font-semibold text-slate-400 uppercase tracking-wider mb-3">録画</h3>
              <div class="space-y-3 bg-slate-800/50 rounded-xl p-4">
                <SettingRow label="ショートカットキー" desc="録画の開始・停止に使うキーボードショートカットです">
                  <span class="text-sm text-slate-400 bg-slate-700/50 px-3 py-1 rounded-lg">{s().recording.hotkey}</span>
                </SettingRow>
                <SettingRow label="フレームレート (FPS)" desc="1秒あたりのキャプチャ枚数。高いほど滑らかですがファイルサイズが増えます">
                  <select value={s().recording.fps} onChange={(e) => updateField("recording", "fps", parseInt(e.target.value))} class="bg-slate-700 rounded-lg px-3 py-1 text-sm">
                    <option value="24">24</option>
                    <option value="30">30</option>
                    <option value="60">60</option>
                  </select>
                </SettingRow>
                <SettingRow label="システム音声" desc="PCから出力されている音声（アプリの音など）を一緒に録音します">
                  <input type="checkbox" checked={s().recording.capture_system_audio} onChange={(e) => updateField("recording", "capture_system_audio", e.target.checked)} class="rounded" />
                </SettingRow>
                <SettingRow label="マイク" desc="マイク入力を録音します。ナレーション付き動画に便利です">
                  <input type="checkbox" checked={s().recording.capture_microphone} onChange={(e) => updateField("recording", "capture_microphone", e.target.checked)} class="rounded" />
                </SettingRow>
                <SettingRow label="キー入力の詳細を記録" desc="OFF（既定）ではカテゴリのみ記録。ONにするとパスワード等も平文で録画ディレクトリに残るので注意">
                  <input
                    type="checkbox"
                    checked={s().recording.record_key_labels ?? false}
                    onChange={(e) => updateField("recording", "record_key_labels", e.target.checked)}
                    class="rounded"
                  />
                </SettingRow>
                <SettingRow label="録画モード" desc="画面全体・特定ウィンドウ・指定範囲から選べます">
                  <select
                    value={s().recording.recording_mode.type}
                    onChange={(e) => {
                      const v = e.target.value;
                      if (v === "Display") {
                        setRecordingMode({ type: "Display" });
                      } else if (v === "Window") {
                        refreshWindows();
                        setRecordingMode({ type: "Window", hwnd: 0, title: "", rect: [0, 0, 0, 0] });
                      } else if (v === "Area") {
                        setRecordingMode({ type: "Area", x: 0, y: 0, width: 1920, height: 1080 });
                      }
                    }}
                    class="bg-slate-700 rounded-lg px-3 py-1 text-sm"
                  >
                    <option value="Display">画面全体</option>
                    <option value="Window">ウィンドウ</option>
                    <option value="Area">範囲指定</option>
                  </select>
                </SettingRow>
                <Show when={s().recording.recording_mode.type === "Window"}>
                  <SettingRow label="対象ウィンドウ" desc="録画するウィンドウを選択してください">
                    <div class="flex items-center gap-2">
                      <select
                        onChange={(e) => {
                          const idx = parseInt(e.target.value);
                          const win = windows()[idx];
                          if (win) {
                            setRecordingMode({ type: "Window", hwnd: win.hwnd, title: win.title, rect: win.rect });
                          }
                        }}
                        class="bg-slate-700 rounded-lg px-3 py-1 text-sm max-w-[200px]"
                      >
                        <option value="">選択...</option>
                        <For each={windows()}>
                          {(win, i) => <option value={i()}>{win.title}</option>}
                        </For>
                      </select>
                      <button onClick={refreshWindows} class="p-1 rounded hover:bg-slate-600 text-slate-400" title="更新">
                        <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                          <path d="M1 4v6h6M23 20v-6h-6" /><path d="M20.49 9A9 9 0 0 0 5.64 5.64L1 10m22 4l-4.64 4.36A9 9 0 0 1 3.51 15" />
                        </svg>
                      </button>
                    </div>
                  </SettingRow>
                </Show>
                <Show when={s().recording.recording_mode.type === "Area"}>
                  <div class="grid grid-cols-2 gap-2">
                    <div class="flex items-center gap-2">
                      <label class="text-xs text-slate-400 w-6">X</label>
                      <input type="number" value={(s().recording.recording_mode as { x: number }).x || 0}
                        onChange={(e) => {
                          const mode = s().recording.recording_mode as { type: "Area"; x: number; y: number; width: number; height: number };
                          setRecordingMode({ ...mode, x: parseInt(e.target.value) || 0 });
                        }}
                        class="bg-slate-700 rounded-lg px-2 py-1 text-sm w-full" />
                    </div>
                    <div class="flex items-center gap-2">
                      <label class="text-xs text-slate-400 w-6">Y</label>
                      <input type="number" value={(s().recording.recording_mode as { y: number }).y || 0}
                        onChange={(e) => {
                          const mode = s().recording.recording_mode as { type: "Area"; x: number; y: number; width: number; height: number };
                          setRecordingMode({ ...mode, y: parseInt(e.target.value) || 0 });
                        }}
                        class="bg-slate-700 rounded-lg px-2 py-1 text-sm w-full" />
                    </div>
                    <div class="flex items-center gap-2">
                      <label class="text-xs text-slate-400 w-6">W</label>
                      <input type="number" value={(s().recording.recording_mode as { width: number }).width || 1920}
                        onChange={(e) => {
                          const mode = s().recording.recording_mode as { type: "Area"; x: number; y: number; width: number; height: number };
                          setRecordingMode({ ...mode, width: parseInt(e.target.value) || 1920 });
                        }}
                        class="bg-slate-700 rounded-lg px-2 py-1 text-sm w-full" />
                    </div>
                    <div class="flex items-center gap-2">
                      <label class="text-xs text-slate-400 w-6">H</label>
                      <input type="number" value={(s().recording.recording_mode as { height: number }).height || 1080}
                        onChange={(e) => {
                          const mode = s().recording.recording_mode as { type: "Area"; x: number; y: number; width: number; height: number };
                          setRecordingMode({ ...mode, height: parseInt(e.target.value) || 1080 });
                        }}
                        class="bg-slate-700 rounded-lg px-2 py-1 text-sm w-full" />
                    </div>
                  </div>
                </Show>
              </div>
            </section>

            {/* ===== エフェクト ===== */}
            <section>
              <h3 class="text-sm font-semibold text-slate-400 uppercase tracking-wider mb-3">エフェクト</h3>
              <div class="space-y-3 bg-slate-800/50 rounded-xl p-4">
                <SettingRow label="自動ズームの強さ" desc="操作に合わせて注目箇所へ自動でズームするときの倍率です。オフで自動ズームしません">
                  <select value={s().effects.zoom_strength} onChange={(e) => updateField("effects", "zoom_strength", e.target.value)} class="bg-slate-700 rounded-lg px-3 py-1 text-sm">
                    <option value="Off">オフ</option>
                    <option value="Subtle">控えめ (1.5x)</option>
                    <option value="Standard">標準 (1.8x)</option>
                    <option value="Strong">強め (2.2x)</option>
                  </select>
                </SettingRow>
                <SettingRow label="クリックエフェクト" desc="クリック位置にリング状のアニメーションを表示します">
                  <input type="checkbox" checked={s().effects.click_ring_enabled} onChange={(e) => updateField("effects", "click_ring_enabled", e.target.checked)} class="rounded" />
                </SettingRow>
                <SettingRow label="キー表示" desc="押されたキーをバッジとして画面に表示します">
                  <input type="checkbox" checked={s().effects.key_badge_enabled} onChange={(e) => updateField("effects", "key_badge_enabled", e.target.checked)} class="rounded" />
                </SettingRow>
                <SettingRow label="カーソル補間" desc="マウスカーソルの動きをなめらかに補間します">
                  <input type="checkbox" checked={s().effects.cursor_smoothing} onChange={(e) => updateField("effects", "cursor_smoothing", e.target.checked)} class="rounded" />
                </SettingRow>
                <SettingRow label="モーションブラー" desc="ズーム・パン中に動きのブレを加えて映像に臨場感を出します">
                  <input type="checkbox" checked={s().effects.motion_blur_enabled} onChange={(e) => updateField("effects", "motion_blur_enabled", e.target.checked)} class="rounded" />
                </SettingRow>
              </div>
            </section>

            {/* ===== スタイル ===== */}
            <section>
              <h3 class="text-sm font-semibold text-slate-400 uppercase tracking-wider mb-3">スタイル</h3>
              <div class="space-y-3 bg-slate-800/50 rounded-xl p-4">
                <SettingRow label="角丸 (px)" desc="動画の角の丸みをピクセル単位で指定します。0で角丸なし">
                  <input type="number" min="0" max="48" step="1" value={s().style.border_radius} onChange={(e) => updateField("style", "border_radius", parseInt(e.target.value) || 0)} class={numInput()} />
                </SettingRow>
                <SettingRow label="影" desc="動画の周囲にドロップシャドウを表示して立体感を出します">
                  <input type="checkbox" checked={s().style.shadow_enabled} onChange={(e) => updateField("style", "shadow_enabled", e.target.checked)} class="rounded" />
                </SettingRow>
              </div>
            </section>

            {/* ===== 出力 ===== */}
            <section>
              <h3 class="text-sm font-semibold text-slate-400 uppercase tracking-wider mb-3">出力</h3>
              <div class="space-y-3 bg-slate-800/50 rounded-xl p-4">
                <SettingRow label="出力形式" desc="エクスポート時のデフォルトのファイル形式です">
                  <select value={s().output.default_format} onChange={(e) => updateField("output", "default_format", e.target.value)} class="bg-slate-700 rounded-lg px-3 py-1 text-sm">
                    <option value="Mp4">MP4</option>
                    <option value="Gif">GIF</option>
                    <option value="WebM">WebM</option>
                  </select>
                </SettingRow>
                <SettingRow label="品質プリセット" desc="解像度とフレームレートの組み合わせです">
                  <select value={s().output.default_quality} onChange={(e) => updateField("output", "default_quality", e.target.value)} class="bg-slate-700 rounded-lg px-3 py-1 text-sm">
                    <option value="Social">ソーシャル (1080p/30fps)</option>
                    <option value="HighQuality">高画質 (元解像度/60fps)</option>
                    <option value="Lightweight">軽量 (720p/24fps)</option>
                  </select>
                </SettingRow>
                <div class="space-y-1">
                  <SettingRow label="保存先フォルダ" desc="エクスポートした動画ファイルの保存先ディレクトリです">
                    <span />
                  </SettingRow>
                  <input
                    type="text"
                    value={s().output.save_directory}
                    onChange={(e) => updateField("output", "save_directory", e.target.value)}
                    class="w-full bg-slate-700 rounded-lg px-3 py-1.5 text-sm text-slate-200"
                    placeholder="C:\Users\...\Videos\Snappi"
                  />
                </div>
              </div>
            </section>
          </div>
        )}
      </Show>

      <div class="px-6 py-4 border-t border-slate-700/50">
        <button onClick={handleSave} class="w-full py-2.5 px-4 rounded-xl font-medium transition-all bg-purple-600 hover:bg-purple-700 text-white">
          {saved() ? "保存しました" : "設定を保存"}
        </button>
      </div>
    </div>
  );
}
