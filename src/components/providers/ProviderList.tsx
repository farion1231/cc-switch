import {
  useEffect,
  useMemo,
  useRef,
  useState,
  useCallback,
} from "react";
import { AnimatePresence, motion } from "framer-motion";
import { Search, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import { providersApi } from "@/lib/api/providers";
import {
  useOpenClawLiveProviderIds,
  useOpenClawDefaultModel,
} from "@/hooks/useOpenClaw";
import { useStreamCheck } from "@/hooks/useStreamCheck";
import { ProviderCard } from "@/components/providers/ProviderCard";
import { ProviderEmptyState } from "@/components/providers/ProviderEmptyState";
import {
  useAutoFailoverEnabled,
  useFailoverQueue,
  useAddToFailoverQueue,
  useRemoveFromFailoverQueue,
} from "@/lib/query/failover";
import {
  useCurrentOmoProviderId,
  useCurrentOmoSlimProviderId,
} from "@/lib/query/omo";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { settingsApi } from "@/lib/api/settings";

interface ProviderListProps {
  providers: Record<string, Provider>;
  currentProviderId: string;
  appId: AppId;
  onSwitch: (provider: Provider) => void;
  onEdit: (provider: Provider) => void;
  onDelete: (provider: Provider) => void;
  onRemoveFromConfig?: (provider: Provider) => void;
  onDisableOmo?: () => void;
  onDisableOmoSlim?: () => void;
  onDuplicate: (provider: Provider) => void;
  onConfigureUsage?: (provider: Provider) => void;
  onOpenWebsite: (url: string) => void;
  onOpenTerminal?: (provider: Provider) => void;
  onCreate?: () => void;
  isLoading?: boolean;
  isProxyRunning?: boolean;
  isProxyTakeover?: boolean;
  activeProviderId?: string;
  onSetAsDefault?: (provider: Provider) => void;
}

export function ProviderList({
  providers,
  currentProviderId,
  appId,
  onSwitch,
  onEdit,
  onDelete,
  onRemoveFromConfig,
  onDisableOmo,
  onDisableOmoSlim,
  onDuplicate,
  onConfigureUsage,
  onOpenWebsite,
  onOpenTerminal,
  onCreate,
  isLoading = false,
  isProxyRunning = false,
  isProxyTakeover = false,
  activeProviderId,
  onSetAsDefault,
}: ProviderListProps) {
  const { t, i18n } = useTranslation();
  const { checkProvider, isChecking } = useStreamCheck(appId);
  const queryClient = useQueryClient();
  const listRef = useRef<HTMLDivElement>(null);
  const [draggingId, setDraggingId] = useState<string | null>(null);

  // 计算排序后的供应商列表
  const sortedProviders = useMemo(() => {
    const locale = i18n.language === "zh" ? "zh-CN" : "en-US";
    return Object.values(providers).sort((a, b) => {
      if (a.sortIndex !== undefined && b.sortIndex !== undefined) {
        return a.sortIndex - b.sortIndex;
      }
      if (a.sortIndex !== undefined) return -1;
      if (b.sortIndex !== undefined) return 1;

      const timeA = a.createdAt ?? 0;
      const timeB = b.createdAt ?? 0;
      if (timeA && timeB && timeA !== timeB) {
        return timeA - timeB;
      }

      return a.name.localeCompare(b.name, locale);
    });
  }, [providers, i18n.language]);

  const { data: opencodeLiveIds } = useQuery({
    queryKey: ["opencodeLiveProviderIds"],
    queryFn: () => providersApi.getOpenCodeLiveProviderIds(),
    enabled: appId === "opencode",
  });

  const { data: openclawLiveIds } = useOpenClawLiveProviderIds(
    appId === "openclaw",
  );

  const isProviderInConfig = useCallback(
    (providerId: string): boolean => {
      if (appId === "opencode") {
        return opencodeLiveIds?.includes(providerId) ?? false;
      }
      if (appId === "openclaw") {
        return openclawLiveIds?.includes(providerId) ?? false;
      }
      return true;
    },
    [appId, opencodeLiveIds, openclawLiveIds],
  );

  const { data: openclawDefaultModel } = useOpenClawDefaultModel(
    appId === "openclaw",
  );

  const isProviderDefaultModel = useCallback(
    (providerId: string): boolean => {
      if (appId !== "openclaw" || !openclawDefaultModel?.primary) return false;
      return openclawDefaultModel.primary.startsWith(providerId + "/");
    },
    [appId, openclawDefaultModel],
  );

  // 故障转移相关
  const { data: isAutoFailoverEnabled } = useAutoFailoverEnabled(appId);
  const { data: failoverQueue } = useFailoverQueue(appId);
  const addToQueue = useAddToFailoverQueue();
  const removeFromQueue = useRemoveFromFailoverQueue();

  const isFailoverModeActive =
    isProxyTakeover === true && isAutoFailoverEnabled === true;

  const isOpenCode = appId === "opencode";
  const { data: currentOmoId } = useCurrentOmoProviderId(isOpenCode);
  const { data: currentOmoSlimId } = useCurrentOmoSlimProviderId(isOpenCode);

  const getFailoverPriority = useCallback(
    (providerId: string): number | undefined => {
      if (!isFailoverModeActive || !failoverQueue) return undefined;
      const index = failoverQueue.findIndex(
        (item) => item.providerId === providerId,
      );
      return index >= 0 ? index + 1 : undefined;
    },
    [isFailoverModeActive, failoverQueue],
  );

  const isInFailoverQueue = useCallback(
    (providerId: string): boolean => {
      if (!isFailoverModeActive || !failoverQueue) return false;
      return failoverQueue.some((item) => item.providerId === providerId);
    },
    [isFailoverModeActive, failoverQueue],
  );

  const handleToggleFailover = useCallback(
    (providerId: string, enabled: boolean) => {
      if (enabled) {
        addToQueue.mutate({ appType: appId, providerId });
      } else {
        removeFromQueue.mutate({ appType: appId, providerId });
      }
    },
    [appId, addToQueue, removeFromQueue],
  );

  const [searchTerm, setSearchTerm] = useState("");
  const [isSearchOpen, setIsSearchOpen] = useState(false);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const [showStreamCheckConfirm, setShowStreamCheckConfirm] = useState(false);
  const [pendingTestProvider, setPendingTestProvider] =
    useState<Provider | null>(null);

  const { data: settings } = useQuery({
    queryKey: ["settings"],
    queryFn: () => settingsApi.get(),
  });

  const handleTest = useCallback(
    (provider: Provider) => {
      if (!settings?.streamCheckConfirmed) {
        setPendingTestProvider(provider);
        setShowStreamCheckConfirm(true);
      } else {
        checkProvider(provider.id, provider.name);
      }
    },
    [checkProvider, settings?.streamCheckConfirmed],
  );

  const handleStreamCheckConfirm = async () => {
    setShowStreamCheckConfirm(false);
    try {
      if (settings) {
        const { webdavSync: _, ...rest } = settings;
        await settingsApi.save({ ...rest, streamCheckConfirmed: true });
        await queryClient.invalidateQueries({ queryKey: ["settings"] });
      }
    } catch (error) {
      console.error("Failed to save stream check confirmed:", error);
    }
    if (pendingTestProvider) {
      checkProvider(pendingTestProvider.id, pendingTestProvider.name);
      setPendingTestProvider(null);
    }
  };

  const importMutation = useMutation({
    mutationFn: async (): Promise<boolean> => {
      if (appId === "opencode") {
        const count = await providersApi.importOpenCodeFromLive();
        return count > 0;
      }
      if (appId === "openclaw") {
        const count = await providersApi.importOpenClawFromLive();
        return count > 0;
      }
      return providersApi.importDefault(appId);
    },
    onSuccess: (imported) => {
      if (imported) {
        queryClient.invalidateQueries({ queryKey: ["providers", appId] });
        toast.success(t("provider.importCurrentDescription"));
      } else {
        toast.info(t("provider.noProviders"));
      }
    },
    onError: (error: Error) => {
      toast.error(error.message);
    },
  });

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const key = event.key.toLowerCase();
      if ((event.metaKey || event.ctrlKey) && key === "f") {
        event.preventDefault();
        setIsSearchOpen(true);
        return;
      }

      if (key === "escape") {
        setIsSearchOpen(false);
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);

  useEffect(() => {
    if (isSearchOpen) {
      const frame = requestAnimationFrame(() => {
        searchInputRef.current?.focus();
        searchInputRef.current?.select();
      });
      return () => cancelAnimationFrame(frame);
    }
  }, [isSearchOpen]);

  const filteredProviders = useMemo(() => {
    const keyword = searchTerm.trim().toLowerCase();
    if (!keyword) return sortedProviders;
    return sortedProviders.filter((provider) => {
      const fields = [provider.name, provider.notes, provider.websiteUrl];
      return fields.some((field) =>
        field?.toString().toLowerCase().includes(keyword),
      );
    });
  }, [searchTerm, sortedProviders]);

  // 核心计算函数：根据鼠标的 Y 坐标，计算应该把拖拽的元素放在哪个元素前面
  const getDragAfterElement = useCallback(
    (container: HTMLDivElement, y: number): Element | null => {
      const draggableElements = [
        ...container.querySelectorAll(
          '[data-provider-id]:not([data-dragging="true"])',
        ),
      ];

      return draggableElements.reduce<
        { offset: number; element: Element | null }
      >(
        (closest, child) => {
          const box = child.getBoundingClientRect();
          const offset = y - box.top - box.height / 2;

          if (offset < 0 && offset > closest.offset) {
            return { offset: offset, element: child };
          } else {
            return closest;
          }
        },
        { offset: Number.NEGATIVE_INFINITY, element: null },
      ).element;
    },
    [],
  );

  // 处理拖拽经过容器 - 实时重排 DOM
  const handleDragOver = useCallback(
    (e: React.DragEvent<HTMLDivElement>) => {
      e.preventDefault();
      e.dataTransfer.dropEffect = "move";

      const container = listRef.current;
      if (!container) return;

      const draggingElement = container.querySelector('[data-dragging="true"]');
      if (!draggingElement) return;

      const afterElement = getDragAfterElement(container, e.clientY);

      if (afterElement == null) {
        container.appendChild(draggingElement);
      } else {
        container.insertBefore(draggingElement, afterElement);
      }
    },
    [getDragAfterElement],
  );

  // 处理拖拽进入容器
  const handleDragEnter = useCallback((e: React.DragEvent<HTMLDivElement>) => {
    e.preventDefault();
  }, []);

  // 处理放置 - 从 DOM 读取最终顺序并同步状态
  const handleDrop = useCallback(
    async (e: React.DragEvent<HTMLDivElement>) => {
      e.preventDefault();

      const container = listRef.current;
      if (!container) {
        setDraggingId(null);
        return;
      }

      // 从 DOM 读取当前顺序
      const providerElements = container.querySelectorAll("[data-provider-id]");
      const newOrder: string[] = [];

      providerElements.forEach((el) => {
        const id = (el as HTMLElement).dataset.providerId;
        if (id) {
          newOrder.push(id);
        }
      });

      // 准备更新数据
      const updates = newOrder.map((id, index) => ({
        id,
        sortIndex: index,
      }));

      try {
        await providersApi.updateSortOrder(updates, appId);
        await queryClient.invalidateQueries({
          queryKey: ["providers", appId],
        });
        await queryClient.invalidateQueries({
          queryKey: ["failoverQueue", appId],
        });

        // 更新托盘菜单
        try {
          await providersApi.updateTrayMenu();
        } catch (trayError) {
          console.error("Failed to update tray menu after sort", trayError);
        }

        toast.success(
          t("provider.sortUpdated", {
            defaultValue: "排序已更新",
          }),
          { closeButton: true },
        );
      } catch (error) {
        console.error("Failed to update provider sort order", error);
        toast.error(
          t("provider.sortUpdateFailed", {
            defaultValue: "排序更新失败",
          }),
        );
      }

      setDraggingId(null);
    },
    [appId, queryClient, t],
  );

  // 处理拖拽开始
  const handleDragStart = useCallback((providerId: string) => {
    setDraggingId(providerId);
  }, []);

  // 处理拖拽结束
  const handleDragEnd = useCallback(() => {
    setTimeout(() => {
      setDraggingId((current) => {
        if (current !== null) {
          return null;
        }
        return current;
      });
    }, 0);
  }, []);

  if (isLoading) {
    return (
      <div className="space-y-3">
        {[0, 1, 2].map((index) => (
          <div
            key={index}
            className="w-full border border-dashed rounded-lg h-28 border-muted-foreground/40 bg-muted/40"
          />
        ))}
      </div>
    );
  }

  if (sortedProviders.length === 0) {
    return (
      <ProviderEmptyState
        appId={appId}
        onCreate={onCreate}
        onImport={() => importMutation.mutate()}
      />
    );
  }

  return (
    <div className="mt-4 space-y-4">
      <AnimatePresence>
        {isSearchOpen && (
          <motion.div
            key="provider-search"
            initial={{ opacity: 0, y: -8, scale: 0.98 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: -8, scale: 0.98 }}
            transition={{ duration: 0.18, ease: "easeOut" }}
            className="fixed left-1/2 top-[6.5rem] z-40 w-[min(90vw,26rem)] -translate-x-1/2 sm:right-6 sm:left-auto sm:translate-x-0"
          >
            <div className="p-4 space-y-3 border shadow-md rounded-2xl border-white/10 bg-background/95 shadow-black/20 backdrop-blur-md">
              <div className="relative flex items-center gap-2">
                <Search className="absolute w-4 h-4 -translate-y-1/2 pointer-events-none left-3 top-1/2 text-muted-foreground" />
                <Input
                  ref={searchInputRef}
                  value={searchTerm}
                  onChange={(event) => setSearchTerm(event.target.value)}
                  placeholder={t("provider.searchPlaceholder", {
                    defaultValue: "Search name, notes, or URL...",
                  })}
                  aria-label={t("provider.searchAriaLabel", {
                    defaultValue: "Search providers",
                  })}
                  className="pr-16 pl-9"
                />
                {searchTerm && (
                  <Button
                    variant="ghost"
                    size="sm"
                    className="absolute text-xs -translate-y-1/2 right-11 top-1/2"
                    onClick={() => setSearchTerm("")}
                  >
                    {t("common.clear", { defaultValue: "Clear" })}
                  </Button>
                )}
                <Button
                  variant="ghost"
                  size="icon"
                  className="ml-auto"
                  onClick={() => setIsSearchOpen(false)}
                  aria-label={t("provider.searchCloseAriaLabel", {
                    defaultValue: "Close provider search",
                  })}
                >
                  <X className="w-4 h-4" />
                </Button>
              </div>
              <div className="flex flex-wrap items-center justify-between gap-2 text-[11px] text-muted-foreground">
                <span>
                  {t("provider.searchScopeHint", {
                    defaultValue: "Matches provider name, notes, and URL.",
                  })}
                </span>
                <span>
                  {t("provider.searchCloseHint", {
                    defaultValue: "Press Esc to close",
                  })}
                </span>
              </div>
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      {filteredProviders.length === 0 ? (
        <div className="px-6 py-8 text-sm text-center border border-dashed rounded-lg border-border text-muted-foreground">
          {t("provider.noSearchResults", {
            defaultValue: "No providers match your search.",
          })}
        </div>
      ) : (
        <div
          ref={listRef}
          className="space-y-3 relative"
          onDragOver={handleDragOver}
          onDragEnter={handleDragEnter}
          onDrop={handleDrop}
        >
          {filteredProviders.map((provider) => {
            const isOmo = provider.category === "omo";
            const isOmoSlim = provider.category === "omo-slim";
            const isOmoCurrent = isOmo && provider.id === (currentOmoId || "");
            const isOmoSlimCurrent =
              isOmoSlim && provider.id === (currentOmoSlimId || "");
            return (
              <div
                key={provider.id}
                data-provider-id={provider.id}
                data-dragging={draggingId === provider.id}
              >
                <ProviderCard
                  provider={provider}
                  isCurrent={
                    isOmo
                      ? isOmoCurrent
                      : isOmoSlim
                        ? isOmoSlimCurrent
                        : provider.id === currentProviderId
                  }
                  appId={appId}
                  isInConfig={isProviderInConfig(provider.id)}
                  isOmo={isOmo}
                  isOmoSlim={isOmoSlim}
                  onSwitch={onSwitch}
                  onEdit={onEdit}
                  onDelete={onDelete}
                  onRemoveFromConfig={onRemoveFromConfig}
                  onDisableOmo={onDisableOmo}
                  onDisableOmoSlim={onDisableOmoSlim}
                  onDuplicate={onDuplicate}
                  onConfigureUsage={onConfigureUsage}
                  onOpenWebsite={onOpenWebsite}
                  onOpenTerminal={onOpenTerminal}
                  onTest={
                    appId !== "opencode" && appId !== "openclaw"
                      ? handleTest
                      : undefined
                  }
                  isTesting={isChecking(provider.id)}
                  isProxyRunning={isProxyRunning}
                  isProxyTakeover={isProxyTakeover}
                  isAutoFailoverEnabled={isFailoverModeActive}
                  failoverPriority={getFailoverPriority(provider.id)}
                  isInFailoverQueue={isInFailoverQueue(provider.id)}
                  onToggleFailover={(enabled) =>
                    handleToggleFailover(provider.id, enabled)
                  }
                  activeProviderId={activeProviderId}
                  isDefaultModel={isProviderDefaultModel(provider.id)}
                  onSetAsDefault={
                    onSetAsDefault ? () => onSetAsDefault(provider) : undefined
                  }
                  isDragging={draggingId === provider.id}
                  onDragStart={() => handleDragStart(provider.id)}
                  onDragEnd={handleDragEnd}
                />
              </div>
            );
          })}
        </div>
      )}

      <ConfirmDialog
        isOpen={showStreamCheckConfirm}
        variant="info"
        title={t("confirm.streamCheck.title")}
        message={t("confirm.streamCheck.message")}
        confirmText={t("confirm.streamCheck.confirm")}
        onConfirm={() => void handleStreamCheckConfirm()}
        onCancel={() => {
          setShowStreamCheckConfirm(false);
          setPendingTestProvider(null);
        }}
      />
    </div>
  );
}
