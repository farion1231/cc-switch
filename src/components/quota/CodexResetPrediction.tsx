import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { CircleAlert } from "lucide-react";
import { useSettingsQuery } from "@/lib/query/queries";
import { useCodexResetsStatus } from "@/lib/query/subscription";
import { HelpTip } from "@/components/ui/help-tip";
import {
  codexResetsChipText,
  codexResetsDetailLines,
  codexResetsDetailText,
  codexResetsSignalText,
} from "./codexResetsPrediction";

/** 倒计时每 30 秒重算一次 */
const TICK_MS = 30 * 1000;

/**
 * 标题行里的社区重置预测入口（codex-resets.com）：
 * 平时是安静的「重置预测 !」；有预测时带一句短摘要（「重置预测 · 60%」「预计 2h54m」）。
 * 悬浮卡标题行左端是「重置预测」、右端是信号状态（同一行，左右对齐），
 * 下面列上次重置 / 常规重置和来源声明。
 * 与额度行的「x 后重置」倒计时互不影响，也不占额度列的位置。
 *
 * 设置「Codex 重置预测」关闭时整体不渲染、也不发请求。
 */
export function CodexResetPrediction() {
  const { t, i18n } = useTranslation();
  const { data: settings } = useSettingsQuery();
  const enabled = settings?.codexResetPrediction ?? false;
  const { data: status, isLoading } = useCodexResetsStatus(enabled);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!enabled) return;
    const timer = window.setInterval(() => setNow(Date.now()), TICK_MS);
    return () => window.clearInterval(timer);
  }, [enabled]);

  if (!enabled) return null;

  // 详情只列上次重置和常规重置：信号状态在标题行右侧，不再重复
  const lines = status
    ? codexResetsDetailLines(status, now).filter((line) => line.key !== "watch")
    : [];

  return (
    <span className="inline-flex min-w-0 shrink-0 items-center gap-1 text-caption text-fg-3 transition-colors hover:text-fg-2">
      <span className="truncate tabular-nums">
        {codexResetsChipText(t, status, now)}
      </span>
      <HelpTip
        title={t("quota.resetPrediction.label")}
        titleExtra={codexResetsSignalText(t, status, isLoading, now)}
        icon={<CircleAlert className="h-3.5 w-3.5" strokeWidth={1.5} />}
        side="bottom"
      >
        <span className="flex flex-col items-start gap-0.5 text-left">
          {lines.map((line) => (
            <span key={line.key}>
              {codexResetsDetailText(t, i18n.language, line, now)}
            </span>
          ))}
          <span className="text-fg-3">{t("quota.resetPrediction.source")}</span>
        </span>
      </HelpTip>
    </span>
  );
}
