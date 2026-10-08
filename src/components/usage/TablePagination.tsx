import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { HoverTip } from "@/components/ui/hover-tip";
import { fmtInt, getLocaleFromLanguage } from "./format";

interface TablePaginationProps {
  /** 从 0 开始的页码 */
  page: number;
  totalPages: number;
  /** 总条数，显示在左侧 */
  total: number;
  onPageChange: (page: number) => void;
}

const NAV_BUTTON =
  "inline-flex h-7 w-7 items-center justify-center rounded-control text-fg-2 transition-colors hover:bg-subtle hover:text-fg-1 disabled:pointer-events-none disabled:opacity-45";

/** 用量页表格共用的翻页条：总数、上一页、可输入的页码、下一页。 */
export function TablePagination({
  page,
  totalPages,
  total,
  onPageChange,
}: TablePaginationProps) {
  const { t, i18n } = useTranslation();
  const locale = getLocaleFromLanguage(
    i18n.resolvedLanguage || i18n.language || "en",
  );
  const [pageDraft, setPageDraft] = useState<string | null>(null);

  // 页码从外部变了（翻页、筛选重置）时丢掉没提交的输入
  useEffect(() => {
    setPageDraft(null);
  }, [page]);

  const commitPageDraft = () => {
    if (pageDraft == null) return;
    const trimmed = pageDraft.trim();
    setPageDraft(null);
    if (!/^\d+$/.test(trimmed)) return;
    const parsed = Number(trimmed);
    if (parsed < 1 || parsed > totalPages) return;
    onPageChange(parsed - 1);
  };

  return (
    <div className="flex h-10 items-center gap-2 text-caption text-fg-3">
      <span className="tabular-nums">{t("usage.totalRecords", { total })}</span>
      <div className="flex-1" />
      <HoverTip content={t("usage.prevPage")}>
        <button
          type="button"
          className={NAV_BUTTON}
          aria-label={t("usage.prevPage")}
          disabled={page === 0}
          onClick={() => onPageChange(Math.max(0, page - 1))}
        >
          <ChevronLeft className="h-4 w-4" />
        </button>
      </HoverTip>
      <span className="flex items-center gap-1 tabular-nums text-fg-2">
        <input
          type="text"
          inputMode="numeric"
          aria-label={t("usage.pageInputPlaceholder")}
          className="h-6 w-9 rounded-[4px] border border-transparent bg-transparent text-center text-caption text-fg-1 transition-[border-color,box-shadow] hover:border-border-strong focus:border-ring focus:bg-surface focus:outline-none focus:ring-[3px] focus:ring-ring/20"
          value={pageDraft ?? String(page + 1)}
          onChange={(event) => setPageDraft(event.target.value)}
          onFocus={(event) => event.target.select()}
          onBlur={commitPageDraft}
          onKeyDown={(event) => {
            if (event.key === "Enter") commitPageDraft();
            if (event.key === "Escape") setPageDraft(null);
          }}
        />
        <span>/ {fmtInt(totalPages, locale)}</span>
      </span>
      <HoverTip content={t("usage.nextPage")}>
        <button
          type="button"
          className={NAV_BUTTON}
          aria-label={t("usage.nextPage")}
          disabled={page >= totalPages - 1}
          onClick={() => onPageChange(Math.min(totalPages - 1, page + 1))}
        >
          <ChevronRight className="h-4 w-4" />
        </button>
      </HoverTip>
    </div>
  );
}

export const TABLE_PAGE_SIZE = 20;

/**
 * 前端分页：数据一次性拿到、在客户端切页的表格（供应商、模型、定价）用。
 * `resetKey` 变了（换时间范围 / 筛选）回到第一页；自动刷新只换数据不换 key，
 * 停在当前页，数据变少时把页码收回到最后一页。
 */
export function useClientPagination<T>(
  rows: readonly T[],
  resetKey: string = "",
  pageSize: number = TABLE_PAGE_SIZE,
) {
  const [page, setPage] = useState(0);
  const totalPages = Math.max(1, Math.ceil(rows.length / pageSize));

  useEffect(() => {
    setPage(0);
  }, [resetKey]);

  const currentPage = Math.min(page, totalPages - 1);
  const pageRows = rows.slice(
    currentPage * pageSize,
    (currentPage + 1) * pageSize,
  );

  return {
    page: currentPage,
    setPage,
    totalPages,
    total: rows.length,
    pageRows,
  };
}
