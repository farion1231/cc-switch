import { X, Download } from "lucide-react";
import { useUpdate } from "@/contexts/UpdateContext";
import { useTranslation } from "react-i18next";

interface UpdateBadgeProps {
  className?: string;
  onClick?: () => void;
}

export function UpdateBadge({ className = "", onClick }: UpdateBadgeProps) {
  const { hasUpdate, updateInfo, isDismissed, dismissUpdate } = useUpdate();
  const { t } = useTranslation();

  // 如果没有更新或已关闭，不显示
  if (!hasUpdate || isDismissed || !updateInfo) {
    return null;
  }

  return (
    <div
      className={`
        flex items-center gap-1.5 px-2.5 py-1
        bg-white dark:bg-gray-800
        border border-border-default 
        rounded-lg text-xs
        shadow-sm
        transition-all duration-200
        ${onClick ? "cursor-pointer hover:bg-gray-50 dark:hover:bg-gray-750" : ""}
        ${className}
      `}
      role={onClick ? "button" : undefined}
      tabIndex={onClick ? 0 : -1}
      onClick={onClick}
      onKeyDown={(e) => {
        if (!onClick) return;
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onClick();
        }
      }}
    >
      <Download className="w-3 h-3 text-blue-500 dark:text-blue-400" />
      <span className="text-gray-700 dark:text-gray-300 font-medium">
        {t("settings.updateBadge")}
      </span>
      <button
        onClick={(e) => {
          e.stopPropagation();
          dismissUpdate();
        }}
        className="
          ml-1 -mr-0.5 p-0.5 rounded
          hover:bg-gray-100 dark:hover:bg-gray-700
          transition-colors
          focus:outline-none focus:ring-2 focus:ring-blue-500/20
        "
        aria-label={t("common.close")}
      >
        <X className="w-3 h-3 text-muted-foreground" />
      </button>
    </div>
  );
}
