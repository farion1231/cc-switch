import { CSS } from "@dnd-kit/utilities";
import { DndContext, closestCenter, type Modifier } from "@dnd-kit/core";
import {
  SortableContext,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { useCallback, useMemo, type CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "@/lib/toast";
import type { OpenClawProviderConfig, Provider } from "@/types";
import { getOpencodeDeclaredModels } from "@/components/providers/forms/helpers/opencodeFormUtils";
import type { AppId } from "@/lib/api";
import { providersApi } from "@/lib/api/providers";
import {
  extractErrorMessage,
  translatePiProviderMutationError,
} from "@/utils/errorUtils";
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
  useCurrentOmoProviderId,
  useCurrentOmoSlimProviderId,
} from "@/lib/query/omo";
import { HelpTip } from "@/components/ui/help-tip";
import { Notice } from "@/components/ui/notice";
import { searchAnchor } from "@/lib/searchReveal";
import { usePiCurrentState } from "@/lib/query/pi";
import { isPiFamilyApp } from "@/config/appConfig";
import { isHermesReadOnlyProvider } from "@/config/hermesProviderPresets";
import {
  DshAccountCard,
  useDshAccountStatus,
} from "@/components/providers/DshAccount";
import {
  buildAdditiveSections,
  buildLaunchSections,
  buildSwitchSections,
  type CardPresentation,
  type ProviderSection,
  type SwitchModeInput,
} from "@/components/providers/presentation";

/** 切换式应用（Claude Code / Codex / Gemini CLI / Grok Build）的模式状态和动作，由供应商页传入。 */
export type SwitchModeProps = Omit<SwitchModeInput, "app" | "t" | "providers">;

const verticalDragModifiers: Modifier[] = [
  ({ transform }) => ({ ...transform, x: 0 }),
];

interface ProviderListProps {
  providers: Record<string, Provider>;
  currentProviderId: string;
  appId: AppId;
  /** 直连切换 / 共存式的添加 / Pi 的启用 / Claude Desktop 的切换 */
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
  /** OpenClaw 的默认模型、Hermes 的当前供应商 */
  onSetAsDefault?: (provider: Provider, modelId?: string) => void;
  /** 切换式应用必传：按模式 tab 算卡片 */
  switchMode?: SwitchModeProps;
  isLoading?: boolean;
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
  onSetAsDefault,
  switchMode,
  isLoading = false,
}: ProviderListProps) {
  const { t } = useTranslation();
  const { checkProvider, isChecking } = useStreamCheck(appId);
  const { sortedProviders, sensors, handleDragEnd } = useDragSort(
    providers,
    appId,
  );

  const {
    data: opencodeLiveIds,
    isPending: isOpencodeLiveIdsPending,
    error: opencodeLiveIdsError,
  } = useQuery({
    queryKey: ["opencodeLiveProviderIds"],
    queryFn: () => providersApi.getOpenCodeLiveProviderIds(),
    enabled: appId === "opencode",
  });
  const { data: opencodeDefaultModel } = useQuery({
    queryKey: ["opencodeDefaultModel"],
    queryFn: () => providersApi.getOpenCodeDefaultModel(),
    enabled: appId === "opencode",
  });
  const {
    data: openclawLiveIds,
    isPending: isOpenclawLiveIdsPending,
    error: openclawLiveIdsError,
  } = useOpenClawLiveProviderIds(appId === "openclaw");
  const {
    data: hermesLiveIds,
    isPending: isHermesLiveIdsPending,
    error: hermesLiveIdsError,
  } = useHermesLiveProviderIds(appId === "hermes");
  const { data: hermesModelConfig } = useHermesModelConfig(appId === "hermes");
  const { data: openclawDefaultModel } = useOpenClawDefaultModel(
    appId === "openclaw",
  );
  const isOpenCode = appId === "opencode";
  const { data: currentOmoId } = useCurrentOmoProviderId(isOpenCode);
  const { data: currentOmoSlimId } = useCurrentOmoSlimProviderId(isOpenCode);
  const {
    data: piCurrentState,
    isSuccess: isPiCurrentStateSuccess,
    isPending: isPiCurrentStatePending,
    isError: isPiCurrentStateError,
    error: piCurrentStateError,
  } = usePiCurrentState(isPiFamilyApp(appId), appId === "omp" ? "omp" : "pi");
  const isPiStateReady = !isPiFamilyApp(appId) || isPiCurrentStateSuccess;
  // 累加式应用要等 live 里有哪些供应商读回来（成功或失败）才知道卡片该进哪个分区；
  // 在那之前按加载中画骨架，不然卡片先全落进「可添加」，读回来再整体搬到「已添加」。
  // 一张卡都没有时不用等，直接出空状态
  const isLiveMembershipPending =
    (appId === "opencode" && isOpencodeLiveIdsPending) ||
    (appId === "openclaw" && isOpenclawLiveIdsPending) ||
    (appId === "hermes" && isHermesLiveIdsPending) ||
    (isPiFamilyApp(appId) && isPiCurrentStatePending);

  const isInConfig = useCallback(
    (provider: Provider): boolean => {
      switch (appId) {
        case "kimi":
        case "dsh":
        case "mcode":
          return provider.meta?.liveConfigManaged === true;
        case "opencode":
          return opencodeLiveIds?.includes(provider.id) ?? false;
        case "openclaw":
          return openclawLiveIds?.includes(provider.id) ?? false;
        case "hermes":
          return hermesLiveIds?.includes(provider.id) ?? false;
        case "pi":
        case "omp":
          return isPiStateReady
            ? (piCurrentState?.enabledProviderIds.includes(provider.id) ??
                false)
            : false;
        default:
          return true;
      }
    },
    [
      appId,
      opencodeLiveIds,
      openclawLiveIds,
      hermesLiveIds,
      isPiStateReady,
      piCurrentState,
    ],
  );

  const handleTest = useCallback(
    (provider: Provider) => {
      checkProvider(provider.id, provider.name);
    },
    [checkProvider],
  );

  const queryClient = useQueryClient();
  const importMutation = useMutation({
    mutationFn: async (): Promise<boolean> => {
      if (appId === "opencode") {
        return (await providersApi.importOpenCodeFromLive()) > 0;
      }
      if (appId === "openclaw") {
        return (await providersApi.importOpenClawFromLive()) > 0;
      }
      if (appId === "hermes") {
        return (await providersApi.importHermesFromLive()) > 0;
      }
      if (appId === "claude-desktop") {
        return (await providersApi.importClaudeDesktopFromClaude()) > 0;
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
    onError: (error: unknown) => {
      // Tauri invoke 的 reject 值是后端序列化出的纯字符串而非 Error 对象，
      // 取 .message 只会得到 undefined（空 toast）。
      toast.error(extractErrorMessage(error) || t("settings.importFailed"));
      // 导入失败前也可能已产生需要上屏的副作用：GrokBuild 官方登录态下点
      // 导入，命令层会先补种官方条目、随后才因 live 不可导入而报错。
      queryClient.invalidateQueries({ queryKey: ["providers", appId] });
    },
  });

  // DSH 登录了 DeepSeek 账号时，「已添加」最前面画一张官方账号卡（不是供应商行，见 DshAccountCard）
  const { data: dshAccount } = useDshAccountStatus(appId === "dsh");
  const dshAccountCard =
    appId === "dsh" && dshAccount?.signedIn ? dshAccount : null;
  const showDshAccountCard = Boolean(dshAccountCard);

  const sections = useMemo<ProviderSection[]>(() => {
    if (switchMode) {
      // 队列剔掉已不存在的 id：序号和上下移要按完整队列算
      const known = new Set(sortedProviders.map((p) => p.id));
      return buildSwitchSections({
        ...switchMode,
        app: appId,
        t,
        providers: sortedProviders,
        queue: switchMode.queue.filter((id) => known.has(id)),
      });
    }
    if (appId === "agy") {
      return buildLaunchSections({
        app: appId,
        t,
        providers: sortedProviders,
        openTerminal: (provider) => onOpenTerminal?.(provider),
      });
    }
    const defaultPrimary =
      (appId === "openclaw"
        ? openclawDefaultModel?.primary
        : appId === "opencode"
          ? opencodeDefaultModel
          : null) ?? "";
    const slash = defaultPrimary.indexOf("/");
    return buildAdditiveSections({
      app: appId,
      t,
      providers: sortedProviders,
      isInConfig,
      currentOmoId,
      currentOmoSlimId,
      defaultModel:
        slash > 0
          ? {
              providerId: defaultPrimary.slice(0, slash),
              model: defaultPrimary.slice(slash + 1),
            }
          : null,
      defaultModelOptions: (provider) => {
        if (appId === "opencode") {
          return getOpencodeDeclaredModels(
            provider.id,
            provider.settingsConfig,
          );
        }
        const config = provider.settingsConfig as OpenClawProviderConfig;
        if (!Array.isArray(config?.models)) return [];
        return config.models
          .filter((model) => typeof model.id === "string" && model.id.trim())
          .map((model) => ({ id: model.id, name: model.name }));
      },
      hermesCurrentId: hermesModelConfig?.provider ?? null,
      isHermesManaged: (provider) =>
        isHermesReadOnlyProvider(provider.settingsConfig),
      piStateUnavailable: isPiFamilyApp(appId) && !isPiStateReady,
      kimiCurrentId: currentProviderId,
      extraAdded: showDshAccountCard ? 1 : 0,
      actions: {
        add: onSwitch,
        remove: (provider) =>
          onRemoveFromConfig
            ? onRemoveFromConfig(provider)
            : onDelete(provider),
        disableOmo: (provider) =>
          provider.category === "omo-slim"
            ? onDisableOmoSlim?.()
            : onDisableOmo?.(),
        setDefault: (provider, modelId) => onSetAsDefault?.(provider, modelId),
      },
    });
  }, [
    switchMode,
    appId,
    t,
    sortedProviders,
    onSwitch,
    onOpenTerminal,
    openclawDefaultModel?.primary,
    opencodeDefaultModel,
    isInConfig,
    currentOmoId,
    currentOmoSlimId,
    hermesModelConfig?.provider,
    isPiStateReady,
    currentProviderId,
    showDshAccountCard,
    onRemoveFromConfig,
    onDelete,
    onDisableOmo,
    onDisableOmoSlim,
    onSetAsDefault,
  ]);

  const piStateRawError =
    isPiFamilyApp(appId) && isPiCurrentStateError
      ? extractErrorMessage(piCurrentStateError)
      : "";
  const piStateError =
    translatePiProviderMutationError(piStateRawError, t) || piStateRawError;
  const piStateErrorNotice =
    isPiFamilyApp(appId) && isPiCurrentStateError ? (
      <Notice
        tone="warning"
        title={t(`${appId}.current.readFailed`, {
          defaultValue: "无法读取 Pi 当前配置",
        })}
      >
        {t(`${appId}.current.stateUnavailableHint`)}
        {piStateError ? ` ${piStateError}` : ""}
      </Notice>
    ) : null;

  // 累加式应用读不了 live 配置（如 YAML 写坏了）时，列表和空状态看起来跟没配置过一样，
  // 添加 / 移除又会报错，所以在顶部说清楚
  const liveIdsError =
    appId === "opencode"
      ? opencodeLiveIdsError
      : appId === "openclaw"
        ? openclawLiveIdsError
        : appId === "hermes"
          ? hermesLiveIdsError
          : null;
  const liveIdsErrorMessage = liveIdsError
    ? extractErrorMessage(liveIdsError)
    : "";
  const liveConfigErrorNotice = liveIdsError ? (
    <Notice
      tone="warning"
      title={t("provider.liveConfigReadFailed", {
        app: t(`apps.${appId}`),
        defaultValue: "无法读取 {{app}} 配置文件",
      })}
    >
      {t("provider.liveConfigReadFailedHint", {
        defaultValue: "修好配置文件之前，添加、移除供应商都会失败。",
      })}
      {liveIdsErrorMessage ? ` ${liveIdsErrorMessage}` : ""}
    </Notice>
  ) : null;

  if (isLoading || (isLiveMembershipPending && sortedProviders.length > 0)) {
    return (
      <div className="space-y-2">
        {[0, 1, 2].map((index) => (
          <div
            key={index}
            className="h-[60px] w-full rounded-panel border border-dashed border-border bg-subtle"
          />
        ))}
      </div>
    );
  }

  if (sortedProviders.length === 0 && !dshAccountCard) {
    return (
      <div className="space-y-4">
        {piStateErrorNotice}
        {liveConfigErrorNotice}
        <ProviderEmptyState
          appId={appId}
          onCreate={isPiFamilyApp(appId) ? undefined : onCreate}
          onImport={
            isPiFamilyApp(appId) ||
            appId === "mcode" ||
            appId === "kimi" ||
            appId === "dsh"
              ? undefined
              : () => importMutation.mutate()
          }
        />
      </div>
    );
  }

  // 「当前」只用来决定官方订阅 / OAuth 额度自动刷新：Pi 没有当前项，OMO / Hermes 各看自己的当前项；
  // Antigravity 没有「启用」，每张卡都随时能打开，额度一律照常显示
  const isCurrentFor = (provider: Provider) => {
    if (isPiFamilyApp(appId)) return false;
    if (appId === "agy") return true;
    if (provider.category === "omo") return provider.id === currentOmoId;
    if (provider.category === "omo-slim")
      return provider.id === currentOmoSlimId;
    if (appId === "hermes") return provider.id === hermesModelConfig?.provider;
    return provider.id === currentProviderId;
  };

  const renderCard = (provider: Provider, presentation: CardPresentation) => (
    <SortableProviderCard
      key={provider.id}
      provider={provider}
      appId={appId}
      presentation={presentation}
      isCurrent={isCurrentFor(provider)}
      onEdit={onEdit}
      onDelete={onDelete}
      onDuplicate={onDuplicate}
      onConfigureUsage={
        onConfigureUsage ? (item) => onConfigureUsage(item) : () => undefined
      }
      onOpenWebsite={onOpenWebsite}
      // Antigravity 的「打开终端」已经是主按钮，「更多」菜单里不再重复
      onOpenTerminal={appId === "agy" ? undefined : onOpenTerminal}
      onTest={handleTest}
      isTesting={isChecking(provider.id)}
    />
  );

  const renderSectionTitle = (section: ProviderSection) => (
    <div className="flex items-center gap-1 text-caption font-semibold text-fg-2">
      <h2 className="m-0 text-caption font-semibold">{section.title}</h2>
      {section.help && (
        <HelpTip title={section.help.title}>{section.help.body}</HelpTip>
      )}
      {section.note && (
        <span className="ms-1 font-normal text-fg-3">{section.note}</span>
      )}
    </div>
  );

  // 卡片列表铺满主区域，和页头同宽
  return (
    <div className="space-y-4">
      {piStateErrorNotice}
      {liveConfigErrorNotice}
      <DndContext
        sensors={sensors}
        modifiers={verticalDragModifiers}
        collisionDetection={closestCenter}
        onDragEnd={handleDragEnd}
      >
        {sections.map((section) => (
          <section key={section.key} className="space-y-2">
            {section.title && (
              <div className="pt-1">{renderSectionTitle(section)}</div>
            )}
            {section.key === "added" &&
              showDshAccountCard &&
              dshAccountCard && <DshAccountCard status={dshAccountCard} />}
            {section.items.length === 0 &&
            section.emptyText &&
            !(section.key === "added" && showDshAccountCard) ? (
              <div className="rounded-panel border border-dashed border-border px-4 py-3 text-caption text-fg-3">
                {section.emptyText}
              </div>
            ) : (
              <SortableContext
                items={section.items.map((item) => item.provider.id)}
                strategy={verticalListSortingStrategy}
              >
                <div className="space-y-2">
                  {section.items.map((item) =>
                    renderCard(item.provider, item.presentation),
                  )}
                </div>
              </SortableContext>
            )}
          </section>
        ))}
      </DndContext>
    </div>
  );
}

interface SortableProviderCardProps {
  provider: Provider;
  appId: AppId;
  presentation: CardPresentation;
  isCurrent: boolean;
  onEdit: (provider: Provider) => void;
  onDelete: (provider: Provider) => void;
  onDuplicate: (provider: Provider) => void;
  onConfigureUsage: (provider: Provider) => void;
  onOpenWebsite: (url: string) => void;
  onOpenTerminal?: (provider: Provider) => void;
  onTest?: (provider: Provider) => void;
  isTesting: boolean;
}

function SortableProviderCard(props: SortableProviderCardProps) {
  const {
    setNodeRef,
    attributes,
    listeners,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: props.provider.id });

  const style: CSSProperties = {
    transform: CSS.Translate.toString(transform),
    transition,
    position: "relative",
    zIndex: isDragging ? 10 : undefined,
  };

  return (
    <div
      ref={setNodeRef}
      style={style}
      className="rounded-panel"
      {...searchAnchor(`provider:${props.provider.id}`)}
    >
      <ProviderCard
        {...props}
        dragHandleProps={{ attributes, listeners, isDragging }}
      />
    </div>
  );
}
