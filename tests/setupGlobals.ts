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

// Polyfill EventSource for jsdom/happy-dom.
// jsdom does not implement the SSE client, but `useProxyEventStream` constructs
// one from an effect on App mount. This stub only needs to be inert: no test
// drives SSE delivery through the real transport, it just has to let App mount.
if (typeof globalThis.EventSource === "undefined") {
  const EVENT_SOURCE_CONNECTING = 0;
  const EVENT_SOURCE_OPEN = 1;
  const EVENT_SOURCE_CLOSED = 2;

  globalThis.EventSource = class EventSource {
    static readonly CONNECTING = EVENT_SOURCE_CONNECTING;
    static readonly OPEN = EVENT_SOURCE_OPEN;
    static readonly CLOSED = EVENT_SOURCE_CLOSED;

    onopen: ((event: Event) => void) | null = null;
    onmessage: ((event: MessageEvent) => void) | null = null;
    onerror: ((event: Event) => void) | null = null;

    readonly url: string;
    readonly withCredentials = false;

    readyState: number = EVENT_SOURCE_CONNECTING;

    private listeners = new Map<string, Set<EventListenerOrEventListenerObject>>();

    constructor(url: string | URL, _init?: EventSourceInit) {
      this.url = String(url);
    }

    addEventListener(
      type: string,
      listener: EventListenerOrEventListenerObject | null,
    ) {
      if (!listener) return;
      const set = this.listeners.get(type) ?? new Set();
      set.add(listener);
      this.listeners.set(type, set);
    }

    removeEventListener(
      type: string,
      listener: EventListenerOrEventListenerObject | null,
    ) {
      if (!listener) return;
      this.listeners.get(type)?.delete(listener);
    }

    dispatchEvent(event: Event): boolean {
      const set = this.listeners.get(event.type);
      set?.forEach((listener) => {
        if (typeof listener === "function") {
          listener(event);
        } else {
          listener.handleEvent(event);
        }
      });
      return true;
    }

    close() {
      this.readyState = EVENT_SOURCE_CLOSED;
      this.listeners.clear();
    }
  } as unknown as typeof globalThis.EventSource;
}

if (!Element.prototype.hasPointerCapture) {
  Element.prototype.hasPointerCapture = () => false;
}

if (!Element.prototype.setPointerCapture) {
  Element.prototype.setPointerCapture = () => {};
}

if (!Element.prototype.releasePointerCapture) {
  Element.prototype.releasePointerCapture = () => {};
}
