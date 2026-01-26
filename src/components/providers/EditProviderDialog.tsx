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
import { providersApi, vscodeApi, configApi, type AppId } from "@/lib/api";
import { extractDifference, isPlainObject } from "@/utils/configMerge";
import { extractTomlDifference } from "@/utils/tomlConfigMerge";

interface EditProviderDialogProps {
  open: boolean;
  provider: Provider | null;
  onOpenChange: (open: boolean) => void;
  onSubmit: (provider: Provider) => Promise<void> | void;
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

      try {
        const currentId = await providersApi.getCurrent(appId);
        if (currentId && provider.id === currentId) {
          try {
            const live = (await vscodeApi.getLiveProviderSettings(
              appId,
            )) as Record<string, unknown>;
            if (!cancelled && live && typeof live === "object") {
              // 检查是否启用了通用配置
              const metaByApp = provider.meta?.commonConfigEnabledByApp;
              const commonConfigEnabled =
                metaByApp?.[appId as "claude" | "codex" | "gemini"] ??
                provider.meta?.commonConfigEnabled ??
                false;

              if (commonConfigEnabled) {
                // 从 live 配置中提取自定义部分（去除通用配置）
                try {
                  const commonSnippet =
                    await configApi.getCommonConfigSnippet(appId);
                  if (commonSnippet && commonSnippet.trim()) {
                    if (appId === "codex") {
                      // Codex: 处理 TOML 格式的 config 字段
                      const liveConfig =
                        (live as { auth?: unknown; config?: string }).config ??
                        "";
                      const { customToml, error } = extractTomlDifference(
                        liveConfig,
                        commonSnippet.trim(),
                      );
                      if (!error) {
                        setLiveSettings({
                          ...live,
                          config: customToml,
                        });
                      } else {
                        setLiveSettings(live);
                      }
                    } else if (appId === "gemini") {
                      // Gemini: common config is stored as JSON {"KEY": "VALUE"} format
                      const liveEnv =
                        (live as { env?: Record<string, string> }).env ?? {};
                      // Parse common config as JSON
                      const commonEnvObj = JSON.parse(
                        commonSnippet.trim(),
                      ) as Record<string, unknown>;
                      // Convert to string record, filtering out non-string values
                      const commonEnvStrObj: Record<string, string> = {};
                      for (const [key, value] of Object.entries(commonEnvObj)) {
                        if (typeof value === "string") {
                          commonEnvStrObj[key] = value;
                        }
                      }
                      if (
                        isPlainObject(liveEnv) &&
                        isPlainObject(commonEnvStrObj)
                      ) {
                        const { customConfig } = extractDifference(
                          liveEnv,
                          commonEnvStrObj,
                        );
                        setLiveSettings({
                          ...live,
                          env: customConfig,
                        });
                      } else {
                        setLiveSettings(live);
                      }
                    } else {
                      // Claude: 处理 JSON 格式
                      const commonConfig = JSON.parse(commonSnippet.trim());
                      if (isPlainObject(live) && isPlainObject(commonConfig)) {
                        const { customConfig } = extractDifference(
                          live,
                          commonConfig,
                        );
                        setLiveSettings(customConfig);
                      } else {
                        setLiveSettings(live);
                      }
                    }
                  } else {
                    setLiveSettings(live);
                  }
                } catch {
                  // 提取失败时使用原始 live 配置
                  setLiveSettings(live);
                }
              } else {
                setLiveSettings(live);
              }
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
  }, [
    open,
    provider?.id,
    provider?.meta,
    appId,
    hasLoadedLive,
    isProxyTakeover,
  ]); // 添加 provider?.meta 依赖

  const initialSettingsConfig = useMemo(() => {
    return (liveSettings ?? provider?.settingsConfig ?? {}) as Record<
      string,
      unknown
    >;
  }, [liveSettings, provider?.settingsConfig]); // 只依赖 settingsConfig，不依赖整个 provider

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
    provider?.meta, // 需要依赖 meta 以便正确初始化 testConfig 和 proxyConfig
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

      const updatedProvider: Provider = {
        ...provider,
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

      await onSubmit(updatedProvider);
      onOpenChange(false);
    },
    [onSubmit, onOpenChange, provider],
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
        initialData={initialData}
        showButtons={false}
      />
    </FullScreenPanel>
  );
}
