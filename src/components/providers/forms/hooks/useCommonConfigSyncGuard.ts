import { useCallback, useMemo, useRef } from "react";

export interface CommonConfigSyncGuard {
  /** 记录一个即将写入配置的程序性变更。 */
  schedule: (base: string, next: string) => void;
  /**
   * 在“配置变化”同步 effect 中使用。当前值是自己刚写入的旧值/新值回显时
   * 返回 true，调用方应跳过本次内容推断。
   */
  skip: (current: string) => boolean;
}

/**
 * 追踪程序性配置写入的回显窗口，避免 setTimeout(0) 重置标记的竞态：
 * - 写入前 `schedule(base, next)`；
 * - effect 先看到 base（本次提交的旧 props）→ skip；
 * - 下一次渲染看到 next → 清除保护并 skip；
 * - 期间出现其它值（用户编辑/外部重置）→ 清除保护并放行内容推断。
 *
 * 返回值是稳定引用，语义上等同 `useRef` 的容器，调用方不必把它列进依赖数组。
 */
export function useCommonConfigSyncGuard(): CommonConfigSyncGuard {
  const baseRef = useRef<string | null>(null);
  const pendingRef = useRef<string | null>(null);

  const clear = useCallback(() => {
    baseRef.current = null;
    pendingRef.current = null;
  }, []);

  const schedule = useCallback((base: string, next: string) => {
    baseRef.current = base;
    pendingRef.current = next;
  }, []);

  const skip = useCallback(
    (current: string): boolean => {
      if (pendingRef.current === null) return false;

      if (current === pendingRef.current) {
        // 程序性写入已落地，本次 effect 无需再处理
        clear();
        return true;
      }
      if (current === baseRef.current) {
        // 还没渲染到新值前的旧值回显
        return true;
      }

      // 既不是旧值也不是新值：用户编辑/外部重置获胜，恢复内容推断
      clear();
      return false;
    },
    [clear],
  );

  return useMemo(() => ({ schedule, skip }), [schedule, skip]);
}
