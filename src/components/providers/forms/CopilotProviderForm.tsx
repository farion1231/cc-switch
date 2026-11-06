import { useCallback, useEffect, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { Download, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Form, FormLabel } from "@/components/ui/form";
import { HelpTip } from "@/components/ui/help-tip";
import { Notice } from "@/components/ui/notice";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import JsonEditor from "@/components/JsonEditor";
import { useDarkMode } from "@/hooks/useDarkMode";
import { toast } from "@/lib/toast";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import { resolveManagedAccountId } from "@/lib/authBinding";
import type { ProviderFormData } from "@/lib/schemas/provider";
import type { ProviderCategory, ProviderMeta } from "@/types";
import {
  COPILOT_ENV,
  COPILOT_OFFICIAL_PRESET_ID,
  COPILOT_API_FORMATS,
  applyCopilotApiFormat,
  copilotApiFormatOf,
  compactCopilotEnv,
  copilotEnvOf,
  copilotOfficialPreset,
  copilotProviderPresets,
  formatCopilotHeaders,
  type CopilotApiFormatId,
  type CopilotEnv,
} from "@/config/copilotProviderPresets";
import { normalizeRequestHeaders } from "./helpers/requestHeaders";
import { BasicFormFields, REQUIRED_LABEL } from "./BasicFormFields";
import { ApiKeySection, EndpointField, ModelInputWithFetch } from "./shared";
import { RequestHeadersEditor } from "./RequestHeadersEditor";
import { ProviderPresetSelector } from "./ProviderPresetSelector";
import { CopilotAuthSection } from "./CopilotAuthSection";
import type { ProviderFormProps } from "./ProviderForm";

const PRESETS = [
  { id: COPILOT_OFFICIAL_PRESET_ID, preset: copilotOfficialPreset },
  ...copilotProviderPresets.map((preset, index) => ({
    id: String(index),
    preset,
  })),
];

/** `COPILOT_PROVIDER_HEADERS` 的换行文本 ⇄ 编辑器里的键值对。 */
function parseHeaders(text: string | undefined): Record<string, string> {
  const headers: Record<string, string> = {};
  for (const line of (text ?? "").split(/\r?\n|\\n/)) {
    const index = line.indexOf(":");
    if (index <= 0) continue;
    const name = line.slice(0, index).trim();
    if (name) headers[name] = line.slice(index + 1).trim();
  }
  return headers;
}

/** Copilot CLI `--reasoning-effort` 的取值（与后端 `REASONING_EFFORTS` 一致）。 */
const REASONING_EFFORTS = [
  "none",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
] as const;
const DEFAULT_EFFORT = "default";

function reasoningEffortOf(settingsConfig: unknown): string {
  const value = (settingsConfig as { reasoningEffort?: unknown } | undefined)
    ?.reasoningEffort;
  return typeof value === "string" &&
    (REASONING_EFFORTS as readonly string[]).includes(value.trim())
    ? value.trim()
    : "";
}

/** 卡片配置：`env` 之外只有可选的 `reasoningEffort`，没设时不写这个键。 */
function cardConfig(env: CopilotEnv, reasoningEffort: string) {
  return reasoningEffort ? { env, reasoningEffort } : { env };
}

const presetModels = (models?: string[]): FetchedModel[] =>
  (models ?? []).map((id) => ({ id, ownedBy: null }));

export function CopilotProviderForm({
  initialData,
  onSubmit,
  onCancel,
  submitLabel,
  showButtons = true,
  onSubmittingChange,
  onSubmitReadyChange,
  onManageAuthAccounts,
}: ProviderFormProps) {
  const { t } = useTranslation();
  const darkMode = useDarkMode();
  const isEdit = Boolean(initialData);
  const [env, setEnv] = useState<CopilotEnv>(() =>
    copilotEnvOf(initialData?.settingsConfig),
  );
  const [reasoningEffort, setReasoningEffort] = useState(() =>
    reasoningEffortOf(initialData?.settingsConfig),
  );
  const [text, setText] = useState(() =>
    JSON.stringify(
      cardConfig(
        copilotEnvOf(initialData?.settingsConfig),
        reasoningEffortOf(initialData?.settingsConfig),
      ),
      null,
      2,
    ),
  );
  const [jsonValid, setJsonValid] = useState(true);
  // 请求头编辑器的行放在组件状态里：新行的草稿键（draft-header:<时间戳>）本身带冒号，
  // 不能经 COPILOT_PROVIDER_HEADERS 的「名称: 值」文本往返，否则会变成真的请求头。
  const [headers, setHeaders] = useState<Record<string, string>>(() =>
    parseHeaders(
      copilotEnvOf(initialData?.settingsConfig)[COPILOT_ENV.headers],
    ),
  );
  const [presetId, setPresetId] = useState(
    isEdit ? "custom" : COPILOT_OFFICIAL_PRESET_ID,
  );
  const [category, setCategory] = useState<ProviderCategory>(
    initialData?.category ?? (isEdit ? "custom" : "official"),
  );
  const [accountId, setAccountId] = useState<string | null>(() =>
    resolveManagedAccountId(initialData?.meta, "github_copilot"),
  );
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const [busy, setBusy] = useState(false);
  const preset =
    presetId === "custom" || presetId === COPILOT_OFFICIAL_PRESET_ID
      ? undefined
      : copilotProviderPresets[Number(presetId)];
  const isOfficial = category === "official";

  const form = useForm<ProviderFormData>({
    defaultValues: {
      name: initialData?.name ?? (isEdit ? "" : copilotOfficialPreset.name),
      notes: initialData?.notes ?? "",
      websiteUrl:
        initialData?.websiteUrl ??
        (isEdit ? "" : copilotOfficialPreset.websiteUrl),
      icon: initialData?.icon ?? (isEdit ? "" : "githubcopilot"),
      iconColor: initialData?.iconColor ?? "",
      settingsConfig: text,
    },
  });
  const name = form.watch("name");

  const update = (
    next: CopilotEnv,
    effort = reasoningEffort,
    nextHeaders = parseHeaders(next[COPILOT_ENV.headers]),
  ) => {
    setEnv(next);
    setHeaders(nextHeaders);
    setReasoningEffort(effort);
    setText(JSON.stringify(cardConfig(next, effort), null, 2));
    setJsonValid(true);
  };
  const setField = (key: string, value: string) =>
    update({ ...env, [key]: value });

  const baseUrl = env[COPILOT_ENV.baseUrl] ?? "";
  const apiKey = env[COPILOT_ENV.apiKey] ?? "";
  const model = env[COPILOT_ENV.model] ?? "";
  const apiFormat = copilotApiFormatOf(env);
  const providerType = env[COPILOT_ENV.type] || "openai";
  const ready = Boolean(
    jsonValid &&
      name.trim() &&
      (isOfficial ||
        (/^https?:\/\//.test(baseUrl.trim()) &&
          (model.trim() ||
            env[COPILOT_ENV.modelId]?.trim() ||
            env[COPILOT_ENV.wireModel]?.trim()))),
  );
  useEffect(() => {
    onSubmitReadyChange?.(ready);
  }, [ready, onSubmitReadyChange]);

  const handleFetchModels = useCallback(() => {
    const url = baseUrl.trim();
    const key = apiKey.trim() || env[COPILOT_ENV.bearerToken]?.trim() || "";
    if (!url || !key) {
      showFetchModelsError(null, t, {
        hasApiKey: Boolean(key),
        hasBaseUrl: Boolean(url),
      });
      return;
    }
    setIsFetchingModels(true);
    fetchModelsForConfig(url, key, false, undefined, undefined, {
      // 与 Pi 共用取模型命令：Anthropic 用 x-api-key，其余用 Bearer。
      apiFormat:
        providerType === "anthropic" ? "anthropic-messages" : undefined,
      requestHeaders: parseHeaders(env[COPILOT_ENV.headers]),
    })
      .then((models) => {
        setFetchedModels(models);
        if (models.length === 0) {
          toast.info(t("providerForm.fetchModelsEmpty"));
        } else {
          toast.success(
            t("providerForm.fetchModelsSuccess", { count: models.length }),
          );
        }
      })
      .catch((err) => {
        console.warn("[ModelFetch] Failed:", err);
        showFetchModelsError(err, t);
      })
      .finally(() => setIsFetchingModels(false));
  }, [apiKey, baseUrl, env, providerType, t]);

  const presetSelector = (
    <ProviderPresetSelector
      selectedPresetId={presetId}
      presetEntries={PRESETS}
      presetCategoryLabels={{
        official: t("providerForm.categoryOfficial"),
        custom: t("providerPreset.custom"),
        cn_official: t("providerForm.categoryCnOfficial"),
        third_party: t("providerPreset.group.thirdparty"),
        aggregator: t("providerPreset.group.thirdparty"),
        cloud_provider: t("providerPreset.group.cloud"),
        local: t("providerPreset.group.local"),
      }}
      category={category}
      onPresetChange={(id) => {
        setPresetId(id);
        const next =
          id === COPILOT_OFFICIAL_PRESET_ID
            ? copilotOfficialPreset
            : id === "custom"
              ? undefined
              : copilotProviderPresets[Number(id)];
        setCategory(next?.category ?? "custom");
        update(
          next
            ? { ...next.settingsConfig.env }
            : {
                [COPILOT_ENV.baseUrl]: "",
                [COPILOT_ENV.type]: "openai",
                [COPILOT_ENV.wireApi]: "completions",
                [COPILOT_ENV.apiKey]: "",
                [COPILOT_ENV.model]: "",
              },
          "",
        );
        setFetchedModels(presetModels(next?.models));
        form.reset({
          name: next?.nameKey ? t(next.nameKey) : (next?.name ?? ""),
          notes: "",
          websiteUrl: next?.websiteUrl ?? "",
          icon: next?.icon ?? "",
          iconColor: next?.iconColor ?? "",
          settingsConfig: "",
        });
      }}
    />
  );

  const submit = form.handleSubmit(async (identity) => {
    if (!ready || busy) return;
    setBusy(true);
    onSubmittingChange?.(true);
    try {
      const headers = normalizeRequestHeaders(
        parseHeaders(env[COPILOT_ENV.headers]),
      );
      let saved = compactCopilotEnv({
        ...env,
        [COPILOT_ENV.headers]: formatCopilotHeaders(headers),
      });
      if (isOfficial) {
        // 官方卡只用 GitHub 登录：不留任何自带模型字段。
        saved = saved[COPILOT_ENV.model]
          ? { [COPILOT_ENV.model]: saved[COPILOT_ENV.model] }
          : {};
      }
      const meta: ProviderMeta = { ...(initialData?.meta ?? {}) };
      delete meta.githubAccountId;
      delete meta.authBinding;
      if (isOfficial && accountId) {
        meta.authBinding = {
          source: "managed_account",
          authProvider: "github_copilot",
          accountId,
        };
      }
      await onSubmit({
        ...identity,
        name: identity.name.trim(),
        meta,
        presetId: presetId === "custom" ? undefined : presetId,
        presetCategory: category,
        settingsConfig: JSON.stringify(cardConfig(saved, reasoningEffort)),
      });
    } finally {
      setBusy(false);
      onSubmittingChange?.(false);
    }
  });

  return (
    <Form {...form}>
      <form id="provider-form" className="space-y-6" onSubmit={submit}>
        {!isEdit && presetSelector}
        <Notice
          title={
            isOfficial
              ? t("copilot.form.officialTitle")
              : t("copilot.form.byokTitle")
          }
        >
          {isOfficial
            ? t("copilot.form.officialHint")
            : t("copilot.form.byokHint")}
        </Notice>
        <fieldset
          disabled={busy || !jsonValid}
          className="min-w-0 space-y-6 border-0 p-0 disabled:opacity-50"
        >
          <BasicFormFields form={form} />
          {isOfficial ? (
            <CopilotAuthSection
              mode="select"
              selectedAccountId={accountId}
              onAccountSelect={setAccountId}
              onManageAccounts={
                onManageAuthAccounts
                  ? () => onManageAuthAccounts("github_copilot")
                  : undefined
              }
            />
          ) : (
            <>
              <div className="space-y-2">
                <Label htmlFor="copilot-api-format">
                  {t("copilot.form.apiFormat")}
                </Label>
                <Select
                  value={apiFormat}
                  onValueChange={(value) =>
                    update(
                      applyCopilotApiFormat(env, value as CopilotApiFormatId),
                    )
                  }
                >
                  <SelectTrigger id="copilot-api-format" className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {COPILOT_API_FORMATS.map((format) => (
                      <SelectItem key={format.id} value={format.id}>
                        {format.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <ApiKeySection
                id="copilot-api-key"
                value={apiKey}
                onChange={(value) => setField(COPILOT_ENV.apiKey, value)}
                category={category}
                shouldShowLink={Boolean(preset?.apiKeyUrl)}
                websiteUrl={preset?.apiKeyUrl ?? ""}
                isPartner={preset?.isPartner}
                partnerPromotionKey={preset?.partnerPromotionKey}
                required={category !== "local"}
              />
              <EndpointField
                id="copilot-base-url"
                label={t("copilot.form.baseUrl")}
                value={baseUrl}
                onChange={(value) => setField(COPILOT_ENV.baseUrl, value)}
                placeholder="https://api.example.com/v1"
                help={t("copilot.form.baseUrlHint")}
                showManageButton={false}
              />
            </>
          )}
          <div className="space-y-2">
            <div className="flex items-center justify-between gap-3">
              <div className="flex items-center gap-0.5">
                <FormLabel
                  htmlFor="copilot-model"
                  className={isOfficial ? undefined : REQUIRED_LABEL}
                >
                  {t("copilot.form.model")}
                </FormLabel>
                <HelpTip title={t("copilot.form.model")}>
                  {isOfficial
                    ? t("copilot.form.officialModelHint")
                    : t("copilot.form.byokModelHint")}
                </HelpTip>
              </div>
              {!isOfficial && (
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={handleFetchModels}
                  disabled={isFetchingModels}
                  className="h-7 gap-1"
                >
                  {isFetchingModels ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  ) : (
                    <Download className="h-3.5 w-3.5" />
                  )}
                  {t("providerForm.fetchModels")}
                </Button>
              )}
            </div>
            <ModelInputWithFetch
              id="copilot-model"
              value={model}
              onChange={(value) => setField(COPILOT_ENV.model, value)}
              placeholder={isOfficial ? "claude-sonnet-4.6" : "gpt-5.4"}
              fetchedModels={
                fetchedModels.length > 0
                  ? fetchedModels
                  : isOfficial
                    ? presetModels(copilotOfficialPreset.models)
                    : []
              }
              isLoading={isFetchingModels}
            />
          </div>
          <div className="space-y-2">
            <div className="flex items-center gap-0.5">
              <Label htmlFor="copilot-reasoning-effort">
                {t("copilot.form.reasoningEffort")}
              </Label>
              <HelpTip title={t("copilot.form.reasoningEffort")}>
                {t("copilot.form.reasoningEffortHint")}
              </HelpTip>
            </div>
            <Select
              value={reasoningEffort || DEFAULT_EFFORT}
              onValueChange={(value) =>
                update(env, value === DEFAULT_EFFORT ? "" : value)
              }
            >
              <SelectTrigger id="copilot-reasoning-effort" className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={DEFAULT_EFFORT}>
                  {t("copilot.form.reasoningEffortDefault")}
                </SelectItem>
                {REASONING_EFFORTS.map((effort) => (
                  <SelectItem key={effort} value={effort}>
                    {effort}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          {!isOfficial && (
            <>
              <div className="grid grid-cols-2 gap-3">
                <div className="space-y-2">
                  <Label htmlFor="copilot-max-prompt">
                    {t("copilot.form.maxPromptTokens")}
                  </Label>
                  <Input
                    id="copilot-max-prompt"
                    type="number"
                    min={1}
                    step={1}
                    inputMode="numeric"
                    value={env[COPILOT_ENV.maxPromptTokens] ?? ""}
                    onChange={(e) =>
                      setField(COPILOT_ENV.maxPromptTokens, e.target.value)
                    }
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="copilot-max-output">
                    {t("copilot.form.maxOutputTokens")}
                  </Label>
                  <Input
                    id="copilot-max-output"
                    type="number"
                    min={1}
                    step={1}
                    inputMode="numeric"
                    value={env[COPILOT_ENV.maxOutputTokens] ?? ""}
                    onChange={(e) =>
                      setField(COPILOT_ENV.maxOutputTokens, e.target.value)
                    }
                  />
                </div>
              </div>
              <RequestHeadersEditor
                headers={headers}
                onHeadersChange={(next) => {
                  const text = formatCopilotHeaders(
                    normalizeRequestHeaders(next),
                  );
                  const nextEnv: CopilotEnv = { ...env };
                  if (text) nextEnv[COPILOT_ENV.headers] = text;
                  else delete nextEnv[COPILOT_ENV.headers];
                  update(nextEnv, reasoningEffort, next);
                }}
              />
            </>
          )}
        </fieldset>
        <div className="space-y-2">
          <div className="flex items-center gap-0.5">
            <Label htmlFor="copilot-settings-config">
              {t("provider.configJson")}
            </Label>
            <HelpTip title={t("provider.configJson")}>
              {t("copilot.form.envJsonHint")}
            </HelpTip>
          </div>
          <JsonEditor
            id="copilot-settings-config"
            ariaLabel={t("provider.configJson")}
            value={text}
            darkMode={darkMode}
            language="json"
            showValidation
            height={Math.max(1, text.split("\n").length) * 20 + 20}
            onChange={(next) => {
              setText(next);
              try {
                const parsed = JSON.parse(next) as unknown;
                const valid =
                  parsed !== null &&
                  typeof parsed === "object" &&
                  !Array.isArray(parsed);
                setJsonValid(valid);
                if (valid) {
                  const parsedEnv = copilotEnvOf(parsed);
                  setEnv(parsedEnv);
                  setHeaders(parseHeaders(parsedEnv[COPILOT_ENV.headers]));
                  setReasoningEffort(reasoningEffortOf(parsed));
                }
              } catch {
                setJsonValid(false);
              }
            }}
          />
        </div>
        {showButtons && (
          <div className="flex justify-end gap-2">
            <Button
              type="button"
              variant="outline"
              disabled={busy}
              onClick={onCancel}
            >
              {t("common.cancel")}
            </Button>
            <Button type="submit" disabled={busy || !ready}>
              {submitLabel}
            </Button>
          </div>
        )}
      </form>
    </Form>
  );
}
