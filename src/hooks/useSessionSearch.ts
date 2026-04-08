import { useCallback, useMemo } from "react";
import FlexSearch from "flexsearch";
import type { SessionMeta } from "@/types";

interface UseSessionSearchOptions {
  sessions: SessionMeta[];
  providerFilter: string;
}

interface UseSessionSearchResult {
  search: (query: string) => SessionMeta[];
}

/**
 * 使用 FlexSearch 实现会话全文搜索
 * 索引会话元数据（标题、摘要、项目目录等）
 */
export function useSessionSearch({
  sessions,
  providerFilter,
}: UseSessionSearchOptions): UseSessionSearchResult {
  const index = useMemo(() => {
    // 使用 forward tokenizer 支持中文前缀搜索
    const nextIndex = new FlexSearch.Index({
      tokenize: "forward",
      resolution: 9,
    });

    sessions.forEach((session, idx) => {
      const metaContent = [
        session.sessionId,
        session.title,
        session.summary,
        session.projectDir,
        session.sourcePath,
      ]
        .filter(Boolean)
        .join(" ");

      nextIndex.add(idx, metaContent);
    });

    return nextIndex;
  }, [sessions]);

  // 搜索函数
  const search = useCallback(
    (query: string): SessionMeta[] => {
      const needle = query.trim().toLowerCase();

      // 先按 provider 过滤
      let filtered = sessions;
      if (providerFilter !== "all") {
        filtered = sessions.filter((s) => s.providerId === providerFilter);
      }

      // 如果没有搜索词，返回按时间排序的结果
      if (!needle) {
        return [...filtered].sort((a, b) => {
          const aTs = a.lastActiveAt ?? a.createdAt ?? 0;
          const bTs = b.lastActiveAt ?? b.createdAt ?? 0;
          return bTs - aTs;
        });
      }

      // 使用 FlexSearch 搜索
      const results = index.search(needle, { limit: 100 }) as number[];

      // 转换为 session 并过滤
      const matchedSessions = results
        .map((idx) => sessions[idx])
        .filter(
          (session) =>
            session &&
            (providerFilter === "all" || session.providerId === providerFilter),
        );

      // 按时间排序
      return matchedSessions.sort((a, b) => {
        const aTs = a.lastActiveAt ?? a.createdAt ?? 0;
        const bTs = b.lastActiveAt ?? b.createdAt ?? 0;
        return bTs - aTs;
      });
    },
    [index, providerFilter, sessions],
  );

  return useMemo(() => ({ search }), [search]);
}
