import { For, Show, type JSX } from "solid-js";

/** オン・オフのスイッチ */
export function ToggleSwitch(props: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <button
      type="button"
      role="switch"
      class="switch"
      aria-checked={props.checked}
      aria-label={props.label}
      onClick={() => props.onChange(!props.checked)}
    />
  );
}

/** 並べたオプションカードから1つを選ぶ */
export function OptionCards<T extends string>(props: {
  label: string;
  value: T;
  options: { value: T; label: string; desc?: string }[];
  onChange: (v: T) => void;
  columns?: number;
}) {
  return (
    <div
      role="radiogroup"
      aria-label={props.label}
      class="option-grid"
      style={{ "grid-template-columns": `repeat(${props.columns ?? props.options.length}, minmax(0, 1fr))` }}
    >
      <For each={props.options}>
        {(opt) => (
          <button
            type="button"
            role="radio"
            class="option-card"
            aria-checked={props.value === opt.value}
            onClick={() => props.onChange(opt.value)}
          >
            {opt.label}
            <Show when={opt.desc}>
              <span class="option-desc">{opt.desc}</span>
            </Show>
          </button>
        )}
      </For>
    </div>
  );
}

/** 設定の1行（左に名前と説明、右に操作） */
export function SettingRow(props: { label: string; desc?: string; children: JSX.Element }) {
  return (
    <div class="flex items-center justify-between gap-4">
      <div class="min-w-0">
        <div class="text-body-sm font-semibold">{props.label}</div>
        <Show when={props.desc}>
          <div class="text-caption text-on-surface-dim">{props.desc}</div>
        </Show>
      </div>
      {props.children}
    </div>
  );
}

/** 設定のまとまりの見出しとその中身 */
export function Field(props: { label: string; desc?: string; children: JSX.Element }) {
  return (
    <div class="flex flex-col gap-1.5">
      <div>
        <div class="text-body-sm font-semibold">{props.label}</div>
        <Show when={props.desc}>
          <div class="text-caption text-on-surface-dim">{props.desc}</div>
        </Show>
      </div>
      {props.children}
    </div>
  );
}
