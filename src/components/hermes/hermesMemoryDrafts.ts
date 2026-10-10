import { useSyncExternalStore } from "react";
import type { HermesMemoryKind } from "@/types";

/**
 * Hermes 记忆页的草稿。保存按钮在页头（App 里渲染），编辑区在面板里，两边不在同一棵
 * 子树下，所以草稿放在这个小 store 里共享：
 * - draft：编辑区里的内容；saved：最近一次读到或保存成功的内容。两者不同 = 有未保存修改。
 * - 两份记忆各自一份草稿，切页签不丢；离开记忆页（面板卸载）时清空，回来重新读文件。
 */
export interface HermesMemoryDraftState {
  active: HermesMemoryKind;
  draft: Partial<Record<HermesMemoryKind, string>>;
  saved: Partial<Record<HermesMemoryKind, string>>;
}

const initialState = (): HermesMemoryDraftState => ({
  active: "memory",
  draft: {},
  saved: {},
});

let state = initialState();
const listeners = new Set<() => void>();

function update(next: Partial<HermesMemoryDraftState>) {
  state = { ...state, ...next };
  listeners.forEach((listener) => listener());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

const getState = () => state;

export const hermesMemoryDrafts = {
  getState,
  subscribe,
  setActive(kind: HermesMemoryKind) {
    if (state.active !== kind) update({ active: kind });
  },
  /** 第一次读到文件内容时填进草稿；之后的重新读取不覆盖正在编辑的内容。 */
  hydrate(kind: HermesMemoryKind, content: string) {
    if (state.saved[kind] !== undefined) return;
    update({
      draft: { ...state.draft, [kind]: content },
      saved: { ...state.saved, [kind]: content },
    });
  },
  edit(kind: HermesMemoryKind, content: string) {
    update({ draft: { ...state.draft, [kind]: content } });
  },
  /** 保存成功：以保存出去的内容为准（保存途中又改过的话，仍算有未保存修改）。 */
  markSaved(kind: HermesMemoryKind, content: string) {
    update({ saved: { ...state.saved, [kind]: content } });
  },
  reset() {
    update(initialState());
  },
};

export function isHermesMemoryDirty(
  current: HermesMemoryDraftState,
  kind: HermesMemoryKind,
) {
  const draft = current.draft[kind];
  return draft !== undefined && draft !== current.saved[kind];
}

export function useHermesMemoryDrafts() {
  return useSyncExternalStore(subscribe, getState, getState);
}
