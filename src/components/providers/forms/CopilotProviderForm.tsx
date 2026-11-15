import { useCallback, useEffect, useRef, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { ChevronRight, Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Form, FormLabel } from "@/components/ui/form";
import { HelpTip } from "@/components/ui/help-tip";
import { HoverTip } from "@/components/ui/hover-tip";
import { ImeSafeInput } from "@/components/ui/ime-safe-input";
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
import { useLatestRef } from "@/hooks/useLatestRef";
import { useModelMetadataFill } from "@/hooks/useModelMetadataFill";
import { toast } from "@/lib/toast";
import { cn } from "@/lib/utils";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import { resolveManagedAccountId } from "@/lib/authBinding";
import { useProvidersQuery } from "@/lib/query/queries";
import type { ProviderFormData } from "@/lib/schemas/provider";
import type { ProviderCategory, ProviderMeta } from "@/types";
import {
  COPILOT_API_FORMATS,
  COPILOT_OFFICIAL_PRESET_ID,
  COPILOT_REASONING_EFFORTS,
  applyCopilotApiFormat,
  copilotApiFormatOf,
  copilotCatalogOutputLimit,
  copilotByokConfigOf,
  copilotEnvOf,
  copilotOfficialPreset,
  copilotProviderPresets,
  emptyCopilotByokConfig,
  isCopilotOfficialCard,
  isValidCopilotProviderId,
  normalizeCopilotProviderId,
  reasoningEffortOf,
  type CopilotApiFormatId,
  type CopilotByokConfig,
  type CopilotModel,
  type CopilotProviderNode,
} from "@/config/copilotProviderPresets";
import { piPresetModelSources } from "@/config/presetModelMetadata";
import {
  findRequestHeaderValue,
  normalizeRequestHeaders,
} from "./helpers/requestHeaders";
import { BasicFormFields, REQUIRED_LABEL } from "./BasicFormFields";
import {
  ApiKeySection,
  EndpointField,
  ModelDropdown,
  ModelInputWithFetch,
  NativeConfigEditor,
} from "./shared";
import {
  FetchedModelPicker,
  type FetchedModelSelection,
} from "./FetchedModelPicker";
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

const DEFAULT_EFFORT = "default";

const presetModels = (models?: string[]): FetchedModel[] =>
  (models ?? []).map((id) => ({ id, ownedBy: null }));

type Connection = Omit<CopilotByokConfig, "reasoningEffort">;

/** 数值上限：没填是 undefined，填了必须是正整数。 */
const NUMBER_FIELDS = [
  "maxContextWindowTokens",
  "maxPromptTokens",
  "maxOutputTokens",
] as const;
type NumberField = (typeof NUMBER_FIELDS)[number];
const NUMBER_FIELD_LABELS: Record<NumberField, string> = {
  maxContextWindowTokens: "copilot.form.maxContextWindowTokens",
  maxPromptTokens: "copilot.form.maxPromptTokens",
  maxOutputTokens: "copilot.form.maxOutputTokens",
};
/** Copilot 接受的 token 上限：0 到 u32 上限的整数（和后端校验一致）。 */
const MAX_TOKEN_LIMIT = 4294967295;
const validNumber = (value: unknown) =>
  value === undefined ||
  (typeof value === "number" &&
    Number.isInteger(value) &&
    value >= 0 &&
    value <= MAX_TOKEN_LIMIT);

/** 卡片配置：没设推理强度时不写这个键。 */
function withEffort<T extends object>(config: T, reasoningEffort: string) {
  return reasoningEffort ? { ...config, reasoningEffort } : config;
}

/** 保存前收拾：去掉空字符串和空对象，请求头去掉草稿行。 */
function compactConnection(config: Connection): Connection {
  const provider: CopilotProviderNode = {
    ...config.provider,
    baseUrl: config.provider.baseUrl.trim(),
  };
  for (const key of ["apiKey", "bearerToken"] as const) {
    const value = provider[key];
    if (typeof value === "string" && value.trim()) provider[key] = value.trim();
    else delete provider[key];
  }
  const headers = normalizeRequestHeaders(provider.headers ?? {});
  if (Object.keys(headers).length) provider.headers = headers;
  else delete provider.headers;
  if (provider.azure) {
    const azure = { ...provider.azure };
    if (typeof azure.apiVersion === "string" && azure.apiVersion.trim()) {
      azure.apiVersion = azure.apiVersion.trim();
    } else {
      delete azure.apiVersion;
    }
    if (Object.keys(azure).length) provider.azure = azure;
    else delete provider.azure;
  }
  const models = config.models.map((model) => {
    const next: CopilotModel = { ...model, id: model.id.trim() };
    for (const key of ["name", "wireModel"] as const) {
      const value = next[key];
      if (typeof value === "string" && value.trim()) next[key] = value.trim();
      else delete next[key];
    }
    for (const key of NUMBER_FIELDS) {
      if (next[key] === undefined) delete next[key];
    }
    return next;
  });
  return { provider, models };
}

/**
 * 编辑框里显示这一家在 providers.json 里的样子：供应商条目补上 `name`，模型条目补上 `provider`。
 */
function toNativeText(config: Connection, id: string): string {
  return JSON.stringify(
    {
      providers: [{ name: id, ...config.provider }],
      models: config.models.map(({ id: modelId, ...rest }) => ({
        id: modelId,
        provider: id,
        ...rest,
      })),
    },
    null,
    2,
  );
}

type NativeError =
  | { kind: "syntax"; detail: string }
  | { kind: "scope" | "schema" };
const isObject = (value: unknown): value is Record<string, unknown> =>
  Boolean(value) && typeof value === "object" && !Array.isArray(value);

/** 编辑框改回配置：只认这一家（name / provider 都是卡片 id）的一个供应商和它的模型。 */
const PROVIDER_STRING_KEYS = [
  "type",
  "wireApi",
  "transport",
  "baseUrl",
  "apiKey",
  "bearerToken",
  "modelProvider",
] as const;
const MODEL_STRING_KEYS = ["id", "name", "wireModel", "modelId"] as const;
const MODEL_NUMBER_KEYS = [
  "maxPromptTokens",
  "maxContextWindowTokens",
  "maxOutputTokens",
] as const;

const isOptionalString = (value: unknown) =>
  value === undefined || typeof value === "string";

/** 表单会直接读的字段类型对不上（比如 apiKey 写成数字）就当 schema 错误，不进状态。 */
function nativeProviderTypesOk(entry: Record<string, unknown>): boolean {
  if (!PROVIDER_STRING_KEYS.every((key) => isOptionalString(entry[key])))
    return false;
  const { headers, azure } = entry;
  if (
    headers !== undefined &&
    !(
      isObject(headers) &&
      Object.values(headers).every((value) => typeof value === "string")
    )
  )
    return false;
  if (
    azure !== undefined &&
    !(isObject(azure) && isOptionalString(azure.apiVersion))
  )
    return false;
  return true;
}

function nativeModelTypesOk(model: Record<string, unknown>): boolean {
  return (
    MODEL_STRING_KEYS.every((key) => isOptionalString(model[key])) &&
    MODEL_NUMBER_KEYS.every(
      (key) => model[key] === undefined || typeof model[key] === "number",
    )
  );
}

function fromNativeText(
  text: string,
  id: string,
): { ok: true; config: Connection } | { ok: false; error: NativeError } {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch (error) {
    return {
      ok: false,
      error: { kind: "syntax", detail: String((error as Error).message) },
    };
  }
  if (!isObject(parsed)) return { ok: false, error: { kind: "schema" } };
  if (
    Object.keys(parsed).some((key) => key !== "providers" && key !== "models")
  )
    return { ok: false, error: { kind: "scope" } };
  const { providers, models = [] } = parsed;
  if (!Array.isArray(providers) || !Array.isArray(models))
    return { ok: false, error: { kind: "schema" } };
  if (providers.length !== 1) return { ok: false, error: { kind: "scope" } };
  const entry = providers[0];
  if (
    !isObject(entry) ||
    typeof entry.baseUrl !== "string" ||
    !nativeProviderTypesOk(entry)
  )
    return { ok: false, error: { kind: "schema" } };
  if (entry.name !== id) return { ok: false, error: { kind: "scope" } };
  const { name: _name, ...provider } = entry;
  const result: CopilotModel[] = [];
  for (const model of models) {
    if (
      !isObject(model) ||
      typeof model.id !== "string" ||
      !nativeModelTypesOk(model)
    )
      return { ok: false, error: { kind: "schema" } };
    if (model.provider !== undefined && model.provider !== id)
      return { ok: false, error: { kind: "scope" } };
    const { provider: _provider, ...rest } = model;
    result.push(rest as CopilotModel);
  }
  return {
    ok: true,
    config: {
      provider: provider as CopilotProviderNode,
      models: result,
    },
  };
}

export function CopilotProviderForm({
  providerId,
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
  const isEdit = Boolean(initialData);
  const initialConfig = initialData?.settingsConfig;
  const [presetId, setPresetId] = useState(
    isEdit ? "custom" : COPILOT_OFFICIAL_PRESET_ID,
  );
  const [category, setCategory] = useState<ProviderCategory>(
    initialData?.category ?? (isEdit ? "custom" : "official"),
  );
  // 编辑时按配置认（没有 provider 节点的旧卡也是官方卡），新建时按选的预设
  const isOfficial = isEdit
    ? isCopilotOfficialCard({
        category: initialData?.category,
        settingsConfig: initialConfig,
      })
    : category === "official";
  const [officialModel, setOfficialModel] = useState(
    () => copilotEnvOf(initialConfig).COPILOT_MODEL ?? "",
  );
  const [reasoningEffort, setReasoningEffort] = useState(() =>
    reasoningEffortOf(initialConfig),
  );
  const [connection, setConnection] = useState<Connection>(() => {
    const { reasoningEffort: _effort, ...rest } = isEdit
      ? copilotByokConfigOf(initialConfig)
      : emptyCopilotByokConfig();
    return rest;
  });
  const connectionRef = useLatestRef(connection);
  // 请求头编辑器的行放在组件状态里：新行的草稿键（draft-header:<时间戳>）只在编辑器里，
  // 配置里只放规整过的请求头
  const [headers, setHeaders] = useState<Record<string, string>>(
    () => connection.provider.headers ?? {},
  );
  // 模型行的稳定 key：删掉中间一行时展开状态不串到下一行
  const rowSeq = useRef(0);
  const newRowKey = () => `row-${++rowSeq.current}`;
  const [rowKeys, setRowKeys] = useState<string[]>(() =>
    connection.models.map(() => newRowKey()),
  );
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());
  const [cardId, setCardId] = useState(providerId ?? "");
  const textId = (isEdit ? providerId : cardId) || "custom";
  const [text, setText] = useState(() => toNativeText(connection, textId));
  const [textError, setTextError] = useState<NativeError | null>(null);
  const [accountId, setAccountId] = useState<string | null>(() =>
    resolveManagedAccountId(initialData?.meta, "github_copilot"),
  );
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const [busy, setBusy] = useState(false);
  const { data: existing } = useProvidersQuery("copilot");
  const preset =
    presetId === "custom" || presetId === COPILOT_OFFICIAL_PRESET_ID
      ? undefined
      : copilotProviderPresets[Number(presetId)];

  const form = useForm<ProviderFormData>({
    defaultValues: {
      name: initialData?.name ?? (isEdit ? "" : copilotOfficialPreset.name),
      notes: initialData?.notes ?? "",
      websiteUrl:
        initialData?.websiteUrl ??
        (isEdit ? "" : copilotOfficialPreset.websiteUrl),
      icon: initialData?.icon ?? (isEdit ? "" : "githubcopilot"),
      iconColor: initialData?.iconColor ?? "",
      settingsConfig: "",
    },
  });
  const name = form.watch("name");

  const updateConnection = (
    next: Connection,
    keys?: string[],
    id: string = textId,
  ) => {
    connectionRef.current = next;
    setConnection(next);
    if (keys) setRowKeys(keys);
    setText(toNativeText(next, id));
    setTextError(null);
  };
  const updateProvider = (patch: Partial<CopilotProviderNode>) =>
    updateConnection({
      ...connectionRef.current,
      provider: { ...connectionRef.current.provider, ...patch },
    });
  const updateModel = (index: number, patch: Partial<CopilotModel>) => {
    const current = connectionRef.current;
    updateConnection({
      ...current,
      models: current.models.map((model, i) =>
        i === index ? { ...model, ...patch } : model,
      ),
    });
  };
  const resetConnection = (next: Connection) => {
    setHeaders(next.provider.headers ?? {});
    setExpanded(new Set());
    return next.models.map(() => newRowKey());
  };

  const { provider, models } = connection;
  const baseUrl = provider.baseUrl ?? "";
  const apiKey = provider.apiKey ?? "";
  const credential = apiKey.trim() || provider.bearerToken?.trim() || "";
  const providerType = provider.type || "openai";
  const apiFormat = copilotApiFormatOf(provider);
  const duplicate =
    !isEdit &&
    Object.prototype.hasOwnProperty.call(existing?.providers ?? {}, cardId);
  const idValid = isEdit || isValidCopilotProviderId(cardId);
  const modelIds = models.map((model) => model.id.trim());
  const duplicateModelIds = new Set(
    modelIds.filter((id, index) => id && modelIds.indexOf(id) !== index),
  );
  const modelsValid =
    models.length > 0 &&
    modelIds.every(Boolean) &&
    duplicateModelIds.size === 0 &&
    models.every((model) =>
      NUMBER_FIELDS.every((key) => validNumber(model[key])),
    );
  const ready = Boolean(
    name.trim() &&
      (isOfficial ||
        (!textError &&
          idValid &&
          !duplicate &&
          /^https?:\/\//.test(baseUrl.trim()) &&
          modelsValid)),
  );
  useEffect(() => {
    onSubmitReadyChange?.(ready);
  }, [ready, onSubmitReadyChange]);

  const headersKey = JSON.stringify(provider.headers ?? {});
  const fetchGeneration = useRef(0);
  useEffect(() => {
    // 地址、Key、协议或请求头变了，旧的模型列表不再适用（官方卡用预设列表，不清）
    if (isOfficial) return;
    setFetchedModels((prev) => (prev.length === 0 ? prev : []));
    setIsFetchingModels(false);
    return () => {
      fetchGeneration.current += 1;
    };
  }, [isOfficial, providerType, baseUrl, credential, headersKey]);

  const handleFetchModels = useCallback(() => {
    const url = baseUrl.trim();
    const requestHeaders = normalizeRequestHeaders(
      JSON.parse(headersKey) as Record<string, string>,
    );
    if (!url || !credential) {
      showFetchModelsError(null, t, {
        hasApiKey: Boolean(credential),
        hasBaseUrl: Boolean(url),
      });
      return;
    }
    const generation = ++fetchGeneration.current;
    setIsFetchingModels(true);
    fetchModelsForConfig(
      url,
      credential,
      false,
      undefined,
      findRequestHeaderValue(requestHeaders, "user-agent"),
      {
        // 与 Pi 共用取模型命令：Anthropic 用 x-api-key，其余用 Bearer。
        apiFormat:
          providerType === "anthropic" ? "anthropic-messages" : undefined,
        requestHeaders,
      },
    )
      .then((result) => {
        if (generation !== fetchGeneration.current) return;
        const unique = [
          ...new Map(result.map((model) => [model.id, model])).values(),
        ];
        setFetchedModels(unique);
        if (unique.length === 0) toast.info(t("providerForm.fetchModelsEmpty"));
      })
      .catch((err) => {
        if (generation !== fetchGeneration.current) return;
        console.warn("[ModelFetch] Failed:", err);
        showFetchModelsError(err, t);
      })
      .finally(() => {
        if (generation === fetchGeneration.current) setIsFetchingModels(false);
      });
  }, [baseUrl, credential, headersKey, providerType, t]);

  // 选中的模型按 Pi 预设和 models.dev 补上下文 / 输出上限（只补没填的）
  const fillModelMetadata = useModelMetadataFill({
    baseUrl,
    presets: piPresetModelSources,
    prefetch: fetchedModels.length > 0,
  });
  const fillModelMetadataFor = (modelId: string, silent?: boolean) =>
    fillModelMetadata(
      modelId,
      (metadata) => {
        const current = connectionRef.current;
        const index = current.models.findIndex((m) => m.id === modelId);
        if (index < 0) return false;
        const model = current.models[index];
        const patch: Partial<CopilotModel> = {};
        if (
          model.maxContextWindowTokens === undefined &&
          metadata.contextWindow
        )
          patch.maxContextWindowTokens = metadata.contextWindow;
        const outputLimit = copilotCatalogOutputLimit(
          metadata.maxOutputTokens,
          model.maxContextWindowTokens ?? metadata.contextWindow,
        );
        if (model.maxOutputTokens === undefined && outputLimit !== undefined)
          patch.maxOutputTokens = outputLimit;
        if (Object.keys(patch).length === 0) return false;
        updateModel(index, patch);
        return true;
      },
      { silent },
    );

  const handleApplyFetchedModels = ({ add, remove }: FetchedModelSelection) => {
    const removed = new Set(remove);
    const current = connectionRef.current;
    const kept = current.models
      .map((model, index) => ({ model, key: rowKeys[index] }))
      .filter(({ model }) => !removed.has(model.id));
    const configured = new Set(kept.map(({ model }) => model.id));
    const added = add.filter((id) => !configured.has(id));
    const next = [
      ...kept,
      ...added.map((id) => ({
        model: { id } as CopilotModel,
        key: newRowKey(),
      })),
    ];
    updateConnection(
      { ...current, models: next.map(({ model }) => model) },
      next.map(({ key }) => key),
    );
    // 弹窗里已经显示过这些参数，补完不再提示。
    for (const id of added) fillModelMetadataFor(id, true);
  };

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
        setOfficialModel("");
        setReasoningEffort("");
        const nextConnection: Connection =
          next && "provider" in next.settingsConfig
            ? structuredClone({
                provider: next.settingsConfig.provider,
                models: next.settingsConfig.models,
              })
            : emptyCopilotByokConfig();
        const nextId =
          next && id !== COPILOT_OFFICIAL_PRESET_ID ? next.providerKey : "";
        setCardId(nextId);
        updateConnection(
          nextConnection,
          resetConnection(nextConnection),
          nextId || "custom",
        );
        setFetchedModels([]);
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
      const model = officialModel.trim();
      const settingsConfig = isOfficial
        ? // 官方卡只用 GitHub 登录：不留任何自带模型字段
          withEffort(
            { env: model ? { COPILOT_MODEL: model } : {} },
            reasoningEffort,
          )
        : withEffort(compactConnection(connection), reasoningEffort);
      await onSubmit({
        ...identity,
        name: identity.name.trim(),
        meta,
        providerKey: isOfficial ? undefined : isEdit ? providerId : cardId,
        presetId: presetId === "custom" ? undefined : presetId,
        presetCategory: category,
        settingsConfig: JSON.stringify(settingsConfig),
      });
    } finally {
      setBusy(false);
      onSubmittingChange?.(false);
    }
  });

  const toggleExpanded = (key: string) =>
    setExpanded((current) => {
      const next = new Set(current);
      if (!next.delete(key)) next.add(key);
      return next;
    });

  const reasoningEffortField = (
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
          setReasoningEffort(value === DEFAULT_EFFORT ? "" : value)
        }
      >
        <SelectTrigger id="copilot-reasoning-effort" className="w-full">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={DEFAULT_EFFORT}>
            {t("copilot.form.reasoningEffortDefault")}
          </SelectItem>
          {COPILOT_REASONING_EFFORTS.map((effort) => (
            <SelectItem key={effort} value={effort}>
              {effort}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );

  const providerIdField = (
    <div className="space-y-2">
      <Label htmlFor="copilot-provider-id" className={REQUIRED_LABEL}>
        {t("opencode.providerKey")}
      </Label>
      <ImeSafeInput
        id="copilot-provider-id"
        value={isEdit ? (providerId ?? "") : cardId}
        disabled={isEdit}
        normalize={normalizeCopilotProviderId}
        onValueChange={(next) => {
          setCardId(next);
          setText(toNativeText(connectionRef.current, next || "custom"));
          setTextError(null);
        }}
        placeholder="my-provider"
      />
      <p
        className={duplicate ? "text-xs text-destructive" : "text-xs text-fg-2"}
      >
        {duplicate
          ? t("opencode.providerKeyDuplicate")
          : isEdit
            ? t("opencode.providerKeyLockedHint")
            : t("copilot.form.providerIdHint", { id: cardId || "my-provider" })}
      </p>
    </div>
  );

  const modelList = (
    <div className="space-y-3 border-l border-border pl-3">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-1">
          <FormLabel className={REQUIRED_LABEL}>
            {t("opencode.models")}
          </FormLabel>
          <HelpTip title={t("opencode.models")}>
            {t("copilot.form.byokModelsHint", { id: textId })}
          </HelpTip>
        </div>
        <div className="flex gap-1">
          <FetchedModelPicker
            models={fetchedModels}
            configuredModelIds={modelIds.filter(Boolean)}
            onApply={handleApplyFetchedModels}
            onFetch={handleFetchModels}
            isFetching={isFetchingModels}
            metadataSource={{ baseUrl, presets: piPresetModelSources }}
          />
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-7 gap-1"
            onClick={() =>
              updateConnection(
                {
                  ...connectionRef.current,
                  models: [...connectionRef.current.models, { id: "" }],
                },
                [...rowKeys, newRowKey()],
              )
            }
          >
            <Plus className="h-3.5 w-3.5" />
            {t("opencode.addModel")}
          </Button>
        </div>
      </div>
      {models.length === 0 ? (
        <p className="py-2 text-sm text-fg-2">{t("opencode.noModels")}</p>
      ) : (
        <div className="space-y-2">
          <div className="mb-1 flex items-center gap-2 px-1 text-xs text-fg-2">
            <span className="w-9" />
            <span className={cn("flex-1", REQUIRED_LABEL)}>
              {t("opencode.modelId")}
            </span>
            <span className="flex-1">{t("opencode.modelName")}</span>
            <span className="w-9" />
          </div>
          {models.map((model, index) => {
            const key = rowKeys[index] ?? `row-fallback-${index}`;
            const isOpen = expanded.has(key);
            return (
              <div key={key} className="space-y-2">
                <div className="flex items-center gap-2">
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    onClick={() => toggleExpanded(key)}
                    aria-label={t("opencode.toggleModelDetails")}
                    aria-expanded={isOpen}
                    className="h-9 w-9 shrink-0"
                  >
                    <ChevronRight
                      className={cn(
                        "h-4 w-4 transition-transform motion-reduce:transition-none",
                        isOpen && "rotate-90",
                      )}
                    />
                  </Button>
                  <div className="flex min-w-0 flex-1 gap-1">
                    <Input
                      id={`copilot-model-${key}`}
                      value={model.id}
                      onChange={(e) =>
                        updateModel(index, { id: e.target.value })
                      }
                      placeholder="model-id"
                      aria-label={t("opencode.modelId")}
                      aria-invalid={duplicateModelIds.has(model.id.trim())}
                      className="min-w-0 flex-1"
                    />
                    {fetchedModels.length > 0 && (
                      <ModelDropdown
                        models={fetchedModels}
                        onSelect={(id) => {
                          updateModel(index, { id });
                          fillModelMetadataFor(id);
                        }}
                      />
                    )}
                  </div>
                  <Input
                    id={`copilot-model-name-${key}`}
                    value={typeof model.name === "string" ? model.name : ""}
                    onChange={(e) =>
                      updateModel(index, { name: e.target.value || undefined })
                    }
                    placeholder={t("opencode.modelName")}
                    aria-label={t("opencode.modelName")}
                    className="min-w-0 flex-1"
                  />
                  <HoverTip content={t("common.delete")}>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      aria-label={t("common.delete")}
                      className="h-9 w-9 shrink-0 text-fg-2 hover:text-destructive"
                      onClick={() =>
                        updateConnection(
                          {
                            ...connectionRef.current,
                            models: connectionRef.current.models.filter(
                              (_, i) => i !== index,
                            ),
                          },
                          rowKeys.filter((_, i) => i !== index),
                        )
                      }
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </HoverTip>
                </div>
                {isOpen && (
                  <div className="ml-9 grid gap-3 border-l-2 border-muted pl-4 sm:grid-cols-2">
                    <div className="space-y-2 sm:col-span-2">
                      <div className="flex items-center gap-0.5">
                        <Label htmlFor={`copilot-wire-model-${key}`}>
                          {t("copilot.form.wireModel")}
                        </Label>
                        <HelpTip title={t("copilot.form.wireModel")}>
                          {t("copilot.form.wireModelHint")}
                        </HelpTip>
                      </div>
                      <Input
                        id={`copilot-wire-model-${key}`}
                        value={
                          typeof model.wireModel === "string"
                            ? model.wireModel
                            : ""
                        }
                        onChange={(e) =>
                          updateModel(index, {
                            wireModel: e.target.value || undefined,
                          })
                        }
                        placeholder={model.id || "model-id"}
                      />
                    </div>
                    {NUMBER_FIELDS.map((field) => (
                      <div key={field} className="space-y-2">
                        <Label htmlFor={`copilot-${field}-${key}`}>
                          {t(NUMBER_FIELD_LABELS[field])}
                        </Label>
                        <Input
                          id={`copilot-${field}-${key}`}
                          type="number"
                          min={0}
                          max={MAX_TOKEN_LIMIT}
                          step={1}
                          inputMode="numeric"
                          value={model[field] ?? ""}
                          aria-invalid={!validNumber(model[field])}
                          onChange={(e) =>
                            updateModel(index, {
                              [field]:
                                e.target.value === ""
                                  ? undefined
                                  : Number(e.target.value),
                            })
                          }
                        />
                      </div>
                    ))}
                  </div>
                )}
              </div>
            );
          })}
          {duplicateModelIds.size > 0 && (
            <p role="alert" className="text-xs text-destructive">
              {t("copilot.form.duplicateModelId", {
                model: [...duplicateModelIds].join(", "),
              })}
            </p>
          )}
        </div>
      )}
    </div>
  );

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
          disabled={busy || (!isOfficial && Boolean(textError))}
          className="min-w-0 space-y-6 border-0 p-0 disabled:opacity-50"
        >
          <BasicFormFields
            form={form}
            beforeNameSlot={isOfficial ? undefined : providerIdField}
          />
          {isOfficial ? (
            <>
              <CopilotAuthSection
                mode="select"
                selectedAccountId={accountId}
                onAccountSelect={setAccountId}
                noneOptionLabel={t("copilot.form.followCliLogin")}
                onManageAccounts={
                  onManageAuthAccounts
                    ? () => onManageAuthAccounts("github_copilot")
                    : undefined
                }
              />
              <div className="space-y-2">
                <div className="flex items-center gap-0.5">
                  <FormLabel htmlFor="copilot-model">
                    {t("copilot.form.model")}
                  </FormLabel>
                  <HelpTip title={t("copilot.form.model")}>
                    {t("copilot.form.officialModelHint")}
                  </HelpTip>
                </div>
                <ModelInputWithFetch
                  id="copilot-model"
                  value={officialModel}
                  onChange={setOfficialModel}
                  placeholder="claude-sonnet-5.5"
                  fetchedModels={presetModels(copilotOfficialPreset.models)}
                  isLoading={false}
                />
              </div>
              {reasoningEffortField}
            </>
          ) : (
            <>
              <div className="space-y-2">
                <Label htmlFor="copilot-api-format">
                  {t("copilot.form.apiFormat")}
                </Label>
                <Select
                  value={apiFormat}
                  onValueChange={(value) =>
                    updateConnection({
                      ...connectionRef.current,
                      provider: applyCopilotApiFormat(
                        connectionRef.current.provider,
                        value as CopilotApiFormatId,
                      ),
                    })
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
              {providerType === "azure" && (
                <div className="space-y-2">
                  <Label htmlFor="copilot-azure-api-version">
                    {t("copilot.form.azureApiVersion")}
                  </Label>
                  <Input
                    id="copilot-azure-api-version"
                    value={
                      typeof provider.azure?.apiVersion === "string"
                        ? provider.azure.apiVersion
                        : ""
                    }
                    onChange={(e) =>
                      updateProvider({
                        azure: {
                          ...(provider.azure ?? {}),
                          apiVersion: e.target.value,
                        },
                      })
                    }
                    placeholder="2025-04-01-preview"
                  />
                </div>
              )}
              <ApiKeySection
                id="copilot-api-key"
                value={apiKey}
                onChange={(value) => updateProvider({ apiKey: value })}
                category={category}
                shouldShowLink={Boolean(preset?.apiKeyUrl)}
                websiteUrl={preset?.apiKeyUrl ?? ""}
                isPartner={preset?.isPartner}
                partnerPromotionKey={preset?.partnerPromotionKey}
                required={category !== "local" && !provider.bearerToken}
              />
              <EndpointField
                id="copilot-base-url"
                label={t("copilot.form.baseUrl")}
                value={baseUrl}
                onChange={(value) => updateProvider({ baseUrl: value })}
                placeholder="https://api.example.com/v1"
                help={t("copilot.form.baseUrlHint")}
                showManageButton={false}
              />
              <RequestHeadersEditor
                headers={headers}
                onHeadersChange={(next) => {
                  setHeaders(next);
                  const normalized = normalizeRequestHeaders(next);
                  const nextProvider = { ...connectionRef.current.provider };
                  if (Object.keys(normalized).length)
                    nextProvider.headers = normalized;
                  else delete nextProvider.headers;
                  updateConnection({
                    ...connectionRef.current,
                    provider: nextProvider,
                  });
                }}
              />
              {modelList}
              {reasoningEffortField}
            </>
          )}
        </fieldset>
        {!isOfficial && (
          <NativeConfigEditor
            appId="copilot"
            file="providers.json"
            id="copilot-settings-config"
            value={text}
            onChange={(next) => {
              setText(next);
              const parsed = fromNativeText(next, textId);
              setTextError(parsed.ok ? null : parsed.error);
              if (parsed.ok) {
                connectionRef.current = parsed.config;
                setConnection(parsed.config);
                if (parsed.config.models.length !== rowKeys.length) {
                  setRowKeys(parsed.config.models.map(() => newRowKey()));
                }
                setHeaders(parsed.config.provider.headers ?? {});
              }
            }}
            error={
              !textError
                ? null
                : textError.kind === "syntax"
                  ? t("providerForm.nativeConfig.invalid", {
                      format: "JSON",
                      error: textError.detail,
                    })
                  : textError.kind === "scope"
                    ? t("providerForm.nativeConfig.scope", { section: textId })
                    : t("providerForm.nativeConfig.schema")
            }
          />
        )}
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
