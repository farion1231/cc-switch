import "cross-fetch/polyfill";
import { vi } from "vitest";
import { server } from "./server";

const TAURI_ENDPOINT = "http://tauri.local";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (command: string, payload: Record<string, unknown> = {}) => {
    // 测试替身同步执行桌面网关的 generation 前置契约；这样集成测试不会因 MSW
    // 忽略字段而放过无法被真实后端接受的远端请求。
    if (
      command === "remote_invoke" &&
      (typeof payload.generation !== "number" || payload.generation < 0)
    ) {
      throw new Error("remote_invoke requires a non-negative generation");
    }

    const response = await fetch(`${TAURI_ENDPOINT}/${command}`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(payload ?? {}),
    });

    if (!response.ok) {
      const text = await response.text();
      throw new Error(text || `Invoke failed for ${command}`);
    }

    const text = await response.text();
    if (!text) return undefined;
    try {
      return JSON.parse(text);
    } catch {
      return text;
    }
  },
}));

const listeners = new Map<string, Set<(event: { payload: unknown }) => void>>();

const ensureListenerSet = (event: string) => {
  if (!listeners.has(event)) {
    listeners.set(event, new Set());
  }
  return listeners.get(event)!;
};

export const emitTauriEvent = (event: string, payload: unknown) => {
  const handlers = listeners.get(event);
  handlers?.forEach((handler) => handler({ payload }));
};

vi.mock("@tauri-apps/api/event", () => ({
  listen: async (
    event: string,
    handler: (event: { payload: unknown }) => void,
  ) => {
    const set = ensureListenerSet(event);
    set.add(handler);
    return () => {
      set.delete(handler);
    };
  },
}));

// jsdom 没有 Tauri 窗口对象；提供完整的无副作用替身，避免 App 生命周期测试产生误导性错误日志。
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    isMaximized: async () => false,
    onResized: async () => () => undefined,
    setDecorations: async () => undefined,
    minimize: async () => undefined,
    toggleMaximize: async () => undefined,
    close: async () => undefined,
  }),
}));

// Ensure the MSW server is referenced so tree shaking doesn't remove imports
void server;

vi.mock("@tauri-apps/api/path", () => ({
  homeDir: async () => "/home/mock",
  join: async (...segments: string[]) => segments.join("/"),
}));
