// Polyfill ResizeObserver for jsdom/happy-dom
if (typeof globalThis.ResizeObserver === "undefined") {
  globalThis.ResizeObserver = class ResizeObserver {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof globalThis.ResizeObserver;
}

const storage = new Map<string, string>();

if (
  typeof globalThis.localStorage === "undefined" ||
  typeof globalThis.localStorage?.getItem !== "function"
) {
  Object.defineProperty(globalThis, "localStorage", {
    value: {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => {
        storage.set(key, String(value));
      },
      removeItem: (key: string) => {
        storage.delete(key);
      },
      clear: () => {
        storage.clear();
      },
      key: (index: number) => Array.from(storage.keys())[index] ?? null,
      get length() {
        return storage.size;
      },
    },
    configurable: true,
  });
}


// Mock window.matchMedia for theme provider tests
if (typeof globalThis.matchMedia === "undefined") {
  Object.defineProperty(globalThis, "matchMedia", {
    value: (query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addListener: () => {},
      removeListener: () => {},
      addEventListener: () => {},
      removeEventListener: () => {},
      dispatchEvent: () => false,
    }),
    configurable: true,
  });
}


// Mock Tauri environment for tests
// This ensures isTauri() returns true and the Tauri API layer is used
if (typeof globalThis.window !== "undefined") {
  // @ts-ignore - Tauri global
  globalThis.window.__TAURI__ = globalThis.window.__TAURI__ || {};
}
