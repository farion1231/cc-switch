import { ChevronRight } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import type { AppId } from "@/lib/api";
import {
  enabledPromptOf,
  promptAppOf,
  usePromptListQuery,
} from "@/lib/query/prompts";

/**
 * 应用页页头的「提示词：<启用中的那条> ›」（Main.dc.html 第 37–40 行）：既显示状态，也一键去提示词页。
 * 只在支持提示词的应用页出现；Claude Desktop 显示 Claude Code 的那条。
 */
export function PromptEntryButton({
  app,
  onOpen,
}: {
  app: AppId;
  onOpen: () => void;
}) {
  const { t } = useTranslation();
  const promptApp = promptAppOf(app);
  const { data } = usePromptListQuery(promptApp);
  if (!promptApp) return null;

  const name = enabledPromptOf(data)?.name ?? t("prompts.entryNone");
  const label = t("prompts.entryLabel", { name });

  return (
    <Button
      variant="quiet"
      size="regular"
      onClick={onOpen}
      title={label}
      aria-label={t("prompts.entryAria", { name })}
      className="min-w-0 max-w-[200px] shrink gap-1 pe-2 ps-2.5 text-fg-2"
    >
      <span className="min-w-0 truncate">{label}</span>
      <ChevronRight aria-hidden="true" className="h-3.5 w-3.5 shrink-0" />
    </Button>
  );
}
