import { createSignal } from "solid-js";

/** 履歴に残す最大件数 */
const MAX_HISTORY = 100;

/**
 * 元に戻す・やり直すができる値。
 *
 * - `commit(next)`: 1 回の編集として確定する
 * - ドラッグのように連続する編集は `begin()` → `preview(next)`（何度でも）→ `end()` とし、
 *   `begin()` 時点の値を 1 件だけ履歴に残す（値が変わらなければ残さない）
 */
export function createHistory<T>(initial: T) {
  const [present, setPresent] = createSignal<T>(initial);
  const [past, setPast] = createSignal<T[]>([]);
  const [future, setFuture] = createSignal<T[]>([]);
  let pending: T | null = null;

  const push = (value: T) => {
    setPast((p) => [...p, value].slice(-MAX_HISTORY));
    setFuture([]);
  };

  return {
    present,
    canUndo: () => past().length > 0,
    canRedo: () => future().length > 0,
    commit(next: T) {
      if (pending !== null) this.end();
      push(present());
      setPresent(() => next);
    },
    begin() {
      if (pending === null) pending = present();
    },
    preview(next: T) {
      setPresent(() => next);
    },
    end() {
      if (pending !== null && pending !== present()) push(pending);
      pending = null;
    },
    undo() {
      const p = past();
      if (p.length === 0) return;
      setFuture((f) => [present(), ...f]);
      setPast(p.slice(0, -1));
      setPresent(() => p[p.length - 1]);
    },
    redo() {
      const f = future();
      if (f.length === 0) return;
      setPast((p) => [...p, present()]);
      setFuture(f.slice(1));
      setPresent(() => f[0]);
    },
    /** 履歴を消して値を置き換える（読み込み時） */
    reset(value: T) {
      pending = null;
      setPast([]);
      setFuture([]);
      setPresent(() => value);
    },
  };
}

export type History<T> = ReturnType<typeof createHistory<T>>;
