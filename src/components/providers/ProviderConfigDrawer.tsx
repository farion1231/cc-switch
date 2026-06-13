import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import { Badge } from "@/components/ui/badge";
import { Checkbox } from "@/components/ui/checkbox";
import { ProviderIcon } from "@/components/ProviderIcon";
import { ProviderActions } from "@/components/providers/ProviderActions";
import { isHermesReadOnlyProvider } from "@/config/hermesProviderPresets";
import {
  getGroupCommonConfigCandidates,
  type GroupCommonConfigKey,
} from "@/lib/provider-management/providerGroupCommonConfig";
import { extractProviderSummary } from "@/lib/provider-management/providerSummary";
import { cn } from "@/lib/utils";

const COMMON_CONFIG_KEYS: GroupCommonConfigKey[] = [
  "apiKey",
  "baseUrl",
  "modelMapping",
  "apiFormat",
  "customUserAgent",
];

export interface ProviderConfigDrawerState {
  isCurrent: boolean;
  isDefaultModel?: boolean;
  isInConfig: boolean;
  isOmo: boolean;
  isOmoSlim: boolean;
  isInFailoverQueue: boolean;
}

interface ProviderConfigDrawerProps {
  groupId: string;
  groupLabel: string;
  providers: Provider[];
  primaryProvider: Provider;
  appId: AppId;
  getProviderState: (provider: Provider) => ProviderConfigDrawerState;
  onSwitch: (provider: Provider) => void;
  onEdit: (provider: Provider) => void;
  onDelete: (provider: Provider) => void;
  onDuplicate: (provider: Provider) => void;
  onRemoveFromConfig?: (provider: Provider) => void;
  onDisableOmo?: () => void;
  onDisableOmoSlim?: () => void;
  onConfigureUsage?: (provider: Provider) => void;
  onOpenTerminal?: (provider: Provider) => void;
  onOpenCodexSessions?: (provider: Provider) => void;
  onTest?: (provider: Provider) => void;
  isTesting?: (providerId: string) => boolean;
  isProxyTakeover?: boolean;
  isAutoFailoverEnabled?: boolean;
  onToggleFailover?: (providerId: string, enabled: boolean) => void;
  onSetAsDefault?: (provider: Provider) => void;
  onApplyGroupCommonConfig: (
    provider: Provider,
    keys: GroupCommonConfigKey[],
  ) => void;
}

const enabledKeys = (provider: Provider): GroupCommonConfigKey[] => {
  const enabled = provider.meta?.groupCommonConfigEnabled ?? {};
  return COMMON_CONFIG_KEYS.filter((key) => enabled[key]);
};

export function ProviderConfigDrawer({
  groupId,
  groupLabel,
  providers,
  primaryProvider,
  appId,
  getProviderState,
  onSwitch,
  onEdit,
  onDelete,
  onDuplicate,
  onRemoveFromConfig,
  onDisableOmo,
  onDisableOmoSlim,
  onConfigureUsage,
  onOpenTerminal,
  onOpenCodexSessions,
  onTest,
  isTesting,
  isProxyTakeover = false,
  isAutoFailoverEnabled = false,
  onToggleFailover,
  onSetAsDefault,
  onApplyGroupCommonConfig,
}: ProviderConfigDrawerProps) {
  const { t } = useTranslation();
  const sourceCandidates = useMemo(
    () => getGroupCommonConfigCandidates(primaryProvider, appId),
    [appId, primaryProvider],
  );
  const sourceFields = COMMON_CONFIG_KEYS.flatMap((key) => {
    const field = sourceCandidates[key];
    return field ? [field] : [];
  });

  return (
    <div
      className="rounded-lg border border-border bg-muted/25"
      data-testid={`provider-config-drawer-${groupId}`}
    >
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-border px-3 py-2">
        <div className="min-w-0">
          <div className="truncate text-sm font-medium">{groupLabel}</div>
          <div className="text-[11px] text-muted-foreground">
            {t("provider.management.groupDrawerCount", {
              count: providers.length,
              defaultValue: `${providers.length} configs`,
            })}
          </div>
        </div>
        <div className="flex min-w-0 flex-wrap justify-end gap-1.5">
          {sourceFields.map((field) => (
            <Badge
              key={field.key}
              variant="outline"
              className="max-w-[12rem] rounded-md font-normal"
              title={field.displayValue}
            >
              <span className="truncate">
                {field.label}: {field.displayValue}
              </span>
            </Badge>
          ))}
        </div>
      </div>

      <div className="divide-y divide-border">
        {providers.map((provider) => {
          const summary = extractProviderSummary(provider, appId);
          const isPrimary = provider.id === primaryProvider.id;
          const activeKeys = enabledKeys(provider);
          const state = getProviderState(provider);
          const isReadOnly =
            appId === "hermes" &&
            isHermesReadOnlyProvider(provider.settingsConfig);

          return (
            <div
              key={provider.id}
              className="grid gap-3 px-3 py-3 lg:grid-cols-[minmax(0,1.4fr)_minmax(0,1fr)_auto]"
              data-testid={`provider-config-drawer-row-${provider.id}`}
            >
              <div className="flex min-w-0 items-start gap-2">
                <div className="grid h-8 w-8 shrink-0 place-content-center rounded-md border border-border bg-background">
                  <ProviderIcon
                    icon={provider.icon}
                    name={provider.name}
                    color={provider.iconColor}
                    size={18}
                  />
                </div>
                <div className="min-w-0">
                  <div className="flex min-w-0 flex-wrap items-center gap-2">
                    <span className="truncate text-sm font-medium">
                      {provider.meta?.providerVariantLabel ?? provider.name}
                    </span>
                    {isPrimary && (
                      <Badge variant="secondary" className="rounded-md px-1.5">
                        {t("provider.management.groupSource", {
                          defaultValue: "Source",
                        })}
                      </Badge>
                    )}
                  </div>
                  <div className="truncate font-mono text-[11px] text-muted-foreground">
                    {provider.id}
                  </div>
                  <div className="mt-1 flex min-w-0 flex-wrap gap-x-3 gap-y-1 text-xs text-muted-foreground">
                    <span className="max-w-[16rem] truncate">
                      {summary.baseUrlHost ?? summary.baseUrl ?? "-"}
                    </span>
                    {summary.apiKeyFingerprint && (
                      <span className="font-mono">
                        {summary.apiKeyFingerprint}
                      </span>
                    )}
                    {summary.modelSummary && (
                      <span className="max-w-[18rem] truncate">
                        {summary.modelSummary}
                      </span>
                    )}
                  </div>
                </div>
              </div>

              <div className="min-w-0">
                {!isPrimary && sourceFields.length > 0 && (
                  <div className="grid gap-1.5 sm:grid-cols-2">
                    {sourceFields.map((field) => {
                      const checked = activeKeys.includes(field.key);
                      const nextKeys = checked
                        ? activeKeys.filter((key) => key !== field.key)
                        : Array.from(new Set([...activeKeys, field.key]));
                      return (
                        <label
                          key={field.key}
                          className={cn(
                            "flex min-w-0 items-center gap-2 rounded-md border border-border bg-background/70 px-2 py-1.5 text-xs",
                            checked && "border-primary/40 bg-primary/5",
                          )}
                        >
                          <Checkbox
                            checked={checked}
                            onCheckedChange={() =>
                              onApplyGroupCommonConfig(provider, nextKeys)
                            }
                            aria-label={t(
                              "provider.management.useGroupConfigAria",
                              {
                                field: field.label,
                                name: provider.name,
                                defaultValue: `Use group ${field.label} for ${provider.name}`,
                              },
                            )}
                          />
                          <span className="min-w-0">
                            <span className="block truncate font-medium">
                              {field.label}
                            </span>
                            <span className="block truncate text-muted-foreground">
                              {field.displayValue}
                            </span>
                          </span>
                        </label>
                      );
                    })}
                  </div>
                )}
              </div>

              <div className="flex items-center justify-end">
                <ProviderActions
                  appId={appId}
                  isCurrent={state.isCurrent}
                  isInConfig={state.isInConfig}
                  isTesting={isTesting?.(provider.id)}
                  isProxyTakeover={isProxyTakeover}
                  isOmo={state.isOmo || state.isOmoSlim}
                  onSwitch={() => onSwitch(provider)}
                  onEdit={() => onEdit(provider)}
                  onDuplicate={() => onDuplicate(provider)}
                  onTest={onTest ? () => onTest(provider) : undefined}
                  onConfigureUsage={
                    onConfigureUsage
                      ? () => onConfigureUsage(provider)
                      : undefined
                  }
                  onDelete={() => onDelete(provider)}
                  onRemoveFromConfig={
                    onRemoveFromConfig
                      ? () => onRemoveFromConfig(provider)
                      : undefined
                  }
                  onDisableOmo={
                    state.isOmoSlim ? onDisableOmoSlim : onDisableOmo
                  }
                  onOpenTerminal={
                    onOpenTerminal ? () => onOpenTerminal(provider) : undefined
                  }
                  onOpenCodexSessions={
                    appId === "codex" && onOpenCodexSessions
                      ? () => onOpenCodexSessions(provider)
                      : undefined
                  }
                  isAutoFailoverEnabled={isAutoFailoverEnabled}
                  isInFailoverQueue={state.isInFailoverQueue}
                  onToggleFailover={
                    onToggleFailover
                      ? (enabled) => onToggleFailover(provider.id, enabled)
                      : undefined
                  }
                  isOfficialBlockedByProxy={
                    isProxyTakeover && provider.category === "official"
                  }
                  isReadOnly={isReadOnly}
                  isDefaultModel={state.isDefaultModel}
                  onSetAsDefault={
                    onSetAsDefault ? () => onSetAsDefault(provider) : undefined
                  }
                />
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
