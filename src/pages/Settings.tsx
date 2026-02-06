import { createSignal, onMount, Show } from "solid-js";
import { getSettings, saveSettings } from "../lib/commands";
import type { AppSettings } from "../lib/types";

interface Props {
  onClose: () => void;
}

export default function Settings(props: Props) {
  const [settings, setSettings] = createSignal<AppSettings | null>(null);
  const [saved, setSaved] = createSignal(false);

  onMount(async () => {
    try {
      const s = await getSettings();
      setSettings(s);
    } catch (e) {
      console.error("Failed to load settings:", e);
    }
  });

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

  return (
    <div class="flex flex-col h-screen">
      <header class="flex items-center justify-between px-6 py-4 border-b border-slate-700/50">
        <h2 class="text-lg font-semibold text-white">Settings</h2>
        <button onClick={props.onClose} class="p-2 rounded-lg hover:bg-slate-800 transition-colors text-slate-400">
          <svg class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M6 18L18 6M6 6l12 12" />
          </svg>
        </button>
      </header>

      <Show when={settings()} fallback={<div class="flex-1 flex items-center justify-center text-slate-500">Loading...</div>}>
        {(s) => (
          <div class="flex-1 overflow-y-auto px-6 py-4 space-y-6">
            <section>
              <h3 class="text-sm font-semibold text-slate-400 uppercase tracking-wider mb-3">Recording</h3>
              <div class="space-y-3 bg-slate-800/50 rounded-xl p-4">
                <div class="flex items-center justify-between">
                  <label class="text-sm">Shortcut Key</label>
                  <span class="text-sm text-slate-400 bg-slate-700/50 px-3 py-1 rounded-lg">{s().recording.hotkey}</span>
                </div>
                <div class="flex items-center justify-between">
                  <label class="text-sm">FPS</label>
                  <select value={s().recording.fps} onChange={(e) => updateField("recording", "fps", parseInt(e.target.value))} class="bg-slate-700 rounded-lg px-3 py-1 text-sm">
                    <option value="24">24</option>
                    <option value="30">30</option>
                    <option value="60">60</option>
                  </select>
                </div>
                <div class="flex items-center justify-between">
                  <label class="text-sm">System Audio</label>
                  <input type="checkbox" checked={s().recording.capture_system_audio} onChange={(e) => updateField("recording", "capture_system_audio", e.target.checked)} class="rounded" />
                </div>
                <div class="flex items-center justify-between">
                  <label class="text-sm">Microphone</label>
                  <input type="checkbox" checked={s().recording.capture_microphone} onChange={(e) => updateField("recording", "capture_microphone", e.target.checked)} class="rounded" />
                </div>
              </div>
            </section>

            <section>
              <h3 class="text-sm font-semibold text-slate-400 uppercase tracking-wider mb-3">Effects</h3>
              <div class="space-y-3 bg-slate-800/50 rounded-xl p-4">
                <div class="flex items-center justify-between">
                  <label class="text-sm">Auto Zoom</label>
                  <input type="checkbox" checked={s().effects.auto_zoom_enabled} onChange={(e) => updateField("effects", "auto_zoom_enabled", e.target.checked)} class="rounded" />
                </div>
                <div class="flex items-center justify-between">
                  <label class="text-sm">Zoom Level</label>
                  <select value={s().effects.default_zoom_level} onChange={(e) => updateField("effects", "default_zoom_level", parseFloat(e.target.value))} class="bg-slate-700 rounded-lg px-3 py-1 text-sm">
                    <option value="1.5">1.5x</option>
                    <option value="2.0">2.0x</option>
                    <option value="2.5">2.5x</option>
                    <option value="3.0">3.0x</option>
                  </select>
                </div>
                <div class="flex items-center justify-between">
                  <label class="text-sm">Click Ring</label>
                  <input type="checkbox" checked={s().effects.click_ring_enabled} onChange={(e) => updateField("effects", "click_ring_enabled", e.target.checked)} class="rounded" />
                </div>
                <div class="flex items-center justify-between">
                  <label class="text-sm">Key Display</label>
                  <input type="checkbox" checked={s().effects.key_badge_enabled} onChange={(e) => updateField("effects", "key_badge_enabled", e.target.checked)} class="rounded" />
                </div>
                <div class="flex items-center justify-between">
                  <label class="text-sm">Cursor Smoothing</label>
                  <input type="checkbox" checked={s().effects.cursor_smoothing} onChange={(e) => updateField("effects", "cursor_smoothing", e.target.checked)} class="rounded" />
                </div>
              </div>
            </section>

            <section>
              <h3 class="text-sm font-semibold text-slate-400 uppercase tracking-wider mb-3">Style</h3>
              <div class="space-y-3 bg-slate-800/50 rounded-xl p-4">
                <div class="flex items-center justify-between">
                  <label class="text-sm">Border Radius</label>
                  <select value={s().style.border_radius} onChange={(e) => updateField("style", "border_radius", parseInt(e.target.value))} class="bg-slate-700 rounded-lg px-3 py-1 text-sm">
                    <option value="0">0px</option>
                    <option value="8">8px</option>
                    <option value="12">12px</option>
                    <option value="16">16px</option>
                    <option value="24">24px</option>
                  </select>
                </div>
                <div class="flex items-center justify-between">
                  <label class="text-sm">Shadow</label>
                  <input type="checkbox" checked={s().style.shadow_enabled} onChange={(e) => updateField("style", "shadow_enabled", e.target.checked)} class="rounded" />
                </div>
              </div>
            </section>

            <section>
              <h3 class="text-sm font-semibold text-slate-400 uppercase tracking-wider mb-3">Output</h3>
              <div class="space-y-3 bg-slate-800/50 rounded-xl p-4">
                <div class="flex items-center justify-between">
                  <label class="text-sm">Default Format</label>
                  <select value={s().output.default_format} onChange={(e) => updateField("output", "default_format", e.target.value)} class="bg-slate-700 rounded-lg px-3 py-1 text-sm">
                    <option value="Mp4">MP4</option>
                    <option value="Gif">GIF</option>
                    <option value="WebM">WebM</option>
                  </select>
                </div>
                <div class="flex items-center justify-between">
                  <label class="text-sm">Default Quality</label>
                  <select value={s().output.default_quality} onChange={(e) => updateField("output", "default_quality", e.target.value)} class="bg-slate-700 rounded-lg px-3 py-1 text-sm">
                    <option value="Social">Social (1080p/30fps)</option>
                    <option value="HighQuality">High Quality (Original/60fps)</option>
                    <option value="Lightweight">Lightweight (720p/24fps)</option>
                  </select>
                </div>
              </div>
            </section>
          </div>
        )}
      </Show>

      <div class="px-6 py-4 border-t border-slate-700/50">
        <button onClick={handleSave} class="w-full py-2.5 px-4 rounded-xl font-medium transition-all bg-purple-600 hover:bg-purple-700 text-white">
          {saved() ? "Saved!" : "Save Settings"}
        </button>
      </div>
    </div>
  );
}
