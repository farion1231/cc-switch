import React from "react";
import { useTranslation } from "react-i18next";
import { SortField, SortOrder } from "../types";
import { sortFieldLabels } from "../hooks/useProviderSort";
import { ArrowUp, ArrowDown } from "lucide-react";
import { cn } from "../lib/styles";

interface SortSelectorProps {
  currentField: SortField;
  currentOrder: SortOrder;
  onSortChange: (field: SortField, order?: SortOrder) => void;
  className?: string;
}

export const SortSelector: React.FC<SortSelectorProps> = ({
  currentField,
  currentOrder,
  onSortChange,
  className,
}) => {
  const { t, i18n } = useTranslation();
  const isZh = i18n.language === "zh";

  const sortFields: SortField[] = [
    "name",
    "createdAt",
    "lastUsed",
    "priority",
    "contractExpiry",
  ];

  return (
    <div className={cn("flex items-center gap-2", className)}>
      {/* 排序字段选择 */}
      <select
        value={currentField}
        onChange={(e) => onSortChange(e.target.value as SortField)}
        className="px-3 py-1.5 text-sm border border-gray-300 dark:border-gray-600 rounded-md bg-white dark:bg-gray-800 text-gray-900 dark:text-gray-100 focus:outline-none focus:ring-2 focus:ring-blue-500"
      >
        {sortFields.map((field) => (
          <option key={field} value={field}>
            {isZh ? sortFieldLabels[field].zh : sortFieldLabels[field].en}
          </option>
        ))}
      </select>

      {/* 排序顺序切换按钮 */}
      <button
        onClick={() =>
          onSortChange(
            currentField,
            currentOrder === "asc" ? "desc" : "asc"
          )
        }
        className="inline-flex items-center gap-1 px-3 py-1.5 text-sm font-medium rounded-md transition-colors bg-gray-100 hover:bg-gray-200 dark:bg-gray-700 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300"
        title={
          currentOrder === "asc"
            ? isZh
              ? "升序"
              : "Ascending"
            : isZh
              ? "降序"
              : "Descending"
        }
      >
        {currentOrder === "asc" ? (
          <ArrowUp className="h-4 w-4" />
        ) : (
          <ArrowDown className="h-4 w-4" />
        )}
        <span>{currentOrder === "asc" ? (isZh ? "升序" : "Asc") : (isZh ? "降序" : "Desc")}</span>
      </button>
    </div>
  );
};
