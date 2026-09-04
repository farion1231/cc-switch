import { useCallback, useRef } from "react";

export interface CommonConfigSyncGuard {
  /** 开始一个尚未拿到结果的异步合并/剥离操作。 */
  begin: (base: string) => void;
  /** 记录一个即将写入配置的程序性变更。 */
  schedule: (base: string, next: string) => void;
  /** 异步操作过期/失败时清空保护状态。 */
  abort: () => void;
  /**
   * 在“配置变化”同步 effect 中使用。当前值是自己刚写入的旧值/新值回显，
   * 或仍有异步操作在飞时返回 true，调用方应跳过本次内容推断。
   */
  skip: (current: string) => boolean;
}

/**
 * 追踪程序性配置写入的回显窗口，避免 setTimeout(0) 重置标记的竞态：
 * - 写入前 `schedule(base, next)`；
 * - effect 先看到 base（本次提交的旧 props）→ skip；
 * - 下一次渲染看到 next → 清除保护并 skip；
 * - 期间出现其它值（用户编辑/外部重置）→ 清除保护并放行内容推断。
 */
export function useCommonConfigSyncGuard(): CommonConfigSyncGuard {
  const activeRef = useRef(false);
  const baseRef = useRef<string | null>(null);
  const pendingRef = useRef<string | null>(null);

  const begin = useCallback((base: string) => {
    baseRef.current = base;
    pendingRef.current = null;
    activeRef.current = true;
  }, []);

  const schedule = useCallback((base: string, next: string) => {
    baseRef.current = base;
    pendingRef.current = next;
    activeRef.current = true;
  }, []);

  const abort = useCallback(() => {
    activeRef.current = false;
    baseRef.current = null;
    pendingRef.current = null;
  }, []);

  const skip = useCallback((current: string): boolean => {
    if (!activeRef.current) return false;

    const base = baseRef.current;
    const pending = pendingRef.current;

    if (pending === null) {
      // 异步操作结果未落地；base 回显继续跳过，其它值说明被外部改写
      if (current === base) return true;
      activeRef.current = false;
      baseRef.current = null;
      return false;
    }

    if (current === pending) {
      // 程序性写入已落地，本次 effect 无需再处理
      activeRef.current = false;
      baseRef.current = null;
      pendingRef.current = null;
      return true;
    }

    if (current === base) {
      // 还没渲染到新值前的旧值回显
      return true;
    }

    // 既不是旧值也不是新值：用户编辑/外部重置获胜，恢复内容推断
    activeRef.current = false;
    baseRef.current = null;
    pendingRef.current = null;
    return false;
  }, []);

  return { begin, schedule, abort, skip };
}
