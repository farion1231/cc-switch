import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  DndContext,
  KeyboardSensor,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import {
  SortableContext,
  arrayMove,
  rectSortingStrategy,
  sortableKeyboardCoordinates,
  useSortable,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { Check, GripVertical, Loader2, Search, X } from "lucide-react";
import { toast } from "sonner";
import type { CodexCatalogModel, Provider } from "@/types";
import type {
  CodexModelRoutingConfig,
  CodexModelSelection,
} from "@/types/codexModelRouting";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import {
  useCodexModelRouting,
  useSaveCodexModelRouting,
} from "@/lib/query/codexModelRouting";
import {
  isModelRoutingProvider,
  modelRoutingOptions,
  selectRoutedModel,
} from "@/utils/codexModelRouting";
import { extractErrorMessage } from "@/utils/errorUtils";
import { cn } from "@/lib/utils";

const EMPTY: CodexModelRoutingConfig = {
  enabled: false,
  providerName: "CC Switch Router",
  models: [],
};

function Reasoning({ model }: { model?: CodexCatalogModel }) {
  const { t } = useTranslation();
  const levels = Array.isArray(model?.reasoningLevels)
    ? model.reasoningLevels
    : [];
  return (
    <span className="text-xs text-muted-foreground">
      {t("codexRouting.reasoning", { defaultValue: "推理" })}：
      {levels.length
        ? levels.join(" / ")
        : t("codexRouting.inherited", { defaultValue: "供应商默认" })}
      {model?.defaultReasoningLevel
        ? ` · ${t("codexRouting.default", { defaultValue: "默认" })} ${model.defaultReasoningLevel}`
        : ""}
    </span>
  );
}

function SelectedModel({
  entry,
  provider,
  model,
  index,
  disabled,
  onRemove,
}: {
  entry: CodexModelSelection;
  provider?: Provider;
  model?: CodexCatalogModel;
  index: number;
  disabled: boolean;
  onRemove: () => void;
}) {
  const { t } = useTranslation();
  const { attributes, listeners, setNodeRef, transform, transition } =
    useSortable({ id: entry.model, disabled });
  return (
    <div
      ref={setNodeRef}
      style={{ transform: CSS.Transform.toString(transform), transition }}
      className={cn(
        "flex min-w-0 items-center gap-2 rounded-xl border border-border-default bg-muted/20 p-2",
        !model && "border-destructive",
      )}
    >
      <button
        type="button"
        disabled={disabled}
        {...attributes}
        {...listeners}
        aria-label={t("codexRouting.reorder", {
          model: entry.model,
          defaultValue: "排序 {{model}}",
        })}
        className="touch-none rounded p-2 text-muted-foreground hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring"
      >
        <GripVertical className="h-4 w-4" />
      </button>
      <span className="text-xs tabular-nums text-primary">{index + 1}</span>
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-baseline gap-x-2 text-sm">
          <span className="font-semibold">
            {provider?.name ?? entry.providerId}
          </span>
          <span className="break-all font-mono">{entry.model}</span>
        </div>
        {model ? (
          <Reasoning model={model} />
        ) : (
          <span className="text-xs text-destructive">
            {t("codexRouting.missing", {
              defaultValue: "供应商或模型已不存在，请移除后重新选择",
            })}
          </span>
        )}
      </div>
      <button
        type="button"
        disabled={disabled}
        onClick={onRemove}
        className="rounded p-2 text-muted-foreground hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring"
        aria-label={t("codexRouting.remove", {
          model: entry.model,
          defaultValue: "移除 {{model}}",
        })}
      >
        <X className="h-4 w-4" />
      </button>
    </div>
  );
}

export function CodexModelRoutingDialog({
  open,
  onOpenChange,
  providers,
  active,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  providers: Record<string, Provider>;
  active: boolean;
}) {
  const { t } = useTranslation();
  const query = useCodexModelRouting();
  const save = useSaveCodexModelRouting();
  const [draft, setDraft] = useState(EMPTY);
  const [baseline, setBaseline] = useState(EMPTY);
  const [initialized, setInitialized] = useState(false);
  const [search, setSearch] = useState("");
  const [error, setError] = useState("");
  const [replacement, setReplacement] = useState<CodexModelSelection | null>(
    null,
  );
  const [discard, setDiscard] = useState(false);
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  );
  useEffect(() => {
    if (!open) {
      setInitialized(false);
      return;
    }
    if (!initialized && query.data) {
      setDraft(structuredClone(query.data));
      setBaseline(structuredClone(query.data));
      setSearch("");
      setError("");
      setInitialized(true);
    }
  }, [open, query.data, initialized]);

  const groups = useMemo(
    () =>
      Object.values(providers)
        .filter(isModelRoutingProvider)
        .map((provider) => ({
          provider,
          models: modelRoutingOptions(provider),
        })),
    [providers],
  );
  const dirty = JSON.stringify(draft) !== JSON.stringify(baseline);
  const selectedMissing = draft.models.some(
    (entry) =>
      !groups.some(
        (group) =>
          group.provider.id === entry.providerId &&
          group.models.some((model) => model.model === entry.model),
      ),
  );
  const visibleGroups = groups
    .map((group) => ({
      ...group,
      visible: group.models.filter((model) =>
        `${group.provider.name} ${model.model} ${model.displayName ?? ""}`
          .toLowerCase()
          .includes(search.toLowerCase()),
      ),
    }))
    .filter((group) => !search || group.visible.length > 0);
  const pending = save.isPending;
  const close = () => {
    if (!pending) {
      if (dirty) setDiscard(true);
      else onOpenChange(false);
    }
  };
  const choose = (entry: CodexModelSelection) => {
    const existing = draft.models.find((row) => row.model === entry.model);
    if (existing?.providerId === entry.providerId) {
      setDraft({
        ...draft,
        models: draft.models.filter((row) => row.model !== entry.model),
      });
    } else if (existing) {
      setReplacement(entry);
    } else {
      setDraft({ ...draft, models: selectRoutedModel(draft.models, entry) });
    }
  };
  const submit = async () => {
    setError("");
    try {
      const result = await save.mutateAsync(draft);
      setDraft(result.config);
      setBaseline(result.config);
      toast.success(
        result.catalogChanged
          ? t("codexRouting.savedCatalog", {
              defaultValue:
                "已保存。模型菜单或能力已变化，若 Codex 未刷新，请重新打开 Codex。",
            })
          : t(active ? "codexRouting.savedLive" : "codexRouting.savedDraft", {
              defaultValue: active
                ? "已保存，后续请求使用新路由；进行中的请求不受影响。"
                : "配置已保存，未启动服务，也未修改 Codex 配置。",
            }),
      );
    } catch (e) {
      setError(extractErrorMessage(e));
    }
  };

  return (
    <>
      <Dialog
        open={open}
        onOpenChange={(next) => {
          if (!next) close();
        }}
      >
        <DialogContent className="max-w-5xl w-[calc(100%-2rem)]">
          <DialogHeader>
            <div className="flex items-center justify-between gap-4">
              <DialogTitle>
                {t("codexRouting.title", { defaultValue: "Codex 模型路由" })}
              </DialogTitle>
              <Button
                size="icon"
                variant="ghost"
                onClick={close}
                disabled={pending}
                aria-label={t("common.close", { defaultValue: "关闭" })}
              >
                <X className="h-4 w-4" />
              </Button>
            </div>
            <DialogDescription>
              {t("codexRouting.description", {
                defaultValue:
                  "选择已有供应商的模型，合并成一套 Codex 菜单。地址、密钥、协议及推理能力均继承原供应商。",
              })}
            </DialogDescription>
          </DialogHeader>
          <div className="overflow-y-auto px-6 py-5 space-y-6">
            {query.isLoading && (
              <p role="status" className="text-sm text-muted-foreground">
                {t("common.loading", { defaultValue: "加载中…" })}
              </p>
            )}
            {query.isError && (
              <p role="alert" className="text-sm text-destructive">
                {t("codexRouting.loadFailed", {
                  defaultValue: "读取模型路由配置失败",
                })}{" "}
                <Button variant="link" onClick={() => query.refetch()}>
                  {t("common.retry", { defaultValue: "重试" })}
                </Button>
              </p>
            )}
            <div className="grid gap-2 sm:grid-cols-[180px_1fr] sm:items-center">
              <Label htmlFor="codex-router-name">
                {t("codexRouting.providerName", {
                  defaultValue: "Provider 显示名称",
                })}
              </Label>
              <Input
                id="codex-router-name"
                value={draft.providerName}
                maxLength={80}
                disabled={pending || !initialized}
                onChange={(e) =>
                  setDraft({ ...draft, providerName: e.target.value })
                }
              />
              <p className="text-xs text-muted-foreground sm:col-start-2">
                {t("codexRouting.fixedId", {
                  defaultValue:
                    "内部 Provider ID 固定为 custom；更换模型来源不会改变这个身份。",
                })}
              </p>
            </div>
            <section className="space-y-3">
              <div>
                <h3 className="text-sm font-semibold">
                  {t("codexRouting.selected", {
                    count: draft.models.length,
                    defaultValue: "已选择模型 · {{count}}",
                  })}
                </h3>
                <p className="mt-1 text-xs text-muted-foreground">
                  {t("codexRouting.sortHint", {
                    defaultValue:
                      "拖动排序，也可聚焦拖动柄后用空格和方向键排序。首次启用时，第一项作为默认模型。",
                  })}
                </p>
              </div>
              {draft.models.length === 0 ? (
                <p className="rounded-xl border border-dashed border-border-default py-6 text-center text-sm text-muted-foreground">
                  {t("codexRouting.emptySelection", {
                    defaultValue: "从下方点击模型即可添加",
                  })}
                </p>
              ) : (
                <DndContext
                  sensors={sensors}
                  collisionDetection={closestCenter}
                  onDragEnd={({ active: dragged, over }) => {
                    if (!over || pending) return;
                    const from = draft.models.findIndex(
                      (entry) => entry.model === dragged.id,
                    );
                    const to = draft.models.findIndex(
                      (entry) => entry.model === over.id,
                    );
                    if (from >= 0 && to >= 0)
                      setDraft({
                        ...draft,
                        models: arrayMove(draft.models, from, to),
                      });
                  }}
                >
                  <SortableContext
                    items={draft.models.map((entry) => entry.model)}
                    strategy={rectSortingStrategy}
                  >
                    <div className="grid gap-2 md:grid-cols-2">
                      {draft.models.map((entry, index) => (
                        <SelectedModel
                          key={entry.model}
                          entry={entry}
                          index={index}
                          disabled={pending}
                          provider={providers[entry.providerId]}
                          model={groups
                            .find(
                              (group) => group.provider.id === entry.providerId,
                            )
                            ?.models.find(
                              (model) => model.model === entry.model,
                            )}
                          onRemove={() =>
                            setDraft({
                              ...draft,
                              models: draft.models.filter(
                                (row) => row.model !== entry.model,
                              ),
                            })
                          }
                        />
                      ))}
                    </div>
                  </SortableContext>
                </DndContext>
              )}
            </section>
            <section className="space-y-4 border-t border-border-default pt-5">
              <div className="flex flex-wrap items-center justify-between gap-3">
                <h3 className="text-sm font-semibold">
                  {t("codexRouting.available", {
                    defaultValue: "已有供应商的模型",
                  })}
                </h3>
                <div className="relative w-full sm:w-64">
                  <Search className="absolute left-3 top-2.5 h-4 w-4 text-muted-foreground" />
                  <Input
                    className="pl-9"
                    value={search}
                    onChange={(e) => setSearch(e.target.value)}
                    aria-label={t("codexRouting.search", {
                      defaultValue: "搜索供应商或模型",
                    })}
                    placeholder={t("codexRouting.search", {
                      defaultValue: "搜索供应商或模型",
                    })}
                  />
                </div>
              </div>
              {visibleGroups.map(({ provider, visible }) => (
                <div
                  key={provider.id}
                  className="flex flex-col gap-3 sm:flex-row sm:items-start"
                >
                  <span className="shrink-0 self-start rounded-full bg-muted px-3 py-2 text-sm font-semibold sm:w-36 break-words">
                    {provider.name}
                  </span>
                  <div className="flex flex-1 flex-wrap gap-2">
                    {visible.map((model) => {
                      const selected = draft.models.some(
                        (entry) =>
                          entry.providerId === provider.id &&
                          entry.model === model.model,
                      );
                      return (
                        <button
                          key={model.model}
                          type="button"
                          aria-pressed={selected}
                          disabled={pending || !initialized}
                          onClick={() =>
                            choose({
                              providerId: provider.id,
                              model: model.model,
                            })
                          }
                          aria-label={`${provider.name} / ${model.model}`}
                          className={cn(
                            "max-w-full rounded-xl border px-3 py-2 text-left transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50",
                            selected
                              ? "border-primary bg-primary/10 text-primary"
                              : "border-border-default hover:bg-muted",
                          )}
                        >
                          <span className="flex items-center gap-2 text-sm">
                            <span className="w-4 shrink-0">
                              {selected && <Check className="h-4 w-4" />}
                            </span>
                            <span className="break-all font-mono">
                              {model.model}
                            </span>
                          </span>
                          <span className="ml-6 block">
                            <Reasoning model={model} />
                          </span>
                        </button>
                      );
                    })}
                    {!visible.length && (
                      <p className="py-2 text-xs text-muted-foreground">
                        {t("codexRouting.noCatalog", {
                          defaultValue:
                            "还没有模型映射，请先在该供应商的编辑页添加模型。",
                        })}
                      </p>
                    )}
                  </div>
                </div>
              ))}
              {!visibleGroups.length && (
                <p className="py-4 text-sm text-muted-foreground">
                  {t("codexRouting.noProviders", {
                    defaultValue:
                      "没有可选模型。请添加 Codex API Key 供应商并配置模型映射，或调整搜索条件。",
                  })}
                </p>
              )}
            </section>
            <p className="text-xs leading-relaxed text-muted-foreground">
              {t("codexRouting.safetyHint", {
                defaultValue:
                  "首版仅支持 API Key 供应商，同名模型选择一家。不使用全局故障转移。跨站切换后，依赖原站响应 ID 的已有会话可能需要新建；不会清除或改写历史。",
              })}
            </p>
            {error && (
              <p role="alert" className="text-sm text-destructive">
                {error}
              </p>
            )}
          </div>
          <DialogFooter>
            <span className="mr-auto text-xs text-muted-foreground">
              {active
                ? t("codexRouting.activeHint", {
                    defaultValue: "路由运行中 · 保存后对后续请求生效",
                  })
                : t("codexRouting.draftHint", {
                    defaultValue: "仅保存配置 · 不启动服务",
                  })}
            </span>
            <Button variant="outline" onClick={close} disabled={pending}>
              {t("common.close", { defaultValue: "关闭" })}
            </Button>
            <Button
              onClick={() => void submit()}
              disabled={
                !dirty ||
                !initialized ||
                pending ||
                !draft.providerName.trim() ||
                selectedMissing ||
                (active && !draft.models.length)
              }
            >
              {pending && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
              {t("codexRouting.save", { defaultValue: "保存配置" })}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <ConfirmDialog
        isOpen={Boolean(replacement)}
        variant="info"
        title={t("codexRouting.replace", { defaultValue: "更换模型来源" })}
        message={t("codexRouting.replaceConfirm", {
          model: replacement?.model,
          provider: replacement ? providers[replacement.providerId]?.name : "",
          defaultValue:
            "将 {{model}} 改为使用 {{provider}}。模型名称和菜单顺序保持不变，推理等级等能力继承新供应商；保存后生效。",
        })}
        onConfirm={() => {
          if (replacement)
            setDraft({
              ...draft,
              models: selectRoutedModel(draft.models, replacement),
            });
          setReplacement(null);
        }}
        onCancel={() => setReplacement(null)}
      />
      <ConfirmDialog
        isOpen={discard}
        title={t("codexRouting.discard", {
          defaultValue: "放弃未保存的修改？",
        })}
        message={t("codexRouting.discardHint", {
          defaultValue: "已保存的路由配置不受影响。",
        })}
        onConfirm={() => {
          setDiscard(false);
          onOpenChange(false);
        }}
        onCancel={() => setDiscard(false)}
      />
    </>
  );
}
