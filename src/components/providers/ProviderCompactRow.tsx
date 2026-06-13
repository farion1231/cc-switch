import { ChevronDown, ChevronRight } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { ProviderIcon } from "@/components/ProviderIcon";
import { ProviderActions } from "@/components/providers/ProviderActions";
import type { ProviderSummary } from "@/lib/provider-management/providerSummary";
import { isHermesReadOnlyProvider } from "@/config/hermesProviderPresets";
import { cn } from "@/lib/utils";

interface ProviderCompactRowProps {
  provider: Provider;
  summary: ProviderSummary;
  appId: AppId;
  isCurrent: boolean;
  isInConfig: boolean;
  isSelected: boolean;
  onSelectedChange: (selected: boolean) => void;
  isDrawerOpen?: boolean;
  onToggleDrawer?: () => void;
  groupCount?: number;
  isOmo?: boolean;
  isOmoSlim?: boolean;
  onSwitch: () => void;
  onEdit: () => void;
  onDelete: () => void;
  onDuplicate: () => void;
  onConfigureUsage?: () => void;
  onOpenTerminal?: () => void;
  onOpenCodexSessions?: () => void;
  onTest?: () => void;
  isTesting?: boolean;
  isProxyTakeover?: boolean;
  isAutoFailoverEnabled?: boolean;
  failoverPriority?: number;
  isInFailoverQueue?: boolean;
  onToggleFailover?: (enabled: boolean) => void;
  isDefaultModel?: boolean;
  onSetAsDefault?: () => void;
}

export function ProviderCompactRow({
  provider,
  summary,
  appId,
  isCurrent,
  isInConfig,
  isSelected,
  onSelectedChange,
  isDrawerOpen = false,
  onToggleDrawer,
  groupCount = 1,
  isOmo = false,
  isOmoSlim = false,
  onSwitch,
  onEdit,
  onDelete,
  onDuplicate,
  onConfigureUsage,
  onOpenTerminal,
  onOpenCodexSessions,
  onTest,
  isTesting,
  isProxyTakeover = false,
  isAutoFailoverEnabled = false,
  isInFailoverQueue = false,
  onToggleFailover,
  isDefaultModel,
  onSetAsDefault,
}: ProviderCompactRowProps) {
  const { t } = useTranslation();
  const isGrouped = groupCount > 1;
  const isHermesReadOnly =
    appId === "hermes" && isHermesReadOnlyProvider(provider.settingsConfig);

  return (
    <div
      className={cn(
        "grid min-h-14 grid-cols-[auto_auto_minmax(0,1.5fr)_minmax(0,1fr)_minmax(0,1fr)_auto] items-center gap-3 border-b border-border px-3 py-2 text-sm",
        "bg-card hover:bg-muted/40",
        (isCurrent || isDefaultModel) && "bg-blue-500/5",
      )}
      data-testid={`provider-compact-row-${provider.id}`}
    >
      <Checkbox
        checked={isSelected}
        onCheckedChange={(checked) => onSelectedChange(checked === true)}
        aria-label={t("provider.management.selectProvider", {
          name: provider.name,
          defaultValue: `Select ${provider.name}`,
        })}
      />

      <Button
        type="button"
        variant="ghost"
        size="icon"
        className={cn("h-7 w-7", !onToggleDrawer && "invisible")}
        onClick={onToggleDrawer}
        aria-expanded={isDrawerOpen}
        aria-label={t("provider.management.toggleDrawer", {
          name: provider.name,
          defaultValue: `Toggle ${provider.name} details`,
        })}
      >
        {isDrawerOpen ? (
          <ChevronDown className="h-4 w-4" />
        ) : (
          <ChevronRight className="h-4 w-4" />
        )}
      </Button>

      <div className="flex min-w-0 items-center gap-2">
        <div className="grid h-7 w-7 shrink-0 place-content-center rounded-md border border-border bg-muted">
          <ProviderIcon
            icon={provider.icon}
            name={provider.name}
            color={provider.iconColor}
            size={18}
          />
        </div>
        <div className="min-w-0">
          <div className="flex min-w-0 items-center gap-2">
            <span className="truncate font-medium">{provider.name}</span>
            {isGrouped && (
              <Badge variant="secondary" className="shrink-0 rounded-md px-1.5">
                {groupCount}
              </Badge>
            )}
            {(isCurrent || isDefaultModel) && (
              <Badge variant="outline" className="shrink-0 rounded-md px-1.5">
                {t("provider.inUse", { defaultValue: "In use" })}
              </Badge>
            )}
          </div>
          <div className="truncate text-[11px] text-muted-foreground">
            {provider.id}
          </div>
        </div>
      </div>

      <div className="min-w-0 text-xs">
        <div className="truncate text-muted-foreground">
          {summary.baseUrlHost ?? summary.baseUrl ?? provider.websiteUrl ?? "-"}
        </div>
        {summary.apiKeyFingerprint && (
          <div className="truncate font-mono text-[11px] text-muted-foreground">
            {summary.apiKeyFingerprint}
          </div>
        )}
      </div>

      <div className="min-w-0 truncate text-xs text-muted-foreground">
        {summary.modelSummary ??
          summary.apiFormat ??
          summary.providerType ??
          "-"}
      </div>

      <div className="flex justify-end">
        <ProviderActions
          appId={appId}
          isCurrent={isCurrent}
          isInConfig={isInConfig}
          isTesting={isTesting}
          isProxyTakeover={isProxyTakeover}
          isOfficialBlockedByProxy={
            isProxyTakeover && provider.category === "official"
          }
          isReadOnly={isHermesReadOnly}
          isOmo={isOmo || isOmoSlim}
          onSwitch={onSwitch}
          onEdit={onEdit}
          onDuplicate={onDuplicate}
          onTest={onTest}
          onConfigureUsage={onConfigureUsage}
          onDelete={onDelete}
          onOpenTerminal={onOpenTerminal}
          onOpenCodexSessions={onOpenCodexSessions}
          isAutoFailoverEnabled={isAutoFailoverEnabled}
          isInFailoverQueue={isInFailoverQueue}
          onToggleFailover={onToggleFailover}
          isDefaultModel={isDefaultModel}
          onSetAsDefault={onSetAsDefault}
        />
      </div>
    </div>
  );
}
