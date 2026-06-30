// Web-mode logger.
//
// In the desktop (Tauri) build, logging is handled natively on the Rust side.
// In web mode the UI runs in a plain browser, so uncaught errors, rejected
// promises and `console.error`/`console.warn` output would otherwise be lost
// (no one is watching the remote user's devtools console). This module:
//
//   * installs global handlers for `error` / `unhandledrejection`
//   * mirrors `console.error` / `console.warn`
//   * exposes an explicit `webLog` API for lifecycle/debug logging
//
// All captured entries are buffered and shipped in batches to the backend
// `POST /api/v1/logs` endpoint, which re-emits them through the server
// `tracing` pipeline (visible in `RUST_LOG`).

import { isTauri } from "@/lib/environment";

export type WebLogLevel = "debug" | "info" | "warn" | "error";

interface WebLogEntry {
  level: WebLogLevel;
  message: string;
  source: string;
  url: string;
  ts: number;
  stack?: string;
  context?: Record<string, unknown>;
}

const API_BASE_URL = import.meta.env.VITE_API_BASE_URL || "/api/v1";
const LOG_ENDPOINT = `${API_BASE_URL}/logs`;

const MAX_BUFFER = 50;
const FLUSH_INTERVAL_MS = 4000;
const MAX_MESSAGE_LEN = 4000;

// Original console methods, captured before patching so the logger can both
// print to the console and avoid recursing through its own patched methods.
const originalConsole = {
  log: console.log.bind(console),
  info: console.info.bind(console),
  warn: console.warn.bind(console),
  error: console.error.bind(console),
  debug: console.debug.bind(console),
};

let installed = false;
let buffer: WebLogEntry[] = [];
let flushTimer: ReturnType<typeof setInterval> | null = null;
// Guards against infinite recursion: while we are shipping logs (which may call
// fetch and, on failure, console.error), don't re-capture our own output.
let shipping = false;

function truncate(value: string): string {
  if (value.length <= MAX_MESSAGE_LEN) return value;
  return `${value.slice(0, MAX_MESSAGE_LEN)}…`;
}

function stringifyArg(arg: unknown): string {
  if (typeof arg === "string") return arg;
  if (arg instanceof Error) {
    return arg.stack
      ? `${arg.name}: ${arg.message}\n${arg.stack}`
      : `${arg.name}: ${arg.message}`;
  }
  try {
    return JSON.stringify(arg);
  } catch {
    return String(arg);
  }
}

function formatArgs(args: unknown[]): string {
  return truncate(args.map(stringifyArg).join(" "));
}

function enqueue(entry: WebLogEntry): void {
  buffer.push(entry);
  if (buffer.length >= MAX_BUFFER) {
    void flush();
  }
}

async function flush(useBeacon = false): Promise<void> {
  if (buffer.length === 0) return;

  const batch = buffer;
  buffer = [];
  const payload = JSON.stringify({ entries: batch });

  if (
    useBeacon &&
    typeof navigator !== "undefined" &&
    typeof navigator.sendBeacon === "function"
  ) {
    try {
      const blob = new Blob([payload], { type: "application/json" });
      navigator.sendBeacon(LOG_ENDPOINT, blob);
    } catch {
      // best-effort on unload, drop on failure
    }
    return;
  }

  shipping = true;
  try {
    const headers: Record<string, string> = { "Content-Type": "application/json" };
    const token = localStorage.getItem("cc_switch_token");
    if (token) {
      headers["Authorization"] = `Bearer ${token}`;
    }
    await fetch(LOG_ENDPOINT, {
      method: "POST",
      headers,
      body: payload,
      keepalive: true,
    });
  } catch {
    // Network failure shipping logs: re-buffer a bounded tail to retry later,
    // but never let the buffer grow without bound.
    buffer = batch.slice(-MAX_BUFFER).concat(buffer).slice(-MAX_BUFFER);
  } finally {
    shipping = false;
  }
}

function record(
  level: WebLogLevel,
  source: string,
  message: string,
  context?: Record<string, unknown>,
  stack?: string,
): void {
  enqueue({
    level,
    source,
    message: truncate(message),
    context,
    stack: stack ? truncate(stack) : undefined,
    url: typeof window !== "undefined" ? window.location.href : "",
    ts: Date.now(),
  });
}

/**
 * Explicit logging API for web UI lifecycle / debug instrumentation.
 * No-op (console-only) when running inside the Tauri desktop app.
 */
export const webLog = {
  debug(message: string, context?: Record<string, unknown>): void {
    originalConsole.debug(`[web] ${message}`, context ?? "");
    if (installed) record("debug", "webLog", message, context);
  },
  info(message: string, context?: Record<string, unknown>): void {
    originalConsole.info(`[web] ${message}`, context ?? "");
    if (installed) record("info", "webLog", message, context);
  },
  warn(message: string, context?: Record<string, unknown>): void {
    originalConsole.warn(`[web] ${message}`, context ?? "");
    if (installed) record("warn", "webLog", message, context);
  },
  error(message: string, context?: Record<string, unknown>): void {
    originalConsole.error(`[web] ${message}`, context ?? "");
    if (installed) record("error", "webLog", message, context);
  },
};

/**
 * Install global error capture and console mirroring. Safe to call multiple
 * times; only the first call in web mode takes effect.
 */
export function installWebLogger(): void {
  if (installed || isTauri() || typeof window === "undefined") return;
  installed = true;

  // Mirror console.error / console.warn to the backend.
  console.error = (...args: unknown[]) => {
    originalConsole.error(...args);
    if (!shipping) record("error", "console.error", formatArgs(args));
  };
  console.warn = (...args: unknown[]) => {
    originalConsole.warn(...args);
    if (!shipping) record("warn", "console.warn", formatArgs(args));
  };

  // Uncaught runtime errors.
  window.addEventListener("error", (event: ErrorEvent) => {
    const error = event.error as Error | undefined;
    record(
      "error",
      "window.onerror",
      event.message ||
        (error ? `${error.name}: ${error.message}` : "Unknown error"),
      {
        filename: event.filename,
        lineno: event.lineno,
        colno: event.colno,
      },
      error?.stack,
    );
  });

  // Unhandled promise rejections.
  window.addEventListener(
    "unhandledrejection",
    (event: PromiseRejectionEvent) => {
      const reason = event.reason;
      const message =
        reason instanceof Error
          ? `${reason.name}: ${reason.message}`
          : stringifyArg(reason);
      record(
        "error",
        "unhandledrejection",
        message,
        undefined,
        reason instanceof Error ? reason.stack : undefined,
      );
    },
  );

  // Flush on tab hide / unload so logs aren't lost when the user leaves.
  const flushOnExit = () => void flush(true);
  window.addEventListener("beforeunload", flushOnExit);
  window.addEventListener("pagehide", flushOnExit);
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "hidden") flushOnExit();
  });

  flushTimer = setInterval(() => void flush(), FLUSH_INTERVAL_MS);

  webLog.info("web logger installed", {
    userAgent: navigator.userAgent,
    url: window.location.href,
  });
}

/** Stop the logger (primarily for tests). */
export function uninstallWebLogger(): void {
  if (flushTimer) {
    clearInterval(flushTimer);
    flushTimer = null;
  }
  console.error = originalConsole.error;
  console.warn = originalConsole.warn;
  buffer = [];
  installed = false;
}
