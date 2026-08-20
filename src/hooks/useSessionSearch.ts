import { useCallback, useEffect, useMemo, useState } from "react";
import FlexSearch from "flexsearch";
import { sessionsApi } from "@/lib/api";
import type { SessionMeta, SessionSearchSnippet } from "@/types";

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

const CONTENT_SEARCH_DEBOUNCE_MS = 300;
const NO_SNIPPETS = new Map<string, SessionSearchSnippet[]>();

interface ContentSearchState {
  query: string;
  providerFilter: string;
  snippets: Map<string, SessionSearchSnippet[]>;
}

/**
 * 后端会话正文搜索（元数据索引只覆盖标题/摘要/路径，搜不到对话中段的内容）
 *
 * 结果携带产生它的 query 与 providerFilter，只有两者都与当前值相符时才对外暴露；
 * 因此一次慢的后台扫描返回时，不会把上一个关键词的命中泄漏到新的搜索结果里。
 */
export function useSessionContentSearch(query: string, providerFilter: string) {
  const [state, setState] = useState<ContentSearchState | null>(null);
  const [isSearching, setIsSearching] = useState(false);
  const needle = query.trim();

  useEffect(() => {
    if (!needle) {
      setState(null);
      setIsSearching(false);
      return;
    }

    let active = true;
    setIsSearching(true);

    const timer = setTimeout(() => {
      sessionsApi
        .search(needle, providerFilter === "all" ? undefined : providerFilter)
        .then((hits) => {
          if (!active) return;
          setState({
            query: needle,
            providerFilter,
            snippets: new Map(
              hits.map((hit) => [hit.sourcePath, hit.snippets]),
            ),
          });
        })
        .catch(() => {
          // 正文搜索只是元数据索引的增强，失败时退回到元数据结果即可
          if (!active) return;
          setState({ query: needle, providerFilter, snippets: NO_SNIPPETS });
        })
        .finally(() => {
          if (active) setIsSearching(false);
        });
    }, CONTENT_SEARCH_DEBOUNCE_MS);

    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [needle, providerFilter]);

  const snippetsBySource =
    state !== null &&
    state.query === needle &&
    state.providerFilter === providerFilter
      ? state.snippets
      : NO_SNIPPETS;

  return { snippetsBySource, isSearching };
}
