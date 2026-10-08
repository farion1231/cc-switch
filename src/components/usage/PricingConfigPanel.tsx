import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import { Globe, Info, Loader2, Pencil, Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { HoverTip } from "@/components/ui/hover-tip";
import { HelpTip } from "@/components/ui/help-tip";
import { Notice } from "@/components/ui/notice";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { AppGlyph, APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { useModelPricing, useDeleteModelPricing } from "@/lib/query/usage";
import { TablePagination, useClientPagination } from "./TablePagination";
import { proxyApi } from "@/lib/api/proxy";
import type { ModelPricing } from "@/types/usage";
import { cn } from "@/lib/utils";
import { PricingEditModal } from "./PricingEditModal";
import { ModelsDevAutoSyncPanel } from "./ModelsDevAutoSyncPanel";
import { ModelsDevPickerDialog } from "./ModelsDevPickerDialog";
import {
  getResolvedLang,
  joinClauses,
  joinNames,
  parseFiniteNumber,
} from "./format";
import { usageTable } from "./usageTable";

/** 计费默认配置只管这 4 个有路由用量管线的应用 */
const PRICING_APPS = ["claude", "codex", "gemini", "grokbuild"] as const;
type PricingApp = (typeof PRICING_APPS)[number];
type PricingModelSource = "request" | "response";

type SourceState = Record<PricingApp, PricingModelSource>;

const DEFAULT_SOURCES: SourceState = {
  claude: "response",
  codex: "response",
  gemini: "response",
  grokbuild: "response",
};

const errorText = (error: unknown) =>
  error instanceof Error
    ? error.message
    : typeof error === "string"
      ? error
      : "Unknown error";

/** 「$4.00」「$0.075」：至少两位小数，多余的 0 去掉，最多 4 位。 */
export function formatUnitPrice(value: string): string {
  const num = parseFiniteNumber(value);
  if (num == null) return `$${value}`;
  const fixed = num.toFixed(4).replace(/0+$/, "");
  const [whole, frac = ""] = fixed.split(".");
  return `$${whole}.${frac.padEnd(2, "0")}`;
}

function useBillingSources() {
  const { t } = useTranslation();
  const [sources, setSources] = useState<SourceState>(DEFAULT_SOURCES);
  const [isLoading, setIsLoading] = useState(true);

  useEffect(() => {
    let isMounted = true;
    const loadAll = async () => {
      setIsLoading(true);
      try {
        const results = await Promise.all(
          PRICING_APPS.map(async (app) => {
            const source = await proxyApi.getPricingModelSource(app);
            return {
              app,
              source: (source === "request"
                ? "request"
                : "response") as PricingModelSource,
            };
          }),
        );
        if (!isMounted) return;
        const next: SourceState = { ...DEFAULT_SOURCES };
        for (const result of results) next[result.app] = result.source;
        setSources(next);
      } catch (error) {
        toast.error(
          t("settings.globalProxy.pricingLoadFailed", {
            error: errorText(error),
          }),
        );
      } finally {
        if (isMounted) setIsLoading(false);
      }
    };
    void loadAll();
    return () => {
      isMounted = false;
    };
  }, [t]);

  return { sources, setSources, isLoading };
}

function BillingDefaultsDialog({
  open,
  initial,
  onClose,
  onSaved,
}: {
  open: boolean;
  initial: SourceState;
  onClose: () => void;
  onSaved: (next: SourceState) => void;
}) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState<SourceState>(initial);
  const [isSaving, setIsSaving] = useState(false);

  useEffect(() => {
    if (open) setDraft(initial);
  }, [open, initial]);

  const isDirty = PRICING_APPS.some((app) => draft[app] !== initial[app]);

  const save = async () => {
    setIsSaving(true);
    try {
      await Promise.all(
        PRICING_APPS.map((app) =>
          proxyApi.setPricingModelSource(app, draft[app]),
        ),
      );
      toast.success(t("settings.globalProxy.pricingSaved"));
      onSaved({ ...draft });
      onClose();
    } catch (error) {
      toast.error(
        t("settings.globalProxy.pricingSaveFailed", {
          error: errorText(error),
        }),
      );
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next && !isSaving) onClose();
      }}
    >
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>
            {t("settings.globalProxy.pricingDefaultsTitle")}
          </DialogTitle>
          <DialogDescription>
            {t("settings.globalProxy.pricingDefaultsDescription")}
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col px-6 py-2">
          {PRICING_APPS.map((app, index) => (
            <div
              key={app}
              className={cn(
                "flex h-11 items-center gap-3",
                index < PRICING_APPS.length - 1 && "border-b border-border",
              )}
            >
              <AppGlyph app={app} size={16} badgeClassName="bg-surface" />
              <span className="flex-1 text-body text-fg-1">
                {APP_DISPLAY_NAME[app]}
              </span>
              <Select
                value={draft[app]}
                onValueChange={(value) =>
                  setDraft((prev) => ({
                    ...prev,
                    [app]: value as PricingModelSource,
                  }))
                }
                disabled={isSaving}
              >
                <SelectTrigger
                  className="h-7 w-36 text-body"
                  aria-label={`${APP_DISPLAY_NAME[app]} · ${t("settings.globalProxy.pricingModelSourceLabel")}`}
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="response">
                    {t("settings.globalProxy.pricingModelSourceResponse")}
                  </SelectItem>
                  <SelectItem value="request">
                    {t("settings.globalProxy.pricingModelSourceRequest")}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>
          ))}
        </div>
        <DialogFooter>
          <Button
            type="button"
            variant="neutral"
            size="regular"
            onClick={onClose}
            disabled={isSaving}
          >
            {t("common.cancel")}
          </Button>
          <Button
            type="button"
            variant="solid"
            size="regular"
            onClick={() => void save()}
            disabled={isSaving || !isDirty}
          >
            {isSaving && <Loader2 className="h-4 w-4 animate-spin" />}
            {isSaving ? t("common.saving") : t("common.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** 计费默认配置一行：当前配置的摘要 +「修改…」。 */
function BillingDefaultsRow() {
  const { t, i18n } = useTranslation();
  const lang = getResolvedLang(i18n);
  const { sources, setSources, isLoading } = useBillingSources();
  const [dialogOpen, setDialogOpen] = useState(false);

  const groups = (["response", "request"] as const)
    .map((source) => ({
      source,
      apps: PRICING_APPS.filter((app) => sources[app] === source).map(
        (app) => APP_DISPLAY_NAME[app],
      ),
    }))
    .filter((group) => group.apps.length > 0);
  const summary = joinClauses(
    groups.map(
      (group) =>
        `${
          group.source === "response"
            ? t("usage.pricing.byResponse")
            : t("usage.pricing.byRequest")
        } · ${joinNames(group.apps, lang)}`,
    ),
    lang,
  );

  return (
    <>
      <div className="flex items-center gap-3 rounded-panel border border-border px-4 py-2.5">
        <Info
          aria-hidden="true"
          className="h-4 w-4 shrink-0 text-fg-2"
          strokeWidth={1.5}
        />
        <div className="flex min-w-0 flex-1 flex-col">
          <div className="flex min-w-0 items-center gap-0.5">
            <span className="truncate text-body font-semibold text-fg-1">
              {t("settings.globalProxy.pricingDefaultsTitle")}
            </span>
            <HelpTip title={t("usage.pricing.billingHelpTitle")}>
              {t("usage.pricing.billingHelp")}
            </HelpTip>
          </div>
          <span className="truncate text-caption text-fg-2" title={summary}>
            {isLoading ? "…" : summary}
          </span>
        </div>
        <Button
          type="button"
          variant="solid"
          size="compact"
          disabled={isLoading}
          onClick={() => setDialogOpen(true)}
        >
          {t("usage.pricing.edit")}
        </Button>
      </div>
      <BillingDefaultsDialog
        open={dialogOpen}
        initial={sources}
        onClose={() => setDialogOpen(false)}
        onSaved={setSources}
      />
    </>
  );
}

/** 定价子页签（v7 S6）：models.dev 同步、计费默认配置、模型定价表。 */
export function PricingConfigPanel() {
  const { t } = useTranslation();
  const { data: pricing, isLoading, error } = useModelPricing();
  const deleteMutation = useDeleteModelPricing();
  const [editingModel, setEditingModel] = useState<ModelPricing | null>(null);
  const [isAddingNew, setIsAddingNew] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<ModelPricing | null>(null);
  const [pickerOpen, setPickerOpen] = useState(false);

  const handleAddNew = () => {
    setIsAddingNew(true);
    setEditingModel({
      modelId: "",
      displayName: "",
      inputCostPerMillion: "0",
      outputCostPerMillion: "0",
      cacheReadCostPerMillion: "0",
      cacheCreationCostPerMillion: "0",
    });
  };

  const rows = pricing ?? [];
  const pagination = useClientPagination(rows);

  return (
    <div className="flex flex-col gap-3 pt-3">
      <ModelsDevAutoSyncPanel />
      <BillingDefaultsRow />

      <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-2">
        <div className="flex items-center gap-0.5">
          <h3 className="m-0 whitespace-nowrap text-strong font-semibold text-fg-1">
            {t("usage.pricing.tableTitle", { count: rows.length })}
          </h3>
          <HelpTip title={t("usage.pricing.costHelpTitle")}>
            {t("usage.pricing.costHelp")}
          </HelpTip>
        </div>
        <span className="whitespace-nowrap text-caption text-fg-3">
          {t("usage.pricing.unit")}
        </span>
        <div className="flex-1" />
        <Button
          type="button"
          variant="neutral"
          size="compact"
          onClick={() => setPickerOpen(true)}
        >
          <Globe className="h-3.5 w-3.5" />
          {t("usage.importFromModelsDev")}
        </Button>
        <Button
          type="button"
          variant="neutral"
          size="compact"
          onClick={handleAddNew}
        >
          <Plus className="h-3.5 w-3.5" />
          {t("usage.addPricing")}
        </Button>
      </div>

      {isLoading ? (
        <div className={usageTable.skeleton} />
      ) : error ? (
        <Notice
          tone="danger"
          title={`${t("usage.loadPricingError")}: ${String(error)}`}
        />
      ) : (
        <div className={usageTable.scroller}>
          <table
            className={cn(usageTable.table, "min-w-[620px]")}
            aria-label={t("usage.modelPricing")}
          >
            <thead>
              <tr className={usageTable.headRow}>
                <th className={usageTable.th}>{t("usage.model")}</th>
                <th className={usageTable.thEnd}>{t("usage.inputTokens")}</th>
                <th className={usageTable.thEnd}>{t("usage.outputTokens")}</th>
                <th className={usageTable.thEnd}>
                  {t("usage.cacheReadTokens")}
                </th>
                <th className={usageTable.thEnd}>
                  {t("usage.pricing.cacheWrite")}
                </th>
                <th className="w-[72px]" aria-label={t("common.actions")} />
              </tr>
            </thead>
            <tbody>
              {rows.length === 0 ? (
                <tr>
                  <td colSpan={6} className={usageTable.empty}>
                    {t("usage.noPricingData")}
                  </td>
                </tr>
              ) : (
                pagination.pageRows.map((model) => {
                  const name = model.displayName || model.modelId;
                  return (
                    <tr
                      key={model.modelId}
                      className={cn(usageTable.row, "group hover:bg-subtle")}
                    >
                      <td className={usageTable.td}>
                        <span className="flex max-w-[340px] items-baseline gap-2">
                          {model.displayName && (
                            <span className="truncate text-fg-1">
                              {model.displayName}
                            </span>
                          )}
                          <span
                            className="truncate font-mono text-caption text-fg-2"
                            title={model.modelId}
                          >
                            {model.modelId}
                          </span>
                        </span>
                      </td>
                      <td className={usageTable.tdEnd}>
                        {formatUnitPrice(model.inputCostPerMillion)}
                      </td>
                      <td className={usageTable.tdEnd}>
                        {formatUnitPrice(model.outputCostPerMillion)}
                      </td>
                      <td className={usageTable.tdEnd}>
                        {formatUnitPrice(model.cacheReadCostPerMillion)}
                      </td>
                      <td className={cn(usageTable.tdEnd, "text-fg-2")}>
                        {formatUnitPrice(model.cacheCreationCostPerMillion)}
                      </td>
                      <td className="whitespace-nowrap pe-1 text-end">
                        <HoverTip content={t("common.edit")}>
                          <Button
                            type="button"
                            variant="quiet"
                            size="icon-compact"
                            aria-label={t("usage.pricing.editAria", { name })}
                            onClick={() => {
                              setIsAddingNew(false);
                              setEditingModel(model);
                            }}
                          >
                            <Pencil
                              className="h-3.5 w-3.5"
                              strokeWidth={1.75}
                            />
                          </Button>
                        </HoverTip>
                        <HoverTip content={t("common.delete")}>
                          <Button
                            type="button"
                            variant="quiet"
                            size="icon-compact"
                            aria-label={t("usage.pricing.deleteAria", { name })}
                            className="hover:text-danger-text"
                            onClick={() => setDeleteTarget(model)}
                          >
                            <Trash2
                              className="h-3.5 w-3.5"
                              strokeWidth={1.75}
                            />
                          </Button>
                        </HoverTip>
                      </td>
                    </tr>
                  );
                })
              )}
            </tbody>
          </table>
        </div>
      )}
      {!isLoading && !error && (
        <TablePagination
          page={pagination.page}
          totalPages={pagination.totalPages}
          total={pagination.total}
          onPageChange={pagination.setPage}
        />
      )}

      {editingModel && (
        <PricingEditModal
          open={!!editingModel}
          model={editingModel}
          isNew={isAddingNew}
          onClose={() => {
            setEditingModel(null);
            setIsAddingNew(false);
          }}
        />
      )}

      {pickerOpen && (
        <ModelsDevPickerDialog
          open={pickerOpen}
          onClose={() => setPickerOpen(false)}
          onImported={() => setPickerOpen(false)}
        />
      )}

      <ConfirmDialog
        isOpen={!!deleteTarget}
        title={t("usage.deleteConfirmTitle")}
        message={t("usage.deleteConfirmDesc")}
        confirmText={
          deleteMutation.isPending ? t("common.deleting") : t("common.delete")
        }
        variant="destructive"
        onConfirm={() => {
          if (!deleteTarget) return;
          deleteMutation.mutate(deleteTarget.modelId, {
            onSuccess: () => setDeleteTarget(null),
          });
        }}
        onCancel={() => setDeleteTarget(null)}
      />
    </div>
  );
}
