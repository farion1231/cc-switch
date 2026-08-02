import { useEffect, useState } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Form } from "@/components/ui/form";
import { providerSchema, type ProviderFormData } from "@/lib/schemas/provider";
import type {
  AggregateRoutes,
  Provider,
  ProviderCategory,
  ProviderMeta,
} from "@/types";
import { BasicFormFields } from "./BasicFormFields";
import { AggregateProviderFields } from "./AggregateProviderFields";
import type { ProviderFormValues } from "./ProviderForm";
import {
  AGGREGATE_SETTINGS_CONFIG,
  customRoutesToRows,
  validateAggregateRoutes,
  type AggregateCustomRouteRow,
} from "@/utils/aggregateRoutes";

export interface AggregateProviderFormProps {
  appId: "claude" | "codex";
  providerId?: string;
  submitLabel: string;
  onSubmit: (values: ProviderFormValues) => Promise<void> | void;
  onCancel: () => void;
  onSubmittingChange?: (isSubmitting: boolean) => void;
  initialData?: {
    name?: string;
    websiteUrl?: string;
    notes?: string;
    category?: ProviderCategory;
    meta?: ProviderMeta;
    icon?: string;
    iconColor?: string;
  };
  showButtons?: boolean;
  availableProviders?: Provider[];
}

/**
 * 聚合供应商表单：名称 / 备注 / 路由表。启用语义由外层 tab（或编辑入口）表达，
 * 因此这里不再有开关。提交负载与 ProviderForm 曾经的聚合模式保持一致：
 * 占位 settingsConfig + 仅保留聚合路由的 meta + presetCategory = "custom"。
 */
export function AggregateProviderForm({
  appId,
  providerId,
  submitLabel,
  onSubmit,
  onCancel,
  onSubmittingChange,
  initialData,
  showButtons = true,
  availableProviders = [],
}: AggregateProviderFormProps) {
  const { t } = useTranslation();
  const [aggregateRoutes, setAggregateRoutes] = useState<AggregateRoutes>(
    () => initialData?.meta?.aggregateRoutes ?? {},
  );
  // Codex 聚合路由的有序行态（Record 无法保留重复 key，提交校验依赖行态）
  const [aggregateCustomRows, setAggregateCustomRows] = useState<
    AggregateCustomRouteRow[]
  >(() => customRoutesToRows(initialData?.meta?.aggregateRoutes?.custom));

  const form = useForm<ProviderFormData>({
    resolver: zodResolver(providerSchema),
    defaultValues: {
      name: initialData?.name ?? "",
      websiteUrl: initialData?.websiteUrl ?? "",
      notes: initialData?.notes ?? "",
      settingsConfig: JSON.stringify(AGGREGATE_SETTINGS_CONFIG),
      icon: initialData?.icon ?? "",
      iconColor: initialData?.iconColor ?? "",
    },
    mode: "onSubmit",
  });
  const { isSubmitting } = form.formState;

  useEffect(() => {
    onSubmittingChange?.(isSubmitting);
  }, [isSubmitting, onSubmittingChange]);

  const handleSubmit = async (values: ProviderFormData) => {
    if (!values.name.trim()) {
      toast.error(
        t("providerForm.fillSupplierName", {
          defaultValue: "请填写供应商名称",
        }),
      );
      return;
    }

    const validation = validateAggregateRoutes(
      aggregateRoutes,
      appId,
      aggregateCustomRows,
    );
    if (!validation.ok) {
      toast.error(
        validation.reason === "empty"
          ? t("providerForm.aggregate.empty", {
              defaultValue: "Configure at least one aggregate route.",
            })
          : validation.reason === "duplicate"
            ? t("providerForm.aggregate.duplicateKey", {
                key: validation.key,
                defaultValue: "Duplicate model name: {{key}}",
              })
            : t("providerForm.aggregate.incomplete", {
                tier: validation.tier,
                defaultValue:
                  "The {{tier}} route requires both a provider and a model.",
              }),
      );
      return;
    }

    // 与 ProviderForm 聚合模式的 meta 组装保持一致：剥离端点/认证/模型等
    // 普通供应商字段，仅保留聚合路由（编辑场景下其余 meta 原样保留）
    const meta: ProviderMeta = {
      ...(initialData?.meta ?? {}),
      commonConfigEnabled: undefined,
      endpointAutoSelect: undefined,
      claudeDesktopMode: undefined,
      providerType: undefined,
      authBinding: undefined,
      githubAccountId: undefined,
      codexFastMode: undefined,
      codexChatReasoning: undefined,
      promptCacheRouting: undefined,
      customUserAgent: undefined,
      localProxyRequestOverrides: undefined,
      apiFormat: undefined,
      apiKeyField: undefined,
      impersonateClaudeCode: undefined,
      maxOutputTokens: undefined,
      isFullUrl: undefined,
      aggregateRoutes: validation.routes,
    };

    await onSubmit({
      ...values,
      name: values.name.trim(),
      websiteUrl: values.websiteUrl?.trim() ?? "",
      settingsConfig: JSON.stringify(AGGREGATE_SETTINGS_CONFIG),
      presetCategory: "custom",
      meta,
    });
  };

  return (
    <Form {...form}>
      <form
        id="provider-form"
        onSubmit={form.handleSubmit(handleSubmit)}
        className="space-y-6 glass rounded-xl p-6 border border-white/10"
      >
        <BasicFormFields form={form} />

        <AggregateProviderFields
          appId={appId}
          routes={aggregateRoutes}
          onRoutesChange={setAggregateRoutes}
          providers={availableProviders}
          currentProviderId={providerId}
          customRows={aggregateCustomRows}
          onCustomRowsChange={setAggregateCustomRows}
        />

        {showButtons && (
          <div className="flex justify-end gap-2">
            <Button variant="outline" type="button" onClick={onCancel}>
              {t("common.cancel")}
            </Button>
            <Button type="submit" disabled={isSubmitting}>
              {submitLabel}
            </Button>
          </div>
        )}
      </form>
    </Form>
  );
}
