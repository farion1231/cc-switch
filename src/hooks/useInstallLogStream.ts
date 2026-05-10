import { useCallback, useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/**
 * 一行流式安装/卸载日志
 */
export interface InstallLogLine {
  /** 由前端生成的全局唯一 id，用于 React key */
  id: number;
  channelId: string;
  stream: "stdout" | "stderr" | "info" | "error";
  line: string;
  ts: number;
}

export type InstallLogStatus = "idle" | "running" | "success" | "failed" | "cancelled";

interface UseInstallLogStreamReturn {
  channelId: string | null;
  status: InstallLogStatus;
  lines: InstallLogLine[];
  start: () => string;
  finish: (status: Exclude<InstallLogStatus, "idle" | "running">) => void;
  reset: () => void;
}

interface BackendLogPayload {
  channelId: string;
  stream: InstallLogLine["stream"];
  line: string;
  ts: number;
}

interface BackendDonePayload {
  channelId: string;
  success: boolean;
  exitCode: number | null;
  cancelled: boolean;
}

const EVENT_LOG = "install-log";
const EVENT_DONE = "install-log-done";

/**
 * 订阅后端流式安装/卸载日志事件，按 channel_id 过滤后维护行数组与状态。
 *
 * 调用方在触发安装/卸载前先调用 `start()` 拿到一个新的 channelId 并把它
 * 作为参数传给对应的 Tauri command；命令完成或失败后会自动收到 `install-log-done`
 * 事件并把 status 切到 success/failed/cancelled。如果是上层主动失败（如
 * invoke 抛错且后端来不及发 done），调用方可以显式 `finish('failed')`。
 */
export function useInstallLogStream(): UseInstallLogStreamReturn {
  const [channelId, setChannelId] = useState<string | null>(null);
  const [status, setStatus] = useState<InstallLogStatus>("idle");
  const [lines, setLines] = useState<InstallLogLine[]>([]);
  const channelIdRef = useRef<string | null>(null);
  const lineSeqRef = useRef(0);

  useEffect(() => {
    let logUnlisten: UnlistenFn | undefined;
    let doneUnlisten: UnlistenFn | undefined;
    let disposed = false;

    (async () => {
      const offLog = await listen<BackendLogPayload>(EVENT_LOG, (event) => {
        const p = event.payload;
        if (p.channelId !== channelIdRef.current) return;
        lineSeqRef.current += 1;
        const id = lineSeqRef.current;
        setLines((prev) => [
          ...prev,
          {
            id,
            channelId: p.channelId,
            stream: p.stream,
            line: p.line,
            ts: p.ts,
          },
        ]);
      });

      const offDone = await listen<BackendDonePayload>(EVENT_DONE, (event) => {
        const p = event.payload;
        if (p.channelId !== channelIdRef.current) return;
        if (p.cancelled) {
          setStatus("cancelled");
        } else {
          setStatus(p.success ? "success" : "failed");
        }
      });

      if (disposed) {
        offLog();
        offDone();
      } else {
        logUnlisten = offLog;
        doneUnlisten = offDone;
      }
    })();

    return () => {
      disposed = true;
      logUnlisten?.();
      doneUnlisten?.();
    };
  }, []);

  const start = useCallback((): string => {
    const id =
      typeof crypto !== "undefined" && "randomUUID" in crypto
        ? crypto.randomUUID()
        : `ch-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;
    channelIdRef.current = id;
    setChannelId(id);
    setStatus("running");
    setLines([]);
    lineSeqRef.current = 0;
    return id;
  }, []);

  const finish = useCallback(
    (next: Exclude<InstallLogStatus, "idle" | "running">) => {
      setStatus(next);
    },
    [],
  );

  const reset = useCallback(() => {
    channelIdRef.current = null;
    setChannelId(null);
    setStatus("idle");
    setLines([]);
    lineSeqRef.current = 0;
  }, []);

  return { channelId, status, lines, start, finish, reset };
}
