import type { RuntimeSnapshot } from "./types";

type RuntimeListener = () => void;

let snapshot: RuntimeSnapshot = Object.freeze({
  status: "local",
  generation: 0,
});
const listeners = new Set<RuntimeListener>();

export function getRuntimeSnapshot(): RuntimeSnapshot {
  return snapshot;
}

export function subscribeRuntime(listener: RuntimeListener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/**
 * 切换目标的本地过渡态一律视为连接中，包括切回本机。只有桌面后端确认旧 SSH
 * 会话已关闭后才能发布 local，避免过渡窗口中的业务写入误落到本机数据库。
 */
export function createRuntimeTransition(
  previous: RuntimeSnapshot,
  targetId?: string,
): RuntimeSnapshot {
  return {
    status: "connecting",
    generation: previous.generation + 1,
    activeTargetId: targetId,
  };
}

/**
 * API 模块会在 React 组件外同步读取该快照，因此这里使用极小的外部 store，
 * 避免把每个现有 API 方法改造成必须接收 Context 的形式。
 */
export function setRuntimeSnapshot(next: RuntimeSnapshot): void {
  snapshot = Object.freeze({ ...next });
  for (const listener of listeners) listener();
}
