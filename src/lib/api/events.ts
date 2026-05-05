import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { isCliWebUi } from "@/lib/platform";
import { runtimeApi } from "./runtime";
import { getEffectiveRemoteBackendConfig } from "./transport";

export interface BackendEvent<T> {
  event: string;
  id?: number | string;
  payload: T;
}

interface BackendEventEndpoint {
  url: string;
  token?: string;
}

interface RemoteEventEnvelope {
  event?: unknown;
  id?: unknown;
  payload?: unknown;
}

type RemoteEventHandler = (
  event: BackendEvent<unknown>,
) => void | Promise<void>;

interface RemoteEventConnection {
  endpoint: BackendEventEndpoint;
  controller: AbortController;
  handlersByEvent: Map<string, Set<RemoteEventHandler>>;
  handlerCount: number;
}

const WEBUI_EVENT_PATH = "/__cc_switch_webui__/events";
const WEBUI_TOKEN_KEY = "cc-switch-webui-token";
const REMOTE_EVENT_RETRY_MS = 2_000;
const remoteEventConnections = new Map<string, RemoteEventConnection>();

export async function listenBackendEvent<T>(
  event: string,
  handler: (event: BackendEvent<T>) => void | Promise<void>,
): Promise<UnlistenFn> {
  const runtime = await runtimeApi.getCached();
  if (runtime.relation.coLocated) {
    return await listen<T>(event, handler);
  }

  const endpoint = await getBackendEventEndpoint();
  if (!endpoint) {
    return () => {};
  }

  return listenRemoteBackendEvent(event, handler, endpoint);
}

const getBackendEventEndpoint =
  async (): Promise<BackendEventEndpoint | null> => {
    if (isCliWebUi()) {
      return {
        url: WEBUI_EVENT_PATH,
        token: readSessionToken(WEBUI_TOKEN_KEY),
      };
    }

    const remote = await getEffectiveRemoteBackendConfig();
    if (!remote) return null;

    return {
      url: `${remote.url}${WEBUI_EVENT_PATH}`,
      token: remote.token,
    };
  };

const readSessionToken = (key: string): string | undefined => {
  try {
    return window.sessionStorage.getItem(key)?.trim() || undefined;
  } catch {
    return undefined;
  }
};

const listenRemoteBackendEvent = <T>(
  eventName: string,
  handler: (event: BackendEvent<T>) => void | Promise<void>,
  endpoint: BackendEventEndpoint,
): UnlistenFn => {
  const connectionKey = getRemoteEventConnectionKey(endpoint);
  const connection =
    remoteEventConnections.get(connectionKey) ??
    createRemoteEventConnection(connectionKey, endpoint);
  const remoteHandler = handler as RemoteEventHandler;
  const handlers = connection.handlersByEvent.get(eventName) ?? new Set();
  handlers.add(remoteHandler);
  connection.handlersByEvent.set(eventName, handlers);
  connection.handlerCount += 1;

  let listening = true;
  return () => {
    if (!listening) return;
    listening = false;

    const eventHandlers = connection.handlersByEvent.get(eventName);
    if (eventHandlers) {
      eventHandlers.delete(remoteHandler);
      if (eventHandlers.size === 0) {
        connection.handlersByEvent.delete(eventName);
      }
    }

    connection.handlerCount -= 1;
    if (connection.handlerCount === 0) {
      connection.controller.abort();
      remoteEventConnections.delete(connectionKey);
    }
  };
};

const createRemoteEventConnection = (
  connectionKey: string,
  endpoint: BackendEventEndpoint,
): RemoteEventConnection => {
  const connection: RemoteEventConnection = {
    endpoint,
    controller: new AbortController(),
    handlersByEvent: new Map(),
    handlerCount: 0,
  };
  remoteEventConnections.set(connectionKey, connection);
  void pumpRemoteEvents(connectionKey, connection);
  return connection;
};

const getRemoteEventConnectionKey = (endpoint: BackendEventEndpoint): string =>
  `${endpoint.url}\n${endpoint.token ?? ""}`;

const pumpRemoteEvents = async (
  connectionKey: string,
  connection: RemoteEventConnection,
): Promise<void> => {
  const { signal } = connection.controller;
  while (!signal.aborted) {
    try {
      await readRemoteEventStream(connection);
    } catch (error) {
      if (signal.aborted) return;
      console.error("Backend event stream failed", error);
    }

    if (!signal.aborted) {
      await waitBeforeReconnect(signal);
    }
  }

  if (remoteEventConnections.get(connectionKey) === connection) {
    remoteEventConnections.delete(connectionKey);
  }
};

const readRemoteEventStream = async (
  connection: RemoteEventConnection,
): Promise<void> => {
  const { endpoint } = connection;
  const { signal } = connection.controller;
  const response = await fetch(endpoint.url, {
    method: "GET",
    headers: {
      accept: "text/event-stream",
      ...(endpoint.token ? { "x-cc-switch-webui-token": endpoint.token } : {}),
    },
    cache: "no-store",
    signal,
  });

  if (!response.ok) {
    throw new Error(`Backend event stream failed with HTTP ${response.status}`);
  }
  if (!response.body) {
    throw new Error("Backend event stream response has no body");
  }

  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";

  try {
    while (!signal.aborted) {
      const { done, value } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true }).replace(/\r\n/g, "\n");
      buffer = dispatchBufferedSseEvents(buffer, connection);
    }

    buffer += decoder.decode().replace(/\r\n/g, "\n");
    dispatchBufferedSseEvents(`${buffer}\n\n`, connection);
  } finally {
    reader.releaseLock();
  }
};

const dispatchBufferedSseEvents = (
  buffer: string,
  connection: RemoteEventConnection,
): string => {
  let remaining = buffer;
  let separatorIndex = remaining.indexOf("\n\n");

  while (separatorIndex >= 0) {
    const frame = remaining.slice(0, separatorIndex);
    dispatchSseFrame(frame, connection);
    remaining = remaining.slice(separatorIndex + 2);
    separatorIndex = remaining.indexOf("\n\n");
  }

  return remaining;
};

const dispatchSseFrame = (
  frame: string,
  connection: RemoteEventConnection,
): void => {
  const data = frame
    .split("\n")
    .filter((line) => line.startsWith("data:"))
    .map((line) => line.slice("data:".length).trimStart())
    .join("\n");
  if (!data) return;

  let envelope: RemoteEventEnvelope;
  try {
    envelope = JSON.parse(data) as RemoteEventEnvelope;
  } catch {
    return;
  }

  if (typeof envelope.event !== "string") return;

  const handlers = connection.handlersByEvent.get(envelope.event);
  if (!handlers || handlers.size === 0) return;

  const backendEvent: BackendEvent<unknown> = {
    event: envelope.event,
    payload: envelope.payload,
  };
  if (typeof envelope.id === "number" || typeof envelope.id === "string") {
    backendEvent.id = envelope.id;
  }

  for (const handler of [...handlers]) {
    void Promise.resolve(handler(backendEvent)).catch((error: unknown) => {
      console.error(
        `Backend event handler failed for '${envelope.event}'`,
        error,
      );
    });
  }
};

const waitBeforeReconnect = (signal: AbortSignal): Promise<void> =>
  new Promise((resolve) => {
    if (signal.aborted) {
      resolve();
      return;
    }

    const timeout = window.setTimeout(resolve, REMOTE_EVENT_RETRY_MS);
    signal.addEventListener(
      "abort",
      () => {
        window.clearTimeout(timeout);
        resolve();
      },
      { once: true },
    );
  });
