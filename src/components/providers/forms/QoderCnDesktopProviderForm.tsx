import { useEffect, useState } from "react";
import { zodResolver } from "@hookform/resolvers/zod";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Form } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  qoderCnDesktopApi,
  qoderCnGatewayQueryKey,
  type QoderUpstreamFormat,
} from "@/lib/api/qoderCnDesktop";
import { providerSchema, type ProviderFormData } from "@/lib/schemas/provider";
import { toast } from "@/lib/toast";
import {
  extractErrorMessage,
  translateQoderProviderMutationError,
} from "@/utils/errorUtils";
import { BasicFormFields, REQUIRED_LABEL } from "./BasicFormFields";
import type { ProviderFormProps } from "./ProviderForm";
import { ApiKeySection } from "./shared/ApiKeySection";
import { EndpointField } from "./shared/EndpointField";

const formats: Record<QoderUpstreamFormat, string> = {
  openai_responses: "OpenAI Responses",
  openai_chat: "OpenAI Chat Completions",
  anthropic: "Anthropic Messages",
};

export function QoderCnDesktopProviderForm({
  providerId,
  initialData,
  onSubmit,
  onCancel,
  onSubmittingChange,
}: ProviderFormProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const settings = initialData?.settingsConfig;
  const initialFormat = initialData?.meta?.apiFormat;
  const [format, setFormat] = useState<QoderUpstreamFormat>(
    initialFormat && initialFormat !== "gemini_native"
      ? initialFormat
      : "openai_responses",
  );
  const [baseUrl, setBaseUrl] = useState(
    typeof settings?.base_url === "string" ? settings.base_url : "",
  );
  const [model, setModel] = useState(
    typeof settings?.model === "string" ? settings.model : "",
  );
  const [apiKey, setApiKey] = useState("");
  const [anthropicApiKey, setAnthropicApiKey] = useState(
    initialData?.meta?.apiKeyField === "ANTHROPIC_API_KEY",
  );
  const [sourceId, setSourceId] = useState("");
  const form = useForm<ProviderFormData>({
    resolver: zodResolver(providerSchema),
    defaultValues: {
      name: initialData?.name ?? "",
      websiteUrl: initialData?.websiteUrl ?? "",
      notes: initialData?.notes ?? "",
      icon: initialData?.icon ?? "",
      iconColor: initialData?.iconColor ?? "",
      settingsConfig: "{}",
    },
  });
  const { data: gateway } = useQuery({
    queryKey: qoderCnGatewayQueryKey,
    queryFn: qoderCnDesktopApi.status,
    enabled: !providerId,
  });
  const copyProvider = useMutation({
    mutationFn: qoderCnDesktopApi.import,
    onSuccess: async () => {
      await queryClient.invalidateQueries({
        queryKey: ["providers", "qoder-cn-desktop"],
      });
      onCancel();
    },
    onError: (error) => {
      const message = extractErrorMessage(error);
      toast.error(
        translateQoderProviderMutationError(message, t) ||
          message ||
          t("common.error"),
      );
    },
  });

  useEffect(() => {
    onSubmittingChange?.(form.formState.isSubmitting || copyProvider.isPending);
  }, [form.formState.isSubmitting, copyProvider.isPending, onSubmittingChange]);

  const save = form.handleSubmit(async (values) => {
    if (!values.name.trim()) {
      form.setError("name", {
        message: t("qoderCnDesktop.errors.nameAndModelRequired"),
      });
      return;
    }
    await onSubmit({
      ...values,
      settingsConfig: JSON.stringify({
        base_url: baseUrl.trim().replace(/\/+$/, ""),
        model: model.trim(),
        apiKey: apiKey.trim(),
      }),
      meta: {
        ...initialData?.meta,
        apiFormat: format,
        apiKeyField:
          format === "anthropic" && anthropicApiKey
            ? "ANTHROPIC_API_KEY"
            : undefined,
      },
    });
  });

  return (
    <Form {...form}>
      <form id="provider-form" onSubmit={save} className="space-y-5">
        {!providerId && Boolean(gateway?.importableProviders.length) && (
          <div className="flex gap-2">
            <Select value={sourceId} onValueChange={setSourceId}>
              <SelectTrigger
                aria-label={t("qoderCnDesktop.source")}
                className="flex-1"
              >
                <SelectValue placeholder={t("qoderCnDesktop.source")} />
              </SelectTrigger>
              <SelectContent>
                {gateway?.importableProviders.map((source) => (
                  <SelectItem key={source.id} value={source.id}>
                    {source.name} · {source.model}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Button
              type="button"
              variant="neutral"
              disabled={
                !sourceId ||
                copyProvider.isPending ||
                form.formState.isSubmitting
              }
              onClick={() => copyProvider.mutate(sourceId)}
            >
              {t("qoderCnDesktop.import")}
            </Button>
          </div>
        )}
        <BasicFormFields form={form} />
        <EndpointField
          id="qoder-base-url"
          label={t("providerForm.apiEndpoint")}
          value={baseUrl}
          onChange={setBaseUrl}
          placeholder="https://api.example.com/v1"
        />
        <ApiKeySection
          id="qoder-api-key"
          value={apiKey}
          onChange={setApiKey}
          required={!providerId}
          shouldShowLink={false}
          websiteUrl=""
          hint={providerId ? t("qoderCnDesktop.replaceKey") : undefined}
        />
        <div className="grid gap-4 sm:grid-cols-2">
          <div className="space-y-2">
            <Label htmlFor="qoder-format">{t("qoderCnDesktop.format")}</Label>
            <Select
              value={format}
              onValueChange={(value) => setFormat(value as QoderUpstreamFormat)}
            >
              <SelectTrigger id="qoder-format">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {Object.entries(formats).map(([value, label]) => (
                  <SelectItem key={value} value={value}>
                    {label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-2">
            <Label htmlFor="qoder-model" className={REQUIRED_LABEL}>
              {t("qoderCnDesktop.upstreamModel")}
            </Label>
            <Input
              id="qoder-model"
              value={model}
              required
              onChange={(event) => setModel(event.target.value)}
              autoComplete="off"
            />
          </div>
        </div>
        {format === "anthropic" && (
          <div className="flex items-center gap-2">
            <Checkbox
              id="qoder-anthropic-api-key"
              checked={anthropicApiKey}
              onCheckedChange={(value) => setAnthropicApiKey(value === true)}
            />
            <Label htmlFor="qoder-anthropic-api-key">
              {t("qoderCnDesktop.xApiKey")}
            </Label>
          </div>
        )}
      </form>
    </Form>
  );
}
