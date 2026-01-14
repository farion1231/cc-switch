import React, { useState } from "react";
import { Play, Wand2, Eye, EyeOff, Save } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { Provider, UsageScript, UsageData } from "@/types";
import { usageApi, type AppId } from "@/lib/api";
import { extractCodexBaseUrl } from "@/utils/providerConfigUtils";
import JsonEditor from "./JsonEditor";
import * as prettier from "prettier/standalone";
import * as parserBabel from "prettier/parser-babel";
import * as pluginEstree from "prettier/plugins/estree";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { cn } from "@/lib/utils";

interface UsageScriptModalProps {
  provider: Provider;
  appId: AppId;
  isOpen: boolean;
  onClose: () => void;
  onSave: (script: UsageScript) => void;
}

// 预设模板键名（用于国际化）
const TEMPLATE_KEYS = {
  CUSTOM: "custom",
  GENERAL: "general",
  NEW_API: "newapi",
} as const;

// 生成预设模板的函数（支持国际化）
const generatePresetTemplates = (
  t: (key: string) => string,
): Record<string, string> => ({
  [TEMPLATE_KEYS.CUSTOM]: `({
  request: {
    url: "",
    method: "GET",
    headers: {}
  },
  extractor: function(response) {
    return {
      remaining: 0,
      unit: "USD"
    };
  }
})`,

  [TEMPLATE_KEYS.GENERAL]: `({
  request: {
    url: "{{baseUrl}}/user/balance",
    method: "GET",
    headers: {
      "Authorization": "Bearer {{apiKey}}",
      "User-Agent": "cc-switch/1.0"
    }
  },
  extractor: function(response) {
    return {
      isValid: response.is_active || true,
      remaining: response.balance,
      unit: "USD"
    };
  }
})`,

  [TEMPLATE_KEYS.NEW_API]: `({
  request: {
    url: "{{baseUrl}}/api/user/self",
    method: "GET",
    headers: {
      "Content-Type": "application/json",
      "Authorization": "Bearer {{accessToken}}",
      "New-Api-User": "{{userId}}"
    },
  },
  extractor: function (response) {
    if (response.success && response.data) {
      return {
        planName: response.data.group || "${t("usageScript.defaultPlan")}",
        remaining: response.data.quota / 500000,
        used: response.data.used_quota / 500000,
        total: (response.data.quota + response.data.used_quota) / 500000,
        unit: "USD",
      };
    }
    return {
      isValid: false,
      invalidMessage: response.message || "${t("usageScript.queryFailedMessage")}"
    };
  },
})`,
});

// 模板名称国际化键映射
const TEMPLATE_NAME_KEYS: Record<string, string> = {
  [TEMPLATE_KEYS.CUSTOM]: "usageScript.templateCustom",
  [TEMPLATE_KEYS.GENERAL]: "usageScript.templateGeneral",
  [TEMPLATE_KEYS.NEW_API]: "usageScript.templateNewAPI",
};

const UsageScriptModal: React.FC<UsageScriptModalProps> = ({
  provider,
  appId,
  isOpen,
  onClose,
  onSave,
}) => {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  // 生成带国际化的预设模板
  const PRESET_TEMPLATES = generatePresetTemplates(t);

  // 从 provider 的 settingsConfig 中提取 API Key 和 Base URL
  const getProviderCredentials = (): {
    apiKey: string | undefined;
    baseUrl: string | undefined;
  } => {
    try {
      const config = provider.settingsConfig;
      if (!config) return { apiKey: undefined, baseUrl: undefined };

      // 处理不同应用的配置格式
      if (appId === "claude") {
        // Claude: { env: { ANTHROPIC_AUTH_TOKEN | ANTHROPIC_API_KEY, ANTHROPIC_BASE_URL } }
        const env = (config as any).env || {};
        return {
          apiKey: env.ANTHROPIC_AUTH_TOKEN || env.ANTHROPIC_API_KEY,
          baseUrl: env.ANTHROPIC_BASE_URL,
        };
      } else if (appId === "codex") {
        // Codex: { auth: { OPENAI_API_KEY }, config: TOML string with base_url }
        const auth = (config as any).auth || {};
        const configToml = (config as any).config || "";
        return {
          apiKey: auth.OPENAI_API_KEY,
          baseUrl: extractCodexBaseUrl(configToml),
        };
      } else if (appId === "gemini") {
        // Gemini: { env: { GEMINI_API_KEY, GOOGLE_GEMINI_BASE_URL } }
        const env = (config as any).env || {};
        return {
          apiKey: env.GEMINI_API_KEY,
          baseUrl: env.GOOGLE_GEMINI_BASE_URL,
        };
      }
      return { apiKey: undefined, baseUrl: undefined };
    } catch (error) {
      console.error("Failed to extract provider credentials:", error);
      return { apiKey: undefined, baseUrl: undefined };
    }
  };

  const providerCredentials = getProviderCredentials();

  const [script, setScript] = useState<UsageScript>(() => {
    const savedScript = provider.meta?.usage_script;
    const defaultScript = {
      enabled: false,
      language: "javascript" as const,
      code: PRESET_TEMPLATES[TEMPLATE_KEYS.GENERAL],
      timeout: 10,
    };

    if (!savedScript) {
      return defaultScript;
    }

    return savedScript;
  });

  const [testing, setTesting] = useState(false);

  // 🔧 失焦时的验证（严格）- 仅确保有效整数
  const validateTimeout = (value: string): number => {
    const num = Number(value);
    if (isNaN(num) || value.trim() === "") {
      return 10;
    }
    if (!Number.isInteger(num)) {
      toast.warning(
        t("usageScript.timeoutMustBeInteger") || "超时时间必须为整数",
      );
    }
    if (num < 0) {
      toast.error(
        t("usageScript.timeoutCannotBeNegative") || "超时时间不能为负数",
      );
      return 10;
    }
    return Math.floor(num);
  };

  // 🔧 失焦时的验证（严格）- 自动查询间隔
  const validateAndClampInterval = (value: string): number => {
    const num = Number(value);
    if (isNaN(num) || value.trim() === "") {
      return 0;
    }
    if (!Number.isInteger(num)) {
      toast.warning(
        t("usageScript.intervalMustBeInteger") || "自动查询间隔必须为整数",
      );
    }
    if (num < 0) {
      toast.error(
        t("usageScript.intervalCannotBeNegative") || "自动查询间隔不能为负数",
      );
      return 0;
    }
    const clamped = Math.max(0, Math.min(1440, Math.floor(num)));
    if (clamped !== num && num > 0) {
      toast.info(
        t("usageScript.intervalAdjusted", { value: clamped }) ||
          `自动查询间隔已调整为 ${clamped} 分钟`,
      );
    }
    return clamped;
  };

  const [selectedTemplate, setSelectedTemplate] = useState<string | null>(
    () => {
      const existingScript = provider.meta?.usage_script;
      // 优先使用保存的 templateType
      if (existingScript?.templateType) {
        return existingScript.templateType;
      }
      // 向后兼容：根据字段推断模板类型
      // 检测 NEW_API 模板（有 accessToken 或 userId）
      if (existingScript?.accessToken || existingScript?.userId) {
        return TEMPLATE_KEYS.NEW_API;
      }
      // 检测 GENERAL 模板（有 apiKey 或 baseUrl）
      if (existingScript?.apiKey || existingScript?.baseUrl) {
        return TEMPLATE_KEYS.GENERAL;
      }
      // 新配置或无凭证：默认使用 GENERAL（与默认代码模板一致）
      return TEMPLATE_KEYS.GENERAL;
    },
  );

  const [showApiKey, setShowApiKey] = useState(false);
  const [showAccessToken, setShowAccessToken] = useState(false);

  const handleSave = () => {
    if (script.enabled && !script.code.trim()) {
      toast.error(t("usageScript.scriptEmpty"));
      return;
    }
    if (script.enabled && !script.code.includes("return")) {
      toast.error(t("usageScript.mustHaveReturn"), { duration: 5000 });
      return;
    }
    // 保存时记录当前选择的模板类型
    const scriptWithTemplate = {
      ...script,
      templateType: selectedTemplate as
        | "custom"
        | "general"
        | "newapi"
        | undefined,
    };
    onSave(scriptWithTemplate);
    onClose();
  };

  const handleTest = async () => {
    setTesting(true);
    try {
      const result = await usageApi.testScript(
        provider.id,
        appId,
        script.code,
        script.timeout,
        script.apiKey,
        script.baseUrl,
        script.accessToken,
        script.userId,
        selectedTemplate as "custom" | "general" | "newapi" | undefined,
      );
      if (result.success && result.data && result.data.length > 0) {
        const summary = result.data
          .map((plan: UsageData) => {
            const planInfo = plan.planName ? `[${plan.planName}]` : "";
            return `${planInfo} ${t("usage.remaining")} ${plan.remaining} ${plan.unit}`;
          })
          .join(", ");
        toast.success(`${t("usageScript.testSuccess")}${summary}`, {
          duration: 3000,
          closeButton: true,
        });

        // 🔧 测试成功后，更新主界面列表的用量查询缓存
        queryClient.setQueryData(["usage", provider.id, appId], result);
      } else {
        toast.error(
          `${t("usageScript.testFailed")}: ${result.error || t("endpointTest.noResult")}`,
          {
            duration: 5000,
          },
        );
      }
    } catch (error: any) {
      toast.error(
        `${t("usageScript.testFailed")}: ${error?.message || t("common.unknown")}`,
        {
          duration: 5000,
        },
      );
    } finally {
      setTesting(false);
    }
  };

  const handleFormat = async () => {
    try {
      const formatted = await prettier.format(script.code, {
        parser: "babel",
        plugins: [parserBabel as any, pluginEstree as any],
        semi: true,
        singleQuote: false,
        tabWidth: 2,
        printWidth: 80,
      });
      setScript({ ...script, code: formatted.trim() });
      toast.success(t("usageScript.formatSuccess"), {
        duration: 1000,
        closeButton: true,
      });
    } catch (error: any) {
      toast.error(
        `${t("usageScript.formatFailed")}: ${error?.message || t("jsonEditor.invalidJson")}`,
        {
          duration: 3000,
        },
      );
    }
  };

  const handleUsePreset = (presetName: string) => {
    const preset = PRESET_TEMPLATES[presetName];
    if (preset) {
      if (presetName === TEMPLATE_KEYS.CUSTOM) {
        // 🔧 自定义模式：用户应该在脚本中直接写完整 URL 和凭证，而不是依赖变量替换
        // 这样可以避免同源检查导致的问题
        // 如果用户想使用变量，需要手动在配置中设置 baseUrl/apiKey
        setScript({
          ...script,
          code: preset,
          // 清除凭证，用户可选择手动输入或保持空
          apiKey: undefined,
          baseUrl: undefined,
          accessToken: undefined,
          userId: undefined,
        });
      } else if (presetName === TEMPLATE_KEYS.GENERAL) {
        setScript({
          ...script,
          code: preset,
          accessToken: undefined,
          userId: undefined,
        });
      } else if (presetName === TEMPLATE_KEYS.NEW_API) {
        setScript({
          ...script,
          code: preset,
          apiKey: undefined,
        });
      }
      setSelectedTemplate(presetName);
    }
  };

  const shouldShowCredentialsConfig =
    selectedTemplate === TEMPLATE_KEYS.GENERAL ||
    selectedTemplate === TEMPLATE_KEYS.NEW_API;

  const footer = (
    <>
      <div className="flex gap-2">
        <Button
          variant="secondary"
          size="sm"
          onClick={handleTest}
          disabled={!script.enabled || testing}
        >
          <Play size={14} className="mr-1" />
          {testing ? t("usageScript.testing") : t("usageScript.testScript")}
        </Button>
        <Button
          variant="outline"
          size="sm"
          onClick={handleFormat}
          disabled={!script.enabled}
          title={t("usageScript.format")}
        >
          <Wand2 size={14} className="mr-1" />
          {t("usageScript.format")}
        </Button>
      </div>

      <div className="flex gap-2">
        <Button
          variant="outline"
          onClick={onClose}
          className="border-border/20 hover:bg-accent hover:text-accent-foreground"
        >
          {t("common.cancel")}
        </Button>
        <Button
          onClick={handleSave}
          className="bg-primary text-primary-foreground hover:bg-primary/90"
        >
          <Save size={16} className="mr-2" />
          {t("usageScript.saveConfig")}
        </Button>
      </div>
    </>
  );

  return (
    <FullScreenPanel
      isOpen={isOpen}
      title={`${t("usageScript.title")} - ${provider.name}`}
      onClose={onClose}
      footer={footer}
    >
      <div className="glass rounded-xl border border-white/10 px-6 py-4 flex items-center justify-between gap-4">
        <p className="text-base font-medium leading-none text-foreground">
          {t("usageScript.enableUsageQuery")}
        </p>
        <Switch
          checked={script.enabled}
          onCheckedChange={(checked) =>
            setScript({ ...script, enabled: checked })
          }
          aria-label={t("usageScript.enableUsageQuery")}
        />
      </div>

      {script.enabled && (
        <div className="space-y-6">
          {/* 预设模板选择 */}
          <div className="space-y-4 glass rounded-xl border border-white/10 p-6">
            <Label className="text-base font-medium">
              {t("usageScript.presetTemplate")}
            </Label>
            <div className="flex gap-2 flex-wrap">
              {Object.keys(PRESET_TEMPLATES).map((name) => {
                const isSelected = selectedTemplate === name;
                return (
                  <Button
                    key={name}
                    type="button"
                    variant={isSelected ? "default" : "outline"}
                    size="sm"
                    className={cn(
                      "rounded-lg border",
                      isSelected
                        ? "shadow-sm"
                        : "bg-background text-muted-foreground hover:bg-accent hover:text-accent-foreground",
                    )}
                    onClick={() => handleUsePreset(name)}
                  >
                    {t(TEMPLATE_NAME_KEYS[name])}
                  </Button>
                );
              })}
            </div>

            {/* 自定义模式：变量提示和具体值 */}
            {selectedTemplate === TEMPLATE_KEYS.CUSTOM && (
              <div className="space-y-2 border-t border-white/10 pt-3">
                <h4 className="text-sm font-medium text-foreground">
                  {t("usageScript.supportedVariables")}
                </h4>
                <div className="space-y-1 text-xs">
                  {/* baseUrl */}
                  <div className="flex items-center gap-2 py-1">
                    <code className="text-emerald-500 dark:text-emerald-400 font-mono shrink-0">
                      {"{{baseUrl}}"}
                    </code>
                    <span className="text-muted-foreground/50">=</span>
                    {providerCredentials.baseUrl ? (
                      <code className="text-foreground/70 break-all font-mono">
                        {providerCredentials.baseUrl}
                      </code>
                    ) : (
                      <span className="text-muted-foreground/50 italic">
                        {t("common.notSet") || "未设置"}
                      </span>
                    )}
                  </div>

                  {/* apiKey */}
                  <div className="flex items-center gap-2 py-1">
                    <code className="text-emerald-500 dark:text-emerald-400 font-mono shrink-0">
                      {"{{apiKey}}"}
                    </code>
                    <span className="text-muted-foreground/50">=</span>
                    {providerCredentials.apiKey ? (
                      <>
                        {showApiKey ? (
                          <code className="text-foreground/70 break-all font-mono">
                            {providerCredentials.apiKey}
                          </code>
                        ) : (
                          <code className="text-foreground/70 font-mono">
                            ••••••••
                          </code>
                        )}
                        <button
                          type="button"
                          onClick={() => setShowApiKey(!showApiKey)}
                          className="text-muted-foreground hover:text-foreground transition-colors ml-1"
                          aria-label={
                            showApiKey
                              ? t("apiKeyInput.hide")
                              : t("apiKeyInput.show")
                          }
                        >
                          {showApiKey ? (
                            <EyeOff size={12} />
                          ) : (
                            <Eye size={12} />
                          )}
                        </button>
                      </>
                    ) : (
                      <span className="text-muted-foreground/50 italic">
                        {t("common.notSet") || "未设置"}
                      </span>
                    )}
                  </div>
                </div>
              </div>
            )}

            {/* 凭证配置 */}
            {shouldShowCredentialsConfig && (
              <div className="space-y-4">
                <div className="flex items-start justify-between">
                  <h4 className="text-sm font-medium text-foreground">
                    {t("usageScript.credentialsConfig")}
                  </h4>
                  <p className="text-xs text-muted-foreground">
                    {t("usageScript.credentialsHint")}
                  </p>
                </div>

                <div className="grid gap-4 md:grid-cols-2">
                  {selectedTemplate === TEMPLATE_KEYS.GENERAL && (
                    <>
                      <div className="space-y-2">
                        <Label htmlFor="usage-api-key">
                          API Key{" "}
                          <span className="text-xs text-muted-foreground font-normal">
                            ({t("usageScript.optional")})
                          </span>
                        </Label>
                        <div className="relative">
                          <Input
                            id="usage-api-key"
                            type={showApiKey ? "text" : "password"}
                            value={script.apiKey || ""}
                            onChange={(e) =>
                              setScript({ ...script, apiKey: e.target.value })
                            }
                            placeholder={t("usageScript.apiKeyPlaceholder")}
                            autoComplete="off"
                            className="border-white/10"
                          />
                          {script.apiKey && (
                            <button
                              type="button"
                              onClick={() => setShowApiKey(!showApiKey)}
                              className="absolute inset-y-0 right-0 flex items-center pr-3 text-muted-foreground hover:text-foreground transition-colors"
                              aria-label={
                                showApiKey
                                  ? t("apiKeyInput.hide")
                                  : t("apiKeyInput.show")
                              }
                            >
                              {showApiKey ? (
                                <EyeOff size={16} />
                              ) : (
                                <Eye size={16} />
                              )}
                            </button>
                          )}
                        </div>
                      </div>

                      <div className="space-y-2">
                        <Label htmlFor="usage-base-url">
                          {t("usageScript.baseUrl")}{" "}
                          <span className="text-xs text-muted-foreground font-normal">
                            ({t("usageScript.optional")})
                          </span>
                        </Label>
                        <Input
                          id="usage-base-url"
                          type="text"
                          value={script.baseUrl || ""}
                          onChange={(e) =>
                            setScript({ ...script, baseUrl: e.target.value })
                          }
                          placeholder={t("usageScript.baseUrlPlaceholder")}
                          autoComplete="off"
                          className="border-white/10"
                        />
                      </div>
                    </>
                  )}

                  {selectedTemplate === TEMPLATE_KEYS.NEW_API && (
                    <>
                      <div className="space-y-2">
                        <Label htmlFor="usage-newapi-base-url">
                          {t("usageScript.baseUrl")}
                        </Label>
                        <Input
                          id="usage-newapi-base-url"
                          type="text"
                          value={script.baseUrl || ""}
                          onChange={(e) =>
                            setScript({ ...script, baseUrl: e.target.value })
                          }
                          placeholder="https://api.newapi.com"
                          autoComplete="off"
                          className="border-white/10"
                        />
                      </div>

                      <div className="space-y-2">
                        <Label htmlFor="usage-access-token">
                          {t("usageScript.accessToken")}
                        </Label>
                        <div className="relative">
                          <Input
                            id="usage-access-token"
                            type={showAccessToken ? "text" : "password"}
                            value={script.accessToken || ""}
                            onChange={(e) =>
                              setScript({
                                ...script,
                                accessToken: e.target.value,
                              })
                            }
                            placeholder={t(
                              "usageScript.accessTokenPlaceholder",
                            )}
                            autoComplete="off"
                            className="border-white/10"
                          />
                          {script.accessToken && (
                            <button
                              type="button"
                              onClick={() =>
                                setShowAccessToken(!showAccessToken)
                              }
                              className="absolute inset-y-0 right-0 flex items-center pr-3 text-muted-foreground hover:text-foreground transition-colors"
                              aria-label={
                                showAccessToken
                                  ? t("apiKeyInput.hide")
                                  : t("apiKeyInput.show")
                              }
                            >
                              {showAccessToken ? (
                                <EyeOff size={16} />
                              ) : (
                                <Eye size={16} />
                              )}
                            </button>
                          )}
                        </div>
                      </div>

                      <div className="space-y-2">
                        <Label htmlFor="usage-user-id">
                          {t("usageScript.userId")}
                        </Label>
                        <Input
                          id="usage-user-id"
                          type="text"
                          value={script.userId || ""}
                          onChange={(e) =>
                            setScript({ ...script, userId: e.target.value })
                          }
                          placeholder={t("usageScript.userIdPlaceholder")}
                          autoComplete="off"
                          className="border-white/10"
                        />
                      </div>
                    </>
                  )}
                </div>
              </div>
            )}

            {/* 通用配置（始终显示） */}
            <div className="grid gap-4 md:grid-cols-2 pt-4 border-t border-white/10">
              {/* 超时时间 */}
              <div className="space-y-2">
                <Label htmlFor="usage-timeout">
                  {t("usageScript.timeoutSeconds")}
                </Label>
                <Input
                  id="usage-timeout"
                  type="number"
                  min={0}
                  value={script.timeout ?? 10}
                  onChange={(e) =>
                    setScript({
                      ...script,
                      timeout: validateTimeout(e.target.value),
                    })
                  }
                  onBlur={(e) =>
                    setScript({
                      ...script,
                      timeout: validateTimeout(e.target.value),
                    })
                  }
                  className="border-white/10"
                />
              </div>

              {/* 自动查询间隔 */}
              <div className="space-y-2">
                <Label htmlFor="usage-interval">
                  {t("usageScript.autoIntervalMinutes")}
                </Label>
                <Input
                  id="usage-interval"
                  type="number"
                  min={0}
                  max={1440}
                  value={
                    script.autoQueryInterval ?? script.autoIntervalMinutes ?? 0
                  }
                  onChange={(e) =>
                    setScript({
                      ...script,
                      autoQueryInterval: validateAndClampInterval(
                        e.target.value,
                      ),
                    })
                  }
                  onBlur={(e) =>
                    setScript({
                      ...script,
                      autoQueryInterval: validateAndClampInterval(
                        e.target.value,
                      ),
                    })
                  }
                  className="border-white/10"
                />
              </div>
            </div>
          </div>

          {/* 提取器代码 */}
          <div className="space-y-4 glass rounded-xl border border-white/10 p-6">
            <div className="flex items-center justify-between">
              <Label className="text-base font-medium">
                {t("usageScript.extractorCode")}
              </Label>
              <div className="text-xs text-muted-foreground">
                {t("usageScript.extractorHint")}
              </div>
            </div>
            <JsonEditor
              id="usage-code"
              value={script.code || ""}
              onChange={(value) => setScript({ ...script, code: value })}
              height={480}
              language="javascript"
              showMinimap={false}
            />
          </div>

          {/* 帮助信息 */}
          <div className="glass rounded-xl border border-white/10 p-6 text-sm text-foreground/90">
            <h4 className="font-medium mb-2">{t("usageScript.scriptHelp")}</h4>
            <div className="space-y-3 text-xs">
              <div>
                <strong>{t("usageScript.configFormat")}</strong>
                <pre className="mt-1 p-2 bg-black/20 text-foreground rounded border border-white/10 text-[10px] overflow-x-auto">
                  {`({
  request: {
    url: "{{baseUrl}}/api/usage",
    method: "POST",
    headers: {
      "Authorization": "Bearer {{apiKey}}",
      "User-Agent": "cc-switch/1.0"
    }
  },
  extractor: function(response) {
    return {
      isValid: !response.error,
      remaining: response.balance,
      unit: "USD"
    };
  }
})`}
                </pre>
              </div>

              <div>
                <strong>{t("usageScript.extractorFormat")}</strong>
                <ul className="mt-1 space-y-0.5 ml-2">
                  <li>{t("usageScript.fieldIsValid")}</li>
                  <li>{t("usageScript.fieldInvalidMessage")}</li>
                  <li>{t("usageScript.fieldRemaining")}</li>
                  <li>{t("usageScript.fieldUnit")}</li>
                  <li>{t("usageScript.fieldPlanName")}</li>
                  <li>{t("usageScript.fieldTotal")}</li>
                  <li>{t("usageScript.fieldUsed")}</li>
                  <li>{t("usageScript.fieldExtra")}</li>
                </ul>
              </div>

              <div className="text-muted-foreground">
                <strong>{t("usageScript.tips")}</strong>
                <ul className="mt-1 space-y-0.5 ml-2">
                  <li>
                    {t("usageScript.tip1", {
                      apiKey: "{{apiKey}}",
                      baseUrl: "{{baseUrl}}",
                    })}
                  </li>
                  <li>{t("usageScript.tip2")}</li>
                  <li>{t("usageScript.tip3")}</li>
                </ul>
              </div>
            </div>
          </div>
        </div>
      )}
    </FullScreenPanel>
  );
};

export default UsageScriptModal;
