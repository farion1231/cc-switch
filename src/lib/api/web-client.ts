const API_BASE_URL = import.meta.env.VITE_API_BASE_URL || "/api/v1";

import { webLog } from "@/lib/webLogger";

let authToken: string | null = localStorage.getItem("cc_switch_token");

export function setAuthToken(token: string) {
  authToken = token;
  localStorage.setItem("cc_switch_token", token);
}

export function getAuthToken(): string | null {
  return authToken;
}

export function clearAuthToken() {
  authToken = null;
  localStorage.removeItem("cc_switch_token");
}

async function fetchWithAuth(
  url: string,
  options: RequestInit = {},
): Promise<Response> {
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...(options.headers as Record<string, string>),
  };

  if (authToken) {
    headers["Authorization"] = `Bearer ${authToken}`;
  }

  const method = options.method ?? "GET";
  const startedAt = Date.now();

  // Avoid feeding the log-ingest endpoint's own traffic back through the logger.
  const isLogEndpoint = url.startsWith("/logs");
  if (!isLogEndpoint) {
    webLog.debug(`api request ${method} ${url}`);
  }

  let response: Response;
  try {
    response = await fetch(`${API_BASE_URL}${url}`, {
      ...options,
      headers,
    });
  } catch (error) {
    if (!isLogEndpoint) {
      webLog.error(`api network error ${method} ${url}`, {
        error: error instanceof Error ? error.message : String(error),
        durationMs: Date.now() - startedAt,
      });
    }
    throw error;
  }

  if (!isLogEndpoint && !response.ok) {
    webLog.warn(`api response ${response.status} ${method} ${url}`, {
      status: response.status,
      statusText: response.statusText,
      durationMs: Date.now() - startedAt,
    });
  }

  if (response.status === 401) {
    clearAuthToken();
    window.dispatchEvent(new CustomEvent("auth:expired"));
    throw new Error("Unauthorized");
  }

  return response;
}

interface ApiEnvelope<T> {
  success: boolean;
  data: T;
  error?: string | null;
}

async function parseApiEnvelope<T>(
  response: Response,
  method = "GET",
  url = "",
): Promise<ApiEnvelope<T>> {
  const responseText = await response.text();
  const statusLabel = `${response.status}${response.statusText ? ` ${response.statusText}` : ""}`;
  const isLogEndpoint = url.startsWith("/logs");

  if (!responseText) {
    if (!isLogEndpoint) {
      webLog.warn(`api empty body ${method} ${url}`, {
        status: response.status,
      });
    }
    throw new Error(`HTTP ${statusLabel}`);
  }

  let payload: ApiEnvelope<T>;
  try {
    payload = JSON.parse(responseText) as ApiEnvelope<T>;
  } catch {
    if (!isLogEndpoint) {
      webLog.warn(`api invalid json ${method} ${url}`, {
        status: response.status,
        bodyPreview: responseText.slice(0, 200),
      });
    }
    throw new Error(`HTTP ${statusLabel}`);
  }

  if (!payload.success) {
    if (!isLogEndpoint) {
      webLog.warn(`api error ${method} ${url}`, {
        status: response.status,
        error: payload.error ?? null,
      });
    }
    throw new Error(payload.error || `HTTP ${statusLabel}`);
  }

  return payload;
}

export async function get<T>(url: string): Promise<T> {
  const response = await fetchWithAuth(url, { method: "GET" });
  const data = await parseApiEnvelope<T>(response, "GET", url);
  return data.data;
}

export async function post<T>(url: string, body?: unknown): Promise<T> {
  const response = await fetchWithAuth(url, {
    method: "POST",
    body: body ? JSON.stringify(body) : undefined,
  });
  const data = await parseApiEnvelope<T>(response, "POST", url);
  return data.data;
}

export async function put<T>(url: string, body?: unknown): Promise<T> {
  const response = await fetchWithAuth(url, {
    method: "PUT",
    body: body ? JSON.stringify(body) : undefined,
  });
  const data = await parseApiEnvelope<T>(response, "PUT", url);
  return data.data;
}

export async function del<T>(url: string): Promise<T> {
  const response = await fetchWithAuth(url, {
    method: "DELETE",
  });
  const data = await parseApiEnvelope<T>(response, "DELETE", url);
  return data.data;
}

export function connectWebSocket(
  onMessage: (data: unknown) => void,
): () => void {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  const token = getAuthToken() || "";
  const wsUrl = `${protocol}//${window.location.host}/ws?token=${encodeURIComponent(token)}`;
  const ws = new WebSocket(wsUrl);

  ws.onopen = () => {
    webLog.info("websocket connected", { url: wsUrl });
  };

  ws.onmessage = (event) => {
    try {
      const data = JSON.parse(event.data);
      onMessage(data);
    } catch (e) {
      webLog.error("websocket message parse failed", {
        error: e instanceof Error ? e.message : String(e),
      });
    }
  };

  ws.onclose = () => {
    webLog.info("websocket disconnected");
  };

  ws.onerror = (error) => {
    webLog.error("websocket error", { detail: String(error) });
  };

  return () => {
    ws.close();
  };
}

export function connectTerminalWebSocket(
  providerId: string,
  app: string,
  onData: (data: Uint8Array) => void,
  onReady: () => void,
  onError: (error: string) => void,
  onClose: () => void,
): {
  send: (data: Uint8Array) => void;
  resize: (cols: number, rows: number) => void;
  close: () => void;
} {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  const token = getAuthToken();
  const wsUrl = `${protocol}//${window.location.host}/ws/terminal?provider=${encodeURIComponent(providerId)}&app=${encodeURIComponent(app)}&token=${encodeURIComponent(token || "")}`;
  const ws = new WebSocket(wsUrl);

  ws.binaryType = "arraybuffer";

  let isReady = false;

  ws.onopen = () => {
    webLog.info("terminal websocket connected", { provider: providerId, app });
    // Send auth token as first message if needed
    if (authToken) {
      // Note: auth is handled via headers in query params for WebSocket upgrade
    }
  };

  ws.onmessage = (event) => {
    if (typeof event.data === "string") {
      try {
        const data = JSON.parse(event.data);
        if (data.status === "ready") {
          isReady = true;
          onReady();
        } else if (data.error) {
          onError(data.error);
        }
      } catch (e) {
        webLog.error("terminal websocket message parse failed", {
          error: e instanceof Error ? e.message : String(e),
        });
      }
    } else if (event.data instanceof ArrayBuffer) {
      const bytes = new Uint8Array(event.data);
      if (bytes.length > 0 && bytes[0] === 0x00) {
        // Binary protocol: 0x00 prefix for stdout/stderr data
        onData(bytes.slice(1));
      }
    }
  };

  ws.onclose = () => {
    webLog.info("terminal websocket disconnected", {
      provider: providerId,
      app,
    });
    onClose();
  };

  ws.onerror = (error) => {
    webLog.error("terminal websocket error", {
      provider: providerId,
      app,
      detail: String(error),
    });
    onError("Connection error");
  };

  return {
    send: (data: Uint8Array) => {
      if (ws.readyState === WebSocket.OPEN && isReady) {
        // Binary protocol: 0x00 prefix for stdin data
        const message = new Uint8Array(data.length + 1);
        message[0] = 0x00;
        message.set(data, 1);
        ws.send(message);
      }
    },
    resize: (cols: number, rows: number) => {
      if (ws.readyState === WebSocket.OPEN && isReady) {
        // Binary protocol: 0x01 prefix for resize event
        const resizeData = JSON.stringify({ cols, rows });
        const encoder = new TextEncoder();
        const jsonBytes = encoder.encode(resizeData);
        const message = new Uint8Array(jsonBytes.length + 1);
        message[0] = 0x01;
        message.set(jsonBytes, 1);
        ws.send(message);
      }
    },
    close: () => {
      ws.close();
    },
  };
}
