import { useCallback, useMemo } from "react";
import type { SessionMeta } from "@/types";

interface UseSessionSearchOptions {
  sessions: SessionMeta[];
  providerFilter: string;
}

interface UseSessionSearchResult {
  search: (query: string) => SessionMeta[];
}

/** 组合附加符号（重音等），归一化时去掉，`café` 与 `cafe` 互相能搜到 */
const COMBINING_MARKS = /[̀-ͯ]/g;
/** 词与词之间的分隔：字母、数字以外的一切（空白、标点、路径分隔符等） */
const SEPARATORS = /[^\p{L}\p{N}]+/u;

/** 相邻的重复字符合成一个，让 `hello` 与 `helo` 互相能搜到（沿用原来的搜索口径） */
const collapseRepeats = (term: string) => {
  let out = "";
  let prev = "";
  for (const char of term) {
    if (char !== prev) out += char;
    prev = char;
  }
  return out;
};

/** 归一化（去重音、小写）后按分隔切成词 */
export const searchTerms = (text: string): string[] =>
  text
    .normalize("NFKD")
    .replace(COMBINING_MARKS, "")
    .toLowerCase()
    .split(SEPARATORS)
    .filter(Boolean)
    .map(collapseRepeats);

// 每个会话对象的可搜索文本只算一次；会话列表刷新后旧对象随之回收
const haystacks = new WeakMap<SessionMeta, string>();

const haystackOf = (session: SessionMeta) => {
  let text = haystacks.get(session);
  if (text === undefined) {
    // 词之间用空格连起来：查询词本身不含分隔符，所以不会跨词匹配
    text = searchTerms(
      [
        session.sessionId,
        session.title,
        session.summary,
        session.projectDir,
        session.sourcePath,
      ]
        .filter(Boolean)
        .join(" "),
    ).join(" ");
    haystacks.set(session, text);
  }
  return text;
};

/**
 * 会话元数据搜索（会话 ID、标题、摘要、项目目录、文件路径）。
 *
 * 查询按与元数据相同的方式归一化、切词，每个词都要在某个字段里以子串出现。
 * 直接线性扫描：两千个会话的可搜索文本约 3ms 算完、单次查询约 1ms，
 * 比建全子串索引（同样数据约 0.5 秒）便宜得多，也不用分片或缓存索引。
 */
export function useSessionSearch({
  sessions,
  providerFilter,
}: UseSessionSearchOptions): UseSessionSearchResult {
  const filteredByProvider = useMemo(() => {
    if (providerFilter === "all") return sessions;
    return sessions.filter((s) => s.providerId === providerFilter);
  }, [sessions, providerFilter]);

  const search = useCallback(
    (query: string): SessionMeta[] => {
      if (!query.trim()) {
        return [...filteredByProvider].sort((a, b) => {
          const aTs = a.lastActiveAt ?? a.createdAt ?? 0;
          const bTs = b.lastActiveAt ?? b.createdAt ?? 0;
          return bTs - aTs;
        });
      }

      const terms = searchTerms(query);
      // 只有标点之类的查询没有可匹配的词，和原来一样什么都不返回
      if (terms.length === 0) return [];

      return filteredByProvider.filter((session) => {
        const text = haystackOf(session);
        return terms.every((term) => text.includes(term));
      });
    },
    [filteredByProvider],
  );

  return { search };
}
