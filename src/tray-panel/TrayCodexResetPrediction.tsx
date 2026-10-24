import { useState } from "react";
import { useTranslation } from "react-i18next";
import { CircleAlert } from "lucide-react";
import { useSettingsQuery } from "@/lib/query";
import { useCodexResetsStatus } from "@/lib/query/subscription";
import {
  codexResetsDetailLines,
  codexResetsDetailText,
  codexResetsSignalText,
} from "@/components/quota/codexResetsPrediction";
import { cn } from "@/lib/utils";

/**
 * 托盘面板里 Codex 官方行的社区重置预测（codex-resets.com）：
 * 额度（每周剩余等）下面一行，「重置预测 !」靠左、状态（当前没有重置信号 /
 * 重置信号 · 概率 60% / 预计 2h54m …）靠右，左右对齐、中间留空；
 * 点 ! 在行下展开详情（上次重置 / 常规重置 / 来源声明），状态不重复。
 * 主面板应用行和子面板供应商行共用。
 *
 * 设置「Codex 重置预测」关闭时整体不渲染、也不发请求。
 */
export function TrayCodexResetPrediction() {
  const { t, i18n } = useTranslation();
  const { data: settings } = useSettingsQuery();
  const enabled = settings?.codexResetPrediction ?? false;
  const { data: status, isLoading } = useCodexResetsStatus(enabled);
  const [open, setOpen] = useState(false);

  if (!enabled) return null;

  const now = Date.now();
  // 详情只列上次重置和常规重置：状态在行右端，展开不再重复
  const lines = status
    ? codexResetsDetailLines(status, now).filter((line) => line.key !== "watch")
    : [];

  return (
    <span className="mt-1 block">
      <span
        role="button"
        tabIndex={0}
        aria-expanded={open}
        aria-label={t("quota.resetPrediction.label")}
        onClick={(event) => {
          // 外层整行是切换供应商 / 展开子面板的按钮，点 ! 只展开详情
          event.stopPropagation();
          setOpen((value) => !value);
        }}
        onKeyDown={(event) => {
          if (event.key === "Enter" || event.key === " ") {
            event.stopPropagation();
            event.preventDefault();
            setOpen((value) => !value);
          }
        }}
        className="flex w-full items-center justify-between gap-2 text-left text-[11px] text-muted-foreground"
      >
        <span className="flex shrink-0 items-center gap-1">
          {t("quota.resetPrediction.label")}
          <CircleAlert
            className="h-3 w-3 shrink-0 opacity-80"
            strokeWidth={1.5}
            aria-hidden
          />
        </span>
        <span className="min-w-0 truncate tabular-nums">
          {codexResetsSignalText(t, status, isLoading, now)}
        </span>
      </span>
      {open && (
        <span
          className={cn(
            "mt-0.5 flex flex-col items-start gap-0.5 text-[10.5px] leading-4",
            "rounded-md bg-black/[0.05] px-1.5 py-1 text-muted-foreground dark:bg-white/[0.07]",
          )}
        >
          {lines.map((line) => (
            <span key={line.key}>
              {codexResetsDetailText(t, i18n.language, line, now)}
            </span>
          ))}
          <span className="opacity-75">
            {t("quota.resetPrediction.source")}
          </span>
        </span>
      )}
    </span>
  );
}
