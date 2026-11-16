import { useEffect, useState } from "react";
import { z } from "zod";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Form } from "@/components/ui/form";
import { ImeSafeInput } from "@/components/ui/ime-safe-input";
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
import { useProvidersQuery } from "@/lib/query/queries";
import type { ProviderFormData } from "@/lib/schemas/provider";
import {
  ATOMCODE_PROVIDER_TYPES,
  ATOMCODE_VENDOR_PROVIDER_LABELS,
  atomCodeCustomTemplate,
  atomcodeProviderPresets,
  normalizeAtomCodeProvider,
  type AtomCodeProviderPreset,
  type AtomCodeProviderType,
} from "@/config/atomcodeProviderPresets";
import { BasicFormFields } from "./BasicFormFields";
import ApiKeyInput from "./ApiKeyInput";
import { ProviderPresetSelector } from "./ProviderPresetSelector";
import type { ProviderFormProps } from "./ProviderForm";
import { FetchedModelPicker } from "./FetchedModelPicker";
import {
  fetchAtomCodeModels,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";

type AtomCodeConfig = AtomCodeProviderPreset["settingsConfig"];

const configSchema = z
  .object({
    account: z
      .object({
        provider: z.string(),
        base_url: z.string(),
        api_key: z.string().optional(),
      })
      .passthrough(),
    models: z.record(
      z.string(),
      z
        .object({
          model: z.string(),
          context_window: z.number().optional(),
        })
        .passthrough(),
    ),
  })
  .passthrough();

// 与后端 atomcode_config::validate_provider 同款：字母、数字、`-`、`_`、`.`
const KEY_PATTERN = /^[a-zA-Z0-9._-]+$/;
const normalizeKey = (value: string) => value.replace(/[^a-zA-Z0-9._-]/g, "");

// 表单里模型行的稳定 id：改键时行不重建、输入框焦点不丢（同 ClaudeStackModelsField）
let nextModelRowId = 0;
const createModelRowId = () => `atomcode-model-${(nextModelRowId += 1)}`;

export function AtomCodeProviderForm({
  providerId,
  initialData,
  onSubmit,
  onCancel,
  submitLabel,
  showButtons = true,
  onSubmittingChange,
  onSubmitReadyChange,
}: ProviderFormProps) {
  const { t } = useTranslation();
  const isDarkMode = useDarkMode();
  const isEdit = Boolean(initialData);
  const [config, setConfig] = useState<AtomCodeConfig>(() => {
    const raw = initialData?.settingsConfig as
      | Partial<AtomCodeConfig>
      | undefined;
    const template = atomCodeCustomTemplate();
    return {
      account: {
        // 早期版本写过 wire 名（claude / openai_responses / gemini），AtomCode 只认
        // 预设 id，加载时归一化，避免编辑旧账号时下拉空白、保存后又写回旧值
        provider: normalizeAtomCodeProvider(
          raw?.account?.provider ?? template.account.provider,
        ) as AtomCodeProviderType,
        base_url: raw?.account?.base_url ?? template.account.base_url,
        api_key: raw?.account?.api_key ?? template.account.api_key,
      },
      models: raw?.models && typeof raw.models === "object" ? raw.models : {},
    };
  });
  const [jsonText, setJsonText] = useState(() =>
    JSON.stringify(config, null, 2),
  );
  const [jsonValid, setJsonValid] = useState(true);
  const [presetId, setPresetId] = useState("custom");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  // 模型键 → 稳定行 id：改键时行不重建，输入框焦点不丢
  const [rowIds, setRowIds] = useState<Record<string, string>>(() =>
    Object.fromEntries(
      Object.keys(config.models).map((key) => [key, createModelRowId()]),
    ),
  );
  const [keyConflict, setKeyConflict] = useState(false);
  // 冲突所在行（旧键）：把红框绑到具体输入框，输入不静默回滚
  const [conflictKey, setConflictKey] = useState<string | undefined>(undefined);
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const syncRowIds = (models: AtomCodeConfig["models"]) =>
    setRowIds((prev) => {
      const next: Record<string, string> = {};
      let changed = Object.keys(prev).length !== Object.keys(models).length;
      for (const key of Object.keys(models)) {
        next[key] = prev[key] ?? createModelRowId();
        if (!prev[key]) changed = true;
      }
      return changed ? next : prev;
    });
  // 账号键即 config.toml 的 provider_accounts.<id>，供应商存在后固定
  const [providerKey, setProviderKey] = useState(providerId ?? "");
  const { data: existing } = useProvidersQuery("atomcode");
  const keyTaken =
    !isEdit &&
    Object.prototype.hasOwnProperty.call(
      existing?.providers ?? {},
      providerKey,
    );
  const keyInvalid =
    !isEdit && providerKey !== "" && !KEY_PATTERN.test(providerKey);
  const preset =
    presetId === "custom"
      ? undefined
      : atomcodeProviderPresets[Number(presetId)];
  const category = initialData?.category ?? preset?.category ?? "custom";

  const form = useForm<ProviderFormData>({
    defaultValues: {
      name: initialData?.name ?? "",
      notes: initialData?.notes ?? "",
      websiteUrl: initialData?.websiteUrl ?? "",
      icon: initialData?.icon ?? "",
      iconColor: initialData?.iconColor ?? "",
      settingsConfig: jsonText,
    },
  });
  const name = form.watch("name");
  const hasModels = Object.keys(config.models ?? {}).length > 0;
  const modelsValid = Object.entries(config.models ?? {}).every(
    ([key, model]) => key.trim() && model?.model?.trim(),
  );
  const ready = Boolean(
    jsonValid &&
      name.trim() &&
      (isEdit || (providerKey && !keyInvalid && !keyTaken)) &&
      hasModels &&
      modelsValid &&
      // 改键重名未解决时禁止保存：draft 从未提交，保存会写入与界面显示不符的旧键
      !keyConflict,
  );
  useEffect(() => {
    onSubmitReadyChange?.(ready);
  }, [ready, onSubmitReadyChange]);

  const update = (next: AtomCodeConfig, text?: string) => {
    setConfig(next);
    syncRowIds(next.models);
    setJsonText(text ?? JSON.stringify(next, null, 2));
    setJsonValid(true);
    // 只在模型键集合变化时清冲突：改其他字段（显示名 / 上下文 / base_url 等）
    // 不应抹掉「该模型键已存在」的红框与提示——draft 还停在冲突键上
    const prevKeys = Object.keys(config.models ?? {});
    const nextKeys = Object.keys(next.models ?? {});
    if (
      prevKeys.length !== nextKeys.length ||
      prevKeys.some((k) => !nextKeys.includes(k))
    ) {
      setKeyConflict(false);
      setConflictKey(undefined);
    }
  };
  const updateAccount = (next: Partial<AtomCodeConfig["account"]>) =>
    update({
      ...config,
      account: {
        ...(config.account ?? {}),
        ...next,
      } as AtomCodeConfig["account"],
    });

  const choosePreset = (id: string) => {
    setPresetId(id);
    const selected =
      id === "custom" ? undefined : atomcodeProviderPresets[Number(id)];
    const next: AtomCodeConfig = selected?.settingsConfig
      ? JSON.parse(JSON.stringify(selected.settingsConfig))
      : atomCodeCustomTemplate();
    update(next);
    setProviderKey(selected?.providerKey ?? "");
    form.reset({
      name: selected?.nameKey
        ? String(t(selected.nameKey))
        : (selected?.name ?? ""),
      notes: "",
      websiteUrl: selected?.websiteUrl ?? "",
      icon: selected?.icon ?? "",
      iconColor: selected?.iconColor ?? "",
      settingsConfig: JSON.stringify(next, null, 2),
    });
  };

  const addModel = () => {
    const models = { ...config.models };
    let index = 1;
    while (models[`model-${index}`]) index += 1;
    models[`model-${index}`] = { model: "" };
    update({ ...config, models });
  };
  const removeModel = (key: string) => {
    const models = { ...config.models };
    delete models[key];
    update({ ...config, models });
  };
  const updateModel = (
    oldKey: string,
    next: { key?: string; model?: string; contextWindow?: string },
  ) => {
    const renamedKey =
      next.key !== undefined && next.key !== oldKey ? next.key : undefined;
    if (
      renamedKey !== undefined &&
      Object.prototype.hasOwnProperty.call(config.models, renamedKey)
    ) {
      // 改键重名：提示且不覆盖既有键；保留该行输入框的当前 draft，不静默回滚
      setKeyConflict(true);
      setConflictKey(oldKey);
      return;
    }
    setKeyConflict(false);
    setConflictKey(undefined);
    if (renamedKey !== undefined) {
      // 改键时行 id 跟着键走，行不重建
      setRowIds((prev) => {
        const moved = prev[oldKey];
        if (!moved) return prev;
        const nextIds = { ...prev };
        delete nextIds[oldKey];
        nextIds[renamedKey] = moved;
        return nextIds;
      });
    }
    const models: AtomCodeConfig["models"] = {};
    for (const [key, model] of Object.entries(config.models)) {
      if (key !== oldKey) {
        models[key] = model;
        continue;
      }
      const entry: AtomCodeConfig["models"][string] = { ...model };
      if (renamedKey !== undefined) {
        // 改键：按新键重排（保持行序，交给后面统一插入）
        models[renamedKey] = entry;
        continue;
      }
      if (next.model !== undefined) entry.model = next.model;
      if (next.contextWindow !== undefined) {
        const size = Number(next.contextWindow);
        if (
          next.contextWindow.trim() === "" ||
          !Number.isFinite(size) ||
          size <= 0
        )
          delete entry.context_window;
        else entry.context_window = size;
      }
      models[key] = entry;
    }
    update({ ...config, models });
  };

  const handleFetchModels = async () => {
    const provider = config.account?.provider ?? "openai-compatible";
    const baseUrl = config.account?.base_url ?? "";
    const apiKey = config.account?.api_key ?? "";

    if (!apiKey.trim()) {
      showFetchModelsError("API key required", t, {
        hasApiKey: false,
        hasBaseUrl: true,
      });
      return;
    }

    setIsFetchingModels(true);
    try {
      const models = await fetchAtomCodeModels(provider, baseUrl, apiKey);
      setFetchedModels(models);
    } catch (err) {
      showFetchModelsError(err, t, {
        hasApiKey: Boolean(apiKey.trim()),
        hasBaseUrl: true,
      });
    } finally {
      setIsFetchingModels(false);
    }
  };

  const handleApplyModels = (selection: {
    add: string[];
    remove: string[];
  }) => {
    const models = { ...config.models };
    // 移除取消勾选的模型
    for (const modelId of selection.remove) {
      const key = Object.keys(models).find((k) => models[k]?.model === modelId);
      if (key) delete models[key];
    }
    // 添加新勾选的模型
    for (const modelId of selection.add) {
      // 生成唯一键；模型 id 全为非法字符时 sanitize 会产出空键，
      // 回退到 model-N 兜底，避免「点了没反应」
      const sanitized = modelId.replace(/[^a-zA-Z0-9._-]/g, "-");
      let key = sanitized || "model";
      let index = 1;
      while (models[key]) {
        key = `${sanitized || "model"}-${index}`;
        index += 1;
      }
      models[key] = { model: modelId };
    }
    update({ ...config, models });
  };

  // 空字符串也回退默认：Radix Select 不允许 SelectItem value=""
  const currentProvider = config.account?.provider || "openai-compatible";
  // 协议选项 + 当前值（厂商预设 id 不在协议列表里时补一条，label 用 registry 显示名）
  const providerTypeOptions: { value: string; label: string }[] =
    ATOMCODE_PROVIDER_TYPES.some((type) => type.value === currentProvider)
      ? ATOMCODE_PROVIDER_TYPES
      : [
          ...ATOMCODE_PROVIDER_TYPES,
          {
            value: currentProvider,
            label:
              ATOMCODE_VENDOR_PROVIDER_LABELS[currentProvider] ??
              currentProvider,
          },
        ];

  const presetEntries = atomcodeProviderPresets.map((entry, index) => ({
    id: String(index),
    preset: entry,
  }));
  const presetSelector = !initialData && (
    <ProviderPresetSelector
      selectedPresetId={presetId}
      presetEntries={presetEntries}
      presetCategoryLabels={{
        custom: t("providerPreset.custom"),
        cn_official: t("providerForm.categoryCnOfficial"),
        aggregator: t("providerForm.categoryAggregation"),
        third_party: t("providerForm.categoryThirdParty"),
      }}
      onPresetChange={choosePreset}
      category={category}
    />
  );

  return (
    <Form {...form}>
      <form
        id="provider-form"
        className="space-y-6"
        onSubmit={form.handleSubmit(async (identity) => {
          if (!ready || busy) return;
          setBusy(true);
          onSubmittingChange?.(true);
          setError("");
          try {
            await onSubmit({
              ...identity,
              name: identity.name.trim(),
              meta: initialData?.meta,
              providerKey: isEdit ? providerId : providerKey,
              presetCategory: category,
              settingsConfig: JSON.stringify(config),
            });
          } catch (submitError) {
            setError(String(submitError));
          } finally {
            setBusy(false);
            onSubmittingChange?.(false);
          }
        })}
      >
        {presetSelector}
        {error && (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        )}
        <fieldset
          disabled={busy || !jsonValid}
          className="min-w-0 space-y-6 border-0 p-0 disabled:opacity-50"
        >
          <BasicFormFields
            form={form}
            beforeNameSlot={
              <div className="space-y-2">
                <Label htmlFor="atomcode-provider-key">
                  {t("opencode.providerKey")}
                  <span aria-hidden="true" className="text-destructive ml-1">
                    *
                  </span>
                </Label>
                <ImeSafeInput
                  id="atomcode-provider-key"
                  value={providerKey}
                  onValueChange={setProviderKey}
                  normalize={normalizeKey}
                  disabled={isEdit}
                  placeholder={t("opencode.providerKeyPlaceholder")}
                  autoComplete="off"
                  className={keyTaken || keyInvalid ? "border-destructive" : ""}
                />
                <p
                  className={
                    keyTaken || keyInvalid
                      ? "text-xs text-destructive"
                      : "text-xs text-fg-2"
                  }
                >
                  {keyTaken
                    ? t("opencode.providerKeyDuplicate")
                    : keyInvalid
                      ? t("atomcode.form.providerKeyInvalid")
                      : isEdit
                        ? t("opencode.providerKeyLockedHint")
                        : t("atomcode.form.providerKeyHint")}
                </p>
              </div>
            }
          />
          <div className="grid gap-4 sm:grid-cols-2">
            <div className="space-y-2">
              <Label htmlFor="atomcode-provider-type">
                {t("atomcode.form.providerType")}
              </Label>
              <Select
                value={config.account?.provider || "openai-compatible"}
                onValueChange={(value) =>
                  updateAccount({ provider: value as AtomCodeProviderType })
                }
              >
                <SelectTrigger id="atomcode-provider-type">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {/* 厂商预设写的是 AtomCode 厂商 id（openrouter / moonshot …），
                      不在协议选项里，补一行当前值，编辑时下拉才有显示 */}
                  {providerTypeOptions.map((type) => (
                    <SelectItem key={type.value} value={type.value}>
                      {type.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-2">
              <Label htmlFor="atomcode-base-url">
                {t("atomcode.form.baseUrl")}
              </Label>
              <ImeSafeInput
                id="atomcode-base-url"
                value={config.account?.base_url ?? ""}
                onValueChange={(base_url) => updateAccount({ base_url })}
                placeholder="https://api.example.com/v1"
                autoComplete="off"
              />
              <p className="text-xs text-fg-2">
                {t("atomcode.form.baseUrlHint")}
              </p>
            </div>
            <div className="sm:col-span-2">
              <ApiKeyInput
                id="atomcode-api-key"
                label={t("atomcode.form.apiKey")}
                value={config.account?.api_key ?? ""}
                onChange={(api_key) => updateAccount({ api_key })}
                placeholder={t("atomcode.form.apiKeyPlaceholder")}
                hint={t("atomcode.form.apiKeyHint")}
              />
            </div>
          </div>
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <Label>
                {t("atomcode.form.models")}
                <span aria-hidden="true" className="text-destructive ml-1">
                  *
                </span>
              </Label>
              <div className="flex gap-2">
                <FetchedModelPicker
                  models={fetchedModels}
                  configuredModelIds={Object.values(config.models ?? {}).map(
                    (m) => m.model,
                  )}
                  onApply={handleApplyModels}
                  onFetch={handleFetchModels}
                  isFetching={isFetchingModels}
                />
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={addModel}
                >
                  <Plus className="mr-1 h-4 w-4" />
                  {t("atomcode.form.addModel")}
                </Button>
              </div>
            </div>
            {!hasModels && (
              <p className="text-xs text-fg-2">{t("atomcode.form.noModels")}</p>
            )}
            {keyConflict && (
              <p className="text-xs text-destructive">
                {t("atomcode.form.modelKeyDuplicate")}
              </p>
            )}
            {hasModels && (
              <div className="flex items-center gap-2" aria-hidden="true">
                <div className="w-44 shrink-0 text-xs text-fg-2">
                  {t("atomcode.form.modelId")}
                </div>
                <div className="min-w-0 flex-1 text-xs text-fg-2">
                  {t("atomcode.form.modelKey")}
                </div>
                <div className="w-36 shrink-0 text-xs text-fg-2">
                  {t("atomcode.form.contextWindow")}
                </div>
                <div className="size-10 shrink-0" />
              </div>
            )}
            <div className="space-y-2">
              {Object.entries(config.models ?? {}).map(([key, model]) => (
                <div
                  key={rowIds[key] ?? key}
                  className="flex items-start gap-2"
                >
                  <div className="w-44 shrink-0">
                    <ImeSafeInput
                      aria-label={t("atomcode.form.modelId")}
                      value={model?.model ?? ""}
                      onValueChange={(next) =>
                        updateModel(key, { model: next })
                      }
                      placeholder="deepseek-flash"
                      autoComplete="off"
                    />
                  </div>
                  <div className="min-w-0 flex-1">
                    <ImeSafeInput
                      aria-label={t("atomcode.form.modelKey")}
                      value={key}
                      onValueChange={(next) => updateModel(key, { key: next })}
                      normalize={normalizeKey}
                      keepDraftOnBlur
                      placeholder={t("atomcode.form.modelKeyPlaceholder")}
                      autoComplete="off"
                      className={
                        conflictKey === key ? "border-destructive" : ""
                      }
                    />
                  </div>
                  <div className="w-36 shrink-0">
                    <ImeSafeInput
                      aria-label={t("atomcode.form.contextWindow")}
                      value={model?.context_window?.toString() ?? ""}
                      onValueChange={(next) =>
                        updateModel(key, { contextWindow: next })
                      }
                      placeholder={t("atomcode.form.contextWindowPlaceholder")}
                      autoComplete="off"
                    />
                  </div>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    aria-label={t("atomcode.form.removeModel")}
                    onClick={() => removeModel(key)}
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </div>
              ))}
            </div>
          </div>
          <div className="space-y-2">
            <Label htmlFor="atomcode-settings-config">
              {t("provider.configJson")}
            </Label>
            <JsonEditor
              id="atomcode-settings-config"
              ariaLabel={t("provider.configJson")}
              value={jsonText}
              darkMode={isDarkMode}
              language="json"
              showValidation
              height={Math.max(1, jsonText.split("\n").length) * 20 + 20}
              onChange={(text) => {
                setJsonText(text);
                try {
                  const next = configSchema.parse(JSON.parse(text));
                  setConfig(next as unknown as AtomCodeConfig);
                  syncRowIds((next as unknown as AtomCodeConfig).models ?? {});
                  setJsonValid(true);
                  setKeyConflict(false); // JSON 路径解决冲突后同样要清掉提示
                  setConflictKey(undefined);
                } catch {
                  setJsonValid(false);
                }
              }}
            />
          </div>
        </fieldset>
        {showButtons && (
          <div className="flex justify-end gap-2">
            <Button
              type="button"
              variant="outline"
              onClick={onCancel}
              disabled={busy}
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
