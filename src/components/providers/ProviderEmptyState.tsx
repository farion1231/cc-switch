import { Download, KeyRound, Layers, Plus, Server } from "lucide-react";
import { useTranslation } from "react-i18next";
import { HelpTip } from "@/components/ui/help-tip";
import { Button } from "@/components/ui/button";
import type { AppId } from "@/lib/api/types";
import { appHas } from "@/config/appSpecs";

interface ProviderEmptyStateProps {
  appId: AppId;
  onCreate?: () => void;
  onImport?: () => void;
  /** 应用内部直连模式：仅展示已登录的官方账号，引导至授权中心登录、至聚合页配置外部供应商 */
  officialOnly?: boolean;
  /** 不支持官方登录的应用：仅展示通过聚合模式提供的供应商，引导至聚合页配置 */
  aggregationOnly?: boolean;
  onOpenAuthCenter?: () => void;
  onOpenAggregation?: () => void;
}

/**
 * 供应商页空状态（v7 PANELS_BRIEF 空状态规则）：列表区居中，标题 15/600 +
 * 操作按钮。辅助说明收进帮助卡；页头已有实心的「添加供应商」，这里一律描边。
 */
export function ProviderEmptyState({
  appId,
  onCreate,
  onImport,
  officialOnly = false,
  aggregationOnly = false,
  onOpenAuthCenter,
  onOpenAggregation,
}: ProviderEmptyStateProps) {
  const { t } = useTranslation();

  if (officialOnly) {
    return (
      <div className="flex flex-col items-center justify-center px-6 py-16 text-center">
        <div className="mb-5 flex h-14 w-14 items-center justify-center rounded-2xl border border-border bg-subtle text-fg-2">
          <KeyRound className="h-6 w-6" strokeWidth={1.5} aria-hidden="true" />
        </div>
        <h2 className="text-section text-fg-1">
          {t("provider.officialEmptyTitle", {
            defaultValue: "尚未登录官方账号",
          })}
        </h2>
        <p className="mt-2 max-w-lg text-body text-fg-2">
          {t("provider.officialEmptyDescription", {
            defaultValue:
              "所有官方登录统一在「授权中心」管理；请前往授权中心登录账号，完成后即可在此直连切换。外部供应商请在「聚合」页面配置。",
          })}
        </p>
        <div className="mt-5 flex flex-wrap items-center justify-center gap-2">
          {onOpenAuthCenter && (
            <Button variant="neutral" size="regular" onClick={onOpenAuthCenter}>
              <KeyRound className="h-4 w-4" />
              {t("provider.goToAuthCenter", { defaultValue: "前往授权中心" })}
            </Button>
          )}
          {onOpenAggregation && (
            <Button
              variant={onOpenAuthCenter ? "quiet" : "neutral"}
              size="regular"
              onClick={onOpenAggregation}
            >
              <Layers className="h-4 w-4" />
              {t("aggregation.goToPage", { defaultValue: "前往聚合页" })}
            </Button>
          )}
        </div>
      </div>
    );
  }

  if (aggregationOnly) {
    return (
      <div className="flex flex-col items-center justify-center px-6 py-16 text-center">
        <div className="mb-5 flex h-14 w-14 items-center justify-center rounded-2xl border border-border bg-subtle text-fg-2">
          <Layers className="h-6 w-6" strokeWidth={1.5} aria-hidden="true" />
        </div>
        <h2 className="text-section text-fg-1">
          {t("provider.externalOnlyEmptyTitle", {
            defaultValue: "请在「聚合」中配置外部供应商",
          })}
        </h2>
        <p className="mt-2 max-w-lg text-body text-fg-2">
          {t("provider.externalOnlyEmptyDescription", {
            defaultValue:
              "此应用不支持官方登录；请前往「聚合」页面添加外部供应商并指定给此应用，完成后会显示在这里。",
          })}
        </p>
        {onOpenAggregation && (
          <div className="mt-5 flex flex-wrap items-center justify-center gap-2">
            <Button variant="neutral" size="regular" onClick={onOpenAggregation}>
              <Layers className="h-4 w-4" />
              {t("aggregation.goToPage", { defaultValue: "前往聚合页" })}
            </Button>
          </div>
        )}
      </div>
    );
  }
  // Pi / MiniMax Code / Oh My Pi 的"当前供应商"可以是原生应用自管的内置账号（不在可管理的
  // live 节点里），没有可导入的内容，列表也不提供导入按钮，因此不能沿用
  // "请点击导入当前配置"的通用文案。
  const emptyCopyNs =
    appId === "pi" ||
    appId === "mcode" ||
    appId === "kimi" ||
    appId === "omp" ||
    appId === "workbuddy"
      ? appId
      : appId === "dsh"
        ? "dsh"
        : null;
  // Antigravity 没有切换，这句「切换只替换关键字段」对它不成立
  const showKeyFieldsHint = appHas(appId, "editorView");

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
