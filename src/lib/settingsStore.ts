import { createRoot, createSignal } from "solid-js";
import { getSettings, saveSettings } from "./commands";
import type { AppSettings } from "./types";

/**
 * 設定の共有ストア。ホーム（録画の対象・音声）と設定ダイアログが同じ値を見る。
 * 変更はすぐに保存する（保存ボタンは無い）。
 */
const store = createRoot(() => {
  const [settings, setSettings] = createSignal<AppSettings | null>(null);
  const [saveError, setSaveError] = createSignal<string | null>(null);

  const load = async () => {
    const s = await getSettings();
    // 範囲指定は選べなくした (Issue #1)。保存済みなら画面全体に戻す
    if (s.recording.recording_mode.type === "Area") {
      const fixed = { ...s, recording: { ...s.recording, recording_mode: { type: "Display" as const } } };
      setSettings(fixed);
      await saveSettings(fixed).catch((e) => console.error("save_settings failed:", e));
      return fixed;
    }
    setSettings(s);
    return s;
  };

  let saving: Promise<void> = Promise.resolve();
  /** 設定を変えて保存する。保存は順番に行う */
  const update = (change: (s: AppSettings) => AppSettings) => {
    const current = settings();
    if (!current) return;
    const next = change(current);
    setSettings(next);
    saving = saving.then(() =>
      saveSettings(next)
        .then(() => {
          setSaveError(null);
        })
        .catch((e) => {
          setSaveError(`設定を保存できませんでした: ${e}`);
        }),
    );
  };

  return { settings, saveError, load, update };
});

export const settingsStore = store;

/** localStorage に覚える真偽値（使えない環境では既定値のまま動く） */
export function createStoredFlag(key: string, initial: boolean) {
  let stored = initial;
  try {
    const v = localStorage.getItem(key);
    if (v !== null) stored = v === "1";
  } catch {
    // 使えなければ毎回既定値
  }
  const [value, setValue] = createSignal(stored);
  const set = (next: boolean) => {
    setValue(next);
    try {
      localStorage.setItem(key, next ? "1" : "0");
    } catch {
      // 覚えられなくても動作は続ける
    }
  };
  return [value, set] as const;
}
