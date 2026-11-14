import { useState, useSyncExternalStore } from "react";
import type { Provider } from "@/types";
import type { AppMode } from "@/types/proxy";

/** 正在编辑哪一家，以及从供应商页哪一格打开的（见 ProviderForm 的 `modeView`） */
export interface EditProviderRequest {
  provider: Provider;
  modeView?: AppMode;
}

export interface EditProviderStore {
  open: (provider: Provider, modeView?: AppMode) => void;
  close: () => void;
  subscribe: (listener: () => void) => () => void;
  getSnapshot: () => EditProviderRequest | null;
}

/**
 * 「正在编辑哪个供应商」放在 App 的 state 之外。
 *
 * App 很大，每次渲染都会给供应商列表新的回调，列表和每张卡片都跟着重渲染（几十张卡时
 * 一次就要几十毫秒）。打开、关闭编辑页只是换这一项，只让订阅它的编辑页宿主重渲染，
 * App 和列表不动。
 */
export function createEditProviderStore(): EditProviderStore {
  let current: EditProviderRequest | null = null;
  const listeners = new Set<() => void>();
  const emit = () => listeners.forEach((listener) => listener());
  return {
    open(provider, modeView) {
      current = { provider, modeView };
      emit();
    },
    close() {
      if (!current) return;
      current = null;
      emit();
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    getSnapshot: () => current,
  };
}

/** 组件生命周期内固定的一个 store */
export function useCreateEditProviderStore(): EditProviderStore {
  return useState(createEditProviderStore)[0];
}

export function useEditProviderRequest(
  store: EditProviderStore,
): EditProviderRequest | null {
  return useSyncExternalStore(store.subscribe, store.getSnapshot);
}
