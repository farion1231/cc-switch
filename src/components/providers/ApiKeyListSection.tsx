import { useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { useQueries, useQueryClient } from "@tanstack/react-query";
import {
  DndContext,
  PointerSensor,
  useSensor,
  useSensors,
  closestCenter,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  arrayMove as dndArrayMove,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import {
  Eye,
  EyeOff,
  Plus,
  Trash2,
  CircleDot,
  Loader2,
  GripVertical,
  AlertCircle,
  Clock,
  Pencil,
  X,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { Badge } from "@/components/ui/badge";
import { Label } from "@/components/ui/label";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { ApiKeyStatusBar } from "./ApiKeyStatusBar";
import type { UsageResult } from "@/types";
import { cn } from "@/lib/utils";
import { tagColorClasses } from "@/utils/tagColor";
import {
  useApiKeys,
  pickActiveKey,
  useCreateApiKey,
  useUpdateApiKey,
  useDeleteApiKey,
  useSetActiveApiKey,
  useReorderApiKeys,
} from "@/lib/query/apiKey";
import { usageApi } from "@/lib/api/usage";
import type { ApiKeyDto } from "@/lib/api/apiKey";
import type { AppId } from "@/lib/api";

/**
 * Per-provider API key pool editor.
 *
 * - Lists existing keys (up/down reorder; full drag-sort is a polish pass).
 * - Add / edit / delete / set-active.
 * - Surfaces runtime state (cooldown, failure count, last error) that
 *   the proxy's KeyRing writes back to the DB.
 *
 * Independent of the parent form's `settingsConfig`. The legacy `apiKey`
 * field in the JSON still exists for non-pool clients; Phase 10 will
 * keep that field in sync with `provider_api_keys.is_active=1` when
 * the proxy takes over.
 */
interface ApiKeyListSectionProps {
  appId: AppId;
  /**
   * `null` while the provider hasn't been saved yet (we don't have a
   * primary key to hang keys off of). The section renders an inline
   * hint instead of an empty list.
   */
  providerId: string | null;
}

export function ApiKeyListSection({ appId, providerId }: ApiKeyListSectionProps) {
  const { t } = useTranslation();
  const enabled = !!providerId;
  const { data: keys = [], isLoading } = useApiKeys(
    enabled ? providerId : null,
    appId,
  );
  const activeKey = pickActiveKey(keys);

  // 每把 key 自己的用量：useQueries 扇出，React Query 按 (keyId, appId) 自动
  // 去重——同 keyId 多次出现（拖拽、re-render）只发一次请求。结果按 sortedKeys
  // 的顺序铺到 row 上。注意：query key 故意不带 providerId；keyId 唯一即可。
  // 用户切到另一个 provider 时，cache 里这份 (keyId, appId) 也保留（10 分钟
  // gcTime），不会因为切走立刻扔掉——避免来回切时反复闪"loading"。
  const perKeyQueries = useQueries({
    queries: keys.map((k) => ({
      queryKey: ["keyUsage", k.id, appId] as const,
      queryFn: () => {
        if (!providerId) {
          return Promise.resolve({
            success: false,
            data: undefined,
            error: "no providerId",
          } as UsageResult);
        }
        return usageApi.queryForKey(providerId, k.id, appId);
      },
      enabled: enabled && k.enabled,
      retry: 1,
      retryDelay: 1500,
      staleTime: 5 * 60 * 1000,
      gcTime: 10 * 60 * 1000,
    })),
  });
  // 把 query 结果按 keyId 建索引，避免 O(N) 每次 O(N^2) 找 row 对应数据。
  const usageByKeyId = new Map<string, UsageResult | null>();
  keys.forEach((k, i) => {
    const data = perKeyQueries[i]?.data;
    usageByKeyId.set(k.id, (data as UsageResult | undefined) ?? null);
  });

  const [addingNew, setAddingNew] = useState(false);
  const [editingKeyId, setEditingKeyId] = useState<string | null>(null);
  // 待删除的 key——点击行内回收站图标先弹确认框，确认后才真正调
  // mutation。删除是不可逆操作，给个二次确认比 toast 「已删除」更稳。
  const [pendingDeleteKeyId, setPendingDeleteKeyId] = useState<string | null>(
    null,
  );
  // 5h / 7d 窗口到点重置时由 ApiKeyStatusBar 回调到本 section——invalidate
  // 对应 (keyId, appId) 缓存 + 整个 pool 的 list。React Query 自动 refetch
  // 拿到新窗口的 quota 数据，bar 实时更新。
  // 与 KeyPoolList::handleKeyResetReached 同一语义。
  const queryClient = useQueryClient();
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
  // 任一 quota 100% 的 key id 集合——由 ApiKeyStatusBar 通过 onExhaustedChange
  // 上报。row 用它来给整行加红色背景，弱化「bar 不可见 = 信号丢失」。
  // 用 Set 而非单个 boolean：每把 key 独立上报，互不污染。
  const [exhaustedKeyIds, setExhaustedKeyIds] = useState<Set<string>>(
    () => new Set(),
  );

  // Active key 一律置顶——它是用户最关心的那把，列表里也最容易被误以为在轮换。
  // 其他 key 保持 sortIndex 顺序不变。
  const sortedKeys = activeKey
    ? [
        ...keys.filter((k) => k.id === activeKey.id),
        ...keys.filter((k) => k.id !== activeKey.id),
      ]
    : keys;

  const setActive = useSetActiveApiKey();
  const remove = useDeleteApiKey();
  const reorder = useReorderApiKeys();

  // 拖拽落点回调。PointerSensor + 8px activation 距离防止按钮点击误触发拖拽。
  // 实际 reorder 走的是 arrayMove — 数据库里写回新顺序后，sortedKeys 会重新
  // 计算把 active 放到顶，UI 永远一致。按 DB 顺序算索引，与 active 解耦。
  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: { distance: 8 },
    }),
  );

  const onDragEnd = useCallback(
    (event: DragEndEvent) => {
      const { active, over } = event;
      if (!over || active.id === over.id || !providerId) return;
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
    [providerId, appId, keys, reorder],
  );

  if (!enabled) {
    return (
      <div className="rounded-lg border border-dashed border-border-default p-4 text-sm text-muted-foreground">
        {t("apiKeyList.saveProviderFirst", {
          defaultValue: "保存供应商后可管理多把 API Key",
        })}
      </div>
    );
  }

  if (isLoading) {
    return (
      <div className="flex items-center gap-2 text-sm text-muted-foreground p-2">
        <Loader2 className="h-4 w-4 animate-spin" />
        {t("common.loading", { defaultValue: "加载中…" })}
      </div>
    );
  }

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <div>
          <h4 className="text-sm font-medium text-foreground">
            {t("apiKeyList.title", { defaultValue: "API Key 池" })}
          </h4>
          <p className="text-xs text-muted-foreground mt-0.5">
            {t("apiKeyList.subtitle", {
              defaultValue:
                "为这个供应商维护多把 key。代理模式下会自动轮换，星标 = 写入 settings.json 的那把。",
            })}
          </p>
        </div>
        {!addingNew && (
          <Button
            size="sm"
            variant="outline"
            type="button"
            onClick={() => setAddingNew(true)}
          >
            <Plus className="h-4 w-4 mr-1" />
            {t("apiKeyList.add", { defaultValue: "添加 Key" })}
          </Button>
        )}
      </div>

      {keys.length === 0 && !addingNew ? (
        <div className="rounded-lg border border-dashed border-border-default p-6 text-center text-sm text-muted-foreground">
          {t("apiKeyList.empty", {
            defaultValue: "还没有任何 key，点击右上角添加。",
          })}
        </div>
      ) : (
        <DndContext
          sensors={sensors}
          collisionDetection={closestCenter}
          onDragEnd={onDragEnd}
        >
          <SortableContext
            items={sortedKeys.map((k) => k.id)}
            strategy={verticalListSortingStrategy}
          >
            <ul className="space-y-2">
              {sortedKeys.map((k) =>
                editingKeyId === k.id ? (
                  // 编辑态的 row 不参与排序——拖它会让交互体验很奇怪。
                  <EditKeyInlineForm
                    key={k.id}
                    apiKey={k}
                    onDone={() => setEditingKeyId(null)}
                    onCancel={() => setEditingKeyId(null)}
                  />
                ) : (
                  <ApiKeyRow
                    key={k.id}
                    apiKey={k}
                    isActive={k.id === activeKey?.id}
                    onSetActive={() =>
                      setActive.mutate({
                        providerId,
                        appType: appId,
                        keyId: k.id,
                      })
                    }
                    onDelete={() => setPendingDeleteKeyId(k.id)}
                    onEditStart={() => setEditingKeyId(k.id)}
                    usage={usageByKeyId.get(k.id) ?? null}
                    usageEnabled={
                      // 仅当这把 key 的 query 真的成功且有数据时，才让
                      // ApiKeyStatusBar 渲染配额条。
                      // 注意：这里**不**检查 k.enabled——key 5h/7d 100% 后
                      // 会自动 enabled=false，但用户仍然想看到 100% 的
                      // 红色 row + 进度条（“这把废了”信号），不能让 bar 消失。
                      // per-key query 在 enabled=false 后停止 fetch，但
                      // React Query 缓存保留上一份成功结果——上面 success 检查
                      // 仍为 true，bar 继续渲染。
                      (() => {
                        const r = usageByKeyId.get(k.id);
                        return !!r?.success && !!r.data && r.data.length > 0;
                      })()
                    }
                    isExhausted={exhaustedKeyIds.has(k.id)}
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
                ),
              )}
              {addingNew && (
                <NewKeyInlineForm
                  providerId={providerId}
                  appId={appId}
                  onDone={() => setAddingNew(false)}
                  onCancel={() => setAddingNew(false)}
                />
              )}
            </ul>
          </SortableContext>
        </DndContext>
      )}

      <ConfirmDialog
        isOpen={pendingDeleteKeyId !== null}
        title={t("apiKeyList.deleteConfirmTitle", {
          defaultValue: "Delete API Key?",
        })}
        message={t("apiKeyList.deleteConfirmMessage", {
          label:
            keys.find((k) => k.id === pendingDeleteKeyId)?.label ||
            t("apiKeyList.unnamed", { defaultValue: "(unnamed)" }),
          defaultValue:
            'Are you sure you want to delete "{{label}}"? This action cannot be undone.',
        })}
        confirmText={t("common.delete", { defaultValue: "Delete" })}
        cancelText={t("common.cancel")}
        variant="destructive"
        onConfirm={() => {
          if (pendingDeleteKeyId) {
            remove.mutate(pendingDeleteKeyId);
          }
          setPendingDeleteKeyId(null);
        }}
        onCancel={() => setPendingDeleteKeyId(null)}
      />
    </div>
  );
}

// =====================================================================
// Row
// =====================================================================

interface ApiKeyRowProps {
  apiKey: ApiKeyDto;
  isActive: boolean;
  onSetActive: () => void;
  onDelete: () => void;
  onEditStart: () => void;
  /** account-level usage 数据——下放到每个 key 的 status bar 上 */
  usage?: UsageResult | null;
  /** provider 是否启用了 usage_script */
  usageEnabled?: boolean;
  /** 5h / 7d 窗口到点重置时由 status bar 回调——刷新该 key 的用量缓存 */
  onFiveHourResetReached?: () => void;
  /** 任一 quota 100% 时为 true（由 ApiKeyStatusBar 上报）——row 用这个加红 */
  isExhausted?: boolean;
  /** ApiKeyStatusBar 上报 quota 状态的回调 */
  onExhaustedChange?: (exhausted: boolean) => void;
}

function ApiKeyRow({
  apiKey,
  isActive,
  onSetActive,
  onDelete,
  onEditStart,
  usage,
  usageEnabled = false,
  isExhausted = false,
  onExhaustedChange,
  onFiveHourResetReached,
}: ApiKeyRowProps) {
  const { t } = useTranslation();
  const update = useUpdateApiKey();
  const [showKey, setShowKey] = useState(false);
  const inCooldown = apiKey.cooldownUntil * 1000 > Date.now();

  // dnd-kit 拖拽：只有 GripVertical handle 触发 drag，其它区域（按钮、输入）
  // 仍然保持原有点击行为。CSS.Transform 让 row 在拖动时跟随光标，
  // transition 让松手时动画回归原位。
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: apiKey.id });

  const style: React.CSSProperties = {
    transform: CSS.Transform.toString(transform),
    transition,
    // 拖动中的 row 在视觉上抬起来一层，避免与目标位置混淆。
    zIndex: isDragging ? 10 : undefined,
    opacity: isDragging ? 0.85 : 1,
  };

  return (
    <li
      ref={setNodeRef}
      style={style}
      // 把 dnd-kit 的 listeners 放到 <li> 上，让整行可拖——比之前
      // 「必须抓到 16px 的 ⋮⋮ grip 图标才拖得动」直观得多。
      //
      // 三个细节：
      // 1) `cursor-grab / active:cursor-grabbing` 给整行一个「可拖」指针，
      //    视觉上明确它是 sortable item。
      // 2) `touch-none` = `touch-action: none`，让 PointerSensor 在
      //    移动端能正确捕获手势（不让浏览器先抢去做滚动）。
      // 3) `select-none` 防止拖动时高亮选中 token 文本。
      // 4) `{...listeners} {...attributes}` 在行上展开 dnd-kit 提供的
      //    `onPointerDown / onKeyDown` 等事件 + ARIA attributes。
      // 5) 右侧 actions 列在子 wrapper 上 `e.stopPropagation()`，
      //    点 toggle / set-active / edit / delete 不会被当成「开始拖」
      //    ——PointerSensor 的 distance: 8 已经过滤「点一下就松手」，
      //    但显式 stop 让按钮在拖动阈值临界时也更稳。
      {...listeners}
      {...attributes}
      className={cn(
        "rounded-lg border bg-background p-3 transition-colors relative touch-none select-none",
        isDragging ? "cursor-grabbing" : "cursor-grab",
        isActive
          ? // Active key: 与 provider-list 页面的 SortableKeyRow 用同一套
          // 浅蓝边 + 浅蓝底 + 60% 透明边框——比之前的 4px 左条 + 100% 边框
          // 弱化一档，让 row 看起来还是列表项而不是按钮。两侧面板
          // （编辑 / provider-list）风格一致，扫一眼不卡。
            "border-blue-400/60 bg-blue-50/40 dark:border-blue-700/50 dark:bg-blue-950/20"
          : isExhausted
            ? // 任一 quota 100%——与 provider-list 同一档浅红；红色足以
              // 刺眼提示，且与 active 共用同一「柔和高亮」语汇，不抢戏。
              "border-red-400/60 bg-red-50/40 dark:border-red-700/50 dark:bg-red-950/20"
            : "border-border-default hover:border-border-default/80",
      )}
    >
      {/* items-start 让 grip / content / actions 三列顶部对齐。
          旧版 items-center 在 content 有 5 行（name + code + tags + error + status bar）
          时把 action 按钮挤到中间，视觉上很乱。 */}
      <div className="flex items-start gap-2.5">
        {/* GripVertical 现在是纯视觉提示——实际 drag handle 是 <li> 自身
            （dnd-kit listeners 已经在 li 上展开）。这样用户可以抓整行
            任意位置拖，不必对准 16px 的图标。
            之前这里的「上下移」按钮已删除——拖拽是唯一的 reorder 入口，
            双入口重复且方向冲突（旧 bug：active 钉顶导致上下移看似无反应）。 */}
        <GripVertical
          className="h-4 w-4 mt-1 text-muted-foreground/60 pointer-events-none shrink-0"
          aria-label={t("apiKeyList.dragHandle", {
            defaultValue: "拖动以重新排序",
          })}
        />

        {/* content 列：用 grid 把 Row 1（label）拆成「标签 + 状态 badge /
            actions」三段横向铺开；Row 2-5 用 col-span-2 跨满整行宽。
            比之前「右栏整列放 4 个按钮」省一行高度，row 在编辑表单
            列表里更紧凑。min-w-0 是 grid 子项能真正缩到 0 的开关。
            三个动作按钮（设为默认 / 编辑 / 删除）放在标签行末端，用
            justify-self: end 把它们对齐到行末。 */}
        <div className="grid flex-1 min-w-0 grid-cols-[1fr_auto] gap-x-2 gap-y-2 items-center">
          {/* Row 1: 名字 + 状态 badge（左） — 名字 truncate 让长 label 不
              把 actions 挤出列。actions 在第二个 grid 单元里。 */}
          <div className="flex items-center gap-2 flex-wrap min-w-0">
            <span className="text-sm font-medium truncate min-w-0">
              {apiKey.label || t("apiKeyList.unnamed", { defaultValue: "(未命名)" })}
            </span>
            {isActive && (
              <Badge
                variant="default"
                className="text-[10px] font-bold bg-blue-500 hover:bg-blue-500 text-white gap-1 px-1.5"
              >
                <CircleDot className="h-2.5 w-2.5 fill-current" />
                {t("apiKeyList.active", { defaultValue: "Active" })}
              </Badge>
            )}
            {!apiKey.enabled && (
              <Badge variant="secondary" className="text-[10px]">
                {t("apiKeyList.disabled", { defaultValue: "已停用" })}
              </Badge>
            )}
            {inCooldown && (
              <Badge variant="outline" className="text-[10px] text-amber-600">
                <Clock className="h-3 w-3 mr-1" />
                {t("apiKeyList.cooldown", { defaultValue: "冷却中" })}
              </Badge>
            )}
            {apiKey.failureCount > 0 && !inCooldown && (
              <Badge variant="outline" className="text-[10px] text-orange-600">
                <AlertCircle className="h-3 w-3 mr-1" />
                {Math.min(apiKey.failureCount, 5)}
              </Badge>
            )}
          </div>

          {/* Row 1 actions: 设为默认 / 编辑 / 删除 / 启用停用，与 label
              同一行右端。顺序按"破坏性递增 + 状态控制"：先 setActive
              （设置主用）→ edit（修改元数据）→ delete（破坏性最
              高）；Switch 是状态控件，放最末端方便扫到。
              子 wrapper onPointerDown stopPropagation 防止点按钮被当成
              "开始拖"——PointerSensor 的 distance: 8 已经过滤点击，但
              显式 stop 让 8px 临界抖动也不会误触。Switch 自身是 Radix
              button，其内部 pointer 事件已经处理；外面加一层 stop 让
              toggle 拖动阈值临界时也不误触。 */}
          <div
            className="flex items-center gap-0.5 shrink-0"
            onPointerDown={(e) => e.stopPropagation()}
          >
            <button
              type="button"
              onClick={onSetActive}
              disabled={isActive}
              onPointerDown={(e) => e.stopPropagation()}
              title={t("apiKeyList.setActive", { defaultValue: "设为默认" })}
              aria-label={t("apiKeyList.setActive", { defaultValue: "设为默认" })}
              className={cn(
                "inline-flex h-5 items-center rounded-md px-1.5 text-[10px] font-semibold transition-colors",
                isActive
                  ? "bg-emerald-100 text-emerald-700 dark:bg-emerald-950/40 dark:text-emerald-300 cursor-default"
                  : "text-muted-foreground/80 hover:text-foreground hover:bg-muted/60",
              )}
            >
              {isActive
                ? t("keyPool.activatedLabel", { defaultValue: "Activated" })
                : t("keyPool.activateLabel", { defaultValue: "Activate" })}
            </button>
            <Button
              variant="ghost"
              size="icon"
              type="button"
              onClick={onEditStart}
              className="h-7 w-7"
              title={t("apiKeyList.edit", { defaultValue: "编辑" })}
            >
              <Pencil className="h-3.5 w-3.5 text-muted-foreground" />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              type="button"
              onClick={onDelete}
              className="h-7 w-7 text-muted-foreground hover:text-red-500"
              title={t("apiKeyList.delete", { defaultValue: "删除" })}
            >
              <Trash2 className="h-3.5 w-3.5" />
            </Button>
            <Switch
              checked={apiKey.enabled}
              onCheckedChange={(v) =>
                update.mutate({ keyId: apiKey.id, payload: { enabled: v } })
              }
              aria-label={t("keyPool.toggleRotation", {
                defaultValue: "加入轮换",
              })}
              title={t("keyPool.toggleRotation", {
                defaultValue: "加入轮换",
              })}
              className="ml-1"
            />
          </div>

          {/* Row 2: token 展示块——bg-muted/30 浅灰底 + 等宽字体 + 内边距，
              让长 token 在视觉上是一个独立可识别的「凭据块」，而不是
              一段裸露文本。break-all + whitespace-pre-wrap 处理超长
              token 的换行；flex-1 让 code 吃掉剩余宽度。col-span-2
              让 Row 2 横跨整个 grid（与下面 Row 3-5 一致），不与 Row 1
              的 actions 列抢占空间。 */}
          <div className="col-span-2 flex items-start gap-1.5 rounded-md bg-muted/40 border border-border/40 px-2 py-1.5 text-xs">
            <code className="flex-1 min-w-0 font-mono break-all whitespace-pre-wrap text-foreground/80 leading-relaxed">
              {showKey ? apiKey.apiKey : maskKey(apiKey.apiKey)}
            </code>
            <button
              type="button"
              onClick={() => setShowKey((v) => !v)}
              className="shrink-0 mt-0.5 p-0.5 rounded text-muted-foreground hover:text-foreground hover:bg-background/60 transition-colors"
              aria-label={showKey ? "hide" : "show"}
              title={showKey ? "隐藏" : "显示"}
            >
              {showKey ? (
                <EyeOff className="h-3.5 w-3.5" />
              ) : (
                <Eye className="h-3.5 w-3.5" />
              )}
            </button>
          </div>

          {/* Row 3: tags 单独成行——旧版塞在 token 那行末尾，看起来像
              token 的一部分。每个 tag 独立 Badge 比「· tag1, tag2」
              一串文本更易点击筛选 / 复制。 */}
          {apiKey.tags.length > 0 && (
            <div className="col-span-2 flex items-center gap-1.5 flex-wrap">
              <span className="text-[10px] uppercase tracking-wide text-muted-foreground/70">
                {t("apiKeyList.tags", { defaultValue: "tags" })}
              </span>
              {apiKey.tags.map((tag) => (
                <Badge
                  key={tag}
                  variant="outline"
                  className={cn(
                    "text-[10px] font-normal",
                    // 按 tag 字符串 hash 到固定调色板，确保同名 tag 在
                    // 所有 key 的 row 上颜色一致——视觉锚点稳定。
                    tagColorClasses(tag),
                  )}
                >
                  {tag}
                </Badge>
              ))}
            </div>
          )}

          {/* Row 4: 上次错误信息（与上面 row 间距一致） */}
          {apiKey.lastError && (
            <div className="col-span-2 flex items-start gap-1.5 text-[11px] text-red-500">
              <AlertCircle className="h-3 w-3 mt-0.5 shrink-0" />
              <span className="break-all">{apiKey.lastError}</span>
            </div>
          )}

          {/* Row 5: 5h/7d 配额条（已 i18n 化）—— 编辑页表单里 row 较多，
            默认折叠进度条，仅展示状态行；点 status 行展开。KeyPoolBadge
            不传该 prop，行为不变（默认展开）。col-span-2 让它横跨
            整个 grid 宽度，与 Row 2-4 对齐。 */}
          <div className="col-span-2">
            <ApiKeyStatusBar
              keyId={apiKey.id}
              cooldownUntil={apiKey.cooldownUntil}
              failureCount={apiKey.failureCount}
              enabled={apiKey.enabled}
              usage={usage ?? null}
              usageEnabled={usageEnabled}
              defaultCollapsed
              onExhaustedChange={onExhaustedChange}
              onFiveHourResetReached={onFiveHourResetReached}
            />
          </div>
        </div>
      </div>
    </li>
  );
}

function maskKey(key: string): string {
  if (!key) return "";
  if (key.length <= 8) return "•".repeat(key.length);
  return `${key.slice(0, 4)}${"•".repeat(Math.max(4, key.length - 8))}${key.slice(-4)}`;
}

// =====================================================================
// New key inline form (replaces the modal-based CreateKeyDialog)
// =====================================================================

function NewKeyInlineForm({
  providerId,
  appId,
  onDone,
  onCancel,
}: {
  providerId: string;
  appId: AppId;
  onDone: () => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const [label, setLabel] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [tags, setTags] = useState<string[]>([]);
  const [enabled, setEnabled] = useState(true);
  const create = useCreateApiKey();

  const canSave = label.trim().length > 0 && apiKey.trim().length > 0 && !create.isPending;

  const onSubmit = () => {
    if (!canSave) return;
    create.mutate(
      {
        providerId,
        appType: appId,
        label: label.trim(),
        apiKey: apiKey.trim(),
        tags,
        enabled,
      },
      { onSuccess: onDone, onError: () => {} },
    );
  };

  return (
    <li className="rounded-lg border border-dashed border-blue-400 dark:border-blue-600 bg-blue-50/30 dark:bg-blue-950/10 p-3 space-y-3">
      <div className="text-xs font-medium text-blue-700 dark:text-blue-300">
        {t("apiKeyList.createTitle", { defaultValue: "添加 API Key" })}
      </div>
      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
        <div className="space-y-1.5">
          <Label htmlFor="newkey-label">
            {t("apiKeyList.labelLabel", { defaultValue: "标签" })}
          </Label>
          <Input
            id="newkey-label"
            value={label}
            onChange={(e) => setLabel(e.target.value)}
            placeholder="Primary"
            autoFocus
          />
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="newkey-value">
            {t("apiKeyList.valueLabel", { defaultValue: "API Key" })}
          </Label>
          <Input
            id="newkey-value"
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            type="password"
            autoComplete="off"
            placeholder="sk-…"
          />
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="newkey-tags">
            {t("apiKeyList.tagsLabel", { defaultValue: "标签" })}
          </Label>
          <TagInput
            id="newkey-tags"
            value={tags}
            onChange={setTags}
            placeholder={t("apiKeyList.tagsPlaceholder", {
              defaultValue: "输入后回车添加（prod、region-east）",
            })}
          />
        </div>
      </div>
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <Switch
            checked={enabled}
            onCheckedChange={setEnabled}
            id="newkey-enabled"
          />
          <Label htmlFor="newkey-enabled">
            {t("apiKeyList.enabledLabel", { defaultValue: "启用" })}
          </Label>
        </div>
        <div className="flex items-center gap-2">
          <Button variant="ghost" size="sm" type="button" onClick={onCancel} disabled={create.isPending}>
            {t("common.cancel", { defaultValue: "取消" })}
          </Button>
          <Button size="sm" type="button" onClick={onSubmit} disabled={!canSave}>
            {create.isPending && <Loader2 className="h-4 w-4 mr-1 animate-spin" />}
            {t("common.save", { defaultValue: "保存" })}
          </Button>
        </div>
      </div>
    </li>
  );
}

// =====================================================================
// Edit key inline form (replaces the modal-based EditKeyDialog)
// =====================================================================

function EditKeyInlineForm({
  apiKey,
  onDone,
  onCancel,
}: {
  apiKey: ApiKeyDto;
  onDone: () => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const [label, setLabel] = useState(apiKey.label);
  const [keyValue, setKeyValue] = useState(apiKey.apiKey);
  const [tags, setTags] = useState<string[]>(apiKey.tags);
  const [enabled, setEnabled] = useState(apiKey.enabled);
  const update = useUpdateApiKey();

  const keyChanged = keyValue !== apiKey.apiKey;
  const canSave = label.trim().length > 0 && !update.isPending;

  const onSubmit = () => {
    if (!canSave) return;
    update.mutate(
      {
        keyId: apiKey.id,
        payload: {
          label: label.trim(),
          // Only send apiKey when the user actually changed it.
          apiKey: keyChanged ? keyValue : undefined,
          tags,
          enabled,
        },
      },
      { onSuccess: onDone, onError: () => {} },
    );
  };

  return (
    <li className="rounded-lg border border-dashed border-amber-400 dark:border-amber-600 bg-amber-50/30 dark:bg-amber-950/10 p-3 space-y-3">
      <div className="text-xs font-medium text-amber-700 dark:text-amber-300">
        {t("apiKeyList.editTitle", { defaultValue: "编辑 Key" })}
      </div>
      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
        <div className="space-y-1.5">
          <Label htmlFor={`edit-${apiKey.id}-label`}>
            {t("apiKeyList.labelLabel", { defaultValue: "标签" })}
          </Label>
          <Input
            id={`edit-${apiKey.id}-label`}
            value={label}
            onChange={(e) => setLabel(e.target.value)}
            autoFocus
          />
        </div>
        <div className="space-y-1.5">
          <Label htmlFor={`edit-${apiKey.id}-value`}>
            {t("apiKeyList.valueLabel", { defaultValue: "API Key" })}
          </Label>
          <Input
            id={`edit-${apiKey.id}-value`}
            value={keyValue}
            onChange={(e) => setKeyValue(e.target.value)}
            type="password"
            autoComplete="off"
          />
          <p className="text-[11px] text-muted-foreground">
            {t("apiKeyList.editValueHint", {
              defaultValue: "留空或保持不变则不更新此字段。",
            })}
          </p>
        </div>
        <div className="space-y-1.5">
          <Label htmlFor={`edit-${apiKey.id}-tags`}>
            {t("apiKeyList.tagsLabel", { defaultValue: "标签" })}
          </Label>
          <TagInput
            id={`edit-${apiKey.id}-tags`}
            value={tags}
            onChange={setTags}
            placeholder={t("apiKeyList.tagsPlaceholder", {
              defaultValue: "输入后回车添加（prod、region-east）",
            })}
          />
        </div>
      </div>
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <Switch
            checked={enabled}
            onCheckedChange={setEnabled}
            id={`edit-${apiKey.id}-enabled`}
          />
          <Label htmlFor={`edit-${apiKey.id}-enabled`}>
            {t("apiKeyList.enabledLabel", { defaultValue: "启用" })}
          </Label>
        </div>
        <div className="flex items-center gap-2">
          <Button variant="ghost" size="sm" type="button" onClick={onCancel} disabled={update.isPending}>
            {t("common.cancel", { defaultValue: "取消" })}
          </Button>
          <Button size="sm" type="button" onClick={onSubmit} disabled={!canSave}>
            {update.isPending && <Loader2 className="h-4 w-4 mr-1 animate-spin" />}
            {t("common.save", { defaultValue: "保存" })}
          </Button>
        </div>
      </div>
    </li>
  );
}

// =====================================================================
// TagInput
// =====================================================================

/**
 * 多值 tag 输入器。
 *
 * 行为：
 *  - 输入字符后回车 / 输入逗号 / blur → 把当前 draft 提交成 tag
 *  - 输入框为空时按 Backspace → 删除最后一个 tag（标准 tag-input 习惯）
 *  - 每个 tag 是一个 badge，右侧 ✕ 按钮可单独删除
 *  - 重复 / 空 tag 自动去重过滤
 *
 * 与原生逗号分隔 `<Input>` 的区别：
 *  - 用户可以边看自己刚加的 tag 边输入下一个，不需要"输入完所有再用逗号分"
 *  - 错误编辑（修改中间一个 tag）不会影响其它 tag
 *  - 移动端/中文输入法不需要切换符号键找逗号
 *
 * wrapper 整体 onPointerDown stopPropagation —— 行级 dnd-kit 的
 * listeners 在 <li> 上，这里 stop 防止点 tag / 输入框被当成「开始拖」。
 */
function TagInput({
  id,
  value,
  onChange,
  placeholder,
}: {
  id?: string;
  value: string[];
  onChange: (next: string[]) => void;
  placeholder?: string;
}) {
  const [draft, setDraft] = useState("");

  const commit = (raw: string) => {
    const tag = raw.trim();
    if (!tag) return;
    // 去重——同 tag 已存在就忽略（大小写敏感，避免误合 "Prod" / "prod"）。
    if (value.includes(tag)) {
      setDraft("");
      return;
    }
    onChange([...value, tag]);
    setDraft("");
  };

  const remove = (tag: string) => {
    onChange(value.filter((t) => t !== tag));
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter" || e.key === ",") {
      e.preventDefault();
      commit(draft);
    } else if (e.key === "Backspace" && draft === "" && value.length > 0) {
      e.preventDefault();
      onChange(value.slice(0, -1));
    }
  };

  return (
    <div
      // 关键：stop 掉 pointerdown，row 上的 dnd-kit 不会因为点 input
      // / tag 而误判 drag。
      onPointerDown={(e) => e.stopPropagation()}
      className={cn(
        "flex flex-wrap items-center gap-1.5 min-h-9 w-full rounded-md",
        "border border-input bg-background px-2 py-1.5",
        "focus-within:ring-1 focus-within:ring-ring focus-within:border-ring",
      )}
    >
      {value.map((tag) => (
        <Badge
          key={tag}
          // 不带 variant——variant="secondary" 自带 bg-secondary /
          // text-secondary-foreground，会与下方 tagColorClasses 撞色。
          // 这里我们直接给一组明确的 bg / text，让 hash 调色板独占视觉权重。
          // Badge 内部也 stop 一次——点 ✕ 按钮冒泡到 wrapper 后再
          // 冒泡到 <li> 也会触发 dnd-kit。
          onPointerDown={(e) => e.stopPropagation()}
          className={cn(
            "gap-1 pl-2 pr-1 py-0.5 text-xs font-normal",
            // 与 row 上的 tag 颜色保持一致——同一个 tag 在「行内展示」
            // 和「编辑态 chip」之间视觉连续。
            tagColorClasses(tag),
          )}
        >
          <span>{tag}</span>
          <button
            type="button"
            onClick={() => remove(tag)}
            // ✕ 按钮 stop 防止点击事件触发 input blur 等副作用。
            onPointerDown={(e) => e.stopPropagation()}
            className="ml-0.5 inline-flex h-4 w-4 items-center justify-center rounded-sm text-muted-foreground hover:bg-background hover:text-foreground transition-colors"
            aria-label={`remove ${tag}`}
          >
            <X className="h-3 w-3" />
          </button>
        </Badge>
      ))}
      <Input
        id={id}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={onKeyDown}
        onBlur={() => {
          if (draft.trim()) commit(draft);
        }}
        placeholder={value.length === 0 ? placeholder : undefined}
        // 关键：input 自身 border / bg 全部去掉，让它视觉上和 wrapper
        // 融为一体；flex-1 让 input 吃掉剩余宽度；min-w-0 是 flex 子项
        // 真正能缩到 0 的开关。
        className="flex-1 min-w-[120px] h-7 px-1 border-0 bg-transparent shadow-none focus-visible:ring-0 focus-visible:ring-offset-0"
        autoComplete="off"
      />
    </div>
  );
}

