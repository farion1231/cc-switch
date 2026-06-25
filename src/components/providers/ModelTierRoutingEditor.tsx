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
import { Checkbox } from "@/components/ui/checkbox";
import type { Provider } from "@/types";
import { supportsRouting } from "@/utils/providerRouting";

const TIERS = ["opus", "sonnet", "haiku", "fable"] as const;
type TierKey = (typeof TIERS)[number];

// Radix Select 不允许 value=""（空串），用哨兵值代表「未配置/使用默认」。
const NONE_PROVIDER = "__none__";

// 1M 能力声明的旧式后缀；新版改用显式 supports1m 字段，这里仅用于回退读取/迁移写入。
const ONE_M_SUFFIX = /\s*\[1m\]$/i;

function stripOneMSuffix(model: string): string {
  return model.replace(ONE_M_SUFFIX, "").trimEnd();
}

function readRoute(
  config: ModelTierRoutingConfig,
  appId: ModelTierRoutingApp,
  tier: TierKey,
): {
  providerId: string;
  model: string;
  displayName: string;
  supports1m: boolean;
} {
  const route = config.routes?.[appId]?.[tier];
  const rawModel = route?.model ?? "";
  return {
    providerId: route?.providerId ?? "",
    // 剥离旧式 [1m] 后缀：model 名只承载真实上游模型，1M 能力改由 supports1m 表达。
    model: stripOneMSuffix(rawModel),
    displayName: route?.displayName ?? "",
    // 显式字段优先；旧数据未带字段但 model 名有后缀时回退显示为勾选。
    supports1m: route?.supports1m ?? ONE_M_SUFFIX.test(rawModel),
  };
}

function writeRoute(
  config: ModelTierRoutingConfig,
  appId: ModelTierRoutingApp,
  tier: TierKey,
  next: {
    providerId: string;
    model: string;
    displayName: string;
    supports1m: boolean;
  },
): ModelTierRoutingConfig {
  const appRoutes = { ...(config.routes?.[appId] ?? {}) };
  if (!next.providerId) {
    delete appRoutes[tier];
  } else {
    // 1M 能力走显式字段；剥离 model 名里残留的 [1m] 后缀，让字段成为唯一真相。
    const route: TierRoute = {
      providerId: next.providerId,
      model: stripOneMSuffix(next.model.trim()),
      displayName: next.displayName.trim(),
      supports1m: next.supports1m,
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
    patch: Partial<{
      providerId: string;
      model: string;
      displayName: string;
      supports1m: boolean;
    }>,
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
        <div className="hidden sm:grid grid-cols-[5rem_1fr_1fr_1fr_116px] gap-3 px-1 text-xs font-medium text-muted-foreground">
          <span>{t("home.modelTierRouting.tier")}</span>
          <span>{t("home.modelTierRouting.provider")}</span>
          <span>{t("home.modelTierRouting.modelName")}</span>
          <span>{t("home.modelTierRouting.displayName")}</span>
          <span>
            {t("claudeDesktop.supports1mLabel", {
              defaultValue: "声明支持 1M",
            })}
          </span>
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
              className="grid grid-cols-1 sm:grid-cols-[5rem_1fr_1fr_1fr_116px] gap-2 sm:gap-3 items-center"
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
              <label className="flex h-9 items-center gap-2 text-sm text-muted-foreground">
                <Checkbox
                  checked={route.supports1m}
                  disabled={!route.providerId}
                  onCheckedChange={(checked) =>
                    handleTierChange(tier, { supports1m: checked === true })
                  }
                />
                {t("claudeDesktop.supports1mShort", { defaultValue: "1M" })}
              </label>
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
