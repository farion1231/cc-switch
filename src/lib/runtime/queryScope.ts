import { useMemo, useSyncExternalStore } from "react";
import { getRuntimeSnapshot, subscribeRuntime } from "./store";
import type { RuntimeSnapshot } from "./types";

export type RuntimeQueryScope =
  | readonly ["local", number]
  | readonly ["remote", string, number]
  | readonly ["transition", string | null, number];

/**
 * 把运行目标压缩成可序列化的 Query Key 片段。generation 即使目标 ID 相同也必须保留，
 * 因为重连后的数据库会话和旧会话不是同一份可复用缓存。
 */
export function runtimeQueryScope(
  snapshot: RuntimeSnapshot = getRuntimeSnapshot(),
): RuntimeQueryScope {
  if (snapshot.status === "local") {
    return ["local", snapshot.generation] as const;
  }
  if (snapshot.status === "online" && snapshot.activeTargetId) {
    return ["remote", snapshot.activeTargetId, snapshot.generation] as const;
  }
  return [
    "transition",
    snapshot.activeTargetId ?? null,
    snapshot.generation,
  ] as const;
}

/**
 * 查询 hook 必须直接订阅 runtime store；不能假设 Context Provider 的其他消费者会带动
 * 当前组件重渲染，否则后台目标切换后仍可能继续展示上一台主机的缓存。
 */
export function useRuntimeQueryScope(): RuntimeQueryScope {
  const snapshot = useSyncExternalStore(
    subscribeRuntime,
    getRuntimeSnapshot,
    getRuntimeSnapshot,
  );
  return useMemo(() => runtimeQueryScope(snapshot), [snapshot]);
}
