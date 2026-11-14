import { useEffect, useMemo, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { sessionsApi } from "@/lib/api/sessions";
import { extractErrorMessage } from "@/utils/errorUtils";
import type { SessionContentHit, SessionIndexStatus } from "@/types";

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
  /** 最近一次列表读取是否成功；失败后缓存的时间戳仍会保留 */
  listIsSuccess: boolean;
  /**
   * 列表最近一次成功读取的时间（useQuery 的 dataUpdatedAt）。每次读取成功都同步一次索引：
   * 正文变了而元数据没变时（如 Grok Build 一轮进行中只追加 chat_history），列表数组的引用
   * 会被结构共享保留，不能拿它当正文变化的信号
   */
  listUpdatedAt: number;
  /** 只要这些应用的命中 */
  providerIds: string[];
}

interface UseSessionContentSearchResult {
  /** 键见 getContentHitKey */
  hits: Map<string, SessionContentHit>;
  /** 索引进度；还没拿到时为 null */
  status: SessionIndexStatus | null;
  isSearching: boolean;
  /**
   * 正文查询失败的原因。有值时「没有命中」不代表正文里没有这个词：
   * 索引打不开或查询失败都会走到这里（见 `session_manager/search.rs`）。
   */
  error: string | null;
  /** 重新查一次（失败提示里的重试） */
  retry: () => void;
}

/**
 * 会话正文搜索（后端 SQLite FTS5 索引，见 session_manager/search.rs）。
 * 元数据搜索仍由 useSessionSearch 在前端完成，两者的结果由页面合并。
 */
export function useSessionContentSearch({
  query,
  enabled,
  listIsSuccess,
  listUpdatedAt,
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

  // 会话列表每次读取成功后同步索引；后端只处理有变化的会话，已在跑时只排一次重跑
  useEffect(() => {
    // 空列表也要同步，以清理已被外部删除的会话；首次读取完成前不启动。
    if (!enabled || !listIsSuccess || listUpdatedAt === 0) return;
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
  }, [enabled, listIsSuccess, listUpdatedAt]);

  const providerKey = useMemo(() => [...providerIds].sort(), [providerIds]);

  const search = useQuery({
    queryKey: sessionContentKeys.search(debounced, providerKey),
    queryFn: () => sessionsApi.searchContent(debounced, providerKey),
    enabled: enabled && debounced !== "" && providerKey.length > 0,
    staleTime: 0,
    // 索引读不出来是确定性失败，自动重试只会让提示晚一拍；重试交给界面上的按钮
    retry: false,
  });

  const hits = useMemo(() => {
    const map = new Map<string, SessionContentHit>();
    if (!enabled || !debounced || debounced !== query.trim()) return map;
    search.data?.hits.forEach((hit) =>
      map.set(getContentHitKey(hit.providerId, hit.sourcePath), hit),
    );
    return map;
  }, [enabled, debounced, query, search.data]);

  // 只在当前查询词已经有结果时才算失败：正在防抖 / 输入还没生效不算。
  // 失败后重新查询成功时 react-query 会把 error 清掉，提示随之消失。
  // 用 isError 而不是拿 error 的真值判断：原因本身可能是空串。
  const settled = enabled && debounced !== "" && debounced === query.trim();
  const error =
    settled && search.isError
      ? // 提不出原因时也要给一段非空文本：界面按真值判断失败，
        // 空串会静默退回「没有会话包含该词」
        extractErrorMessage(search.error) || "Unknown search error"
      : null;

  return {
    hits,
    status: enabled ? (status ?? search.data?.status ?? null) : null,
    isSearching: enabled && query.trim() !== "" && search.isFetching,
    error,
    // 索引读不出来时重试只重新查询是不够的：后端会删掉坏索引重建出一个**空**索引，
    // 查询随之成功返回零命中，界面又变回「没有会话包含该词」。重新收录挂在这里是
    // 列表刷新后的同步，而列表有 staleTime，停在页面上点重试不会触发它。
    // 顺手再同步一次：后端按变化处理，已经在跑时只排一次重跑，重复调用无害；
    // 进度事件会让查询重新执行。
    retry: () => {
      void search.refetch();
      sessionsApi
        .syncContentIndex()
        .then(setStatus)
        .catch((error) => console.warn("同步会话正文索引失败", error));
    },
  };
}
