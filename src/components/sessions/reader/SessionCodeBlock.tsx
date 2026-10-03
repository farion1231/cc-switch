import { memo, useCallback, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { copyText } from "@/lib/clipboard";
import { toast } from "@/lib/toast";
import { cn } from "@/lib/utils";
import { highlightText } from "../utils";

/** 行内 `code` 与代码块共用的边框与底色，保证两处观感一致 */
export const SESSION_CODE_SURFACE = "border border-border bg-subtle";

/** 行内代码样式（Markdown 正文与纯文本提问共用） */
export const SESSION_INLINE_CODE_CLASS = cn(
  SESSION_CODE_SURFACE,
  "rounded-[4px] px-1 py-px font-mono text-[0.92em] text-fg-1",
);

export interface SessionCodeBlockProps {
  /** 代码原文（不含围栏） */
  code: string;
  /** 围栏上的语言标记；为空时标签显示 `text` */
  language?: string;
  /** 搜索词：命中部分用 `<mark>` 高亮 */
  searchQuery?: string;
  /** 自定义复制行为；不传时写入剪贴板并弹 toast */
  onCopy?: (code: string) => void;
  className?: string;
}

/**
 * 会话阅读页的代码块：顶栏显示语言标签与复制按钮，正文横向滚动。
 * Markdown 渲染、纯文本提问里的围栏代码、步骤详情都用它。
 */
export const SessionCodeBlock = memo(function SessionCodeBlock({
  code,
  language,
  searchQuery,
  onCopy,
  className,
}: SessionCodeBlockProps) {
  const { t } = useTranslation();
  const label = language?.trim() || "text";

  const handleCopy = useCallback(async () => {
    if (onCopy) {
      onCopy(code);
      return;
    }
    try {
      await copyText(code);
      toast.success(
        t("sessionManager.codeCopied", { defaultValue: "已复制代码" }),
      );
    } catch {
      toast.error(t("common.error", { defaultValue: "复制失败" }));
    }
  }, [code, onCopy, t]);

  let body: ReactNode = code;
  if (searchQuery) body = highlightText(code, searchQuery);

  return (
    <div
      className={cn(
        "my-2 max-w-full overflow-hidden rounded-[8px]",
        SESSION_CODE_SURFACE,
        className,
      )}
    >
      <div className="flex h-[30px] items-center justify-between border-b border-border pe-1 ps-3">
        <span className="truncate font-mono text-caption text-fg-2">
          {label}
        </span>
        <button
          type="button"
          aria-label={t("sessionManager.copyCode", {
            defaultValue: "复制代码",
          })}
          onClick={() => void handleCopy()}
          className="h-6 shrink-0 rounded-control px-2 text-caption text-fg-2 transition-colors hover:bg-selected hover:text-fg-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          {t("sessionManager.copyShort", { defaultValue: "复制" })}
        </button>
      </div>
      <div
        tabIndex={0}
        role="region"
        aria-label={t("sessionManager.codeRegion", {
          defaultValue: "{{lang}} 代码",
          lang: label,
        })}
        className="overflow-x-auto px-3 py-2 outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
      >
        <pre className="m-0 whitespace-pre font-mono text-caption text-fg-1">
          <code data-language={language || undefined}>{body}</code>
        </pre>
      </div>
    </div>
  );
});
