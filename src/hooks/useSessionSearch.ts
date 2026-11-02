import { useCallback, useEffect, useMemo } from "react";
import FlexSearch from "flexsearch";
import type { SessionMeta } from "@/types";
import {
  endSessionTrace,
  markSessionTrace,
  startSessionTrace,
} from "@/lib/sessionPerf";

interface UseSessionSearchOptions {
  sessions: SessionMeta[];
  providerFilter: string;
}

interface UseSessionSearchResult {
  search: (query: string) => SessionMeta[];
}

/** 建索引时每一片最多占用主线程的时间 */
const INDEX_SLICE_MS = 8;

interface SessionIndexBuilder {
  /** 继续建索引，到点就停；返回是否已全部建完 */
  advance: (budgetMs: number) => boolean;
  /** 建完剩下的部分并返回索引 */
  finish: () => InstanceType<typeof FlexSearch.Index>;
}

const createIndexBuilder = (sessions: SessionMeta[]): SessionIndexBuilder => {
  const index = new FlexSearch.Index({
    tokenize: "full",
    resolution: 9,
  });
  let next = 0;

  const addNext = () => {
    const session = sessions[next];
    const metaContent = [
      session.sessionId,
      session.title,
      session.summary,
      session.projectDir,
      session.sourcePath,
    ]
      .filter(Boolean)
      .join(" ");

    index.add(next, metaContent);
    next += 1;
  };

  return {
    advance: (budgetMs) => {
      const deadline = performance.now() + budgetMs;
      while (next < sessions.length && performance.now() < deadline) {
        addNext();
      }
      return next >= sessions.length;
    },
    finish: () => {
      while (next < sessions.length) {
        addNext();
      }
      return index;
    },
  };
};

/**
 * 使用 FlexSearch 实现会话全文搜索
 * 索引会话元数据（标题、摘要、项目目录等）
 *
 * `tokenize: "full"` 建索引很慢（一千多个会话要半秒以上），不能一次做完卡住页面：
 * 平时分片在后台建；还没建完就开始搜索时，当场补完剩下的部分，搜索结果始终完整。
 */
export function useSessionSearch({
  sessions,
  providerFilter,
}: UseSessionSearchOptions): UseSessionSearchResult {
  const filteredByProvider = useMemo(() => {
    if (providerFilter === "all") return sessions;
    return sessions.filter((s) => s.providerId === providerFilter);
  }, [sessions, providerFilter]);

  const builder = useMemo(
    () => createIndexBuilder(filteredByProvider),
    [filteredByProvider],
  );

  useEffect(() => {
    const traceId = `index:${providerFilter}(${filteredByProvider.length})`;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const buildSlice = () => {
      if (builder.advance(INDEX_SLICE_MS)) {
        endSessionTrace(traceId);
        return;
      }
      markSessionTrace(traceId, "firstSlice");
      timer = setTimeout(buildSlice, 0);
    };
    startSessionTrace(traceId);
    timer = setTimeout(buildSlice, 0);
    return () => clearTimeout(timer);
  }, [builder, filteredByProvider.length, providerFilter]);

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

      const results = builder.finish().search(needle, {
        limit: filteredByProvider.length,
      }) as number[];

      return results.map((idx) => filteredByProvider[idx]);
    },
    [builder, filteredByProvider],
  );

  return { search };
}
