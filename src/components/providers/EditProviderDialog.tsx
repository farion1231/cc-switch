import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Save } from "lucide-react";
import { Button } from "@/components/ui/button";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import type { Provider } from "@/types";
import {
  ProviderForm,
  type ProviderFormValues,
} from "@/components/providers/forms/ProviderForm";
import { openclawApi, providersApi, vscodeApi, type AppId } from "@/lib/api";
import { extractCodexBaseUrl } from "@/utils/providerConfigUtils";

/** 深度合并两个对象，source 覆盖 target 的同名字段 */
function deepMerge(
  target: Record<string, unknown>,
  source: Record<string, unknown>,
): Record<string, unknown> {
  const result = { ...target };
  for (const key of Object.keys(source)) {
    const sv = source[key];
    const tv = result[key];
    if (
      sv &&
      typeof sv === "object" &&
      !Array.isArray(sv) &&
      tv &&
      typeof tv === "object" &&
      !Array.isArray(tv)
    ) {
      result[key] = deepMerge(
        tv as Record<string, unknown>,
        sv as Record<string, unknown>,
      );
    } else {
      result[key] = sv;
    }
  }
  return result;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function nonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.trim().length > 0;
}

function preserveClaudeCredentialsFromDb(
  merged: Record<string, unknown>,
  dbSettings: Record<string, unknown>,
): Record<string, unknown> {
  const dbEnv = asRecord(dbSettings.env);
  if (!dbEnv) return merged;

  const protectedEnvKeys = [
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_API_KEY",
    "OPENROUTER_API_KEY",
    "OPENAI_API_KEY",
    "ANTHROPIC_BASE_URL",
  ];
  const preservedEntries = protectedEnvKeys
    .filter((key) => nonEmptyString(dbEnv[key]))
    .map((key) => [key, dbEnv[key]] as const);
  if (preservedEntries.length === 0) return merged;

  return {
    ...merged,
    env: {
      ...(asRecord(merged.env) ?? {}),
      ...Object.fromEntries(preservedEntries),
    },
  };
}

function preserveCodexCredentialsFromDb(
  merged: Record<string, unknown>,
  dbSettings: Record<string, unknown>,
): Record<string, unknown> {
  let result = merged;

  const dbAuth = asRecord(dbSettings.auth);
  const dbApiKey = dbAuth?.OPENAI_API_KEY;
  if (nonEmptyString(dbApiKey)) {
    result = {
      ...result,
      // Third-party provider auth belongs to the provider DB row. Do not merge
      // live OAuth tokens into the stored provider object; the backend preserves
      // official OAuth separately in live auth.json/backup.
      auth: dbAuth,
    };
  }

  const dbConfig = dbSettings.config;
  const liveConfig = result.config;
  const dbBaseUrl = nonEmptyString(dbConfig)
    ? extractCodexBaseUrl(dbConfig)
    : undefined;
  const liveBaseUrl = nonEmptyString(liveConfig)
    ? extractCodexBaseUrl(liveConfig)
    : undefined;

  // Codex 的 config.toml 是字符串，不能像 JSON 一样深度合并。若 DB 里已有
  // provider-specific base_url，则它是该 provider 的 SSOT；避免 live 里缺失/
  // 残留其它 provider 的 base_url 时覆盖表单里的请求地址与 token 投影。
  if (nonEmptyString(dbConfig) && dbBaseUrl && dbBaseUrl !== liveBaseUrl) {
    result = {
      ...result,
      config: dbConfig,
    };
  }

  return result;
}

function mergeSettingsForEdit(
  appId: AppId,
  dbSettings: Record<string, unknown>,
  liveSettingsObj: Record<string, unknown> | null,
): Record<string, unknown> {
  const merged = liveSettingsObj
    ? deepMerge(dbSettings, liveSettingsObj)
    : dbSettings;

  if (appId === "claude") {
    return preserveClaudeCredentialsFromDb(merged, dbSettings);
  }
  if (appId === "codex") {
    return preserveCodexCredentialsFromDb(merged, dbSettings);
  }
  return merged;
}

interface EditProviderDialogProps {
  open: boolean;
  provider: Provider | null;
  onOpenChange: (open: boolean) => void;
  onSubmit: (payload: {
    provider: Provider;
    originalId?: string;
  }) => Promise<void> | void;
  appId: AppId;
  isProxyTakeover?: boolean; // 代理接管模式下不读取 live（避免显示被接管后的代理配置）
}

export function EditProviderDialog({
  open,
  provider,
  onOpenChange,
  onSubmit,
  appId,
  isProxyTakeover = false,
}: EditProviderDialogProps) {
  const { t } = useTranslation();
  const [isFormSubmitting, setIsFormSubmitting] = useState(false);

  // 默认使用传入的 provider.settingsConfig，若当前编辑对象是"当前生效供应商"，则尝试读取实时配置替换初始值
  const [liveSettings, setLiveSettings] = useState<Record<
    string,
    unknown
  > | null>(null);

  // 使用 ref 标记是否已经加载过，防止重复读取覆盖用户编辑
  const [hasLoadedLive, setHasLoadedLive] = useState(false);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      if (!open || !provider) {
        setLiveSettings(null);
        setHasLoadedLive(false);
        return;
      }

      // 关键修复：只在首次打开时加载一次
      if (hasLoadedLive) {
        return;
      }

      // 代理接管模式：Live 配置已被代理改写，读取 live 会导致编辑界面展示代理地址/占位符等内容
      // 因此直接回退到 SSOT（数据库）配置，避免用户困惑与误保存
      if (isProxyTakeover) {
        if (!cancelled) {
          setLiveSettings(null);
          setHasLoadedLive(true);
        }
        return;
      }

      // OpenCode uses additive mode - each provider's config is stored independently in DB
      // Reading live config would return the full opencode.json (with $schema, provider, mcp etc.)
      // instead of just the provider fragment, causing incorrect nested structure on save
      if (appId === "opencode") {
        if (!cancelled) {
          setLiveSettings(null);
          setHasLoadedLive(true);
        }
        return;
      }

      if (appId === "openclaw") {
        try {
          const live = await openclawApi.getLiveProvider(provider.id);
          if (!cancelled && live && typeof live === "object") {
            setLiveSettings(live);
          } else if (!cancelled) {
            setLiveSettings(null);
          }
        } catch {
          if (!cancelled) {
            setLiveSettings(null);
          }
        } finally {
          if (!cancelled) {
            setHasLoadedLive(true);
          }
        }
        return;
      }

      try {
        const currentId = await providersApi.getCurrent(appId);
        if (currentId && provider.id === currentId) {
          try {
            const live = (await vscodeApi.getLiveProviderSettings(
              appId,
            )) as Record<string, unknown>;
            if (!cancelled && live && typeof live === "object") {
              setLiveSettings(live);
              setHasLoadedLive(true);
            }
          } catch {
            // 读取实时配置失败则回退到 SSOT（不打断编辑流程）
            if (!cancelled) {
              setLiveSettings(null);
              setHasLoadedLive(true);
            }
          }
        } else {
          if (!cancelled) {
            setLiveSettings(null);
            setHasLoadedLive(true);
          }
        }
      } finally {
        // no-op
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, [open, provider?.id, appId, hasLoadedLive, isProxyTakeover]); // 只依赖 provider.id，不依赖整个 provider 对象

  const initialSettingsConfig = useMemo(() => {
    // 深度合并：以数据库 SSOT 为底，live 为覆盖层。
    // 这样 live 中缺的字段（如 auth/apiKey/env）不会丢失，
    // 而用户在应用内做的修改（如 hooks、MCP 配置等）也能正确反映。
    const dbSettings =
      provider?.settingsConfig &&
      typeof provider.settingsConfig === "object"
        ? (provider.settingsConfig as Record<string, unknown>)
        : {};
    const liveSettingsObj =
      liveSettings && typeof liveSettings === "object"
        ? (liveSettings as Record<string, unknown>)
        : null;

    const base = mergeSettingsForEdit(appId, dbSettings, liveSettingsObj);

    // Codex 的 modelCatalog 是 cc-switch 私有字段，SSOT 在数据库。Live 的 config.toml
    // 仅在写入时投影出 model_catalog_json 指针；Codex.app 改写配置、代理接管/恢复周期、
    // 来回切换供应商都可能让 Live 丢失该投影，从而 read_live_settings 反解为空。
    // 若放任 Live 覆盖，编辑界面会显示空映射表，保存后连同数据库里的映射一起清空（数据丢失）。
    // 因此始终以数据库 SSOT 的 modelCatalog 为准，仅在数据库确实没有时才回退到 Live 反解结果。
    if (
      appId === "codex" &&
      liveSettingsObj &&
      provider?.settingsConfig &&
      typeof provider.settingsConfig === "object"
    ) {
      const dbCatalog = (provider.settingsConfig as Record<string, unknown>)
        .modelCatalog;
      if (dbCatalog !== undefined) {
        return { ...base, modelCatalog: dbCatalog };
      }
    }

    return Object.keys(base).length > 0
      ? base
      : (provider?.settingsConfig as Record<string, unknown> | undefined) ?? {};
  }, [liveSettings, provider?.settingsConfig, appId]); // 只依赖 settingsConfig，不依赖整个 provider

  // 固定 initialData，防止 provider 对象更新时重置表单
  const initialData = useMemo(() => {
    if (!provider) return null;
    return {
      name: provider.name,
      notes: provider.notes,
      websiteUrl: provider.websiteUrl,
      settingsConfig: initialSettingsConfig,
      category: provider.category,
      meta: provider.meta,
      icon: provider.icon,
      iconColor: provider.iconColor,
    };
  }, [
    open, // 修复：编辑保存后再次打开显示旧数据，依赖 open 确保每次打开时重新读取最新 provider 数据
    provider?.id, // 只依赖 ID，provider 对象更新不会触发重新计算
    provider?.meta, // 需要依赖 meta 以便正确初始化 testConfig
    initialSettingsConfig,
  ]);

  const handleSubmit = useCallback(
    async (values: ProviderFormValues) => {
      if (!provider) return;

      // 注意：values.settingsConfig 已经是最终的配置字符串
      // ProviderForm 已经为不同的 app 类型（Claude/Codex/Gemini）正确组装了配置
      const parsedConfig = JSON.parse(values.settingsConfig) as Record<
        string,
        unknown
      >;
      const nextProviderId =
        (appId === "opencode" || appId === "openclaw") &&
        values.providerKey?.trim()
          ? values.providerKey.trim()
          : provider.id;

      const updatedProvider: Provider = {
        ...provider,
        id: nextProviderId,
        name: values.name.trim(),
        notes: values.notes?.trim() || undefined,
        websiteUrl: values.websiteUrl?.trim() || undefined,
        settingsConfig: parsedConfig,
        icon: values.icon?.trim() || undefined,
        iconColor: values.iconColor?.trim() || undefined,
        ...(values.presetCategory ? { category: values.presetCategory } : {}),
        // 保留或更新 meta 字段
        ...(values.meta ? { meta: values.meta } : {}),
      };

      await onSubmit({
        provider: updatedProvider,
        originalId: provider.id,
      });
      onOpenChange(false);
    },
    [appId, onSubmit, onOpenChange, provider],
  );

  if (!provider || !initialData) {
    return null;
  }

  return (
    <FullScreenPanel
      isOpen={open}
      title={t("provider.editProvider")}
      onClose={() => onOpenChange(false)}
      footer={
        <Button
          type="submit"
          form="provider-form"
          disabled={isFormSubmitting}
          className="bg-primary text-primary-foreground hover:bg-primary/90"
        >
          <Save className="h-4 w-4 mr-2" />
          {t("common.save")}
        </Button>
      }
    >
      <ProviderForm
        appId={appId}
        providerId={provider.id}
        submitLabel={t("common.save")}
        onSubmit={handleSubmit}
        onCancel={() => onOpenChange(false)}
        onSubmittingChange={setIsFormSubmitting}
        initialData={initialData}
        showButtons={false}
        isProxyTakeover={isProxyTakeover}
      />
    </FullScreenPanel>
  );
}
