import { useEffect, useMemo, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { sessionsApi } from "@/lib/api/sessions";
import type {
  SessionContentHit,
  SessionIndexStatus,
  SessionMeta,
} from "@/types";

/** 输入停顿多久后才查正文（毫秒） */
const SEARCH_DEBOUNCE_MS = 250;

export const sessionContentKeys = {
  all: ["sessionContentSearch"] as const,
  search: (query: string, providerIds: string[]) =>
    ["sessionContentSearch", query, providerIds] as const,
};

/** 命中与会话对应用的键：同一会话文件只有一个命中 */
export const getContentHitKey = (
  providerId: string,
  sourcePath?: string | null,
) => `${providerId}:${sourcePath ?? ""}`;

interface UseSessionContentSearchOptions {
  /** 列表的搜索词（未 trim 也可以） */
  query: string;
  /** 「搜索消息正文」开关 */
  enabled: boolean;
  /** 当前会话列表；列表变了（首次加载、刷新）就同步一次索引 */
  sessions: SessionMeta[];
  /** 只要这些应用的命中 */
  providerIds: string[];
}

interface UseSessionContentSearchResult {
  /** 键见 getContentHitKey */
  hits: Map<string, SessionContentHit>;
  /** 索引进度；还没拿到时为 null */
  status: SessionIndexStatus | null;
  isSearching: boolean;
}

/**
 * 会话正文搜索（后端 SQLite FTS5 索引，见 session_manager/search.rs）。
 * 元数据搜索仍由 useSessionSearch 在前端完成，两者的结果由页面合并。
 */
export function useSessionContentSearch({
  query,
  enabled,
  sessions,
  providerIds,
}: UseSessionContentSearchOptions): UseSessionContentSearchResult {
  const queryClient = useQueryClient();
  const [status, setStatus] = useState<SessionIndexStatus | null>(null);
  const [debounced, setDebounced] = useState(() => query.trim());

  useEffect(() => {
    const next = query.trim();
    // 清空是即时的，输入才防抖
    if (!next) {
      setDebounced("");
      return;
    }
    const timer = window.setTimeout(
      () => setDebounced(next),
      SEARCH_DEBOUNCE_MS,
    );
    return () => window.clearTimeout(timer);
  }, [query]);

  // 进度事件：索引每前进一批就重查一次，边建边能搜到
  useEffect(() => {
    if (!enabled) return;
    let unlisten: UnlistenFn | undefined;
    let disposed = false;

    (async () => {
      const off = await listen<SessionIndexStatus>(
        "session-index-status",
        (event) => {
          setStatus(event.payload);
          void queryClient.invalidateQueries({
            queryKey: sessionContentKeys.all,
          });
        },
      );
      if (disposed) {
        off();
      } else {
        unlisten = off;
      }
    })();

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [enabled, queryClient]);

  // 会话列表加载或刷新后同步索引；后端只处理有变化的会话，已在跑时只排一次重跑
  useEffect(() => {
    if (!enabled || sessions.length === 0) return;
    let cancelled = false;
    sessionsApi
      .syncContentIndex()
      .then((next) => {
        if (!cancelled) setStatus(next);
      })
      .catch((error) => console.warn("同步会话正文索引失败", error));
    return () => {
      cancelled = true;
    };
  }, [enabled, sessions]);

  const providerKey = useMemo(() => [...providerIds].sort(), [providerIds]);

  const search = useQuery({
    queryKey: sessionContentKeys.search(debounced, providerKey),
    queryFn: () => sessionsApi.searchContent(debounced, providerKey),
    enabled: enabled && debounced !== "" && providerKey.length > 0,
    staleTime: 0,
  });

  const hits = useMemo(() => {
    const map = new Map<string, SessionContentHit>();
    if (!enabled || !debounced || debounced !== query.trim()) return map;
    search.data?.hits.forEach((hit) =>
      map.set(getContentHitKey(hit.providerId, hit.sourcePath), hit),
    );
    return map;
  }, [enabled, debounced, query, search.data]);

  return {
    hits,
    status: enabled ? (status ?? search.data?.status ?? null) : null,
    isSearching: enabled && query.trim() !== "" && search.isFetching,
  };
}
