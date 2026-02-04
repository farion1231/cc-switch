import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import FlexSearch from "flexsearch";
import type { SessionMeta } from "@/types";

// FlexSearch Index 类型
type FlexSearchIndex = InstanceType<typeof FlexSearch.Index>;

interface UseSessionSearchOptions {
  sessions: SessionMeta[];
  providerFilter: string;
}

interface UseSessionSearchResult {
  search: (query: string) => SessionMeta[];
  isIndexing: boolean;
}

/**
 * 使用 FlexSearch 实现会话全文搜索
 * 索引会话元数据（标题、摘要、项目目录等）
 */
export function useSessionSearch({
  sessions,
  providerFilter,
}: UseSessionSearchOptions): UseSessionSearchResult {
  const [isIndexing, setIsIndexing] = useState(false);

  // 会话元数据索引
  const indexRef = useRef<FlexSearchIndex | null>(null);
  // 索引 ID 到 session 的映射
  const sessionByIdxRef = useRef<SessionMeta[]>([]);

  // 初始化索引
  useEffect(() => {
    setIsIndexing(true);

    // 创建索引实例
    // 使用 forward tokenizer 支持中文前缀搜索
    const index = new FlexSearch.Index({
      tokenize: "forward",
      resolution: 9,
    });

    // 索引所有会话
    sessions.forEach((session, idx) => {
      // 索引会话元数据
      const metaContent = [
        session.sessionId,
        session.title,
        session.summary,
        session.projectDir,
        session.sourcePath,
      ]
        .filter(Boolean)
        .join(" ");

      index.add(idx, metaContent);
    });

    indexRef.current = index;
    sessionByIdxRef.current = sessions;

    setIsIndexing(false);
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

      const index = indexRef.current;

      if (!index) {
        // 索引未就绪，使用简单搜索
        return filtered
          .filter((session) => {
            const haystack = [
              session.sessionId,
              session.title,
              session.summary,
              session.projectDir,
              session.sourcePath,
            ]
              .filter(Boolean)
              .join(" ")
              .toLowerCase();
            return haystack.includes(needle);
          })
          .sort((a, b) => {
            const aTs = a.lastActiveAt ?? a.createdAt ?? 0;
            const bTs = b.lastActiveAt ?? b.createdAt ?? 0;
            return bTs - aTs;
          });
      }

      // 使用 FlexSearch 搜索
      const results = index.search(needle, { limit: 100 }) as number[];

      // 转换为 session 并过滤
      const matchedSessions = results
        .map((idx) => sessionByIdxRef.current[idx])
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
    [sessions, providerFilter],
  );

  return useMemo(() => ({ search, isIndexing }), [search, isIndexing]);
}
