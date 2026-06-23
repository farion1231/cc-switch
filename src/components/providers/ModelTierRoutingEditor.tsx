/**
 * 首页「模型层级路由」编辑器。
 *
 * 在模型层级模式下替代 ProviderList：为每个 Claude 层级（Opus/Sonnet/Haiku/Fable）
 * 配置 provider + 模型名（proxy 改写后的真实模型）+ 展示名（写入可见模型菜单）。
 * 每次 onChange 即时保存（后端 set_model_tier_routing_config 会刷新 live 配置）。
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { providersApi } from "@/lib/api/providers";
import type { ModelTierRoutingConfig, TierRoute } from "@/lib/api/settings";
import type { ModelTierRoutingApp } from "@/hooks/useModelTierRouting";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Input } from "@/components/ui/input";
import type { Provider } from "@/types";
import { supportsRouting } from "@/utils/providerRouting";

const TIERS = ["opus", "sonnet", "haiku", "fable"] as const;
type TierKey = (typeof TIERS)[number];

// Radix Select 不允许 value=""（空串），用哨兵值代表「未配置/使用默认」。
const NONE_PROVIDER = "__none__";

function readRoute(
  config: ModelTierRoutingConfig,
  appId: ModelTierRoutingApp,
  tier: TierKey,
): { providerId: string; model: string; displayName: string } {
  const route = config.routes?.[appId]?.[tier];
  return {
    providerId: route?.providerId ?? "",
    model: route?.model ?? "",
    displayName: route?.displayName ?? "",
  };
}

function writeRoute(
  config: ModelTierRoutingConfig,
  appId: ModelTierRoutingApp,
  tier: TierKey,
  next: { providerId: string; model: string; displayName: string },
): ModelTierRoutingConfig {
  const appRoutes = { ...(config.routes?.[appId] ?? {}) };
  if (!next.providerId) {
    delete appRoutes[tier];
  } else {
    const route: TierRoute = {
      providerId: next.providerId,
      model: next.model.trim(),
      displayName: next.displayName.trim(),
    };
    appRoutes[tier] = route;
  }
  return { ...config, routes: { ...config.routes, [appId]: appRoutes } };
}

interface Props {
  appId: ModelTierRoutingApp;
  config: ModelTierRoutingConfig;
  onChange: (next: ModelTierRoutingConfig) => void;
}

export function ModelTierRoutingEditor({ appId, config, onChange }: Props) {
  const { t } = useTranslation();
  const [providers, setProviders] = useState<Record<string, Provider>>({});

  useEffect(() => {
    providersApi
      .getAll(appId)
      .then((map) => setProviders(map ?? {}))
      .catch((e) => console.error("Failed to load providers:", e));
  }, [appId]);

  const providerList = Object.values(providers).sort(
    (a, b) => (a.sortIndex ?? 0) - (b.sortIndex ?? 0),
  );
  // 不可路由的 provider（如官方账号）排除，避免生成代理无法转发的 route。
  // 判据集中在 supportsRouting，与徽章/接管拦截保持一致。
  const routableProviders = providerList.filter((p) => supportsRouting(p));

  const handleTierChange = (
    tier: TierKey,
    patch: Partial<{ providerId: string; model: string; displayName: string }>,
  ) => {
    onChange(
      writeRoute(config, appId, tier, {
        ...readRoute(config, appId, tier),
        ...patch,
      }),
    );
  };

  return (
    <div className="space-y-3">
      <div className="rounded-xl glass-card p-5 space-y-4">
        <div className="space-y-1">
          <h3 className="text-base font-semibold">
            {t("settings.advanced.modelTierRouting.title")}
          </h3>
          <p className="text-sm text-muted-foreground">
            {t("home.modelTierRouting.editorDescription")}
          </p>
        </div>

        {/* 表头 */}
        <div className="hidden sm:grid grid-cols-[5rem_1fr_1fr_1fr] gap-3 px-1 text-xs font-medium text-muted-foreground">
          <span>{t("home.modelTierRouting.tier")}</span>
          <span>{t("home.modelTierRouting.provider")}</span>
          <span>{t("home.modelTierRouting.modelName")}</span>
          <span>{t("home.modelTierRouting.displayName")}</span>
        </div>

        {TIERS.map((tier) => {
          const route = readRoute(config, appId, tier);
          // 边界：route 已指向官方 provider（脏数据/旧值）。过滤后不在可选项里，
          // 但保留为 disabled 项让 trigger 仍能显示当前值，并标注原因，引导改选。
          const selectedProvider = route.providerId
            ? providerList.find((p) => p.id === route.providerId)
            : undefined;
          const isSelectedNonRoutable =
            !!selectedProvider && !supportsRouting(selectedProvider);
          return (
            <div
              key={tier}
              className="grid grid-cols-1 sm:grid-cols-[5rem_1fr_1fr_1fr] gap-2 sm:gap-3 items-center"
            >
              <span className="capitalize text-sm font-medium px-1">
                {t(`settings.advanced.modelTierRouting.tier.${tier}`)}
              </span>
              <Select
                value={route.providerId || NONE_PROVIDER}
                onValueChange={(v) =>
                  handleTierChange(tier, {
                    providerId: v === NONE_PROVIDER ? "" : v,
                  })
                }
              >
                <SelectTrigger className="h-9 w-full min-w-0 text-sm font-normal">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={NONE_PROVIDER}>
                    {t("settings.advanced.modelTierRouting.noProvider")}
                  </SelectItem>
                  {isSelectedNonRoutable && selectedProvider && (
                    <SelectItem
                      key={selectedProvider.id}
                      value={selectedProvider.id}
                      disabled
                    >
                      {selectedProvider.name}（
                      {t("claudeCode.noRoutingSupport")}）
                    </SelectItem>
                  )}
                  {routableProviders.map((p) => (
                    <SelectItem key={p.id} value={p.id}>
                      {p.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Input
                className="min-w-0"
                value={route.model}
                disabled={!route.providerId}
                placeholder={t(
                  "settings.advanced.modelTierRouting.modelPlaceholder",
                )}
                onChange={(e) =>
                  handleTierChange(tier, { model: e.target.value })
                }
              />
              <Input
                className="min-w-0"
                value={route.displayName}
                disabled={!route.providerId}
                placeholder={t("home.modelTierRouting.displayNamePlaceholder")}
                onChange={(e) =>
                  handleTierChange(tier, { displayName: e.target.value })
                }
              />
            </div>
          );
        })}

        <p className="text-xs text-muted-foreground">
          {t("settings.advanced.modelTierRouting.hint")}
        </p>
      </div>
    </div>
  );
}
