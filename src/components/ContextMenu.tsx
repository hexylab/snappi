import { Show, For, onMount, onCleanup } from "solid-js";

export interface MenuItem {
  label: string;
  action: () => void;
  disabled?: boolean;
}

interface Props {
  items: MenuItem[];
  position: { x: number; y: number } | null;
  onClose: () => void;
}

export default function ContextMenu(props: Props) {
  let menuRef: HTMLDivElement | undefined;

  onMount(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (menuRef && !menuRef.contains(e.target as Node)) {
        props.onClose();
      }
    };
    // 少し遅延させて、右クリックイベント自体で閉じないようにする
    const timer = setTimeout(() => {
      window.addEventListener("mousedown", handleClickOutside);
    }, 10);
    onCleanup(() => {
      clearTimeout(timer);
      window.removeEventListener("mousedown", handleClickOutside);
    });
  });

  return (
    <Show when={props.position}>
      {(pos) => (
        <div
          ref={menuRef}
          class="fixed z-[100] bg-slate-800 border border-slate-600 rounded-lg shadow-xl py-1 min-w-[160px]"
          style={{ left: `${pos().x}px`, top: `${pos().y}px` }}
        >
          <For each={props.items}>
            {(item) => (
              <button
                class="w-full text-left px-3 py-1.5 text-sm text-slate-200 hover:bg-slate-700 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
                onClick={() => { item.action(); props.onClose(); }}
                disabled={item.disabled}
              >
                {item.label}
              </button>
            )}
          </For>
        </div>
      )}
    </Show>
  );
}
