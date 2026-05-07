import { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Plus, Trash2, Loader2, Info } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Alert, AlertDescription } from "@/components/ui/alert";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import type { AppId } from "@/lib/api";
import type { SmartRoutingQueueType } from "@/types/proxy";
import {
  useSmartRoutingEnabled,
  useSetSmartRoutingEnabled,
  useSmartRoutingQueue,
  useAvailableProvidersForSmartRouting,
  useAddToSmartRoutingQueue,
  useRemoveFromSmartRoutingQueue,
} from "@/lib/query/failover";
import { ProviderHealthBadge } from "@/components/providers/ProviderHealthBadge";
import { useProviderHealth } from "@/lib/query/failover";

interface SmartRoutingPanelProps {
  disabled?: boolean;
}

export function SmartRoutingPanel({
  disabled = false,
}: SmartRoutingPanelProps) {
  const { t } = useTranslation();

  return (
    <div className="space-y-4">
      <Alert className="border-blue-500/40 bg-blue-500/10">
        <Info className="h-4 w-4" />
        <AlertDescription className="text-sm">
          {t(
            "proxy.smartRouting.info",
            '智能路由将请求分为"主对话"和"其他请求"两类。主对话指用户直接发起的请求，其他请求包括子Agent、Compact、工具续写等。两类请求分别使用独立的故障转移队列，互不影响。',
          )}
        </AlertDescription>
      </Alert>

      <Tabs defaultValue="claude" className="w-full">
        <TabsList className="grid w-full grid-cols-3">
          <TabsTrigger value="claude">Claude</TabsTrigger>
          <TabsTrigger value="codex">Codex</TabsTrigger>
          <TabsTrigger value="gemini">Gemini</TabsTrigger>
        </TabsList>
        {(["claude", "codex", "gemini"] as const).map((appType) => (
          <TabsContent key={appType} value={appType} className="mt-4 space-y-4">
            <SmartRoutingAppConfig appType={appType} disabled={disabled} />
          </TabsContent>
        ))}
      </Tabs>
    </div>
  );
}

interface SmartRoutingAppConfigProps {
  appType: AppId;
  disabled?: boolean;
}

function SmartRoutingAppConfig({
  appType,
  disabled = false,
}: SmartRoutingAppConfigProps) {
  const { t } = useTranslation();
  const { data: isEnabled = false } = useSmartRoutingEnabled(appType);
  const setEnabled = useSetSmartRoutingEnabled();

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between p-4 rounded-lg bg-muted/50 border border-border/50">
        <div className="space-y-0.5">
          <div className="flex items-center gap-2">
            <span className="text-sm font-medium">
              {t("proxy.smartRouting.enable", {
                defaultValue: "启用智能路由",
              })}
            </span>
            {isEnabled && (
              <span className="px-2 py-0.5 text-xs rounded-full bg-emerald-500/20 text-emerald-600 dark:text-emerald-400">
                {t("common.enabled", { defaultValue: "已开启" })}
              </span>
            )}
          </div>
          <p className="text-xs text-muted-foreground">
            {t("proxy.smartRouting.enableDescription", {
              defaultValue:
                "开启后，主对话和其他请求将分别路由到不同的供应商队列",
            })}
          </p>
        </div>
        <Switch
          checked={isEnabled}
          onCheckedChange={(checked) =>
            setEnabled.mutate({ appType, enabled: checked })
          }
          disabled={disabled || setEnabled.isPending}
        />
      </div>

      {isEnabled && (
        <div className="space-y-4">
          <SmartRoutingQueueManager
            appType={appType}
            queueType="main"
            disabled={disabled}
          />
          <div className="border-t border-border/50" />
          <SmartRoutingQueueManager
            appType={appType}
            queueType="others"
            disabled={disabled}
          />
        </div>
      )}
    </div>
  );
}

interface SmartRoutingQueueManagerProps {
  appType: AppId;
  queueType: SmartRoutingQueueType;
  disabled?: boolean;
}

function SmartRoutingQueueManager({
  appType,
  queueType,
  disabled = false,
}: SmartRoutingQueueManagerProps) {
  const { t } = useTranslation();
  const [selectedProviderId, setSelectedProviderId] = useState<string>("");

  const isMain = queueType === "main";

  const { data: queue = [], isLoading } = useSmartRoutingQueue(
    appType,
    queueType,
  );
  const { data: availableProviders = [] } =
    useAvailableProvidersForSmartRouting(appType);
  const addToQueue = useAddToSmartRoutingQueue();
  const removeFromQueue = useRemoveFromSmartRoutingQueue();

  const handleAdd = async () => {
    if (!selectedProviderId) return;
    try {
      await addToQueue.mutateAsync({
        appType,
        providerId: selectedProviderId,
        queueType,
      });
      setSelectedProviderId("");
    } catch (error) {
      toast.error(String(error));
    }
  };

  const handleRemove = async (providerId: string) => {
    try {
      await removeFromQueue.mutateAsync({
        appType,
        providerId,
        queueType,
      });
    } catch (error) {
      toast.error(String(error));
    }
  };

  if (isLoading) {
    return (
      <div className="flex items-center justify-center p-4">
        <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
      </div>
    );
  }

  const title = isMain
    ? t("proxy.smartRouting.mainQueue", { defaultValue: "主对话 Providers" })
    : t("proxy.smartRouting.othersQueue", {
        defaultValue: "子Agent Providers",
      });

  const description = isMain
    ? t("proxy.smartRouting.mainQueueDescription", {
        defaultValue: "用户直接发起的对话请求将按此队列顺序路由",
      })
    : t("proxy.smartRouting.othersQueueDescription", {
        defaultValue: "子Agent、Compact、工具续写等请求将按此队列顺序路由",
      });

  return (
    <div className="space-y-3">
      <div>
        <h4 className="text-sm font-semibold">{title}</h4>
        <p className="text-xs text-muted-foreground">{description}</p>
      </div>

      <div className="flex items-center gap-2">
        <Select
          value={selectedProviderId}
          onValueChange={setSelectedProviderId}
          disabled={disabled || availableProviders.length === 0}
        >
          <SelectTrigger className="flex-1">
            <SelectValue
              placeholder={t("proxy.smartRouting.selectProvider", {
                defaultValue: "选择供应商添加到队列",
              })}
            />
          </SelectTrigger>
          <SelectContent>
            {availableProviders.map((provider) => (
              <SelectItem key={provider.id} value={provider.id}>
                {provider.name}
                {provider.notes && (
                  <span className="ml-1 text-xs text-muted-foreground">
                    ({provider.notes})
                  </span>
                )}
              </SelectItem>
            ))}
            {availableProviders.length === 0 && (
              <div className="px-2 py-4 text-center text-sm text-muted-foreground">
                {t("proxy.smartRouting.noAvailableProviders", {
                  defaultValue: "没有可添加的供应商",
                })}
              </div>
            )}
          </SelectContent>
        </Select>
        <Button
          onClick={handleAdd}
          disabled={disabled || !selectedProviderId || addToQueue.isPending}
          size="icon"
          variant="outline"
        >
          {addToQueue.isPending ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <Plus className="h-4 w-4" />
          )}
        </Button>
      </div>

      {queue.length === 0 ? (
        <div className="rounded-lg border border-dashed border-muted-foreground/40 p-6 text-center">
          <p className="text-sm text-muted-foreground">
            {t("proxy.smartRouting.queueEmpty", {
              defaultValue: "队列为空，将回退到主故障转移队列",
            })}
          </p>
        </div>
      ) : (
        <div className="space-y-1.5">
          {queue.map((item, index) => (
            <SmartRoutingQueueItem
              key={item.providerId}
              provider={item}
              index={index}
              appType={appType}
              onRemove={handleRemove}
              isRemoving={removeFromQueue.isPending}
            />
          ))}
        </div>
      )}
    </div>
  );
}

interface SmartRoutingQueueItemProps {
  provider: {
    providerId: string;
    providerName: string;
    providerNotes?: string;
  };
  index: number;
  appType: string;
  onRemove: (providerId: string) => void;
  isRemoving: boolean;
}

function SmartRoutingQueueItem({
  provider,
  index,
  appType,
  onRemove,
  isRemoving,
}: SmartRoutingQueueItemProps) {
  const { t } = useTranslation();
  const { data: health } = useProviderHealth(provider.providerId, appType);

  return (
    <div className="flex items-center gap-3 rounded-lg border bg-card p-3 transition-colors">
      <div className="flex h-6 w-6 items-center justify-center rounded-full bg-muted text-xs font-medium">
        {index + 1}
      </div>
      <div className="flex-1 min-w-0">
        <span className="text-sm font-medium truncate block">
          {provider.providerName}
          {provider.providerNotes && (
            <span className="ml-1 text-xs text-muted-foreground">
              ({provider.providerNotes})
            </span>
          )}
        </span>
      </div>
      <ProviderHealthBadge
        consecutiveFailures={health?.consecutive_failures ?? 0}
      />
      <Button
        variant="ghost"
        size="icon"
        className="h-8 w-8 text-muted-foreground hover:text-destructive"
        onClick={() => onRemove(provider.providerId)}
        disabled={isRemoving}
        aria-label={t("common.delete", "删除")}
      >
        {isRemoving ? (
          <Loader2 className="h-4 w-4 animate-spin" />
        ) : (
          <Trash2 className="h-4 w-4" />
        )}
      </Button>
    </div>
  );
}
