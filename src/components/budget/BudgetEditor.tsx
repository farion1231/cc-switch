import { useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { toast } from "sonner";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { budgetSchema, type BudgetFormData } from "@/lib/schemas/budget";
import {
  useCreateBudget,
  useUpdateBudget,
} from "@/lib/query/budget";
import { useModelPricing } from "@/lib/query/usage";
import { useProvidersQuery } from "@/lib/query/queries";
import { KNOWN_APP_TYPES } from "@/types/usage";
import type { TokenBudget } from "@/types/budget";

interface BudgetEditorProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  budget?: TokenBudget; // undefined = 新建, defined = 编辑
  /** 预填推荐值（来自推荐面板） */
  recommendation?: {
    name: string;
    scope: BudgetFormData["scope"];
    scopeValue?: string;
    period: BudgetFormData["period"];
    limitTokens?: number;
    limitUsd?: string;
  };
}

export function BudgetEditor({ open, onOpenChange, budget, recommendation }: BudgetEditorProps) {
  const { t } = useTranslation();
  const isEdit = !!budget;
  const createMutation = useCreateBudget();
  const updateMutation = useUpdateBudget();

  // 拉取 provider / model 列表用于动态选择器
  const { data: claudeProviders } = useProvidersQuery("claude");
  const { data: codexProviders } = useProvidersQuery("codex");
  const { data: geminiProviders } = useProvidersQuery("gemini");
  const { data: modelPricing } = useModelPricing();

  // providers 列表保留 app_type 信息，scope_value 存储为 "app_type:provider_id"
  const providers = useMemo(() => {
    const seen = new Set<string>();
    const list: { id: string; name: string; appType: string }[] = [];
    for (const data of [claudeProviders, codexProviders, geminiProviders]) {
      if (!data?.providers) continue;
      for (const [id, p] of Object.entries(data.providers)) {
        if (!seen.has(id)) {
          seen.add(id);
          list.push({ id, name: p.name || id, appType: data.appType || "" });
        }
      }
    }
    return list.sort((a, b) => a.name.localeCompare(b.name));
  }, [claudeProviders, codexProviders, geminiProviders]);

  const models = useMemo(() => {
    if (!modelPricing) return [];
    // 从 proxy_request_logs 的 model 字段值 = modelPricing.modelId
    return modelPricing
      .map((m) => ({ id: m.modelId, name: m.displayName || m.modelId }))
      .sort((a, b) => a.name.localeCompare(b.name));
  }, [modelPricing]);

  const apps = useMemo(
    () => KNOWN_APP_TYPES.map((a) => ({ id: a, name: a.charAt(0).toUpperCase() + a.slice(1) })),
    [],
  );

  const {
    register,
    handleSubmit,
    reset,
    setValue,
    watch,
    formState: { errors, isSubmitting },
  } = useForm<BudgetFormData>({
    resolver: zodResolver(budgetSchema),
    defaultValues: {
      name: "",
      scope: "global",
      scopeValue: "",
      period: "monthly",
      periodStartDay: 1,
      limitTokens: undefined,
      limitUsd: "",
      enabled: true,
    },
  });

  const scope = watch("scope");
  const period = watch("period");

  // 编辑模式或推荐模式填充表单
  useEffect(() => {
    if (budget) {
      reset({
        name: budget.name,
        scope: budget.scope,
        scopeValue: budget.scopeValue ?? "",
        period: budget.period,
        periodStartDay: budget.periodStartDay,
        limitTokens: budget.limitTokens,
        limitUsd: budget.limitUsd ?? "",
        enabled: budget.enabled,
      });
    } else if (recommendation) {
      reset({
        name: recommendation.name,
        scope: recommendation.scope,
        scopeValue: recommendation.scopeValue ?? "",
        period: recommendation.period,
        periodStartDay: 1,
        limitTokens: recommendation.limitTokens,
        limitUsd: recommendation.limitUsd ?? "",
        enabled: true,
      });
    } else {
      reset({
        name: "",
        scope: "global",
        scopeValue: "",
        period: "monthly",
        periodStartDay: 1,
        limitTokens: undefined,
        limitUsd: "",
        enabled: true,
      });
    }
  }, [budget, recommendation, reset]);

  // scope 切换时清空 scopeValue
  const handleScopeChange = (v: string) => {
    setValue("scope", v as BudgetFormData["scope"]);
    setValue("scopeValue", "");
  };

  const onSubmit = async (data: BudgetFormData) => {
    try {
      if (isEdit && budget) {
        await updateMutation.mutateAsync({
          id: budget.id,
          patch: {
            name: data.name,
            scope: data.scope,
            scopeValue:
              data.scope === "global"
                ? null
                : data.scopeValue || null,
            period: data.period,
            periodStartDay: data.periodStartDay,
            limitTokens: data.limitTokens ?? null,
            limitUsd: data.limitUsd || null,
            enabled: data.enabled,
          },
        });
      } else {
        await createMutation.mutateAsync({
          name: data.name,
          scope: data.scope,
          scopeValue:
            data.scope !== "global" && data.scopeValue
              ? data.scopeValue
              : undefined,
          period: data.period,
          periodStartDay: data.periodStartDay,
          limitTokens: data.limitTokens,
          limitUsd: data.limitUsd || undefined,
          enabled: data.enabled,
        });
      }
      toast.success(t("budget.saved"));
      onOpenChange(false);
    } catch (e: unknown) {
      toast.error(
        e instanceof Error ? e.message : String(e),
      );
    }
  };

  // 根据 scope 类型渲染不同的 scopeValue 选择器
  const renderScopeValueSelector = () => {
    if (scope === "global") return null;

    let items: { id: string; name: string; appType?: string }[] = [];
    if (scope === "app") items = apps;
    else if (scope === "provider") items = providers;
    else if (scope === "model") items = models;

    return (
      <div className="space-y-1.5">
        <Label>{t("budget.scopeValue")}</Label>
        {items.length > 0 ? (
          <Select
            value={watch("scopeValue") || ""}
            onValueChange={(v) => setValue("scopeValue", v)}
          >
            <SelectTrigger>
              <SelectValue placeholder={t("budget.scopeValuePlaceholder")} />
            </SelectTrigger>
            <SelectContent className="max-h-48">
              {items.map((item) => {
                const itemValue = scope === "provider" && item.appType
                  ? `${item.appType}:${item.id}`
                  : item.id;
                return (
                  <SelectItem key={item.id} value={itemValue}>
                    {scope === "provider" && item.appType
                      ? `[${item.appType}] ${item.name}`
                      : item.name}
                  </SelectItem>
                );
              })}
            </SelectContent>
          </Select>
        ) : (
          // fallback: 没有可选项时用文本输入
          <Input
            placeholder={t("budget.scopeValuePlaceholder")}
            {...register("scopeValue")}
          />
        )}
        {errors.scopeValue && (
          <p className="text-xs text-destructive">
            {t(errors.scopeValue.message ?? "")}
          </p>
        )}
      </div>
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>
            {isEdit ? t("budget.edit") : t("budget.add")}
          </DialogTitle>
        </DialogHeader>

        <form onSubmit={handleSubmit(onSubmit)} className="space-y-4">
          {/* 预算名称 */}
          <div className="space-y-1.5">
            <Label htmlFor="name">{t("budget.name")}</Label>
            <Input
              id="name"
              placeholder={t("budget.namePlaceholder")}
              {...register("name")}
            />
            {errors.name && (
              <p className="text-xs text-destructive">
                {t(errors.name.message ?? "")}
              </p>
            )}
          </div>

          {/* 作用域 + 周期 */}
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label>{t("budget.scope")}</Label>
              <Select
                value={scope}
                onValueChange={handleScopeChange}
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="global">{t("budget.scopeGlobal")}</SelectItem>
                  <SelectItem value="app">{t("budget.scopeApp")}</SelectItem>
                  <SelectItem value="provider">{t("budget.scopeProvider")}</SelectItem>
                  <SelectItem value="model">{t("budget.scopeModel")}</SelectItem>
                </SelectContent>
              </Select>
            </div>

            <div className="space-y-1.5">
              <Label>{t("budget.period")}</Label>
              <Select
                value={period}
                onValueChange={(v) => setValue("period", v as BudgetFormData["period"])}
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="daily">{t("budget.periodDaily")}</SelectItem>
                  <SelectItem value="weekly">{t("budget.periodWeekly")}</SelectItem>
                  <SelectItem value="monthly">{t("budget.periodMonthly")}</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>

          {/* 作用域值：动态下拉 */}
          {renderScopeValueSelector()}

          {/* 起始日 */}
          {period !== "daily" && (
            <div className="space-y-1.5">
              <Label htmlFor="periodStartDay">
                {period === "weekly"
                  ? t("budget.periodStartDayWeekly")
                  : t("budget.periodStartDayMonthly")}
              </Label>
              <Input
                id="periodStartDay"
                type="number"
                min={period === "weekly" ? 0 : 1}
                max={period === "weekly" ? 6 : 28}
                {...register("periodStartDay", { valueAsNumber: true })}
              />
              {errors.periodStartDay && (
                <p className="text-xs text-destructive">
                  {t(errors.periodStartDay.message ?? "")}
                </p>
              )}
            </div>
          )}

          {/* 上限 */}
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label htmlFor="limitTokens">{t("budget.limitTokens")}</Label>
              <Input
                id="limitTokens"
                type="number"
                placeholder={t("budget.limitTokensPlaceholder")}
                {...register("limitTokens", { valueAsNumber: true })}
              />
              {errors.limitTokens && (
                <p className="text-xs text-destructive">
                  {t(errors.limitTokens.message ?? "")}
                </p>
              )}
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="limitUsd">{t("budget.limitUsd")}</Label>
              <Input
                id="limitUsd"
                placeholder={t("budget.limitUsdPlaceholder")}
                {...register("limitUsd")}
              />
              {errors.limitUsd && (
                <p className="text-xs text-destructive">
                  {t(errors.limitUsd.message ?? "")}
                </p>
              )}
            </div>
          </div>

          {/* 启用开关 */}
          <div className="flex items-center gap-2">
            <Switch
              id="enabled"
              checked={watch("enabled")}
              onCheckedChange={(v) => setValue("enabled", v)}
            />
            <Label htmlFor="enabled">{t("budget.enabled")}</Label>
          </div>

          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => onOpenChange(false)}
            >
              {t("common.cancel")}
            </Button>
            <Button type="submit" disabled={isSubmitting}>
              {isSubmitting ? t("common.saving") : t("common.save")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
