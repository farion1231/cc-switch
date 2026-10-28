import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { HoverTip } from "@/components/ui/hover-tip";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { fmtInt, getLocaleFromLanguage } from "./format";

interface TablePaginationProps {
  /** 从 0 开始的页码 */
  page: number;
  totalPages: number;
  /** 总条数，显示在左侧 */
  total: number;
  onPageChange: (page: number) => void;
  /** 传了就在总数旁边给「每页 N 条」下拉 */
  pageSize?: number;
  pageSizeOptions?: readonly number[];
  onPageSizeChange?: (size: number) => void;
}

const NAV_BUTTON =
  "inline-flex h-7 w-7 items-center justify-center rounded-control text-fg-2 transition-colors hover:bg-subtle hover:text-fg-1 disabled:pointer-events-none disabled:opacity-45";

/**
 * 用量页表格共用的翻页条：总数、上一页、可输入的页码、下一页。
 * 只有一页时不画——翻不了页，总数在页签计数里已经有；
 * 能改每页条数时，只要条数多过最小一档就画（不然调大以后就找不回来了）。
 */
export function TablePagination({
  page,
  totalPages,
  total,
  onPageChange,
  pageSize,
  pageSizeOptions,
  onPageSizeChange,
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

  const canResize =
    pageSize != null && onPageSizeChange != null && !!pageSizeOptions?.length;
  const resizable = canResize && total > Math.min(...pageSizeOptions!);
  if (totalPages <= 1 && !resizable) return null;

  return (
    <div className="flex h-11 items-center gap-2 border-t border-border bg-subtle px-4 text-caption text-fg-3">
      <span className="tabular-nums">{t("usage.totalRecords", { total })}</span>
      {resizable && (
        <Select
          value={String(pageSize)}
          onValueChange={(next) => {
            const size = Number(next);
            // Radix Select 偶尔回报空串，忽略
            if (pageSizeOptions!.includes(size) && size !== pageSize) {
              onPageSizeChange!(size);
            }
          }}
        >
          <SelectTrigger
            className="h-7 w-auto gap-1 px-2 text-caption text-fg-2"
            aria-label={t("usage.pageSizeLabel")}
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {pageSizeOptions!.map((size) => (
              <SelectItem key={size} value={String(size)}>
                {t("usage.pageSize", { count: size })}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      )}
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
      <span className="flex items-center gap-1.5 tabular-nums text-fg-2">
        <input
          type="text"
          inputMode="numeric"
          aria-label={t("usage.pageInputPlaceholder")}
          className="h-6 w-10 rounded-[4px] border border-border-strong bg-surface text-center text-caption text-fg-1 transition-[border-color,box-shadow] focus:border-ring focus:outline-none focus:ring-[3px] focus:ring-ring/20"
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
