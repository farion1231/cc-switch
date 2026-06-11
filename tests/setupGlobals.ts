import "cross-fetch/polyfill";

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

if (typeof window !== "undefined") {
  const callbackRegistry = new Map<
    number,
    { callback: (payload: unknown) => void; once: boolean }
  >();
  const eventListeners = new Map<string, Map<number, number>>();
  let nextCallbackId = 1;
  let nextEventId = 1;

  const unregisterEventListener = (event: string, eventId: number) => {
    eventListeners.get(event)?.delete(eventId);
  };

  const emitTauriEvent = (event: string, payload: unknown) => {
    const listeners = eventListeners.get(event);
    listeners?.forEach((callbackId, eventId) => {
      const entry = callbackRegistry.get(callbackId);
      entry?.callback({ event, id: eventId, payload });
      if (entry?.once) {
        callbackRegistry.delete(callbackId);
        unregisterEventListener(event, eventId);
      }
    });
  };

  const invokeTauriCommand = async (
    command: string,
    payload: Record<string, unknown> = {},
  ) => {
    if (command === "plugin:event|listen") {
      const { event, handler } = payload as {
        event?: string;
        handler?: number;
      };
      if (!event || typeof handler !== "number") return 0;
      const eventId = nextEventId++;
      const listeners = eventListeners.get(event) ?? new Map<number, number>();
      listeners.set(eventId, handler);
      eventListeners.set(event, listeners);
      return eventId;
    }

    if (command === "plugin:event|unlisten") {
      const { event, eventId } = payload as {
        event?: string;
        eventId?: number;
      };
      if (event && typeof eventId === "number") {
        unregisterEventListener(event, eventId);
      }
      return true;
    }

    if (command === "plugin:event|emit" || command === "plugin:event|emit_to") {
      const { event, payload: eventPayload } = payload as {
        event?: string;
        payload?: unknown;
      };
      if (event) emitTauriEvent(event, eventPayload);
      return true;
    }

    if (command === "plugin:path|resolve_directory") {
      return "/home/mock";
    }

    if (command === "plugin:path|join" || command === "plugin:path|resolve") {
      const { paths = [] } = payload as { paths?: string[] };
      return paths.filter(Boolean).join("/").replace(/\/+/g, "/");
    }

    if (command === "plugin:path|normalize") {
      const { path = "" } = payload as { path?: string };
      return path.replace(/\/+/g, "/");
    }

    if (command === "plugin:window|is_maximized") {
      return false;
    }

    if (command.startsWith("plugin:window|")) {
      return true;
    }

    const response = await fetch(`http://tauri.local/${command}`, {
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
  };

  const tauriInternals = {
    callbacks: callbackRegistry,
    metadata: {
      currentWindow: {
        label: "main",
      },
    },
    invoke: invokeTauriCommand,
    transformCallback: (
      callback?: (payload: unknown) => void,
      once = false,
    ) => {
      const id = nextCallbackId++;
      callbackRegistry.set(id, {
        callback: callback ?? (() => {}),
        once,
      });
      return id;
    },
    unregisterCallback: (id: number) => {
      callbackRegistry.delete(id);
    },
    convertFileSrc: (filePath: string) => filePath,
  };

  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    value: tauriInternals,
    configurable: true,
  });
  Object.defineProperty(window, "__TAURI__", {
    value: {},
    configurable: true,
  });
  Object.defineProperty(window, "__TAURI_EVENT_PLUGIN_INTERNALS__", {
    value: {
      unregisterListener: unregisterEventListener,
    },
    configurable: true,
  });
  Object.defineProperty(globalThis, "__emitTauriEvent", {
    value: emitTauriEvent,
    configurable: true,
  });
}
