import { useEffect } from "react";
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
import type { TokenBudget } from "@/types/budget";

interface BudgetEditorProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  budget?: TokenBudget; // undefined = 新建, defined = 编辑
}

export function BudgetEditor({ open, onOpenChange, budget }: BudgetEditorProps) {
  const { t } = useTranslation();
  const isEdit = !!budget;
  const createMutation = useCreateBudget();
  const updateMutation = useUpdateBudget();

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

  // 编辑模式下填充表单
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
  }, [budget, reset]);

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

          {/* 作用域 */}
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label>{t("budget.scope")}</Label>
              <Select
                value={scope}
                onValueChange={(v) => setValue("scope", v as BudgetFormData["scope"])}
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

            {/* 周期 */}
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

          {/* 作用域值（非 global 时显示） */}
          {scope !== "global" && (
            <div className="space-y-1.5">
              <Label htmlFor="scopeValue">{t("budget.scopeValue")}</Label>
              <Input
                id="scopeValue"
                placeholder={t("budget.scopeValuePlaceholder")}
                {...register("scopeValue")}
              />
              {errors.scopeValue && (
                <p className="text-xs text-destructive">
                  {t(errors.scopeValue.message ?? "")}
                </p>
              )}
            </div>
          )}

          {/* 起始日（非 daily 时显示） */}
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
