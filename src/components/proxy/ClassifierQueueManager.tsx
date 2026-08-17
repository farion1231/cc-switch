/**
 * 分类器队列管理组件
 *
 * Claude Code Auto Mode 在执行 Bash 命令前会先发一条「安全分类器」请求，
 * 客户端对它有硬超时。本组件让用户把响应快的供应商放进专用队列，
 * 并可选在发送前强制关闭思考。
 *
 * - 添加/移除供应商
 * - 队列顺序基于首页供应商列表的 sort_index（与故障转移队列同源）
 */

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Plus, Trash2, Loader2, Info, AlertTriangle } from "lucide-react";
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
import { cn } from "@/lib/utils";
import type { ClassifierQueueItem } from "@/types/proxy";
import type { ProxyAppId } from "@/config/appConfig";
import {
  useClassifierQueue,
  useAvailableProvidersForClassifier,
  useAddToClassifierQueue,
  useRemoveFromClassifierQueue,
  useClassifierConfig,
  useSetClassifierConfig,
} from "@/lib/query/classifier";

interface ClassifierQueueManagerProps {
  appType: ProxyAppId;
  disabled?: boolean;
}

export function ClassifierQueueManager({
  appType,
  disabled = false,
}: ClassifierQueueManagerProps) {
  const { t } = useTranslation();
  const [selectedProviderId, setSelectedProviderId] = useState<string>("");

  const { data: config } = useClassifierConfig(appType);
  const isEnabled = config?.enabled ?? false;
  const forceThinkingOff = config?.forceThinkingOff ?? true;
  const setConfig = useSetClassifierConfig();

  const {
    data: queue,
    isLoading: isQueueLoading,
    error: queueError,
  } = useClassifierQueue(appType);
  const { data: availableProviders, isLoading: isProvidersLoading } =
    useAvailableProvidersForClassifier(appType);

  const addToQueue = useAddToClassifierQueue();
  const removeFromQueue = useRemoveFromClassifierQueue();

  const handleToggleEnabled = async (enabled: boolean) => {
    try {
      await setConfig.mutateAsync({
        appType,
        config: { enabled, forceThinkingOff },
      });
      toast.success(
        enabled
          ? t("classifier.enabled", "分类器队列已启用")
          : t("classifier.disabled", "分类器队列已关闭"),
        { closeButton: true },
      );
    } catch (error) {
      toast.error(
        t("classifier.toggleFailed", "操作失败") + ": " + String(error),
      );
    }
  };

  const handleToggleThinkingOff = async (nextForceThinkingOff: boolean) => {
    try {
      await setConfig.mutateAsync({
        appType,
        config: { enabled: isEnabled, forceThinkingOff: nextForceThinkingOff },
      });
    } catch (error) {
      toast.error(
        t("classifier.toggleFailed", "操作失败") + ": " + String(error),
      );
    }
  };

  const handleAddProvider = async () => {
    if (!selectedProviderId) return;

    try {
      await addToQueue.mutateAsync({
        appType,
        providerId: selectedProviderId,
      });
      setSelectedProviderId("");
      toast.success(
        t("proxy.classifierQueue.addSuccess", "已添加到分类器队列"),
        {
          closeButton: true,
        },
      );
    } catch (error) {
      toast.error(
        t("proxy.classifierQueue.addFailed", "添加失败") + ": " + String(error),
      );
    }
  };

  const handleRemoveProvider = async (providerId: string) => {
    try {
      await removeFromQueue.mutateAsync({ appType, providerId });
      toast.success(
        t("proxy.classifierQueue.removeSuccess", "已从分类器队列移除"),
        { closeButton: true },
      );
    } catch (error) {
      toast.error(
        t("proxy.classifierQueue.removeFailed", "移除失败") +
          ": " +
          String(error),
      );
    }
  };

  if (isQueueLoading) {
    return (
      <div className="flex items-center justify-center p-8">
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
      </div>
    );
  }

  if (queueError) {
    return (
      <Alert variant="destructive">
        <AlertTriangle className="h-4 w-4" />
        <AlertDescription>{String(queueError)}</AlertDescription>
      </Alert>
    );
  }

  return (
    <div className="space-y-4">
      {/* 两个开关：主开关 + 其下从属的「强制关闭思考」 */}
      <div className="rounded-lg bg-muted/50 border border-border/50">
        <div className="flex items-center justify-between p-4">
          <div className="space-y-0.5">
            <div className="flex items-center gap-2">
              <span className="text-sm font-medium">
                {t("proxy.classifier.enable", {
                  defaultValue: "启用分类器队列",
                })}
              </span>
              {isEnabled && (
                <span className="px-2 py-0.5 text-xs rounded-full bg-emerald-500/20 text-emerald-600 dark:text-emerald-400">
                  {t("common.enabled", { defaultValue: "已开启" })}
                </span>
              )}
            </div>
            <p className="text-xs text-muted-foreground">
              {t("proxy.classifier.enableDescription", {
                defaultValue:
                  "开启后，Auto Mode 的安全分类器请求将按队列顺序发往专用供应商；队列为空或全部熔断时自动回落到常规路由链，不会报错。",
              })}
            </p>
          </div>
          <Switch
            checked={isEnabled}
            onCheckedChange={handleToggleEnabled}
            disabled={disabled || setConfig.isPending}
            aria-label={t("proxy.classifier.enable", {
              defaultValue: "启用分类器队列",
            })}
          />
        </div>

        <div className="flex items-center justify-between border-t border-border/50 p-4 pl-10">
          <div className="space-y-0.5">
            <span className="text-sm font-medium">
              {t("proxy.classifier.forceThinkingOff", {
                defaultValue: "强制关闭思考",
              })}
            </span>
            <p className="text-xs text-muted-foreground">
              {t("proxy.classifier.forceThinkingOffDescription", {
                defaultValue:
                  "分类器请求发出前移除 thinking / reasoning_effort / output_config，避免思考往返撞上客户端的超时上限。",
              })}
            </p>
          </div>
          <Switch
            checked={forceThinkingOff}
            onCheckedChange={handleToggleThinkingOff}
            disabled={disabled || !isEnabled || setConfig.isPending}
            aria-label={t("proxy.classifier.forceThinkingOff", {
              defaultValue: "强制关闭思考",
            })}
          />
        </div>
      </div>

      {/* 说明信息 */}
      <Alert className="border-blue-500/40 bg-blue-500/10">
        <Info className="h-4 w-4" />
        <AlertDescription className="text-sm">
          {t(
            "proxy.classifierQueue.info",
            "Claude Code 在执行 Bash 命令前会先发一条安全分类器请求，客户端对它有硬超时，超时即判定分类器不可用并拦下该命令。把响应快、价格低的供应商放进此队列，可避免因思考往返而超时。",
          )}
        </AlertDescription>
      </Alert>

      {/* 添加供应商 */}
      <div className="flex items-center gap-2">
        <Select
          value={selectedProviderId}
          onValueChange={setSelectedProviderId}
          disabled={disabled || isProvidersLoading}
        >
          <SelectTrigger className="flex-1">
            <SelectValue
              placeholder={t(
                "proxy.classifierQueue.selectProvider",
                "选择供应商添加到队列",
              )}
            />
          </SelectTrigger>
          <SelectContent>
            {availableProviders?.map((provider) => (
              <SelectItem key={provider.id} value={provider.id}>
                {provider.name}
                {provider.notes && (
                  <span className="ml-1 text-xs text-muted-foreground">
                    ({provider.notes})
                  </span>
                )}
              </SelectItem>
            ))}
            {(!availableProviders || availableProviders.length === 0) && (
              <div className="px-2 py-4 text-center text-sm text-muted-foreground">
                {t(
                  "proxy.classifierQueue.noAvailableProviders",
                  "没有可添加的供应商",
                )}
              </div>
            )}
          </SelectContent>
        </Select>
        <Button
          onClick={handleAddProvider}
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

      {/* 队列列表 */}
      {!queue || queue.length === 0 ? (
        <div className="rounded-lg border border-dashed border-muted-foreground/40 p-8 text-center">
          <p className="text-sm text-muted-foreground">
            {t(
              "proxy.classifierQueue.empty",
              "分类器队列为空。未配置时分类器请求走常规路由链。",
            )}
          </p>
        </div>
      ) : (
        <div className="space-y-2">
          {queue.map((item, index) => (
            <QueueItem
              key={item.providerId}
              item={item}
              index={index}
              disabled={disabled}
              onRemove={handleRemoveProvider}
              isRemoving={removeFromQueue.isPending}
            />
          ))}
        </div>
      )}

      {/* 队列说明 */}
      {queue && queue.length > 0 && (
        <div className="space-y-1">
          <p className="text-xs text-muted-foreground">
            {t(
              "proxy.classifierQueue.orderHint",
              "队列顺序与首页供应商列表顺序一致，可在首页拖拽调整顺序。",
            )}
          </p>
          <p className="text-xs text-muted-foreground">
            {t(
              "proxy.classifierQueue.modelHint",
              "请确保队列中的供应商支持客户端请求的模型；分类器请求为非流式且携带完整会话上下文（数万 token 起步），上下文窗口过小的模型会失败并顺延到下一家。",
            )}
          </p>
        </div>
      )}
    </div>
  );
}

interface QueueItemProps {
  item: ClassifierQueueItem;
  index: number;
  disabled: boolean;
  onRemove: (providerId: string) => void;
  isRemoving: boolean;
}

function QueueItem({
  item,
  index,
  disabled,
  onRemove,
  isRemoving,
}: QueueItemProps) {
  const { t } = useTranslation();

  return (
    <div
      className={cn(
        "flex items-center gap-3 rounded-lg border bg-card p-3 transition-colors",
      )}
    >
      {/* 序号 */}
      <div className="flex h-6 w-6 items-center justify-center rounded-full bg-muted text-xs font-medium">
        {index + 1}
      </div>

      {/* 供应商名称 */}
      <div className="flex-1 min-w-0">
        <span className="text-sm font-medium truncate block">
          {item.providerName}
          {item.providerNotes && (
            <span className="ml-1 text-xs text-muted-foreground">
              ({item.providerNotes})
            </span>
          )}
        </span>
      </div>

      {/* 删除按钮 */}
      <Button
        variant="ghost"
        size="icon"
        className="h-8 w-8 text-muted-foreground hover:text-destructive"
        onClick={() => onRemove(item.providerId)}
        disabled={disabled || isRemoving}
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
