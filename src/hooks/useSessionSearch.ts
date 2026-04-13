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
  const filteredByProvider = useMemo(() => {
    if (providerFilter === "all") return sessions;
    return sessions.filter((s) => s.providerId === providerFilter);
  }, [sessions, providerFilter]);

  const index = useMemo(() => {
    const nextIndex = new FlexSearch.Index({
      tokenize: "full",
      resolution: 9,
    });

    filteredByProvider.forEach((session, idx) => {
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
  }, [filteredByProvider]);

  const search = useCallback(
    (query: string): SessionMeta[] => {
      const needle = query.trim();

      if (!needle) {
        return [...filteredByProvider].sort((a, b) => {
          const aTs = a.lastActiveAt ?? a.createdAt ?? 0;
          const bTs = b.lastActiveAt ?? b.createdAt ?? 0;
          return bTs - aTs;
        });
      }

      const results = index.search(needle, {
        limit: filteredByProvider.length,
      }) as number[];

      return results.map((idx) => filteredByProvider[idx]);
    },
    [index, filteredByProvider],
  );

  return { search };
}
