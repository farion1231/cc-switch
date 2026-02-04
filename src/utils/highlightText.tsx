import React from "react";

/**
 * 高亮显示搜索匹配的文本
 * @param text 原始文本
 * @param query 搜索关键词
 * @returns 带高亮标记的 React 元素
 */
export function highlightText(text: string, query: string): React.ReactNode {
  if (!query.trim() || !text) return text;

  // 转义正则特殊字符
  const escapedQuery = query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const regex = new RegExp(`(${escapedQuery})`, "gi");
  const parts = text.split(regex);

  if (parts.length === 1) return text;

  return parts.map((part, index) =>
    regex.test(part) ? (
      <mark
        key={index}
        className="px-0.5 rounded bg-yellow-200/80 dark:bg-yellow-700/60 text-inherit"
      >
        {part}
      </mark>
    ) : (
      part
    ),
  );
}

/**
 * 检查文本是否匹配搜索词
 */
export function textMatches(text: string | undefined, query: string): boolean {
  if (!text || !query.trim()) return false;
  return text.toLowerCase().includes(query.toLowerCase().trim());
}
