import { useTranslation } from "react-i18next";
import { Edit2, Trash2, RefreshCw, Globe, Copy, ChevronUp, ChevronDown } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ProviderIcon } from "@/components/ProviderIcon";
import type { UniversalProvider } from "@/types";

interface UniversalProviderCardProps {
  provider: UniversalProvider;
  index: number; // 当前排序位置
  total: number; // 总数（用于判断边界）
  onEdit: (provider: UniversalProvider) => void;
  onDelete: (id: string) => void;
  onSync: (id: string) => void;
  onDuplicate: (provider: UniversalProvider) => void;
  onPriorityUp: (provider: UniversalProvider) => void;
  onPriorityDown: (provider: UniversalProvider) => void;
}

export function UniversalProviderCard({
  provider,
  index,
  total,
  onEdit,
  onDelete,
  onSync,
  onDuplicate,
  onPriorityUp,
  onPriorityDown,
}: UniversalProviderCardProps) {
  const { t } = useTranslation();

  // 获取启用的应用列表
  const enabledApps: string[] = [
    provider.apps.claude ? "Claude" : null,
    provider.apps.codex ? "Codex" : null,
    provider.apps.gemini ? "Gemini" : null,
  ].filter((app): app is string => app !== null);

  return (
    <div className="group relative rounded-xl border border-border/50 bg-card p-4 transition-all hover:border-border hover:shadow-md">
      {/* 头部：图标和名称 */}
      <div className="flex items-start justify-between">
        <div className="flex items-center gap-3">
          <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-accent">
            <ProviderIcon icon={provider.icon} name={provider.name} size={24} />
          </div>
          <div>
            <h3 className="font-semibold text-foreground">{provider.name}</h3>
            <div className="flex items-center gap-2">
              <p className="text-xs text-muted-foreground">
                {provider.providerType}
              </p>
              {/* 优先级徽章 */}
              {total > 1 && (
                <span className="inline-flex items-center rounded bg-emerald-500/10 px-1.5 py-0.5 text-[10px] font-semibold text-emerald-600 dark:text-emerald-400">
                  P{index + 1}
                </span>
              )}
            </div>
          </div>
        </div>

        {/* 优先级调节 + 操作按钮 */}
        <div className="flex items-center gap-1">
          {total > 1 && (
            <div className="flex flex-col gap-0.5 mr-1">
              <button
                type="button"
                className="flex h-4 w-5 items-center justify-center rounded hover:bg-accent disabled:opacity-30 disabled:cursor-not-allowed text-muted-foreground"
                onClick={() => onPriorityUp(provider)}
                disabled={index === 0}
                title="提高优先级"
              >
                <ChevronUp className="h-3 w-3" />
              </button>
              <button
                type="button"
                className="flex h-4 w-5 items-center justify-center rounded hover:bg-accent disabled:opacity-30 disabled:cursor-not-allowed text-muted-foreground"
                onClick={() => onPriorityDown(provider)}
                disabled={index === total - 1}
                title="降低优先级"
              >
                <ChevronDown className="h-3 w-3" />
              </button>
            </div>
          )}
          <div className="flex items-center gap-1 opacity-0 transition-opacity group-hover:opacity-100">
            <Button
              variant="ghost"
              size="icon"
              className="h-8 w-8"
              onClick={() => onSync(provider.id)}
              title={t("universalProvider.sync", { defaultValue: "同步到应用" })}
            >
              <RefreshCw className="h-4 w-4" />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              className="h-8 w-8"
              onClick={() => onDuplicate(provider)}
              title={t("universalProvider.duplicate", { defaultValue: "复制" })}
            >
              <Copy className="h-4 w-4" />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              className="h-8 w-8"
              onClick={() => onEdit(provider)}
              title={t("common.edit", { defaultValue: "编辑" })}
            >
              <Edit2 className="h-4 w-4" />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              className="h-8 w-8 text-destructive hover:text-destructive"
              onClick={() => onDelete(provider.id)}
              title={t("common.delete", { defaultValue: "删除" })}
            >
              <Trash2 className="h-4 w-4" />
            </Button>
          </div>
        </div>
      </div>

      {/* 配置信息 */}
      <div className="mt-4 space-y-2">
        {/* Base URL */}
        <div className="flex items-center gap-2 text-sm">
          <Globe className="h-3.5 w-3.5 text-muted-foreground" />
          <span className="truncate text-muted-foreground">
            {provider.baseUrl || "-"}
          </span>
        </div>

        {/* 启用的应用 */}
        <div className="flex flex-wrap gap-1.5">
          {enabledApps.map((app) => (
            <span
              key={app}
              className="inline-flex items-center rounded-full bg-primary/10 px-2 py-0.5 text-xs font-medium text-primary"
            >
              {app}
            </span>
          ))}
          {enabledApps.length === 0 && (
            <span className="text-xs text-muted-foreground">
              {t("universalProvider.noAppsEnabled", {
                defaultValue: "未启用任何应用",
              })}
            </span>
          )}
        </div>
      </div>

      {/* 备注 */}
      {provider.notes && (
        <p className="mt-3 text-xs text-muted-foreground line-clamp-2">
          {provider.notes}
        </p>
      )}
    </div>
  );
}
