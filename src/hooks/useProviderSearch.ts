import { useState, useMemo, useCallback } from "react";
import { Provider } from "../types";

interface UseProviderSearchOptions {
  providers: Provider[];
  searchFields?: Array<keyof Provider>;
}

export const useProviderSearch = ({
  providers,
  searchFields = ["name", "id", "websiteUrl"],
}: UseProviderSearchOptions) => {
  const [searchQuery, setSearchQuery] = useState("");

  // 模糊匹配函数
  const fuzzyMatch = useCallback((text: string, query: string): boolean => {
    const lowerText = text.toLowerCase();
    const lowerQuery = query.toLowerCase();

    // 简单的模糊匹配：检查查询词是否作为子串存在
    if (lowerText.includes(lowerQuery)) {
      return true;
    }

    // 高级模糊匹配：检查查询词的字符是否按顺序出现
    let queryIndex = 0;
    for (let i = 0; i < lowerText.length && queryIndex < lowerQuery.length; i++) {
      if (lowerText[i] === lowerQuery[queryIndex]) {
        queryIndex++;
      }
    }
    return queryIndex === lowerQuery.length;
  }, []);

  // 搜索过滤
  const filteredProviders = useMemo(() => {
    if (!searchQuery.trim()) {
      return providers;
    }

    const query = searchQuery.trim();

    return providers.filter((provider) => {
      return searchFields.some((field) => {
        const value = provider[field];
        if (value === undefined || value === null) return false;

        // 处理字符串字段
        if (typeof value === "string") {
          return fuzzyMatch(value, query);
        }

        // 处理数组字段（如 tags）
        if (Array.isArray(value)) {
          return value.some((item) =>
            typeof item === "string" ? fuzzyMatch(item, query) : false
          );
        }

        return false;
      });
    });
  }, [providers, searchQuery, searchFields, fuzzyMatch]);

  // 高亮匹配的文本
  const highlightText = useCallback(
    (text: string): { text: string; highlighted: boolean }[] => {
      if (!searchQuery.trim()) {
        return [{ text, highlighted: false }];
      }

      const query = searchQuery.trim().toLowerCase();
      const lowerText = text.toLowerCase();
      const index = lowerText.indexOf(query);

      if (index === -1) {
        return [{ text, highlighted: false }];
      }

      const parts: { text: string; highlighted: boolean }[] = [];

      if (index > 0) {
        parts.push({ text: text.substring(0, index), highlighted: false });
      }

      parts.push({
        text: text.substring(index, index + query.length),
        highlighted: true,
      });

      if (index + query.length < text.length) {
        parts.push({
          text: text.substring(index + query.length),
          highlighted: false,
        });
      }

      return parts;
    },
    [searchQuery]
  );

  // 清空搜索
  const clearSearch = useCallback(() => {
    setSearchQuery("");
  }, []);

  return {
    searchQuery,
    setSearchQuery,
    filteredProviders,
    highlightText,
    clearSearch,
    hasActiveSearch: searchQuery.trim().length > 0,
  };
};
