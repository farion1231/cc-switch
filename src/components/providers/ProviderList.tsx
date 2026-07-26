import { CSS } from "@dnd-kit/utilities";
import {
  DndContext,
  closestCenter,
  pointerWithin,
  useDroppable,
  type CollisionDetection,
  type DragCancelEvent,
  type DragEndEvent,
  type DragOverEvent,
  type DragStartEvent,
} from "@dnd-kit/core";
import {
  arrayMove,
  SortableContext,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";
import { AnimatePresence, motion } from "framer-motion";
import {
  Activity,
  AlertTriangle,
  ChevronDown,
  CheckSquare,
  Folder,
  FolderPlus,
  GripVertical,
  LoaderCircle,
  MoreHorizontal,
  Search,
  TestTube2,
  Trash2,
  Ungroup,
  X,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import { cn } from "@/lib/utils";
import { providersApi } from "@/lib/api/providers";
import { useDragSort } from "@/hooks/useDragSort";
import {
  useOpenClawLiveProviderIds,
  useOpenClawDefaultModel,
} from "@/hooks/useOpenClaw";
import {
  useHermesLiveProviderIds,
  useHermesModelConfig,
} from "@/hooks/useHermes";
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
import { Checkbox } from "@/components/ui/checkbox";
import { Badge } from "@/components/ui/badge";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { isTextEditableTarget } from "@/utils/domUtils";
import {
  modelTestProvider,
  type StreamCheckResult,
} from "@/lib/api/connectivity-check";
import { extractCodexBaseUrl } from "@/utils/providerConfigUtils";

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
  isProxyRunning?: boolean; // 代理服务运行状态
  isProxyTakeover?: boolean; // 代理接管模式（Live配置已被接管）
  activeProviderId?: string; // 代理当前实际使用的供应商 ID（用于故障转移模式下标注绿色边框）
  onSetAsDefault?: (provider: Provider) => void; // OpenClaw: set as default model
}

interface ProviderGroupView {
  id: string;
  name: string;
  sortIndex: number;
  providers: Provider[];
}

const PROVIDER_GROUP_DND_PREFIX = "provider-group:";
const PROVIDER_GROUP_DROP_DND_PREFIX = "provider-group-drop:";

interface ProviderSelectionSwipe {
  pointerId: number;
  checked: boolean;
}

const isWithinGroupMemberDropZone = (
  activeRect: Pick<ClientRect, "top" | "height"> | null,
  overRect: Pick<ClientRect, "top" | "height"> | null,
) => {
  if (!activeRect || !overRect) return true;

  const activeCenter = activeRect.top + activeRect.height / 2;
  const middleStart = overRect.top + overRect.height * 0.25;
  const middleEnd = overRect.top + overRect.height * 0.75;
  return activeCenter > middleStart && activeCenter < middleEnd;
};

type ProviderListItem =
  | {
      kind: "group";
      id: string;
      group: ProviderGroupView;
    }
  | {
      kind: "provider";
      id: string;
      provider: Provider;
    };

interface GroupModelTestSummary {
  total: number;
  completed: number;
  operational: number;
  degraded: number;
  failed: number;
}

const getProviderGroup = (provider: Provider) => {
  const groupId = provider.meta?.providerGroupId?.trim();
  const groupName = provider.meta?.providerGroupName?.trim();
  if (!groupId || !groupName) return null;

  return {
    id: groupId,
    name: groupName,
    sortIndex: provider.meta?.providerGroupSortIndex ?? Number.MAX_SAFE_INTEGER,
  };
};

const getProviderDisplayUrl = (provider: Provider): string | undefined => {
  const notes = provider.notes?.trim();
  if (notes && /^https?:\/\//i.test(notes)) return notes;
  if (provider.websiteUrl?.trim()) return provider.websiteUrl.trim();

  const config = provider.settingsConfig as Record<string, unknown>;
  const env = config?.env as Record<string, unknown> | undefined;
  const envBaseUrl = env?.ANTHROPIC_BASE_URL ?? env?.GOOGLE_GEMINI_BASE_URL;
  if (typeof envBaseUrl === "string" && envBaseUrl.trim()) {
    return envBaseUrl.trim();
  }

  const codexConfig = config?.config;
  if (typeof codexConfig === "string" && codexConfig.includes("base_url")) {
    return extractCodexBaseUrl(codexConfig);
  }

  return undefined;
};

const buildProviderListItems = (
  orderedProviders: Provider[],
  groups: ProviderGroupView[],
): ProviderListItem[] => {
  const groupsById = new Map(groups.map((group) => [group.id, group]));
  const emittedGroupIds = new Set<string>();
  const items: ProviderListItem[] = [];

  orderedProviders.forEach((provider) => {
    const providerGroup = getProviderGroup(provider);
    const group = providerGroup ? groupsById.get(providerGroup.id) : undefined;

    if (group) {
      if (!emittedGroupIds.has(group.id)) {
        emittedGroupIds.add(group.id);
        items.push({
          kind: "group",
          id: `${PROVIDER_GROUP_DND_PREFIX}${group.id}`,
          group,
        });
      }
      return;
    }

    items.push({ kind: "provider", id: provider.id, provider });
  });

  return items;
};

const createProviderGroupId = (name: string) => {
  const slug = name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9\u4e00-\u9fa5]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 36);
  return `group-${slug || "custom"}-${Date.now().toString(36)}`;
};

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
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { checkProvider, isChecking } = useStreamCheck(appId);
  const [modelTestingProviderId, setModelTestingProviderId] = useState<
    string | null
  >(null);
  const [modelTestResults, setModelTestResults] = useState<
    Record<string, StreamCheckResult>
  >({});
  const [groupTestingId, setGroupTestingId] = useState<string | null>(null);
  const [groupModelTestSummaries, setGroupModelTestSummaries] = useState<
    Record<string, GroupModelTestSummary>
  >({});
  const { sortedProviders, sensors, handleDragEnd } = useDragSort(
    providers,
    appId,
  );
  const [isGroupManageMode, setIsGroupManageMode] = useState(false);
  const [selectedProviderIds, setSelectedProviderIds] = useState<Set<string>>(
    () => new Set(),
  );
  const [expandedProviderGroupIds, setExpandedProviderGroupIds] = useState<
    Set<string>
  >(() => new Set());
  const [draggedProviderId, setDraggedProviderId] = useState<string | null>(
    null,
  );
  const [providerGroupDropTargetId, setProviderGroupDropTargetId] = useState<
    string | null
  >(null);
  const providerSelectionSwipeRef = useRef<ProviderSelectionSwipe | null>(null);
  const groupControlsRef = useRef<HTMLDivElement>(null);
  const [groupControlsHeight, setGroupControlsHeight] = useState(0);

  const { data: opencodeLiveIds } = useQuery({
    queryKey: ["opencodeLiveProviderIds"],
    queryFn: () => providersApi.getOpenCodeLiveProviderIds(),
    enabled: appId === "opencode",
  });

  // OpenClaw: 查询 live 配置中的供应商 ID 列表，用于判断 isInConfig
  const { data: openclawLiveIds } = useOpenClawLiveProviderIds(
    appId === "openclaw",
  );

  // Hermes: 查询 live 配置中的供应商 ID 列表，用于判断 isInConfig
  const { data: hermesLiveIds } = useHermesLiveProviderIds(appId === "hermes");

  // Hermes: 读取当前 model.provider，用于判断哪个供应商是"当前激活"（高亮）
  const { data: hermesModelConfig } = useHermesModelConfig(appId === "hermes");
  const hermesCurrentProviderId = hermesModelConfig?.provider;

  // 判断供应商是否已添加到配置（累加模式应用：OpenCode/OpenClaw/Hermes）
  const isProviderInConfig = useCallback(
    (providerId: string): boolean => {
      if (appId === "opencode") {
        return opencodeLiveIds?.includes(providerId) ?? false;
      }
      if (appId === "openclaw") {
        return openclawLiveIds?.includes(providerId) ?? false;
      }
      if (appId === "hermes") {
        return hermesLiveIds?.includes(providerId) ?? false;
      }
      return true; // 其他应用始终返回 true
    },
    [appId, opencodeLiveIds, openclawLiveIds, hermesLiveIds],
  );

  // OpenClaw: query default model to determine which provider is default
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
  const [groupFailoverActionId, setGroupFailoverActionId] = useState<
    string | null
  >(null);

  const isFailoverModeActive =
    isProxyTakeover === true && isAutoFailoverEnabled === true;
  const failoverQueueProviderIds = useMemo(
    () => new Set((failoverQueue ?? []).map((item) => item.providerId)),
    [failoverQueue],
  );

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
      return failoverQueueProviderIds.has(providerId);
    },
    [isFailoverModeActive, failoverQueue, failoverQueueProviderIds],
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

  const addGroupToFailoverQueue = useCallback(
    async (group: ProviderGroupView) => {
      if (!isFailoverModeActive || groupFailoverActionId) return;

      const targets = group.providers.filter(
        (provider) => !failoverQueueProviderIds.has(provider.id),
      );
      if (targets.length === 0) {
        toast.info(
          t("provider.groupFailoverAllQueued", {
            defaultValue: "分组内供应商已全部在故障转移队列中",
          }),
        );
        return;
      }

      setGroupFailoverActionId(group.id);
      try {
        for (const provider of targets) {
          await addToQueue.mutateAsync({
            appType: appId,
            providerId: provider.id,
          });
        }
        toast.success(
          t("provider.groupFailoverAddSuccess", {
            defaultValue: "已将 {{count}} 个分组供应商加入故障转移队列",
            count: targets.length,
          }),
          { closeButton: true },
        );
      } catch (error) {
        console.error("Failed to add provider group to failover queue", error);
        toast.error(
          t("provider.groupFailoverAddFailed", {
            defaultValue: "分组加入故障转移队列失败",
          }),
        );
      } finally {
        setGroupFailoverActionId((current) =>
          current === group.id ? null : current,
        );
      }
    },
    [
      addToQueue,
      appId,
      failoverQueueProviderIds,
      groupFailoverActionId,
      isFailoverModeActive,
      t,
    ],
  );

  const removeGroupFromFailoverQueue = useCallback(
    async (group: ProviderGroupView) => {
      if (!isFailoverModeActive || groupFailoverActionId) return;

      const targets = group.providers.filter((provider) =>
        failoverQueueProviderIds.has(provider.id),
      );
      if (targets.length === 0) {
        toast.info(
          t("provider.groupFailoverNoneQueued", {
            defaultValue: "分组内供应商不在故障转移队列中",
          }),
        );
        return;
      }

      setGroupFailoverActionId(group.id);
      try {
        for (const provider of targets) {
          await removeFromQueue.mutateAsync({
            appType: appId,
            providerId: provider.id,
          });
        }
        toast.success(
          t("provider.groupFailoverRemoveSuccess", {
            defaultValue: "已将 {{count}} 个分组供应商移出故障转移队列",
            count: targets.length,
          }),
          { closeButton: true },
        );
      } catch (error) {
        console.error(
          "Failed to remove provider group from failover queue",
          error,
        );
        toast.error(
          t("provider.groupFailoverRemoveFailed", {
            defaultValue: "分组移出故障转移队列失败",
          }),
        );
      } finally {
        setGroupFailoverActionId((current) =>
          current === group.id ? null : current,
        );
      }
    },
    [
      appId,
      failoverQueueProviderIds,
      groupFailoverActionId,
      isFailoverModeActive,
      removeFromQueue,
      t,
    ],
  );

  const [searchTerm, setSearchTerm] = useState("");
  const [isSearchOpen, setIsSearchOpen] = useState(false);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const { data: claudeDesktopStatus } = useQuery({
    queryKey: ["claudeDesktopStatus"],
    queryFn: () => providersApi.getClaudeDesktopStatus(),
    enabled: appId === "claude-desktop",
    refetchInterval: appId === "claude-desktop" ? 5000 : false,
  });

  // 连通性检查不发真实请求、无封号/计费风险，直接执行（无需确认弹窗）。
  const handleTest = useCallback(
    (provider: Provider) => {
      checkProvider(provider.id, provider.name);
    },
    [checkProvider],
  );

  const handleTestModels = useCallback(
    async (provider: Provider) => {
      setModelTestingProviderId(provider.id);
      try {
        const result = await modelTestProvider(appId, provider.id);
        setModelTestResults((prev) => ({
          ...prev,
          [provider.id]: result,
        }));

        if (result.status === "operational") {
          toast.success(
            t("streamCheck.operational", {
              providerName: provider.name,
              responseTimeMs: result.responseTimeMs,
              defaultValue: `${provider.name} 运行正常 (${result.responseTimeMs}ms)`,
            }),
            { closeButton: true },
          );
        } else if (result.status === "degraded") {
          toast.warning(
            t("streamCheck.degraded", {
              providerName: provider.name,
              responseTimeMs: result.responseTimeMs,
              defaultValue: `${provider.name} 响应较慢 (${result.responseTimeMs}ms)`,
            }),
            { closeButton: true },
          );
        } else if (result.errorCategory === "modelNotFound") {
          toast.error(
            t("streamCheck.modelNotFound", {
              providerName: provider.name,
              model: result.modelUsed,
              defaultValue: `${provider.name} 测试模型 ${result.modelUsed} 不存在或已下架`,
            }),
            {
              description: t("streamCheck.modelNotFoundHint", {
                defaultValue: "",
              }),
              duration: 10000,
              closeButton: true,
            },
          );
        } else {
          toast.error(
            t("streamCheck.failed", {
              providerName: provider.name,
              message: result.message,
              defaultValue: `${provider.name} 检查失败: ${result.message}`,
            }),
            { duration: 8000, closeButton: true },
          );
        }
      } catch (error) {
        console.warn("[ModelTest] Failed:", error);
        toast.error(
          t("streamCheck.error", {
            providerName: provider.name,
            error: String(error),
            defaultValue: `${provider.name} 检查出错: ${String(error)}`,
          }),
          { closeButton: true },
        );
      } finally {
        setModelTestingProviderId((current) =>
          current === provider.id ? null : current,
        );
      }
    },
    [appId, t],
  );

  const handleTestGroupModels = useCallback(
    async (group: ProviderGroupView) => {
      if (groupTestingId || group.providers.length === 0) return;

      const summary: GroupModelTestSummary = {
        total: group.providers.length,
        completed: 0,
        operational: 0,
        degraded: 0,
        failed: 0,
      };
      setGroupTestingId(group.id);
      setGroupModelTestSummaries((current) => ({
        ...current,
        [group.id]: { ...summary },
      }));

      try {
        for (const provider of group.providers) {
          let result: StreamCheckResult;

          try {
            result = await modelTestProvider(appId, provider.id);
          } catch (error) {
            console.warn("[GroupModelTest] Failed:", error);
            result = {
              status: "failed",
              success: false,
              message: String(error),
              testedAt: Date.now(),
              retryCount: 0,
            };
          }

          setModelTestResults((current) => ({
            ...current,
            [provider.id]: result,
          }));
          summary.completed += 1;
          summary[result.status] += 1;
          setGroupModelTestSummaries((current) => ({
            ...current,
            [group.id]: { ...summary },
          }));
        }

        if (summary.failed > 0) {
          toast.error(
            t("provider.groupModelTestFailed", {
              defaultValue:
                "分组模型测试完成：{{operational}} 正常，{{degraded}} 降级，{{failed}} 失败",
              operational: summary.operational,
              degraded: summary.degraded,
              failed: summary.failed,
            }),
            { closeButton: true },
          );
        } else if (summary.degraded > 0) {
          toast.warning(
            t("provider.groupModelTestDegraded", {
              defaultValue:
                "分组模型测试完成：{{operational}} 正常，{{degraded}} 降级",
              operational: summary.operational,
              degraded: summary.degraded,
            }),
            { closeButton: true },
          );
        } else {
          toast.success(
            t("provider.groupModelTestSuccess", {
              defaultValue: "分组模型测试完成：{{operational}} 个供应商正常",
              operational: summary.operational,
            }),
            { closeButton: true },
          );
        }
      } finally {
        setGroupTestingId((current) => (current === group.id ? null : current));
      }
    },
    [appId, groupTestingId, t],
  );

  // Import current live config as default provider
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
      if (appId === "hermes") {
        const count = await providersApi.importHermesFromLive();
        return count > 0;
      }
      if (appId === "claude-desktop") {
        const count = await providersApi.importClaudeDesktopFromClaude();
        return count > 0;
      }
      return providersApi.importDefault(appId);
    },
    onSuccess: (imported) => {
      if (imported) {
        queryClient.invalidateQueries({ queryKey: ["providers", appId] });
        if (appId === "claude-desktop") {
          queryClient.invalidateQueries({ queryKey: ["claudeDesktopStatus"] });
        }
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
      if (event.defaultPrevented) return;

      const key = event.key.toLowerCase();
      if ((event.metaKey || event.ctrlKey) && key === "f") {
        // 正在输入框/可编辑区域中时不抢占 Ctrl+F（例如添加供应商表单里
        // ProviderPresetSelector 的搜索框），避免与其同名快捷键冲突。
        if (isTextEditableTarget(document.activeElement)) return;
        event.preventDefault();
        setIsSearchOpen(true);
        return;
      }

      if (key === "escape") {
        setIsSearchOpen(false);
      }
    };

    globalThis.addEventListener("keydown", handleKeyDown);
    return () => globalThis.removeEventListener("keydown", handleKeyDown);
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

  const providerGroups = useMemo(() => {
    const groupMap = new Map<string, ProviderGroupView>();

    sortedProviders.forEach((provider) => {
      const group = getProviderGroup(provider);
      if (!group) return;

      const existing = groupMap.get(group.id);
      if (existing) {
        existing.providers.push(provider);
        existing.sortIndex = Math.min(existing.sortIndex, group.sortIndex);
        if (!existing.name && group.name) {
          existing.name = group.name;
        }
      } else {
        groupMap.set(group.id, {
          id: group.id,
          name: group.name,
          sortIndex: group.sortIndex,
          providers: [provider],
        });
      }
    });

    return Array.from(groupMap.values()).sort((a, b) => {
      if (a.sortIndex !== b.sortIndex) return a.sortIndex - b.sortIndex;
      return a.name.localeCompare(b.name);
    });
  }, [sortedProviders]);

  const filteredProviderGroups = useMemo(() => {
    const groupMap = new Map(providerGroups.map((group) => [group.id, group]));
    const visibleGroups = new Map<string, ProviderGroupView>();

    filteredProviders.forEach((provider) => {
      const group = getProviderGroup(provider);
      if (!group) {
        return;
      }

      const sourceGroup = groupMap.get(group.id);
      if (!sourceGroup) {
        return;
      }

      const existing = visibleGroups.get(group.id);
      if (existing) {
        existing.providers.push(provider);
      } else {
        visibleGroups.set(group.id, {
          id: sourceGroup.id,
          name: sourceGroup.name,
          sortIndex: sourceGroup.sortIndex,
          providers: [provider],
        });
      }
    });

    return {
      groups: Array.from(visibleGroups.values()).sort((a, b) => {
        if (a.sortIndex !== b.sortIndex) return a.sortIndex - b.sortIndex;
        return a.name.localeCompare(b.name);
      }),
    };
  }, [filteredProviders, providerGroups]);

  const visibleProviderListItems = useMemo(
    () =>
      buildProviderListItems(filteredProviders, filteredProviderGroups.groups),
    [filteredProviderGroups.groups, filteredProviders],
  );

  const allProviderListItems = useMemo(
    () => buildProviderListItems(sortedProviders, providerGroups),
    [providerGroups, sortedProviders],
  );

  useEffect(() => {
    setSelectedProviderIds((current) => {
      const validIds = new Set(sortedProviders.map((provider) => provider.id));
      let changed = false;
      const next = new Set<string>();
      current.forEach((providerId) => {
        if (validIds.has(providerId)) {
          next.add(providerId);
        } else {
          changed = true;
        }
      });
      return changed ? next : current;
    });
  }, [sortedProviders]);

  useEffect(() => {
    setExpandedProviderGroupIds((current) => {
      const knownGroupIds = new Set(providerGroups.map((group) => group.id));
      const next = new Set<string>();

      current.forEach((groupId) => {
        if (knownGroupIds.has(groupId)) {
          next.add(groupId);
        }
      });

      return next.size === current.size ? current : next;
    });
  }, [providerGroups]);

  useEffect(() => {
    if (providerGroups.length > 0) return;

    setIsGroupManageMode(false);
    setSelectedProviderIds(new Set());
  }, [providerGroups.length]);

  useEffect(() => {
    if (providerGroups.length === 0) {
      setGroupControlsHeight(0);
      return;
    }

    const controls = groupControlsRef.current;
    if (!controls) return;

    const updateHeight = () => {
      setGroupControlsHeight(controls.getBoundingClientRect().height);
    };

    updateHeight();
    if (typeof ResizeObserver === "undefined") return;

    const observer = new ResizeObserver(updateHeight);
    observer.observe(controls);
    return () => observer.disconnect();
  }, [providerGroups.length]);

  const selectedProviders = useMemo(
    () =>
      sortedProviders.filter((provider) =>
        selectedProviderIds.has(provider.id),
      ),
    [selectedProviderIds, sortedProviders],
  );

  const refreshProviderViews = useCallback(async () => {
    await queryClient.invalidateQueries({ queryKey: ["providers", appId] });
    await queryClient.refetchQueries({
      queryKey: ["providers", appId],
      type: "active",
    });
    await queryClient.invalidateQueries({ queryKey: ["failoverQueue", appId] });
    try {
      await providersApi.updateTrayMenu();
    } catch (error) {
      console.error(
        "Failed to update tray menu after provider grouping",
        error,
      );
    }
  }, [appId, queryClient]);

  const updateProviderGroupMeta = useCallback(
    async (
      targets: Provider[],
      group: { id: string; name: string; sortIndex?: number } | null,
    ): Promise<boolean> => {
      if (targets.length === 0) return false;

      const updates = targets.map((provider) => {
        const meta = { ...(provider.meta ?? {}) };

        if (group) {
          meta.providerGroupId = group.id;
          meta.providerGroupName = group.name;
          meta.providerGroupSortIndex = group.sortIndex;
        } else {
          delete meta.providerGroupId;
          delete meta.providerGroupName;
          delete meta.providerGroupSortIndex;
        }

        return { ...provider, meta };
      });

      try {
        await Promise.all(
          updates.map((provider) => providersApi.update(provider, appId)),
        );
        await refreshProviderViews();
        toast.success(
          group
            ? t("provider.groupAssignSuccess", {
                defaultValue: "已加入分组",
              })
            : t("provider.groupClearSuccess", {
                defaultValue: "已取消分组",
              }),
          { closeButton: true },
        );
        return true;
      } catch (error) {
        console.error("Failed to update provider group", error);
        toast.error(
          t("provider.groupUpdateFailed", {
            defaultValue: "分组更新失败",
          }),
        );
        return false;
      }
    },
    [appId, refreshProviderViews, t],
  );

  const promptForGroupName = useCallback(() => {
    const name = window.prompt(
      t("provider.groupNamePrompt", {
        defaultValue: "请输入分组名称",
      }),
    );
    const trimmed = name?.trim();
    return trimmed || null;
  }, [t]);

  const createGroupForProviders = useCallback(
    async (targets: Provider[]) => {
      const name = promptForGroupName();
      if (!name) return;
      const updated = await updateProviderGroupMeta(targets, {
        id: createProviderGroupId(name),
        name,
        sortIndex: providerGroups.length,
      });
      if (updated) {
        setSelectedProviderIds(new Set());
      }
    },
    [promptForGroupName, providerGroups.length, updateProviderGroupMeta],
  );

  const assignProvidersToExistingGroup = useCallback(
    async (targets: Provider[], group: ProviderGroupView) => {
      const updated = await updateProviderGroupMeta(targets, {
        id: group.id,
        name: group.name,
        sortIndex: group.sortIndex,
      });
      if (updated) {
        setSelectedProviderIds(new Set());
      }
    },
    [updateProviderGroupMeta],
  );

  const clearProvidersGroup = useCallback(
    async (targets: Provider[]) => {
      await updateProviderGroupMeta(targets, null);
    },
    [updateProviderGroupMeta],
  );

  const resolveProviderGroupDropTarget = useCallback(
    (
      dropTargetId: string,
      allowGroupMemberDrop = false,
    ): ProviderGroupView | undefined => {
      if (dropTargetId.startsWith(PROVIDER_GROUP_DROP_DND_PREFIX)) {
        const groupId = dropTargetId.slice(
          PROVIDER_GROUP_DROP_DND_PREFIX.length,
        );
        return providerGroups.find((group) => group.id === groupId);
      }

      if (!allowGroupMemberDrop) return undefined;

      const overProvider = sortedProviders.find(
        (provider) => provider.id === dropTargetId,
      );
      const overProviderGroup = overProvider
        ? getProviderGroup(overProvider)
        : null;
      if (!overProviderGroup) return undefined;

      return providerGroups.find((group) => group.id === overProviderGroup.id);
    },
    [providerGroups, sortedProviders],
  );

  const clearProviderGroupDragState = useCallback(() => {
    setDraggedProviderId(null);
    setProviderGroupDropTargetId(null);
  }, []);

  const handleProviderDragStart = useCallback(
    (event: DragStartEvent) => {
      const providerId = String(event.active.id);
      setDraggedProviderId(
        sortedProviders.some((provider) => provider.id === providerId)
          ? providerId
          : null,
      );
      setProviderGroupDropTargetId(null);
    },
    [sortedProviders],
  );

  const handleProviderDragOver = useCallback(
    (event: DragOverEvent) => {
      const providerId = String(event.active.id);
      const provider = sortedProviders.find(
        (candidate) => candidate.id === providerId,
      );
      if (!provider || !event.over) {
        setProviderGroupDropTargetId(null);
        return;
      }

      const targetGroup = resolveProviderGroupDropTarget(
        String(event.over.id),
        isWithinGroupMemberDropZone(
          event.active.rect.current.translated,
          event.over.rect,
        ),
      );
      const sourceGroup = getProviderGroup(provider);
      const nextTargetId =
        targetGroup && targetGroup.id !== sourceGroup?.id
          ? targetGroup.id
          : null;

      setProviderGroupDropTargetId((current) =>
        current === nextTargetId ? current : nextTargetId,
      );
    },
    [resolveProviderGroupDropTarget, sortedProviders],
  );

  const groupAwareCollisionDetection = useCallback<CollisionDetection>(
    (args) => {
      if (String(args.active.id).startsWith(PROVIDER_GROUP_DND_PREFIX)) {
        return closestCenter(args);
      }

      const groupDropCollision = pointerWithin(args).find((collision) =>
        String(collision.id).startsWith(PROVIDER_GROUP_DROP_DND_PREFIX),
      );

      return groupDropCollision ? [groupDropCollision] : closestCenter(args);
    },
    [],
  );

  const handleProviderDragCancel = useCallback(
    (_event: DragCancelEvent) => {
      clearProviderGroupDragState();
    },
    [clearProviderGroupDragState],
  );

  const deleteProviderGroup = useCallback(
    async (group: ProviderGroupView) => {
      const confirmed = window.confirm(
        t("provider.groupDeleteConfirm", {
          defaultValue:
            "将删除分组“{{name}}”中的 {{count}} 个供应商。此操作不可恢复，确认继续？",
          name: group.name,
          count: group.providers.length,
        }),
      );
      if (!confirmed) return;

      try {
        const results = await Promise.allSettled(
          group.providers.map((provider) =>
            providersApi.delete(provider.id, appId),
          ),
        );
        const failed = results.filter((result) => result.status === "rejected");
        await refreshProviderViews();

        if (failed.length === 0) {
          toast.success(
            t("provider.groupDeleteSuccess", {
              defaultValue: "已删除分组内供应商",
            }),
            { closeButton: true },
          );
        } else {
          toast.error(
            t("provider.groupDeletePartialFailed", {
              defaultValue: "{{failed}} 个供应商删除失败",
              failed: failed.length,
            }),
          );
        }
      } catch (error) {
        console.error("Failed to delete provider group", error);
        toast.error(
          t("provider.groupDeleteFailed", {
            defaultValue: "分组删除失败",
          }),
        );
      }
    },
    [appId, refreshProviderViews, t],
  );

  const persistProviderListItemOrder = useCallback(
    async (items: ProviderListItem[]) => {
      const orderedProviders = items.flatMap((item) =>
        item.kind === "group" ? item.group.providers : [item.provider],
      );

      try {
        await providersApi.updateSortOrder(
          orderedProviders.map((provider, index) => ({
            id: provider.id,
            sortIndex: index,
          })),
          appId,
        );
        await refreshProviderViews();
        toast.success(
          t("provider.groupSortUpdated", {
            defaultValue: "Provider order updated",
          }),
          { closeButton: true },
        );
      } catch (error) {
        console.error("Failed to reorder provider groups", error);
        toast.error(
          t("provider.groupSortUpdateFailed", {
            defaultValue: "Failed to update provider order",
          }),
        );
      }
    },
    [appId, refreshProviderViews, t],
  );

  const reorderUngroupedProviderAroundGroup = useCallback(
    async (
      provider: Provider,
      targetGroup: ProviderGroupView,
      placement: "before" | "after",
    ) => {
      const activeItem = allProviderListItems.find(
        (item) => item.kind === "provider" && item.provider.id === provider.id,
      );
      if (!activeItem) return;

      const itemsWithoutActiveProvider = allProviderListItems.filter(
        (item) => item.id !== activeItem.id,
      );
      const targetIndex = itemsWithoutActiveProvider.findIndex(
        (item) => item.kind === "group" && item.group.id === targetGroup.id,
      );
      if (targetIndex === -1) return;

      const nextItems = [...itemsWithoutActiveProvider];
      nextItems.splice(
        targetIndex + (placement === "after" ? 1 : 0),
        0,
        activeItem,
      );
      await persistProviderListItemOrder(nextItems);
    },
    [allProviderListItems, persistProviderListItemOrder],
  );

  const reorderProviderGroups = useCallback(
    async (event: DragEndEvent) => {
      const { active, over } = event;
      clearProviderGroupDragState();
      if (!over || active.id === over.id) return;

      const activeId = String(active.id);
      const overId = String(over.id);

      if (!activeId.startsWith(PROVIDER_GROUP_DND_PREFIX)) {
        const activeProvider = sortedProviders.find(
          (provider) => provider.id === activeId,
        );
        const targetGroup = resolveProviderGroupDropTarget(
          overId,
          isWithinGroupMemberDropZone(
            active.rect.current.translated,
            over.rect,
          ),
        );

        if (activeProvider && targetGroup) {
          const activeProviderGroup = getProviderGroup(activeProvider);
          if (activeProviderGroup?.id !== targetGroup.id) {
            await assignProvidersToExistingGroup([activeProvider], targetGroup);
            return;
          }
        }

        const rootTargetGroup = overId.startsWith(PROVIDER_GROUP_DND_PREFIX)
          ? providerGroups.find(
              (group) =>
                group.id === overId.slice(PROVIDER_GROUP_DND_PREFIX.length),
            )
          : undefined;
        if (
          activeProvider &&
          !getProviderGroup(activeProvider) &&
          rootTargetGroup
        ) {
          const activeRect = active.rect.current.translated;
          const placement =
            activeRect &&
            activeRect.top + activeRect.height / 2 >=
              over.rect.top + over.rect.height / 2
              ? "after"
              : "before";
          await reorderUngroupedProviderAroundGroup(
            activeProvider,
            rootTargetGroup,
            placement,
          );
          return;
        }

        handleDragEnd(event);
        return;
      }

      const oldIndex = allProviderListItems.findIndex(
        (item) => item.id === activeId,
      );
      const newIndex = allProviderListItems.findIndex((item) => {
        if (item.id === overId) return true;
        return (
          item.kind === "group" &&
          item.group.providers.some((provider) => provider.id === overId)
        );
      });
      if (oldIndex === -1 || newIndex === -1) return;

      const reordered = arrayMove(allProviderListItems, oldIndex, newIndex);
      const reorderedProviders = reordered.flatMap((item) =>
        item.kind === "group" ? item.group.providers : [item.provider],
      );
      try {
        await providersApi.updateSortOrder(
          reorderedProviders.map((provider, index) => ({
            id: provider.id,
            sortIndex: index,
          })),
          appId,
        );
        await refreshProviderViews();
        toast.success(
          t("provider.groupSortUpdated", {
            defaultValue: "分组排序已更新",
          }),
          { closeButton: true },
        );
      } catch (error) {
        console.error("Failed to reorder provider groups", error);
        toast.error(
          t("provider.groupSortUpdateFailed", {
            defaultValue: "分组排序更新失败",
          }),
        );
      }
    },
    [
      appId,
      allProviderListItems,
      assignProvidersToExistingGroup,
      clearProviderGroupDragState,
      handleDragEnd,
      providerGroups,
      refreshProviderViews,
      reorderUngroupedProviderAroundGroup,
      resolveProviderGroupDropTarget,
      sortedProviders,
      t,
    ],
  );

  const toggleProviderSelected = useCallback(
    (providerId: string, checked: boolean) => {
      setSelectedProviderIds((current) => {
        const next = new Set(current);
        if (checked) {
          next.add(providerId);
        } else {
          next.delete(providerId);
        }
        return next;
      });
    },
    [],
  );

  const clearProviderSelectionSwipe = useCallback(() => {
    providerSelectionSwipeRef.current = null;
  }, []);

  useEffect(() => {
    globalThis.addEventListener("pointerup", clearProviderSelectionSwipe);
    globalThis.addEventListener("pointercancel", clearProviderSelectionSwipe);
    return () => {
      globalThis.removeEventListener("pointerup", clearProviderSelectionSwipe);
      globalThis.removeEventListener(
        "pointercancel",
        clearProviderSelectionSwipe,
      );
    };
  }, [clearProviderSelectionSwipe]);

  useEffect(() => {
    if (!isGroupManageMode) {
      clearProviderSelectionSwipe();
    }
  }, [clearProviderSelectionSwipe, isGroupManageMode]);

  const beginProviderSelectionSwipe = useCallback(
    (
      event: ReactPointerEvent<HTMLDivElement>,
      providerId: string,
      isSelected: boolean,
    ) => {
      if (event.button !== 0) return;

      event.preventDefault();
      event.stopPropagation();
      event.currentTarget.setPointerCapture?.(event.pointerId);
      const checked = !isSelected;
      providerSelectionSwipeRef.current = {
        pointerId: event.pointerId,
        checked,
      };
      toggleProviderSelected(providerId, checked);
    },
    [toggleProviderSelected],
  );

  const extendProviderSelectionSwipe = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>, providerId: string) => {
      const swipe = providerSelectionSwipeRef.current;
      if (!swipe || swipe.pointerId !== event.pointerId) return;

      toggleProviderSelected(providerId, swipe.checked);
    },
    [toggleProviderSelected],
  );

  const continueProviderSelectionSwipe = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      const swipe = providerSelectionSwipeRef.current;
      if (!swipe || swipe.pointerId !== event.pointerId) return;

      const element = document.elementFromPoint(event.clientX, event.clientY);
      const target = element?.closest<HTMLElement>(
        "[data-provider-selection-id]",
      );
      const providerId = target?.dataset.providerSelectionId;
      if (providerId) {
        toggleProviderSelected(providerId, swipe.checked);
      }
    },
    [toggleProviderSelected],
  );

  const allFilteredProvidersSelected =
    filteredProviders.length > 0 &&
    filteredProviders.every((provider) => selectedProviderIds.has(provider.id));

  const toggleAllFilteredProviders = useCallback(() => {
    setSelectedProviderIds((current) => {
      const next = new Set(current);
      if (allFilteredProvidersSelected) {
        filteredProviders.forEach((provider) => next.delete(provider.id));
      } else {
        filteredProviders.forEach((provider) => next.add(provider.id));
      }
      return next;
    });
  }, [allFilteredProvidersSelected, filteredProviders]);

  const claudeDesktopStatusMessages = useMemo(() => {
    if (appId !== "claude-desktop" || !claudeDesktopStatus) return [];

    const messages: string[] = [];
    if (!claudeDesktopStatus.supported) {
      messages.push(
        t("claudeDesktop.statusUnsupported", {
          defaultValue: "当前平台暂不支持 Claude Desktop 3P 配置写入。",
        }),
      );
      return messages;
    }

    if (claudeDesktopStatus.staleRawModels) {
      messages.push(
        t("claudeDesktop.statusStaleRawModels", {
          defaultValue:
            "Claude Desktop profile 中存在非 claude-* 模型名，新版 Claude Desktop 可能拒绝加载；重新切换当前供应商可修复。",
        }),
      );
    }
    if (claudeDesktopStatus.missingRouteMappings) {
      messages.push(
        t("claudeDesktop.statusMissingRouteMappings", {
          defaultValue:
            "当前供应商启用了模型映射，但没有有效路由；请编辑供应商并补全至少一个模型映射。",
        }),
      );
    }
    if (
      claudeDesktopStatus.mode === "proxy" &&
      !claudeDesktopStatus.gatewayTokenConfigured
    ) {
      messages.push(
        t("claudeDesktop.statusGatewayTokenMissing", {
          defaultValue:
            "当前本地路由 token 尚未生成；重新切换该供应商会写入新的本地 token。",
        }),
      );
    }

    const expected = claudeDesktopStatus.expectedBaseUrl?.replace(/\/+$/, "");
    const actual = claudeDesktopStatus.actualBaseUrl?.replace(/\/+$/, "");
    if (expected && actual && expected !== actual) {
      messages.push(
        t("claudeDesktop.statusBaseUrlMismatch", {
          expected,
          actual,
          defaultValue:
            "Claude Desktop profile 指向的地址与当前供应商不一致；当前为 {{actual}}，应为 {{expected}}。重新切换当前供应商可修复。",
        }),
      );
    }

    return messages;
  }, [appId, claudeDesktopStatus, t]);

  const getActiveProviderInGroup = useCallback(
    (group: ProviderGroupView) =>
      group.providers.find((provider) => {
        if (provider.category === "omo") {
          return provider.id === (currentOmoId || "");
        }
        if (provider.category === "omo-slim") {
          return provider.id === (currentOmoSlimId || "");
        }
        if (appId === "openclaw") {
          return isProviderDefaultModel(provider.id);
        }
        if (appId === "opencode") return false;
        if (appId === "hermes") {
          return provider.id === hermesCurrentProviderId;
        }
        return isFailoverModeActive
          ? activeProviderId === provider.id
          : provider.id === currentProviderId;
      }),
    [
      activeProviderId,
      appId,
      currentOmoId,
      currentOmoSlimId,
      currentProviderId,
      hermesCurrentProviderId,
      isFailoverModeActive,
      isProviderDefaultModel,
    ],
  );

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

  const renderProviderGroupMenu = (targets: Provider[], compact = false) => {
    const canClearGroup = targets.some((provider) =>
      Boolean(getProviderGroup(provider)),
    );

    return (
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            variant={compact ? "ghost" : "secondary"}
            size={compact ? "icon" : "sm"}
            className={compact ? "h-8 w-8 p-1" : "h-7 gap-1.5 px-2.5 text-xs"}
            disabled={targets.length === 0}
            title={
              compact
                ? t("provider.joinGroup", { defaultValue: "加入分组" })
                : undefined
            }
            aria-label={t("provider.joinGroup", {
              defaultValue: "加入分组",
            })}
          >
            <FolderPlus className="h-4 w-4" />
            {!compact && t("provider.joinGroup", { defaultValue: "加入分组" })}
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="w-52">
          <DropdownMenuLabel>
            {t("provider.groupAction", { defaultValue: "分组操作" })}
          </DropdownMenuLabel>
          <DropdownMenuItem
            onClick={() => void createGroupForProviders(targets)}
          >
            <FolderPlus className="mr-2 h-4 w-4" />
            {t("provider.createNewGroup", { defaultValue: "新建分组" })}
          </DropdownMenuItem>
          {providerGroups.length > 0 && <DropdownMenuSeparator />}
          {providerGroups.map((group) => (
            <DropdownMenuItem
              key={group.id}
              onClick={() =>
                void assignProvidersToExistingGroup(targets, group)
              }
            >
              <Folder className="mr-2 h-4 w-4" />
              <span className="truncate">{group.name}</span>
            </DropdownMenuItem>
          ))}
          {canClearGroup && (
            <>
              <DropdownMenuSeparator />
              <DropdownMenuItem
                onClick={() => void clearProvidersGroup(targets)}
              >
                <Ungroup className="mr-2 h-4 w-4" />
                {t("provider.clearGroup", { defaultValue: "取消分组" })}
              </DropdownMenuItem>
            </>
          )}
        </DropdownMenuContent>
      </DropdownMenu>
    );
  };

  const renderProviderCard = (provider: Provider) => {
    const isOmo = provider.category === "omo";
    const isOmoSlim = provider.category === "omo-slim";
    const isOmoCurrent = isOmo && provider.id === (currentOmoId || "");
    const isOmoSlimCurrent =
      isOmoSlim && provider.id === (currentOmoSlimId || "");
    const isHermesCurrent =
      appId === "hermes" && hermesCurrentProviderId === provider.id;

    return (
      <SortableProviderCard
        key={provider.id}
        provider={provider}
        isCurrent={
          isOmo
            ? isOmoCurrent
            : isOmoSlim
              ? isOmoSlimCurrent
              : appId === "hermes"
                ? isHermesCurrent
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
        onTest={handleTest}
        onTestModels={handleTestModels}
        isTesting={isChecking(provider.id)}
        isTestingModels={modelTestingProviderId === provider.id}
        modelTestResult={modelTestResults[provider.id]}
        isProxyRunning={isProxyRunning}
        isProxyTakeover={isProxyTakeover}
        isAutoFailoverEnabled={isFailoverModeActive}
        failoverPriority={getFailoverPriority(provider.id)}
        isInFailoverQueue={isInFailoverQueue(provider.id)}
        onToggleFailover={(enabled) =>
          handleToggleFailover(provider.id, enabled)
        }
        activeProviderId={activeProviderId}
        isDefaultModel={
          appId === "hermes"
            ? isHermesCurrent
            : isProviderDefaultModel(provider.id)
        }
        onSetAsDefault={
          onSetAsDefault ? () => onSetAsDefault(provider) : undefined
        }
        isGroupManageMode={isGroupManageMode}
        isSelected={selectedProviderIds.has(provider.id)}
        onToggleSelected={(checked) =>
          toggleProviderSelected(provider.id, checked)
        }
        onSelectionSwipeStart={beginProviderSelectionSwipe}
        onSelectionSwipeEnter={extendProviderSelectionSwipe}
        groupMenu={renderProviderGroupMenu([provider], true)}
      />
    );
  };

  const renderProviderList = () => {
    const sortableItems = [
      ...visibleProviderListItems.map((item) => item.id),
      ...filteredProviderGroups.groups.flatMap((group) =>
        group.providers.map((provider) => provider.id),
      ),
    ];

    return (
      <DndContext
        sensors={sensors}
        collisionDetection={groupAwareCollisionDetection}
        onDragStart={handleProviderDragStart}
        onDragOver={handleProviderDragOver}
        onDragCancel={handleProviderDragCancel}
        onDragEnd={reorderProviderGroups}
      >
        <SortableContext
          items={sortableItems}
          strategy={verticalListSortingStrategy}
        >
          <div
            className="space-y-3"
            onPointerMove={continueProviderSelectionSwipe}
          >
            {visibleProviderListItems.map((item) => {
              if (item.kind === "provider") {
                return renderProviderCard(item.provider);
              }

              const group = item.group;
              const fullGroup =
                providerGroups.find((candidate) => candidate.id === group.id) ??
                group;

              return (
                <SortableProviderGroup
                  key={group.id}
                  group={group}
                  isExpanded={expandedProviderGroupIds.has(group.id)}
                  activeProvider={getActiveProviderInGroup(fullGroup)}
                  isDropTarget={
                    draggedProviderId !== null &&
                    providerGroupDropTargetId === group.id
                  }
                  stickyTopOffset={groupControlsHeight + 8}
                  isTestingModels={groupTestingId === group.id}
                  isTestModelsDisabled={groupTestingId !== null}
                  modelTestSummary={groupModelTestSummaries[group.id]}
                  isFailoverActionsAvailable={isFailoverModeActive}
                  canAddToFailoverQueue={group.providers.some(
                    (provider) => !failoverQueueProviderIds.has(provider.id),
                  )}
                  canRemoveFromFailoverQueue={group.providers.some((provider) =>
                    failoverQueueProviderIds.has(provider.id),
                  )}
                  isFailoverActionRunning={groupFailoverActionId === group.id}
                  onToggleExpanded={() => {
                    setExpandedProviderGroupIds((current) => {
                      const next = new Set(current);
                      if (next.has(group.id)) {
                        next.delete(group.id);
                      } else {
                        next.add(group.id);
                      }
                      return next;
                    });
                  }}
                  onDeleteGroup={() => void deleteProviderGroup(fullGroup)}
                  onClearGroup={() =>
                    void clearProvidersGroup(fullGroup.providers)
                  }
                  onTestModels={() => void handleTestGroupModels(fullGroup)}
                  onAddToFailoverQueue={() =>
                    void addGroupToFailoverQueue(fullGroup)
                  }
                  onRemoveFromFailoverQueue={() =>
                    void removeGroupFromFailoverQueue(fullGroup)
                  }
                >
                  {group.providers.map((provider) =>
                    renderProviderCard(provider),
                  )}
                </SortableProviderGroup>
              );
            })}
          </div>
        </SortableContext>
      </DndContext>
    );
  };

  return (
    <div className="mt-4 space-y-4">
      {claudeDesktopStatusMessages.length > 0 && (
        <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 px-4 py-3 text-sm text-amber-900 dark:text-amber-200">
          <div className="flex items-center gap-2 font-medium">
            <AlertTriangle className="h-4 w-4 shrink-0" />
            {t("claudeDesktop.statusTitle", {
              defaultValue: "Claude Desktop 配置需要检查",
            })}
          </div>
          <ul className="mt-2 space-y-1 text-xs leading-relaxed">
            {claudeDesktopStatusMessages.map((message) => (
              <li key={message}>{message}</li>
            ))}
          </ul>
        </div>
      )}
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

      {providerGroups.length > 0 && (
        <div
          ref={groupControlsRef}
          data-testid="provider-group-controls"
          className="sticky top-0 z-30 rounded-lg border border-cyan-300/70 bg-background/95 px-3 py-2.5 shadow-sm backdrop-blur dark:border-cyan-900/70"
        >
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="flex min-w-0 items-center gap-2 text-sm">
              <Folder className="h-4 w-4 text-muted-foreground" />
              <span className="font-medium">
                {t("provider.groupManage", { defaultValue: "供应商分组" })}
              </span>
              <Badge variant="secondary" className="text-xs">
                {providerGroups.length}
              </Badge>
              {isGroupManageMode && (
                <span className="text-xs text-muted-foreground">
                  {t("provider.groupSelectedCount", {
                    defaultValue: "已选 {{count}} 个",
                    count: selectedProviders.length,
                  })}
                </span>
              )}
            </div>
            <div className="flex flex-wrap items-center gap-2">
              {isGroupManageMode && (
                <>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 px-2.5 text-xs"
                    onClick={toggleAllFilteredProviders}
                  >
                    {allFilteredProvidersSelected
                      ? t("provider.clearFilteredSelection", {
                          defaultValue: "取消全选",
                        })
                      : t("provider.selectAllFiltered", {
                          defaultValue: "全选当前",
                        })}
                  </Button>
                  {renderProviderGroupMenu(selectedProviders)}
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 gap-1.5 px-2.5 text-xs"
                    disabled={selectedProviders.length === 0}
                    onClick={() => void clearProvidersGroup(selectedProviders)}
                  >
                    <Ungroup className="h-3.5 w-3.5" />
                    {t("provider.clearGroup", {
                      defaultValue: "取消分组",
                    })}
                  </Button>
                </>
              )}
              <Button
                data-testid="provider-group-manage-toggle"
                variant={isGroupManageMode ? "secondary" : "outline"}
                size="sm"
                className="h-7 gap-1.5 px-2.5 text-xs"
                onClick={() => {
                  if (isGroupManageMode) {
                    setSelectedProviderIds(new Set());
                  }
                  setIsGroupManageMode((current) => !current);
                }}
              >
                <CheckSquare className="h-3.5 w-3.5" />
                {isGroupManageMode
                  ? t("provider.exitGroupManage", {
                      defaultValue: "退出分组管理",
                    })
                  : t("provider.enterGroupManage", {
                      defaultValue: "分组管理",
                    })}
              </Button>
            </div>
          </div>
        </div>
      )}

      {filteredProviders.length === 0 ? (
        <div className="px-6 py-8 text-sm text-center border border-dashed rounded-lg border-border text-muted-foreground">
          {t("provider.noSearchResults", {
            defaultValue: "No providers match your search.",
          })}
        </div>
      ) : (
        renderProviderList()
      )}
    </div>
  );
}

interface SortableProviderCardProps {
  provider: Provider;
  isCurrent: boolean;
  appId: AppId;
  isInConfig: boolean;
  isOmo: boolean;
  isOmoSlim: boolean;
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
  onTest?: (provider: Provider) => void;
  onTestModels?: (provider: Provider) => void;
  isTesting: boolean;
  isTestingModels: boolean;
  modelTestResult?: StreamCheckResult;
  isProxyRunning: boolean;
  isProxyTakeover: boolean;
  isAutoFailoverEnabled: boolean;
  failoverPriority?: number;
  isInFailoverQueue: boolean;
  onToggleFailover: (enabled: boolean) => void;
  activeProviderId?: string;
  // OpenClaw: default model
  isDefaultModel?: boolean;
  onSetAsDefault?: () => void;
  isGroupManageMode: boolean;
  isSelected: boolean;
  onToggleSelected: (checked: boolean) => void;
  onSelectionSwipeStart: (
    event: ReactPointerEvent<HTMLDivElement>,
    providerId: string,
    isSelected: boolean,
  ) => void;
  onSelectionSwipeEnter: (
    event: ReactPointerEvent<HTMLDivElement>,
    providerId: string,
  ) => void;
  groupMenu: ReactNode;
}

interface SortableProviderGroupProps {
  group: ProviderGroupView;
  isExpanded: boolean;
  activeProvider?: Provider;
  isDropTarget: boolean;
  stickyTopOffset: number;
  isTestingModels: boolean;
  isTestModelsDisabled: boolean;
  modelTestSummary?: GroupModelTestSummary;
  isFailoverActionsAvailable: boolean;
  canAddToFailoverQueue: boolean;
  canRemoveFromFailoverQueue: boolean;
  isFailoverActionRunning: boolean;
  onToggleExpanded: () => void;
  onDeleteGroup: () => void;
  onClearGroup: () => void;
  onTestModels: () => void;
  onAddToFailoverQueue: () => void;
  onRemoveFromFailoverQueue: () => void;
  children: ReactNode;
}

function SortableProviderGroup({
  group,
  isExpanded,
  activeProvider,
  isDropTarget,
  stickyTopOffset,
  isTestingModels,
  isTestModelsDisabled,
  modelTestSummary,
  isFailoverActionsAvailable,
  canAddToFailoverQueue,
  canRemoveFromFailoverQueue,
  isFailoverActionRunning,
  onToggleExpanded,
  onDeleteGroup,
  onClearGroup,
  onTestModels,
  onAddToFailoverQueue,
  onRemoveFromFailoverQueue,
  children,
}: SortableProviderGroupProps) {
  const { t } = useTranslation();
  const {
    setNodeRef,
    attributes,
    listeners,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: `${PROVIDER_GROUP_DND_PREFIX}${group.id}` });
  const { setNodeRef: setGroupDropNodeRef } = useDroppable({
    id: `${PROVIDER_GROUP_DROP_DND_PREFIX}${group.id}`,
  });
  const activeProviderUrl = activeProvider
    ? getProviderDisplayUrl(activeProvider)
    : undefined;

  // A transformed ancestor can stop sticky descendants from following the
  // provider scrollport in WebView. Only apply DnD positioning while needed.
  const hasSortableTransform =
    transform !== null &&
    (transform.x !== 0 ||
      transform.y !== 0 ||
      transform.scaleX !== 1 ||
      transform.scaleY !== 1);
  const style: CSSProperties | undefined = hasSortableTransform
    ? {
        transform: CSS.Transform.toString(transform),
        transition,
      }
    : undefined;
  const headerStyle: CSSProperties = { top: stickyTopOffset };

  return (
    <div
      ref={setNodeRef}
      style={style}
      data-testid={`provider-group-${group.id}`}
      className={cn(
        "relative overflow-visible rounded-xl border border-cyan-300/70 bg-cyan-50/55 p-0 transition-all dark:border-cyan-900/70 dark:bg-cyan-950/20",
        isDropTarget &&
          "border-primary bg-primary/10 shadow-md ring-2 ring-primary/30",
        isDragging && "z-10 scale-[1.01] border-primary shadow-lg",
      )}
    >
      <div
        style={headerStyle}
        data-testid={`provider-group-header-${group.id}`}
        className={cn(
          "sticky z-20 flex items-start gap-2 rounded-[0.7rem] border border-cyan-200/90 bg-cyan-50/95 px-3 py-2.5 shadow-sm backdrop-blur dark:border-cyan-900/80 dark:bg-cyan-950/95",
          isExpanded && "mb-3",
          isDropTarget && "border-primary bg-primary/10",
        )}
      >
        <button
          type="button"
          className={cn(
            "-ml-1 flex-shrink-0 cursor-grab rounded-md p-1.5 text-muted-foreground/60 transition-colors hover:bg-muted hover:text-muted-foreground active:cursor-grabbing",
            isDragging && "cursor-grabbing",
          )}
          aria-label={t("provider.groupDragHandle", {
            defaultValue: "拖动供应商分组",
          })}
          {...attributes}
          {...listeners}
        >
          <GripVertical className="h-4 w-4" />
        </button>
        <div
          ref={setGroupDropNodeRef}
          data-testid={`provider-group-drop-target-${group.id}`}
          className="min-w-0 flex-1"
        >
          <button
            type="button"
            className="flex w-full min-w-0 items-center gap-2 text-left"
            onClick={onToggleExpanded}
          >
            <Folder className="h-4 w-4 shrink-0 text-muted-foreground" />
            <span className="min-w-0 flex-1 truncate text-sm font-semibold">
              {group.name}
            </span>
            <Badge variant="secondary" className="text-xs">
              {group.providers.length}
            </Badge>
          </button>
          {!isExpanded && activeProvider && (
            <div
              data-testid={`provider-group-active-provider-${group.id}`}
              className="mt-1.5 flex min-w-0 w-full items-start gap-1.5 rounded-md border border-emerald-300/70 bg-emerald-100/80 px-2.5 py-1.5 text-emerald-800 dark:border-emerald-800/80 dark:bg-emerald-950/70 dark:text-emerald-200"
            >
              <Activity className="mt-0.5 h-3.5 w-3.5 shrink-0" />
              <div className="min-w-0 flex-1 leading-tight">
                <div className="flex min-w-0 items-center gap-1.5">
                  <span className="shrink-0 text-[11px] font-medium">
                    {t("provider.groupActiveProvider", {
                      defaultValue: "正在使用",
                    })}
                  </span>
                  <span className="min-w-0 truncate text-xs font-semibold">
                    {activeProvider.name}
                  </span>
                </div>
                {activeProviderUrl && (
                  <span
                    data-testid={`provider-group-active-url-${group.id}`}
                    title={activeProviderUrl}
                    className="mt-0.5 block truncate text-[11px] text-emerald-700/85 dark:text-emerald-300/85"
                  >
                    {activeProviderUrl}
                  </span>
                )}
              </div>
            </div>
          )}
        </div>
        <div className="ml-auto flex shrink-0 items-center gap-1">
          {modelTestSummary && (
            <div
              data-testid={`provider-group-model-test-summary-${group.id}`}
              title={t("provider.groupModelTestSummary", {
                defaultValue:
                  "模型测试：{{operational}} 正常，{{degraded}} 降级，{{failed}} 失败",
                operational: modelTestSummary.operational,
                degraded: modelTestSummary.degraded,
                failed: modelTestSummary.failed,
              })}
              className="hidden shrink-0 items-center gap-1.5 text-[10px] sm:flex"
            >
              {isTestingModels && (
                <span className="text-muted-foreground">
                  {modelTestSummary.completed}/{modelTestSummary.total}
                </span>
              )}
              <span className="inline-flex items-center gap-1 text-emerald-700 dark:text-emerald-300">
                <span className="h-1.5 w-1.5 rounded-full bg-emerald-500" />
                {modelTestSummary.operational}
              </span>
              <span className="inline-flex items-center gap-1 text-yellow-700 dark:text-yellow-300">
                <span className="h-1.5 w-1.5 rounded-full bg-yellow-500" />
                {modelTestSummary.degraded}
              </span>
              <span className="inline-flex items-center gap-1 text-red-700 dark:text-red-300">
                <span className="h-1.5 w-1.5 rounded-full bg-red-500" />
                {modelTestSummary.failed}
              </span>
            </div>
          )}
          <Button
            variant="ghost"
            size="icon"
            data-testid={`provider-group-model-test-${group.id}`}
            className="h-7 w-7"
            disabled={isTestModelsDisabled}
            onClick={onTestModels}
            title={t("provider.testGroupModels", {
              defaultValue: "测试分组模型",
            })}
            aria-label={t("provider.testGroupModels", {
              defaultValue: "测试分组模型",
            })}
          >
            {isTestingModels ? (
              <LoaderCircle className="h-4 w-4 animate-spin" />
            ) : (
              <TestTube2 className="h-4 w-4" />
            )}
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7"
            onClick={onToggleExpanded}
            title={
              isExpanded
                ? t("common.collapse", { defaultValue: "收起" })
                : t("common.expand", { defaultValue: "展开" })
            }
            aria-label={
              isExpanded
                ? t("common.collapse", { defaultValue: "收起" })
                : t("common.expand", { defaultValue: "展开" })
            }
          >
            <ChevronDown
              className={cn(
                "h-4 w-4 transition-transform",
                isExpanded && "rotate-180",
              )}
            />
          </Button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                variant="ghost"
                size="icon"
                data-testid={`provider-group-menu-${group.id}`}
                className="h-7 w-7"
                aria-label={t("provider.groupMenu", {
                  defaultValue: "分组菜单",
                })}
              >
                <MoreHorizontal className="h-4 w-4" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-48">
              <DropdownMenuLabel className="truncate">
                {group.name}
              </DropdownMenuLabel>
              {isFailoverActionsAvailable && (
                <>
                  <DropdownMenuItem
                    data-testid={`provider-group-add-failover-${group.id}`}
                    disabled={isFailoverActionRunning || !canAddToFailoverQueue}
                    onClick={onAddToFailoverQueue}
                  >
                    {isFailoverActionRunning ? (
                      <LoaderCircle className="mr-2 h-4 w-4 animate-spin" />
                    ) : (
                      <Activity className="mr-2 h-4 w-4" />
                    )}
                    {t("provider.groupAddToFailoverQueue", {
                      defaultValue: "整组加入故障转移队列",
                    })}
                  </DropdownMenuItem>
                  <DropdownMenuItem
                    data-testid={`provider-group-remove-failover-${group.id}`}
                    disabled={
                      isFailoverActionRunning || !canRemoveFromFailoverQueue
                    }
                    onClick={onRemoveFromFailoverQueue}
                  >
                    {isFailoverActionRunning ? (
                      <LoaderCircle className="mr-2 h-4 w-4 animate-spin" />
                    ) : (
                      <X className="mr-2 h-4 w-4" />
                    )}
                    {t("provider.groupRemoveFromFailoverQueue", {
                      defaultValue: "整组移出故障转移队列",
                    })}
                  </DropdownMenuItem>
                  <DropdownMenuSeparator />
                </>
              )}
              <DropdownMenuItem onClick={onClearGroup}>
                <Ungroup className="mr-2 h-4 w-4" />
                {t("provider.clearGroup", { defaultValue: "取消分组" })}
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem
                className="text-destructive focus:text-destructive"
                onClick={onDeleteGroup}
              >
                <Trash2 className="mr-2 h-4 w-4" />
                {t("provider.deleteGroupProviders", {
                  defaultValue: "删除组内供应商",
                })}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>
      {isExpanded && <div className="space-y-3 px-3 pb-3">{children}</div>}
    </div>
  );
}

function SortableProviderCard({
  provider,
  isCurrent,
  appId,
  isInConfig,
  isOmo,
  isOmoSlim,
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
  onTest,
  onTestModels,
  isTesting,
  isTestingModels,
  modelTestResult,
  isProxyRunning,
  isProxyTakeover,
  isAutoFailoverEnabled,
  failoverPriority,
  isInFailoverQueue,
  onToggleFailover,
  activeProviderId,
  isDefaultModel,
  onSetAsDefault,
  isGroupManageMode,
  isSelected,
  onToggleSelected,
  onSelectionSwipeStart,
  onSelectionSwipeEnter,
  groupMenu,
}: SortableProviderCardProps) {
  const {
    setNodeRef,
    attributes,
    listeners,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: provider.id });

  const style: CSSProperties = {
    transform: CSS.Transform.toString(transform),
    transition,
  };

  return (
    <div
      ref={setNodeRef}
      style={style}
      className="flex items-start gap-2"
      data-testid={`sortable-provider-card-${provider.id}`}
      data-provider-selection-id={isGroupManageMode ? provider.id : undefined}
      onPointerEnter={(event) => {
        if (isGroupManageMode) {
          onSelectionSwipeEnter(event, provider.id);
        }
      }}
    >
      {isGroupManageMode && (
        <div
          data-testid={`provider-selection-control-${provider.id}`}
          data-selected={isSelected ? "true" : "false"}
          role="checkbox"
          tabIndex={0}
          aria-checked={isSelected}
          aria-label="选择供应商"
          className="touch-none select-none pt-5"
          onPointerDown={(event) =>
            onSelectionSwipeStart(event, provider.id, isSelected)
          }
          onKeyDown={(event) => {
            if (event.key !== " " && event.key !== "Enter") return;
            event.preventDefault();
            onToggleSelected(!isSelected);
          }}
        >
          <Checkbox
            checked={isSelected}
            aria-hidden="true"
            tabIndex={-1}
            className="pointer-events-none"
          />
        </div>
      )}
      <div className="min-w-0 flex-1">
        <ProviderCard
          provider={provider}
          isCurrent={isCurrent}
          appId={appId}
          isInConfig={isInConfig}
          isOmo={isOmo}
          isOmoSlim={isOmoSlim}
          onSwitch={onSwitch}
          onEdit={onEdit}
          onDelete={onDelete}
          onRemoveFromConfig={onRemoveFromConfig}
          onDisableOmo={onDisableOmo}
          onDisableOmoSlim={onDisableOmoSlim}
          onDuplicate={onDuplicate}
          onConfigureUsage={
            onConfigureUsage
              ? (item) => onConfigureUsage(item)
              : () => undefined
          }
          onOpenWebsite={onOpenWebsite}
          onOpenTerminal={onOpenTerminal}
          onTest={onTest}
          onTestModels={onTestModels}
          isTesting={isTesting}
          isTestingModels={isTestingModels}
          modelTestResult={modelTestResult}
          isProxyRunning={isProxyRunning}
          isProxyTakeover={isProxyTakeover}
          dragHandleProps={{
            attributes,
            listeners,
            isDragging,
          }}
          isAutoFailoverEnabled={isAutoFailoverEnabled}
          failoverPriority={failoverPriority}
          isInFailoverQueue={isInFailoverQueue}
          onToggleFailover={onToggleFailover}
          activeProviderId={activeProviderId}
          // OpenClaw: default model
          isDefaultModel={isDefaultModel}
          onSetAsDefault={onSetAsDefault}
          groupMenu={groupMenu}
        />
      </div>
    </div>
  );
}
