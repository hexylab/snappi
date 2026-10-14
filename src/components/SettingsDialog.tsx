import { createEffect, Show } from "solid-js";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { showInFolder } from "../lib/commands";
import { FORMAT_OPTIONS, QUALITY_OPTIONS, STRENGTH_OPTIONS } from "../lib/format";
import { createStoredFlag, settingsStore } from "../lib/settingsStore";
import type { AppSettings } from "../lib/types";
import { Field, OptionCards, SettingRow, ToggleSwitch } from "./ui";

interface Props {
  open: boolean;
  onClose: () => void;
}

/** 角丸の範囲と刻み (px) */
const RADIUS_MAX = 48;
const RADIUS_STEP = 4;

/**
 * 設定。変更はすぐに保存する。
 * 書き出しと自動ズームは見えるところに、録画の細かい設定と映像の見た目は「詳細設定」に置く。
 */
export default function SettingsDialog(props: Props) {
  let dialog!: HTMLDialogElement;
  const { settings, update, saveError } = settingsStore;
  const [advancedOpen, setAdvancedOpen] = createStoredFlag("snappi.settings.advanced", false);

  createEffect(() => {
    if (props.open && !dialog.open) {
      if (!settings()) void settingsStore.load();
      dialog.showModal();
    } else if (!props.open && dialog.open) {
      dialog.close();
    }
  });

  const set = <K extends keyof AppSettings>(section: K, patch: Partial<AppSettings[K]>) =>
    update((s) => ({ ...s, [section]: { ...s[section], ...patch } }));

  const chooseFolder = async () => {
    const current = settings()?.output.save_directory;
    const picked = await openDialog({ directory: true, defaultPath: current, title: "書き出し先" });
    if (typeof picked === "string") set("output", { save_directory: picked });
  };

  return (
    <dialog
      ref={dialog}
      class="dialog m-auto"
      aria-labelledby="settings-title"
      onClose={props.onClose}
      onClick={(e) => e.target === dialog && props.onClose()}
    >
      <div class="dialog-head">
        <h2 id="settings-title" class="text-headline-sm">
          設定
        </h2>
        <p class="dialog-subtitle">変更はすぐに保存されます。</p>
      </div>

      <Show when={settings()} fallback={<div class="dialog-body empty-placeholder">読み込み中…</div>}>
        {(s) => (
          <div class="dialog-body">
            <Show when={saveError()}>
              <div class="error-bar">{saveError()}</div>
            </Show>

            <section class="dialog-section">
              <div class="card-title">書き出し</div>
              <Field label="形式" desc="「動画を書き出す」で使います">
                <OptionCards
                  label="形式"
                  value={s().output.default_format}
                  options={FORMAT_OPTIONS}
                  onChange={(v) => set("output", { default_format: v })}
                />
              </Field>
              <Field label="画質">
                <OptionCards
                  label="画質"
                  value={s().output.default_quality}
                  options={QUALITY_OPTIONS}
                  onChange={(v) => set("output", { default_quality: v })}
                />
              </Field>
              <Field label="書き出し先">
                <div class="flex items-center gap-2">
                  <span
                    class="min-w-0 flex-1 truncate rounded-[8px] border border-select-outline px-2.5 py-1 text-body-sm"
                    title={s().output.save_directory}
                  >
                    {s().output.save_directory}
                  </span>
                  <button class="btn btn-secondary btn-tight" onClick={chooseFolder}>
                    変更…
                  </button>
                  <button class="btn btn-secondary btn-tight" onClick={() => void showInFolder(s().output.save_directory)}>
                    開く
                  </button>
                </div>
              </Field>
            </section>

            <section class="dialog-section">
              <div class="card-title">自動ズーム</div>
              <Field
                label="ズームの強さ"
                desc="操作している場所を自動で拡大するときの倍率です。新しい録画に使われます"
              >
                <OptionCards
                  label="ズームの強さ"
                  value={s().effects.zoom_strength}
                  options={STRENGTH_OPTIONS}
                  onChange={(v) => set("effects", { zoom_strength: v })}
                />
              </Field>
              <p class="dialog-tip">
                💡 録画ごとに変えるには、プレビューで「細かく編集」をオンにします。
              </p>
            </section>

            <details
              class="disclosure flex flex-col gap-4"
              open={advancedOpen()}
              onToggle={(e) => setAdvancedOpen(e.currentTarget.open)}
            >
              <summary>詳細設定</summary>

              <section class="dialog-section mt-3">
                <div class="card-title">録画</div>
                <SettingRow label="フレームレート" desc="1秒あたりのコマ数。多いほど動きがなめらかになり、ファイルが大きくなります">
                  <select
                    class="select"
                    aria-label="フレームレート"
                    onChange={(e) => set("recording", { fps: Number(e.currentTarget.value) })}
                  >
                    {[24, 30, 60].map((fps) => (
                      <option value={fps} selected={s().recording.fps === fps}>
                        {fps} fps
                      </option>
                    ))}
                  </select>
                </SettingRow>
                <SettingRow
                  label="キー入力の内容を記録"
                  desc="オンにすると押したキーをそのまま表示できますが、パスワードなどの入力内容も録画に残ります。オフでは「文字」「数字」などの種類だけを記録します"
                >
                  <ToggleSwitch
                    label="キー入力の内容を記録"
                    checked={s().recording.record_key_labels ?? false}
                    onChange={(v) => set("recording", { record_key_labels: v })}
                  />
                </SettingRow>
              </section>

              <section class="dialog-section mt-4">
                <div class="card-title">映像の見た目</div>
                <SettingRow label="クリックを表示" desc="クリックした位置に輪を表示します">
                  <ToggleSwitch
                    label="クリックを表示"
                    checked={s().effects.click_ring_enabled}
                    onChange={(v) => set("effects", { click_ring_enabled: v })}
                  />
                </SettingRow>
                <SettingRow label="押したキーを表示" desc="押したキーを画面に表示します">
                  <ToggleSwitch
                    label="押したキーを表示"
                    checked={s().effects.key_badge_enabled}
                    onChange={(v) => set("effects", { key_badge_enabled: v })}
                  />
                </SettingRow>
                <SettingRow label="カーソルをなめらかに" desc="マウスカーソルの細かい揺れを抑えます">
                  <ToggleSwitch
                    label="カーソルをなめらかに"
                    checked={s().effects.cursor_smoothing}
                    onChange={(v) => set("effects", { cursor_smoothing: v })}
                  />
                </SettingRow>
                <SettingRow label="動きのブレ" desc="ズームや移動のときに残像を加えて、動きを自然に見せます">
                  <ToggleSwitch
                    label="動きのブレ"
                    checked={s().effects.motion_blur_enabled}
                    onChange={(v) => set("effects", { motion_blur_enabled: v })}
                  />
                </SettingRow>
                <SettingRow label="影" desc="画面の周りに影を付けます">
                  <ToggleSwitch
                    label="影"
                    checked={s().style.shadow_enabled}
                    onChange={(v) => set("style", { shadow_enabled: v })}
                  />
                </SettingRow>
                <SettingRow label="角丸" desc="画面の角の丸み">
                  <div class="stepper">
                    <button
                      aria-label="角丸を小さく"
                      disabled={s().style.border_radius <= 0}
                      onClick={() => set("style", { border_radius: Math.max(0, s().style.border_radius - RADIUS_STEP) })}
                    >
                      −
                    </button>
                    <span class="w-12 text-center text-label-sm tabular">{s().style.border_radius}px</span>
                    <button
                      aria-label="角丸を大きく"
                      disabled={s().style.border_radius >= RADIUS_MAX}
                      onClick={() =>
                        set("style", { border_radius: Math.min(RADIUS_MAX, s().style.border_radius + RADIUS_STEP) })
                      }
                    >
                      ＋
                    </button>
                  </div>
                </SettingRow>
              </section>
            </details>
          </div>
        )}
      </Show>

      <div class="dialog-actions">
        <button class="btn btn-secondary" onClick={props.onClose}>
          閉じる
        </button>
      </div>
    </dialog>
  );
}
