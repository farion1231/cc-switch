import { Download, Plus, Server } from "lucide-react";
import { useTranslation } from "react-i18next";
import { HelpTip } from "@/components/ui/help-tip";
import { Button } from "@/components/ui/button";
import type { AppId } from "@/lib/api/types";

interface ProviderEmptyStateProps {
  appId: AppId;
  onCreate?: () => void;
  onImport?: () => void;
}

/**
 * 供应商页空状态（v7 PANELS_BRIEF 空状态规则）：列表区居中，标题 15/600 +
 * 操作按钮。辅助说明收进帮助卡；页头已有实心的「添加供应商」，这里一律描边。
 */
export function ProviderEmptyState({
  appId,
  onCreate,
  onImport,
}: ProviderEmptyStateProps) {
  const { t } = useTranslation();
  // Pi / MiniMax Code 的"当前供应商"可以是原生应用自管的内置账号（不在可管理的
  // live 节点里），没有可导入的内容，列表也不提供导入按钮，因此不能沿用
  // "请点击导入当前配置"的通用文案。
  const emptyCopyNs = appId === "pi" || appId === "mcode" ? appId : null;
  // Antigravity 没有切换，这句「切换只替换关键字段」对它不成立
  const showKeyFieldsHint =
    appId === "claude" ||
    appId === "codex" ||
    appId === "gemini" ||
    appId === "grokbuild";

  return (
    <div className="flex flex-col items-center justify-center px-6 py-16 text-center">
      <div className="mb-5 flex h-14 w-14 items-center justify-center rounded-2xl border border-border bg-subtle text-fg-2">
        <Server className="h-6 w-6" strokeWidth={1.5} aria-hidden="true" />
      </div>
      <h2 className="text-section text-fg-1">
        {emptyCopyNs
          ? t(`${emptyCopyNs}.empty.title`)
          : t("provider.noProviders")}
      </h2>
      {emptyCopyNs && (
        <p className="mt-2 max-w-lg text-body text-fg-2">
          {t(`${emptyCopyNs}.empty.description`)}
        </p>
      )}
      {showKeyFieldsHint && (
        <p className="mt-2 max-w-lg text-body text-fg-2">
          {t("provider.noProvidersDescriptionKeyFields")}
        </p>
      )}
      {(onImport || onCreate) && (
        <div className="mt-5 flex flex-wrap items-center justify-center gap-2">
          {onImport && (
            <Button variant="neutral" size="regular" onClick={onImport}>
              <Download className="h-4 w-4" />
              {appId === "claude-desktop"
                ? t("provider.importFromClaude")
                : t("provider.importCurrent")}
            </Button>
          )}
          {onCreate && (
            <Button variant="neutral" size="regular" onClick={onCreate}>
              <Plus className="h-4 w-4" />
              {t("provider.addProvider")}
            </Button>
          )}
          {!emptyCopyNs && onImport && (
            <HelpTip title={t("provider.noProviders")}>
              {t("provider.noProvidersDescription")}
            </HelpTip>
          )}
        </div>
      )}
    </div>
  );
}
