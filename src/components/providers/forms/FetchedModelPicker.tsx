import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Download, ListChecks, Loader2, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { HoverTip } from "@/components/ui/hover-tip";
import { SearchField } from "@/components/ui/search-field";
import { CHECKBOX_CLASS, V7Dialog } from "@/components/mcp/formBits";
import type { FetchedModel } from "@/lib/api/model-fetch";
import { cn } from "@/lib/utils";

export interface FetchedModelSelection {
  /** 新勾上的模型，按弹窗里的顺序 */
  add: string[];
  /** 取消勾选的已配置模型 */
  remove: string[];
}

interface FetchedModelPickerProps {
  models: FetchedModel[];
  /** 列表里已有的模型 ID（调用方按自己的规则归一，如去掉 [1m]） */
  configuredModelIds: string[];
  onApply: (selection: FetchedModelSelection) => void;
  onFetch: () => void;
  isFetching: boolean;
}

/**
 * 模型列表标题旁的「获取模型列表」按钮和选择弹窗。
 *
 * 拉取成功后自动打开弹窗；拉过之后按钮变成「选择模型」，直接打开，弹窗里再刷新。
 * 勾选状态就是「在不在列表里」：打开时已配置的模型都勾着，取消勾选即移除；点「应用」
 * 才把增删交给调用方，取消则什么都不改。配置里有、供应商没返回的模型单独列在最前面。
 */
export function FetchedModelPicker({
  models,
  configuredModelIds,
  onApply,
  onFetch,
  isFetching,
}: FetchedModelPickerProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  // 点按钮发起的拉取：结束时拿到新列表才打开弹窗（预检失败、出错、空列表都不打开）
  const [awaiting, setAwaiting] = useState<FetchedModel[] | null>(null);

  useEffect(() => {
    if (!awaiting || isFetching) return;
    setAwaiting(null);
    if (models !== awaiting && models.length > 0) setOpen(true);
  }, [awaiting, isFetching, models]);

  const startFetch = () => {
    setAwaiting(models);
    onFetch();
  };

  const hasModels = models.length > 0;

  return (
    <>
      <Button
        type="button"
        variant="outline"
        size="sm"
        onClick={hasModels ? () => setOpen(true) : startFetch}
        disabled={isFetching}
        className="h-7 gap-1"
      >
        {isFetching ? (
          <Loader2 className="h-3.5 w-3.5 animate-spin" />
        ) : hasModels ? (
          <ListChecks className="h-3.5 w-3.5" />
        ) : (
          <Download className="h-3.5 w-3.5" />
        )}
        {hasModels
          ? t("providerForm.modelPicker.title")
          : t("providerForm.fetchModels")}
      </Button>

      {/* 弹窗内容只在打开时挂载：每次打开按当时的列表重新起草勾选 */}
      <V7Dialog open={open} width={560} onOpenChange={setOpen}>
        <FetchedModelPickerBody
          models={models}
          configuredModelIds={configuredModelIds}
          onApply={(selection) => {
            if (selection.add.length > 0 || selection.remove.length > 0) {
              onApply(selection);
            }
            setOpen(false);
          }}
          onCancel={() => setOpen(false)}
          onFetch={onFetch}
          isFetching={isFetching}
        />
      </V7Dialog>
    </>
  );
}

function FetchedModelPickerBody({
  models,
  configuredModelIds,
  onApply,
  onCancel,
  onFetch,
  isFetching,
}: Omit<FetchedModelPickerProps, "onApply"> & {
  onApply: (selection: FetchedModelSelection) => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const configured = [
    ...new Set(configuredModelIds.map((id) => id.trim()).filter(Boolean)),
  ];
  const [query, setQuery] = useState("");
  const [onlySelected, setOnlySelected] = useState(false);
  const [draft, setDraft] = useState<Set<string>>(() => new Set(configured));

  const fetchedIds = new Set(models.map((model) => model.id));
  const extraIds = configured.filter((id) => !fetchedIds.has(id));
  const normalized = query.trim().toLowerCase();
  const matches = (id: string, ownedBy?: string | null) =>
    (!onlySelected || draft.has(id)) &&
    (!normalized ||
      id.toLowerCase().includes(normalized) ||
      !!ownedBy?.toLowerCase().includes(normalized));
  const visibleExtra = extraIds.filter((id) => matches(id));
  const visibleFetched = models.filter((model) =>
    matches(model.id, model.ownedBy),
  );
  const visibleIds = [...visibleExtra, ...visibleFetched.map((m) => m.id)];
  const allIds = [...extraIds, ...models.map((model) => model.id)];
  const selectedCount = allIds.filter((id) => draft.has(id)).length;

  const setChecked = (ids: string[], checked: boolean) =>
    setDraft((prev) => {
      const next = new Set(prev);
      for (const id of ids) {
        if (checked) next.add(id);
        else next.delete(id);
      }
      return next;
    });

  const apply = () => {
    const configuredSet = new Set(configured);
    onApply({
      add: models
        .map((model) => model.id)
        .filter((id) => draft.has(id) && !configuredSet.has(id)),
      remove: configured.filter((id) => !draft.has(id)),
    });
  };

  const renderRow = (id: string, ownedBy: string | null, first: boolean) => (
    <li key={id} className={cn(!first && "border-t border-border")}>
      <label className="flex cursor-pointer items-center gap-2.5 py-2 pe-3 ps-3.5 hover:bg-subtle">
        <input
          type="checkbox"
          className={CHECKBOX_CLASS}
          aria-label={id}
          checked={draft.has(id)}
          onChange={(event) => setChecked([id], event.target.checked)}
        />
        <span className="min-w-0 flex-1 truncate text-body" title={id}>
          {id}
        </span>
        {ownedBy && (
          <span className="max-w-[40%] shrink-0 truncate text-caption text-fg-3">
            {ownedBy}
          </span>
        )}
      </label>
    </li>
  );

  const groupHeading = (label: string) => (
    <li className="sticky top-0 z-[1] border-b border-border bg-subtle px-3.5 py-1.5 text-caption font-medium text-fg-2">
      {label}
    </li>
  );

  return (
    <>
      <div className="flex shrink-0 items-start justify-between gap-3">
        <div className="flex min-w-0 flex-col gap-1">
          <DialogTitle className="text-section">
            {t("providerForm.modelPicker.title")}
          </DialogTitle>
          <DialogDescription className="text-caption text-fg-2">
            {t("providerForm.modelPicker.lead")}
          </DialogDescription>
        </div>
        <HoverTip content={t("providerForm.modelPicker.refresh")}>
          <Button
            type="button"
            variant="quiet"
            size="icon-compact"
            aria-label={t("providerForm.modelPicker.refresh")}
            disabled={isFetching}
            onClick={onFetch}
          >
            <RefreshCw
              className={cn("h-3.5 w-3.5", isFetching && "animate-spin")}
            />
          </Button>
        </HoverTip>
      </div>

      <div className="flex shrink-0 flex-col gap-2">
        <div className="flex items-center gap-2">
          <SearchField
            containerClassName="flex-1"
            value={query}
            onValueChange={setQuery}
            clearLabel={t("common.clear")}
            placeholder={t("providerForm.searchModelPlaceholder")}
            aria-label={t("providerForm.searchModelPlaceholder")}
            autoComplete="off"
            autoFocus
            onKeyDown={(event) => {
              if (event.key === "Enter") event.preventDefault();
            }}
          />
          <Button
            type="button"
            variant="quiet"
            size="compact"
            disabled={visibleIds.length === 0}
            onClick={() => setChecked(visibleIds, true)}
          >
            {t("providerForm.modelPicker.selectAll")}
          </Button>
          <Button
            type="button"
            variant="quiet"
            size="compact"
            disabled={visibleIds.length === 0}
            onClick={() => setChecked(visibleIds, false)}
          >
            {t("providerForm.modelPicker.selectNone")}
          </Button>
        </div>
        <div className="flex items-center justify-between gap-3 text-caption text-fg-2">
          <span role="status">
            {t("providerForm.modelPicker.selectedCount", {
              selected: selectedCount,
              total: allIds.length,
            })}
          </span>
          <label className="flex cursor-pointer items-center gap-1.5">
            <input
              type="checkbox"
              className={CHECKBOX_CLASS}
              checked={onlySelected}
              onChange={(event) => setOnlySelected(event.target.checked)}
            />
            {t("providerForm.modelPicker.onlySelected")}
          </label>
        </div>
      </div>

      <div className="h-80 max-h-[45vh] min-h-0 overflow-y-auto rounded-panel border border-border">
        {isFetching ? (
          <div className="flex h-full items-center justify-center gap-2 text-body text-fg-2">
            <Loader2 className="h-4 w-4 animate-spin" />
            {t("providerForm.fetchingModels")}
          </div>
        ) : visibleIds.length === 0 ? (
          <p className="flex h-full items-center justify-center text-body text-fg-2">
            {t("providerForm.searchModelEmpty")}
          </p>
        ) : (
          <ul className="m-0 list-none p-0">
            {visibleExtra.length > 0 &&
              groupHeading(t("providerForm.modelPicker.notInProvider"))}
            {visibleExtra.map((id, index) => renderRow(id, null, index === 0))}
            {visibleExtra.length > 0 &&
              visibleFetched.length > 0 &&
              groupHeading(t("providerForm.modelPicker.fromProvider"))}
            {visibleFetched.map((model, index) =>
              renderRow(model.id, model.ownedBy, index === 0),
            )}
          </ul>
        )}
      </div>

      <div className="flex shrink-0 justify-end gap-2 pt-1">
        <Button
          type="button"
          variant="neutral"
          size="regular"
          onClick={onCancel}
        >
          {t("common.cancel")}
        </Button>
        <Button
          type="button"
          variant="solid"
          size="regular"
          disabled={isFetching}
          onClick={apply}
        >
          {t("providerForm.modelPicker.apply")}
        </Button>
      </div>
    </>
  );
}
