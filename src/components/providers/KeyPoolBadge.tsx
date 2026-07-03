import { useCallback, useMemo, useState } from "react";
import { useQueries, useQueryClient, useIsFetching } from "@tanstack/react-query";
import { KeyRound, AlertCircle, Clock, Battery, BatteryLow, BatteryMedium, BatteryFull, RefreshCw, Loader2, GripVertical } from "lucide-react";
import {
  DndContext,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  arrayMove as dndArrayMove,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { useTranslation } from "react-i18next";
import type { AppId } from "@/lib/api";
import { usageApi } from "@/lib/api/usage";
import {
  useApiKeys,
  pickActiveKey,
  useSetActiveApiKey,
  useUpdateApiKey,
  useReorderApiKeys,
} from "@/lib/query/apiKey";
import { cn } from "@/lib/utils";
import { ApiKeyStatusBar } from "./ApiKeyStatusBar";
import { Switch } from "@/components/ui/switch";
import { tagColorClasses } from "@/utils/tagColor";
import type { UsageResult } from "@/types";

/**
 * "Battery" status of the active key in a provider's key pool.
 *
 * Computed from the most-restrictive signal we have:
 *   - cooldown > now   → cooling_down
 *   - failure_count > 0 → degraded
 *   - enabled = false   → disabled
 *   - otherwise        → healthy
 *
 * The visual is intentionally battery-shaped so it's glanceable in a list of
 * ProviderCards: the user can spot the red/amber ones without opening the row.
 */
export type KeyHealth = "healthy" | "degraded" | "cooling_down" | "disabled";

interface KeyPoolBadgeProps {
  providerId: string;
  appId: AppId;
  /** When false, suppress the query entirely (e.g. before the provider is saved). */
  enabled?: boolean;
  className?: string;
}

export function KeyPoolBadge({
  providerId,
  appId,
  enabled = true,
  className,
}: KeyPoolBadgeProps) {
  const { t } = useTranslation();
  const { data: keys = [] } = useApiKeys(enabled ? providerId : null, appId);
  const activeKey = pickActiveKey(keys);

  // 徽章只挂在「真正的 key 池」上——keys.length > 1 才挂：
  //   - 单 key / 0 key：ProviderCard 的展开区 KeyPoolList 已经逐把显示
  //     状态，再多挂一个「1 key + 电池」徽章是冗余，挤占标题栏的视觉权重。
  //   - 多 key：徽章充当「一眼扫视」summary——count + 池里 active key 的
  //     健康度，让用户在卡片列表里第一时间看到「这把被冷却了」「这把
  //     失败了」之类，不需要每个 card 都展开。
  // 注：单 key 场景下徽章关闭是个权衡；如果用户反馈说"我想在头部看到
  // 这把 key 的当前状态"，再翻转。这里按现状保持 `> 1` 才显示。
  if (keys.length <= 1) return null;

  const health = computeKeyHealth(activeKey);
  const inCooldown =
    !!activeKey && activeKey.cooldownUntil * 1000 > Date.now();

  const Icon = pickBatteryIcon(health, inCooldown);
  const color = colorForHealth(health);
  const label = t(`keyPool.health.${health}`, {
    defaultValue: defaultLabel(health),
  });

  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[10px] font-semibold",
        color.bg,
        color.text,
        color.border,
        "border",
        className,
      )}
      title={label}
    >
      <KeyRound className="h-3 w-3" />
      {keys.length}
      <Icon className="h-3 w-3" />
    </span>
  );
}

/**
 * Expanded per-key panel shown when the user opens the ProviderCard.
 * Lists every key with its current runtime state — no actions, read-only
 * (the editor lives in the form's ApiKeyListSection).
 *
 * 当 provider 启用了 usage_script 且拥有多把 key 时，每把 key 都会触发
 * 自己的 per-key 用量查询（`queryProviderUsageForKey`），让"7d 配额 30/150"
 * 这种数字精确归到对应的 key 上。React Query 按 (keyId, appId) 去重，
 * pool 里 N 把 key 就有 N 份独立快照，互不污染。
 */
interface KeyPoolListProps {
  providerId: string;
  appId: AppId;
  enabled?: boolean;
  /** 用作 per-key 查询尚未返回时的兜底；正常情况下是 null。 */
  usage?: import("@/types").UsageResult | null;
  usageEnabled?: boolean;
  /** provider 级别的 autoQueryInterval（分钟，0 = 关闭自动刷新）。 */
  autoQueryInterval?: number;
}

export function KeyPoolList({
  providerId,
  appId,
  enabled = true,
  usage: providerUsageFallback,
  usageEnabled = false,
  autoQueryInterval = 0,
}: KeyPoolListProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data: keys = [] } = useApiKeys(enabled ? providerId : null, appId);
  const activeKey = pickActiveKey(keys);
  // 任一 quota 100% 的 key id 集合——由 ApiKeyStatusBar 通过 onExhaustedChange
  // 上报。row 用它来给整行加红色背景。每把 key 独立上报，互不污染。
  const [exhaustedKeyIds, setExhaustedKeyIds] = useState<Set<string>>(
    () => new Set(),
  );

  // 视觉排序：active key 置顶 —— 「激活 = 跳到池顶」是用户预期的默认
  // 行为（与编辑面板里的 sortedKeys 同源）。DB sort_index 不动，纯展示态
  // 重排；用户拖拽时 cursor 仍按 DB 顺序算（避免 active 钉顶造成的「拖到
  // 上方却看似无反应」旧 bug——见 ApiKeyListSection 的 move() 注释）。
  const sortedKeys = activeKey
    ? [
        ...keys.filter((k) => k.id === activeKey.id),
        ...keys.filter((k) => k.id !== activeKey.id),
      ]
    : keys;

  // ─── 手动 reorder + set active ──────────────────────────────
  // Provider list 页面的 KeyPoolList 是个只读 panel，原本没有这两个动作。
  // 编辑面板（ApiKeyListSection）已经支持拖拽 + 设为默认；这里把同样
  // 的入口暴露出来，让用户不用切到「编辑」也能调整 active / 顺序——
  // 切回旧路径「编辑→保存」会触发整个 provider 的 SSOT 重写，没必要。
  const setActive = useSetActiveApiKey();
  const reorder = useReorderApiKeys();
  const updateApiKey = useUpdateApiKey();

  // PointerSensor + 8px activation 距离：避免点按钮被误判为 drag start，
  // 与 ApiKeyListSection 的 row 行为保持一致。
  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: { distance: 8 },
    }),
  );

  const onDragEnd = useCallback(
    (event: DragEndEvent) => {
      const { active, over } = event;
      if (!over || active.id === over.id) return;
      // 按 DB 顺序算索引（不受 active 钉顶影响），与 ApiKeyListSection 一致。
      const oldIndex = keys.findIndex((k) => k.id === active.id);
      const newIndex = keys.findIndex((k) => k.id === over.id);
      if (oldIndex === -1 || newIndex === -1) return;
      const next = dndArrayMove(keys, oldIndex, newIndex);
      reorder.mutate({
        appType: appId,
        providerId,
        orderedIds: next.map((k) => k.id),
      });
    },
    [keys, reorder, appId, providerId],
  );

  // ─── 手动刷新 ───────────────────────────────────────────────
  // 同时刷新三类数据：
  //   1) `["apiKeys", providerId, appId]`——冷却 / 失败计数 / 启用状态
  //   2) `["keyUsage", keyId, appId]`——每把 key 自己的用量配额进度
  //   3) `["usage", providerId, appId]`（若存在）——provider-level 兜底
  // `invalidateQueries` 标记为 stale 并触发 refetch，UI 上 useQueries 的
  // `isFetching` 翻 true → 用 Loader2 旋转图标给出反馈。
  // 注意：disable 状态由后端在「key 达到失败阈值时自动改 enabled=false」，
  // 也会反映在 `apiKeys` query 里——必须 invalidate 它。
  const handleRefresh = () => {
    queryClient.invalidateQueries({
      queryKey: ["apiKeys", providerId, appId],
    });
    keys.forEach((k) => {
      queryClient.invalidateQueries({
        queryKey: ["keyUsage", k.id, appId],
      });
    });
    queryClient.invalidateQueries({
      queryKey: ["usage", providerId, appId],
    });
  };

  // 5h 配额窗口到点重置：每把 key 各自挂 setTimeout，倒计到 0 时由
  // <ApiKeyStatusBar onFiveHourResetReached> 触发本回调。我们只 invalidate
  // 这一把 key 的 `keyUsage`，再顺手 invalidate 一次全局 `apiKeys`
  // （key 可能在 5h 临界点同时解锁 / 重新进入 cooldown）。provider-level
  // 的 `usage` 兜底查询保持不动——它的 reset 节奏独立于单把 key 的 5h。
  //
  // 跟 handleRefresh 区别：
  //   - handleRefresh：用户主动点击按钮，同时刷所有 key，UI 给反馈。
  //   - handleKeyResetReached：纯后台静默触发，5h 窗口到点自动 invalidate，
  //     用户无需操作，UI 上的 `isFetching` 翻动也很短暂（通常 < 500ms）。
  // 不共用 handleRefresh 是为了在后台路径里避免无谓的循环（只刷自己
  // 需要的 key，不扇出到整个 pool）。
  const handleKeyResetReached = useCallback(
    (keyId: string) => {
      queryClient.invalidateQueries({
        queryKey: ["keyUsage", keyId, appId],
      });
      queryClient.invalidateQueries({
        queryKey: ["apiKeys", providerId, appId],
      });
    },
    [queryClient, providerId, appId],
  );

  // 每把 key 自己的用量：useQueries 扇出，React Query 按 (keyId, appId) 自动
  // 去重——同 keyId 多次出现（拖拽、re-render）只发一次请求。`enabled` 双控：
  //   - usageEnabled：父组件判断 provider 启用了 usage_script；
  //   - k.enabled：单把 key 关闭时不消耗一次 API 调用。
  // query key 故意不带 providerId；keyId 唯一即可。
  const perKeyQueries = useQueries({
    queries: keys.map((k) => ({
      queryKey: ["keyUsage", k.id, appId] as const,
      queryFn: () => usageApi.queryForKey(providerId, k.id, appId),
      enabled: usageEnabled && k.enabled,
      retry: 1,
      retryDelay: 1500,
      staleTime:
        autoQueryInterval > 0
          ? autoQueryInterval * 60 * 1000
          : 5 * 60 * 1000,
      gcTime: 10 * 60 * 1000,
      refetchInterval:
        autoQueryInterval > 0
          ? Math.max(autoQueryInterval, 1) * 60 * 1000
          : false,
      refetchIntervalInBackground: true,
      refetchOnWindowFocus: false,
    })),
  });
  // 把 query 结果按 keyId 建索引，避免每次 O(N^2) 找 row 对应数据。
  const usageByKeyId = useMemo(() => {
    const map = new Map<string, UsageResult | null>();
    keys.forEach((k, i) => {
      const data = perKeyQueries[i]?.data as UsageResult | undefined;
      map.set(k.id, data ?? null);
    });
    return map;
  }, [keys, perKeyQueries]);

  // 「refreshing」状态：监听所有相关 query 的 in-flight 数量。
  // 不用局部 boolean 是因为 React Query 已经在管 refetch 生命周期，
  // 直接读它的计数器最准确——即便 handleRefresh 被并发触发也不会丢状态。
  // `isApiKeysFetching` 必须放在 hooks 顶层；放在循环里会违反 Rules of Hooks。
  // per-key 状态直接读 useQueries 返回的 `q.isFetching`，无需再 hook。
  const isApiKeysFetching = useIsFetching({
    queryKey: ["apiKeys", providerId, appId],
  });
  const isPerKeyFetching = perKeyQueries.some((q) => q.isFetching);
  const isRefreshing = isApiKeysFetching > 0 || isPerKeyFetching;

  if (keys.length === 0) return null;

  return (
    <div className="mt-3 space-y-1.5">
      <div className="flex items-center justify-between">
        <h5 className="text-xs font-semibold text-muted-foreground">
          {t("keyPool.listTitle", {
            count: keys.length,
            defaultValue: `API Key 池 (${keys.length})`,
          })}
        </h5>
        <div className="flex items-center gap-2">
          <span className="text-[10px] text-muted-foreground/70">
            {t("keyPool.listHint", {
              defaultValue: "代理模式下自动轮换；编辑面板在「编辑」中",
            })}
          </span>
          {/* 手动刷新：把按钮放在标题行最右侧（listHint 之后），避免与每
              把 key 的状态行抢视觉空间。loading 期间把图标换成 Loader2
              旋转——区别于静态的 RefreshCw，给用户「请求已发出」的明确
              反馈。同时 disable 防止并发点击导致重复 refetch。 */}
          <button
            type="button"
            onClick={handleRefresh}
            disabled={isRefreshing}
            aria-label={t("keyPool.refresh", { defaultValue: "刷新" })}
            title={
              isRefreshing
                ? t("keyPool.refreshing", { defaultValue: "刷新中…" })
                : t("keyPool.refresh", { defaultValue: "刷新" })
            }
            className={cn(
              "inline-flex h-5 w-5 items-center justify-center rounded-md",
              "text-muted-foreground/70 hover:text-foreground hover:bg-muted/60",
              "transition-colors disabled:cursor-not-allowed disabled:opacity-60",
              "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
            )}
          >
            {isRefreshing ? (
              <Loader2 className="h-3 w-3 animate-spin" />
            ) : (
              <RefreshCw className="h-3 w-3" />
            )}
          </button>
        </div>
      </div>
      <DndContext
        sensors={sensors}
        collisionDetection={closestCenter}
        onDragEnd={onDragEnd}
      >
        <SortableContext
          items={sortedKeys.map((k) => k.id)}
          strategy={verticalListSortingStrategy}
        >
          <ul className="space-y-1">
            {sortedKeys.map((k) => {
              const isActive = k.id === activeKey?.id;
              const inCooldown = k.cooldownUntil * 1000 > Date.now();
              const health = computeKeyHealth(k);
              const healthColor = colorForHealth(health);
              // 优先 per-key 数据；尚未返回时回退到父组件传入的 provider-level 快照，
              // 避免第一帧 key 行空白闪烁。
              const perKeyUsage = usageByKeyId.get(k.id) ?? null;
              const rowUsage = perKeyUsage ?? providerUsageFallback ?? null;
              // 与 ApiKeyListSection 保持一致：只在 per-key query 真的成功且有数据
              // 时才让 ApiKeyStatusBar 渲染配额条。否则 rowUsage 是 provider-level
              // 兜底（用的还是老的 active key），不能让它顶替当前 key 的进度条——
              // 那会显示一个"看似正确"但其实跟本 key 无关的百分比，误导用户。
              // 这里强制按 per-key 成功与否决定；provider-level 兜底仍用于上方 key
              // 行的瞬态占位（rowUsage 那一行）。
              const rowUsageEnabled =
                usageEnabled &&
                !!perKeyUsage?.success &&
                !!perKeyUsage.data &&
                perKeyUsage.data.length > 0;

              // 是否锁定「加入轮换」开关——
              // 1) 任一 quota 100%（与 isExhausted 同一信号）
              // 2) API 失效：query 失败、success=false 或 data 为空
              //    （成功但无数据意味着上游没返回有效用量；通常是脚本
              //     模板/网络层错）
              // 3) Key 已进 cooldown（被轮换耗尽或被 proactive rotation 标
              //    记），用户重新打开也只会再被冻回去
              const apiInvalid =
                usageEnabled &&
                (perKeyUsage === null ||
                  perKeyUsage.success !== true ||
                  !perKeyUsage.data ||
                  perKeyUsage.data.length === 0);
              const isRotationBlocked =
                exhaustedKeyIds.has(k.id) || inCooldown || apiInvalid;
              const rotationBlockedReason = exhaustedKeyIds.has(k.id)
                ? t("keyPool.rotationBlockedExhausted", {
                    defaultValue: "5h/7d 配额已耗尽",
                  })
                : inCooldown
                  ? t("keyPool.rotationBlockedCooldown", {
                      defaultValue: "冷却中，轮换已禁用",
                    })
                  : apiInvalid
                    ? t("keyPool.rotationBlockedApiInvalid", {
                        defaultValue: "API 无响应或失效，已禁用轮换",
                      })
                    : undefined;

              return (
                <SortableKeyRow
                  key={k.id}
                  keyId={k.id}
                  isActive={isActive}
                  inCooldown={inCooldown}
                  healthColor={healthColor}
                  label={
                    k.label ||
                    t("keyPool.unnamed", { defaultValue: "(未命名)" })
                  }
                  tags={k.tags}
                  enabled={k.enabled}
                  failureCount={k.failureCount}
                  cooldownUntil={k.cooldownUntil}
                  rowUsage={rowUsage}
                  rowUsageEnabled={rowUsageEnabled}
                  onSetActive={() =>
                    setActive.mutate({
                      providerId,
                      appType: appId,
                      keyId: k.id,
                    })
                  }
                  onToggleEnabled={() =>
                    updateApiKey.mutate({
                      keyId: k.id,
                      payload: { enabled: !k.enabled },
                    })
                  }
                  isExhausted={exhaustedKeyIds.has(k.id)}
                  isRotationBlocked={isRotationBlocked}
                  rotationBlockedReason={rotationBlockedReason}
                  onExhaustedChange={(ex) => {
                    setExhaustedKeyIds((prev) => {
                      const next = new Set(prev);
                      if (ex) next.add(k.id);
                      else next.delete(k.id);
                      return next;
                    });
                  }}
                  onFiveHourResetReached={() => handleKeyResetReached(k.id)}
                />
              );
            })}
          </ul>
        </SortableContext>
      </DndContext>
    </div>
  );
}

// ─────────── Sortable row ───────────

interface SortableKeyRowProps {
  /** Stable id for dnd-kit — must match `SortableContext.items`. */
  keyId: string;
  isActive: boolean;
  inCooldown: boolean;
  healthColor: ReturnType<typeof colorForHealth>;
  label: string;
  tags: string[];
  enabled: boolean;
  failureCount: number;
  cooldownUntil: number;
  rowUsage: UsageResult | null;
  rowUsageEnabled: boolean;
  isExhausted: boolean;
  /** 任一 quota 100% 或 API 失效时为 true，禁用轮换开关 */
  isRotationBlocked: boolean;
  /** 行级禁用原因（用于 hover title 解释为什么 toggle 不能点） */
  rotationBlockedReason?: string;
  onSetActive: () => void;
  onToggleEnabled: () => void;
  onExhaustedChange: (exhausted: boolean) => void;
  onFiveHourResetReached: () => void;
}

/**
 * 单把 key 的可拖拽 row。
 *
 * dnd-kit `useSortable` 让整行可拖——但与编辑面板里的 row 不同，
 * 这里池子里 key 数量通常 2-5 把，pool 一行视觉高度只有 ~50px，
 * 给整行加 `cursor-grab` 反而容易让"点 set-active"误判为 drag。
 * 解法：拖拽 handle 限定到 GripVertical icon，按钮（setActive）
 * 显式 `onPointerDown stopPropagation` 阻断 dnd-kit。
 *
 * 视觉态跟编辑面板一致：active key 蓝边/蓝底，4px 蓝条由
 * border-l-4 提供。
 */
function SortableKeyRow({
  keyId,
  isActive,
  inCooldown,
  healthColor,
  label,
  tags,
  enabled,
  failureCount,
  cooldownUntil,
  rowUsage,
  rowUsageEnabled,
  isExhausted,
  isRotationBlocked,
  rotationBlockedReason,
  onSetActive,
  onToggleEnabled,
  onExhaustedChange,
  onFiveHourResetReached,
}: SortableKeyRowProps) {
  const { t } = useTranslation();
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: keyId });

  const style: React.CSSProperties = {
    transform: CSS.Transform.toString(transform),
    transition,
    zIndex: isDragging ? 10 : undefined,
    opacity: isDragging ? 0.85 : 1,
  };

  return (
    <li
      ref={setNodeRef}
      style={style}
      {...attributes}
      className={cn(
        "rounded-md border px-2 py-1 text-xs",
        isActive
          ? "border-blue-400/60 bg-blue-50/40 dark:border-blue-700/50 dark:bg-blue-950/20"
          : isExhausted
            ? // 任一 quota 100%——浅红底，弱化「bar 不可见 = 信号丢失」的风险。
              "border-red-400/60 bg-red-50/40 dark:border-red-700/50 dark:bg-red-950/20"
            : "border-border-default bg-card/40",
      )}
    >
      {/* 头部一行：drag handle + 标识 + label + 健康徽标 + set-active 按钮 */}
      {/* Grid 布局 5 列：
            [drag] [active-dot] [label+badges] [activate/activated] [rotation-toggle]
            label 列 1fr 占满剩余空间；其它四列 auto。
            Switch 选中状态对应该 key 是否参与轮换——关闭时 KeyRing 的
            next_key 不会选中这把，强制绕过限流 / 余额耗尽的 key。
            单独的「停用」标签已不再展示在 header（Switch 关态本身就是
            视觉锚点），但 ApiKeyStatusBar 仍读 enabled 用于状态行文案。 */}
      <div className="grid grid-cols-[auto_auto_1fr_auto_auto] items-center gap-2">
        <button
          type="button"
          // 让 GripVertical 成为唯一拖拽入口——避免误触 setActive / Switch
          // 时被 dnd-kit 当成"开始拖"。onPointerDown 不需要 stopPropagation：
          // listeners 直接挂在这个 button 上，不会冒泡。
          {...listeners}
          className="flex h-4 w-3 cursor-grab items-center justify-center text-muted-foreground/60 hover:text-foreground active:cursor-grabbing"
          aria-label={t("keyPool.dragHandle", {
            defaultValue: "拖动以重新排序",
          })}
        >
          <GripVertical className="h-3 w-3" />
        </button>
        {/* 激活状态：绿点（在 label 左侧）。灰点（非 active）和绿点
            （active）形状一致，仅颜色变化，保证 row 高度对齐。 */}
        {isActive ? (
          <span
            className="h-2 w-2 flex-shrink-0 rounded-full bg-emerald-500 shadow-[0_0_0_2px_rgba(16,185,129,0.18)]"
            aria-label={t("keyPool.active", {
              defaultValue: "Active",
            })}
          />
        ) : (
          <span
            className="h-2 w-2 flex-shrink-0 rounded-full bg-muted-foreground/40"
            aria-hidden
          />
        )}
        {/* 中间列：label + 健康徽标 + tags。两层结构 —— 上层 label 与徽标
            inline 横排，下层 tags 单独一行。tags 不与徽标同层，避免窄
            行时徽标被挤换行后跟标签挤一团。tag 配色复用 ApiKeyListSection
            里的 hash 调色板（蓝/绿/琥珀/玫瑰/紫/青），保持跨面板视觉
            一致——同一个 tag 在「编辑面板 row」与「provider 列表 row」
            上颜色相同。 */}
        <div className="flex min-w-0 flex-col gap-1">
          <div className="flex items-center gap-1.5 flex-wrap">
            <span className="min-w-0 truncate font-medium">{label}</span>
            {inCooldown ? (
              <span
                className={cn(
                  "inline-flex items-center gap-1 text-[10px]",
                  healthColor.text,
                )}
              >
                <Clock className="h-3 w-3" />
                {t("keyPool.cooldown", { defaultValue: "冷却" })}
              </span>
            ) : failureCount > 0 ? (
              <span
                className={cn(
                  "inline-flex items-center gap-1 text-[10px]",
                  healthColor.text,
                )}
              >
                <AlertCircle className="h-3 w-3" />
                {Math.min(failureCount, 5)}
              </span>
            ) : null}
          </div>
          {tags.length > 0 && (
            <div className="flex flex-wrap items-center gap-1">
              {tags.map((tag) => (
                <span
                  key={tag}
                  className={cn(
                    "rounded-full px-1.5 py-px text-[9px] font-medium",
                    tagColorClasses(tag),
                  )}
                >
                  {tag}
                </span>
              ))}
            </div>
          )}
        </div>
        {/* Activate / Activated 标签：active 时显示绿色"Activated"标签
            且不可点击；非 active 时显示"Activate"按钮，点击触发 setActive。
            两种状态视觉对比强，但形状一致保持 row 高度。
            锁定条件：quota 100% / API 失效 / 冷却中——这把 key 不能切为
            active，否则 5h 桶继续打满无意义；title 提示锁定原因。 */}
        <button
          type="button"
          onClick={onSetActive}
          disabled={isActive || isRotationBlocked}
          onPointerDown={(e) => e.stopPropagation()}
          title={
            isRotationBlocked && !isActive
              ? rotationBlockedReason ??
                t("keyPool.rotationBlocked", {
                  defaultValue: "该 API key 暂时不可用，已禁用轮换",
                })
              : t("keyPool.setActive", { defaultValue: "设为默认" })
          }
          aria-label={t("keyPool.setActive", { defaultValue: "设为默认" })}
          className={cn(
            "inline-flex h-5 items-center rounded-md px-1.5 text-[10px] font-semibold transition-colors",
            isActive
              ? "bg-emerald-100 text-emerald-700 dark:bg-emerald-950/40 dark:text-emerald-300 cursor-default"
              : isRotationBlocked
                ? // 锁定时仍允许视觉看到「这是 Activate 按钮」，但调暗 +
                  //   cursor-not-allowed 让用户感知不能点；hover 不变色
                  "text-muted-foreground/40 cursor-not-allowed"
                : "text-muted-foreground/80 hover:text-foreground hover:bg-muted/60",
          )}
        >
          {isActive
            ? t("keyPool.activatedLabel", { defaultValue: "Activated" })
            : t("keyPool.activateLabel", { defaultValue: "Activate" })}
        </button>
        {/* 轮换参与开关：关掉后 KeyRing.next_key 跳过这把 key，
            但不删 row——用户切回来只需再打开。与编辑面板里的 Switch
            行为一致，复用 useUpdateApiKey。 */}
        <Switch
          checked={enabled}
          onCheckedChange={onToggleEnabled}
          onPointerDown={(e) => e.stopPropagation()}
          aria-label={t("keyPool.toggleRotation", {
            defaultValue: "加入轮换",
          })}
        />
      </div>
      {/* 状态条：冷却倒计时 + 该 key 自己的用量进度条。per-key 数据由
          useQueries 拉取，Rust 端 queryProviderUsageForKey 使用
          provider_api_keys 里这一行自己的 api_key 跑脚本。 */}
      <ApiKeyStatusBar
        keyId={keyId}
        cooldownUntil={cooldownUntil}
        failureCount={failureCount}
        enabled={enabled}
        usage={rowUsage}
        usageEnabled={rowUsageEnabled}
        onFiveHourResetReached={onFiveHourResetReached}
        onExhaustedChange={onExhaustedChange}
      />
    </li>
  );
}

// ─────────── helpers ───────────

function computeKeyHealth(key: {
  enabled: boolean;
  cooldownUntil: number;
  failureCount: number;
} | null | undefined): KeyHealth {
  if (!key) return "disabled";
  if (!key.enabled) return "disabled";
  if (key.cooldownUntil * 1000 > Date.now()) return "cooling_down";
  if (key.failureCount > 0) return "degraded";
  return "healthy";
}

function pickBatteryIcon(
  health: KeyHealth,
  inCooldown: boolean,
): React.ComponentType<{ className?: string }> {
  if (inCooldown) return Clock;
  switch (health) {
    case "healthy":
      return BatteryFull;
    case "degraded":
      return BatteryMedium;
    case "cooling_down":
      return BatteryLow;
    case "disabled":
      return Battery;
    default:
      return Battery;
  }
}

function colorForHealth(health: KeyHealth) {
  switch (health) {
    case "healthy":
      return {
        bg: "bg-emerald-50 dark:bg-emerald-950/30",
        text: "text-emerald-700 dark:text-emerald-300",
        border: "border-emerald-200 dark:border-emerald-800",
      };
    case "degraded":
      return {
        bg: "bg-amber-50 dark:bg-amber-950/30",
        text: "text-amber-700 dark:text-amber-300",
        border: "border-amber-200 dark:border-amber-800",
      };
    case "cooling_down":
      return {
        bg: "bg-orange-50 dark:bg-orange-950/30",
        text: "text-orange-700 dark:text-orange-300",
        border: "border-orange-200 dark:border-orange-800",
      };
    case "disabled":
    default:
      return {
        bg: "bg-slate-100 dark:bg-slate-800/40",
        text: "text-slate-600 dark:text-slate-300",
        border: "border-slate-200 dark:border-slate-700",
      };
  }
}

function defaultLabel(health: KeyHealth): string {
  switch (health) {
    case "healthy":
      return "正常";
    case "degraded":
      return "有失败";
    case "cooling_down":
      return "冷却中";
    case "disabled":
      return "已停用";
  }
}
