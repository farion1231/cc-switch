import { useEffect, useMemo, useRef, useState } from "react";
import type { SessionMeta, SessionSearchHit } from "@/types";
import { sessionsApi } from "@/lib/api/sessions";

interface UseDeepSearchOptions {
  /** 搜索关键词（已经 debounce 过的值） */
  query: string;
  /** 所有 session 元数据，传给后端做全量内容搜索 */
  sessions: SessionMeta[];
  /** 供应商筛选，all 表示不过滤 */
  providerFilter: string;
  /** 是否启用深度搜索。为 false 时直接返回空结果且不发起请求 */
  enabled: boolean;
}

interface UseDeepSearchResult {
  /** 深度搜索命中的 session 列表 */
  hits: SessionSearchHit[];
  /** 是否正在搜索（请求进行中） */
  isSearching: boolean;
  /** 最近一次搜索的错误信息（若有） */
  error: string | null;
}

/**
 * 深度搜索：把已加载的 session 元数据传给 Rust 后端，
 * 后端并行扫描所有 session 文件 / SQLite，返回命中的 session + 匹配片段。
 *
 * 与 useSessionSearch（FlexSearch 元数据索引）互补：
 * - useSessionSearch 覆盖 title/summary/路径，毫秒级
 * - useDeepSearch 覆盖完整对话内容，200-500ms
 *
 * 两者结果在前端合并去重，保证既能秒出元数据命中，又能补全对话内容命中。
 */
export function useDeepSearch({
  query,
  sessions,
  providerFilter,
  enabled,
}: UseDeepSearchOptions): UseDeepSearchResult {
  const [hits, setHits] = useState<SessionSearchHit[]>([]);
  const [isSearching, setIsSearching] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // 用 ref 记录"最新一次请求的标识"，避免乱序响应覆盖新结果。
  const requestIdRef = useRef(0);

  // 按供应商筛选要传给后端的 session 列表
  const scopedSessions = useMemo(() => {
    if (providerFilter === "all") return sessions;
    return sessions.filter((s) => s.providerId === providerFilter);
  }, [sessions, providerFilter]);

  useEffect(() => {
    const needle = query.trim();

    // 空查询或未启用：清空结果，不发起请求
    if (!enabled || !needle) {
      setHits([]);
      setIsSearching(false);
      setError(null);
      return;
    }

    // Clear stale hits from the previous query/filter before starting a new
    // search so sessions that only matched the old term don't remain visible.
    setHits([]);
    const requestId = ++requestIdRef.current;
    setIsSearching(true);
    setError(null);

    let cancelled = false;
    sessionsApi
      .search(needle, scopedSessions)
      .then((result) => {
        if (cancelled || requestId !== requestIdRef.current) return;
        setHits(result);
      })
      .catch((err: unknown) => {
        if (cancelled || requestId !== requestIdRef.current) return;
        const message = err instanceof Error ? err.message : String(err);
        setError(message);
        setHits([]);
      })
      .finally(() => {
        if (cancelled || requestId !== requestIdRef.current) return;
        setIsSearching(false);
      });

    return () => {
      cancelled = true;
    };
  }, [query, scopedSessions, enabled]);

  return { hits, isSearching, error };
}
