import React, { useState } from "react";
import { Play, Wand2, Eye, EyeOff, Save, ExternalLink } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { Provider, UsageScript, UsageData, createUsageScript } from "@/types";
import { usageApi, settingsApi, type AppId } from "@/lib/api";
import { copilotGetUsage, copilotGetUsageForAccount } from "@/lib/api/copilot";
import { useSettingsQuery } from "@/lib/query";
import { resolveManagedAccountId } from "@/lib/authBinding";
import { extractErrorMessage } from "@/utils/errorUtils";
import { useDarkMode } from "@/hooks/useDarkMode";
import {
  extractCodexBaseUrl,
  extractCodexExperimentalBearerToken,
} from "@/utils/providerConfigUtils";
import JsonEditor from "./JsonEditor";
import * as prettier from "prettier/standalone";
import * as parserBabel from "prettier/parser-babel";
import * as pluginEstree from "prettier/plugins/estree";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { cn } from "@/lib/utils";
import { TEMPLATE_TYPES, PROVIDER_TYPES } from "@/config/constants";
import {
  CODING_PLAN_PROVIDERS,
  detectCodingPlanProvider,
} from "@/config/codingPlanProviders";
import { formatUsageDataSummary } from "@/utils/usageDisplay";

/**
 * 火山引擎账号级 AccessKey 的密钥管理页（IAM）。
 * 用量查询走控制面 OpenAPI，需要 AK/SK 签名，与推理 API Key 是两套凭据，
 * 直接给用户一个可点击的直达地址，省得在控制台里翻菜单。
 */
const VOLCENGINE_KEY_CONSOLE_URL =
  "https://console.volcengine.com/iam/keymanage";

interface UsageScriptModalProps {
  provider: Provider;
  appId: AppId;
  isOpen: boolean;
  onClose: () => void;
  onSave: (script: UsageScript) => void;
}

// 生成预设模板的函数（支持国际化）
const generatePresetTemplates = (
  t: (key: string) => string,
): Record<string, string> => ({
  [TEMPLATE_TYPES.CUSTOM]: `({
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

  [TEMPLATE_TYPES.GENERAL]: `({
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

  [TEMPLATE_TYPES.NEW_API]: `({
  request: {
    url: "{{baseUrl}}/api/user/self",
    method: "GET",
    headers: {
      "Content-Type": "application/json",
      "Authorization": "Bearer {{accessToken}}",
      "User-Agent": "cc-switch/1.0",
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

  // GitHub Copilot 模板不需要脚本，使用专用 API
  [TEMPLATE_TYPES.GITHUB_COPILOT]: "",

  // Coding Plan 模板不需要脚本，使用专用 Rust 查询
  [TEMPLATE_TYPES.TOKEN_PLAN]: "",

  // 官方余额查询模板不需要脚本，使用专用 Rust 查询
  [TEMPLATE_TYPES.BALANCE]: "",

  // 官方订阅额度查询不需要脚本，使用 CLI/OAuth 凭据调用官方 API
  [TEMPLATE_TYPES.OFFICIAL_SUBSCRIPTION]: "",
});

// 模板名称国际化键映射
const TEMPLATE_NAME_KEYS: Record<string, string> = {
  [TEMPLATE_TYPES.CUSTOM]: "usageScript.templateCustom",
  [TEMPLATE_TYPES.GENERAL]: "usageScript.templateGeneral",
  [TEMPLATE_TYPES.NEW_API]: "usageScript.templateNewAPI",
  [TEMPLATE_TYPES.GITHUB_COPILOT]: "usageScript.templateCopilot",
  [TEMPLATE_TYPES.TOKEN_PLAN]: "usageScript.templateTokenPlan",
  [TEMPLATE_TYPES.BALANCE]: "usageScript.templateBalance",
  [TEMPLATE_TYPES.OFFICIAL_SUBSCRIPTION]:
    "usageScript.templateOfficialSubscription",
};

/** 官方余额查询供应商检测 */
const BALANCE_PROVIDERS = [
  { id: "deepseek", label: "DeepSeek", pattern: /api\.deepseek\.com/i },
  { id: "stepfun", label: "StepFun", pattern: /api\.stepfun\.(ai|com)/i },
  {
    id: "siliconflow",
    label: "SiliconFlow",
    pattern: /api\.siliconflow\.(cn|com)/i,
  },
  { id: "openrouter", label: "OpenRouter", pattern: /openrouter\.ai/i },
  { id: "novita", label: "Novita AI", pattern: /api\.novita\.ai/i },
] as const;

/** 根据 Base URL 自动检测余额查询供应商 */
function detectBalanceProvider(baseUrl: string | undefined): boolean {
  if (!baseUrl) return false;
  return BALANCE_PROVIDERS.some((bp) => bp.pattern.test(baseUrl));
}

function isOfficialSubscriptionProvider(provider: Provider, appId: AppId) {
  if (!["claude", "codex", "gemini"].includes(appId)) return false;
  if (provider.category === "official") return true;

  const config = provider.settingsConfig as Record<string, any>;
  if (appId === "claude") {
    const baseUrl = config?.env?.ANTHROPIC_BASE_URL;
    return !baseUrl || (typeof baseUrl === "string" && baseUrl.trim() === "");
  }
  if (appId === "codex") {
    const apiKey = config?.auth?.OPENAI_API_KEY;
    const bearerToken =
      typeof config?.config === "string"
        ? extractCodexExperimentalBearerToken(config.config)
        : undefined;
    return (
      !bearerToken &&
      (!apiKey || (typeof apiKey === "string" && apiKey.trim() === ""))
    );
  }
  if (appId === "gemini") {
    const env = config?.env || {};
    const apiKey = env.GEMINI_API_KEY;
    const baseUrl = env.GOOGLE_GEMINI_BASE_URL;
    return (
      (!apiKey || (typeof apiKey === "string" && apiKey.trim() === "")) &&
      (!baseUrl || (typeof baseUrl === "string" && baseUrl.trim() === ""))
    );
  }
  return false;
}

const NATIVE_USAGE_TEMPLATES = new Set<string>([
  TEMPLATE_TYPES.GITHUB_COPILOT,
  TEMPLATE_TYPES.TOKEN_PLAN,
  TEMPLATE_TYPES.BALANCE,
  TEMPLATE_TYPES.OFFICIAL_SUBSCRIPTION,
]);

const UsageScriptModal: React.FC<UsageScriptModalProps> = ({
  provider,
  appId,
  isOpen,
  onClose,
  onSave,
}) => {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data: settingsData } = useSettingsQuery();
  const [showUsageConfirm, setShowUsageConfirm] = useState(false);
  const isDarkMode = useDarkMode();

  // 生成带国际化的预设模板
  const PRESET_TEMPLATES = generatePresetTemplates(t);

  // 从 provider 的 settingsConfig 中提取 API Key 和 Base URL
  const getProviderCredentials = (): {
    apiKey: string | undefined;
    baseUrl: string | undefined;
  } => {
    const trimTrailingSlash = (url: string | undefined) =>
      typeof url === "string" ? url.replace(/\/+$/, "") : url;
    const raw = ((): {
      apiKey: string | undefined;
      baseUrl: string | undefined;
    } => {
      try {
        const config = provider.settingsConfig;
        if (!config) return { apiKey: undefined, baseUrl: undefined };

        // 处理不同应用的配置格式
        if (appId === "claude" || appId === "claude-desktop") {
          // Claude / Claude Desktop: { env: { ANTHROPIC_AUTH_TOKEN | ANTHROPIC_API_KEY, ANTHROPIC_BASE_URL } }
          // Key fallbacks mirror the backend resolver (Provider::resolve_usage_credentials).
          const env = (config as any).env || {};
          return {
            apiKey:
              env.ANTHROPIC_AUTH_TOKEN ||
              env.ANTHROPIC_API_KEY ||
              env.OPENROUTER_API_KEY ||
              env.GOOGLE_API_KEY,
            baseUrl: env.ANTHROPIC_BASE_URL,
          };
        } else if (appId === "codex") {
          // Codex: { auth: { OPENAI_API_KEY }, config: TOML string with base_url }
          const auth = (config as any).auth || {};
          const configToml = (config as any).config || "";
          const apiKey =
            typeof auth.OPENAI_API_KEY === "string" &&
            auth.OPENAI_API_KEY.trim()
              ? auth.OPENAI_API_KEY
              : extractCodexExperimentalBearerToken(configToml);
          return {
            apiKey,
            baseUrl: extractCodexBaseUrl(configToml),
          };
        } else if (appId === "gemini") {
          // Gemini: { env: { GEMINI_API_KEY, GOOGLE_GEMINI_BASE_URL } }
          // Key fallback mirrors the backend resolver (Provider::resolve_usage_credentials).
          const env = (config as any).env || {};
          return {
            apiKey: env.GEMINI_API_KEY || env.GOOGLE_API_KEY,
            baseUrl: env.GOOGLE_GEMINI_BASE_URL,
          };
        } else if (appId === "hermes") {
          // Hermes: settingsConfig 顶层扁平（snake_case，对应 config.yaml）
          return {
            apiKey: (config as any).api_key,
            baseUrl: (config as any).base_url,
          };
        } else if (appId === "openclaw") {
          // OpenClaw: settingsConfig 顶层扁平（camelCase，对应 openclaw.json）
          return {
            apiKey: (config as any).apiKey,
            baseUrl: (config as any).baseUrl,
          };
        } else if (appId === "opencode") {
          // OpenCode (OMO): 凭据嵌在 options.{baseURL, apiKey}（SDK options 对象）
          const options = (config as any).options || {};
          return {
            apiKey: options.apiKey,
            baseUrl: options.baseURL,
          };
        }
        return { apiKey: undefined, baseUrl: undefined };
      } catch (error) {
        console.error("Failed to extract provider credentials:", error);
        return { apiKey: undefined, baseUrl: undefined };
      }
    })();
    // Trim the trailing slash to mirror the backend resolver
    // (Provider::resolve_usage_credentials), so `{{baseUrl}}/path` never
    // produces a double slash regardless of which path runs the query.
    return { apiKey: raw.apiKey, baseUrl: trimTrailingSlash(raw.baseUrl) };
  };

  const providerCredentials = getProviderCredentials();
  const isOfficialSubscription = isOfficialSubscriptionProvider(
    provider,
    appId,
  );

  const [script, setScript] = useState<UsageScript>(() => {
    const savedScript = provider.meta?.usage_script;
    if (savedScript) {
      const normalizedScript = createUsageScript(savedScript);
      if (
        isOfficialSubscription &&
        normalizedScript.templateType !== TEMPLATE_TYPES.OFFICIAL_SUBSCRIPTION
      ) {
        return createUsageScript();
      }
      // 已有配置：如果是 coding_plan 但没有 codingPlanProvider，自动检测填充
      if (
        normalizedScript.templateType === TEMPLATE_TYPES.TOKEN_PLAN &&
        !normalizedScript.codingPlanProvider
      ) {
        return {
          ...normalizedScript,
          codingPlanProvider:
            detectCodingPlanProvider(providerCredentials.baseUrl) || "kimi",
        };
      }
      return normalizedScript;
    }

    const autoDetected = detectCodingPlanProvider(providerCredentials.baseUrl);
    if (autoDetected) {
      return createUsageScript({ codingPlanProvider: autoDetected });
    }

    if (detectBalanceProvider(providerCredentials.baseUrl)) {
      return createUsageScript();
    }

    if (isOfficialSubscription) {
      return createUsageScript();
    }

    return createUsageScript({
      code: PRESET_TEMPLATES[TEMPLATE_TYPES.GENERAL],
    });
  });

  const [testing, setTesting] = useState(false);

  // {{apiKey}}/{{baseUrl}} 实际注入值，镜像后端 resolve_script_credentials 的
  // 优先级：脚本配置中的显式非空值优先（旧配置可能携带），否则回退供应商凭据。
  const effectiveScriptCredentials = {
    apiKey: script.apiKey?.trim() || providerCredentials.apiKey,
    baseUrl:
      script.baseUrl?.trim().replace(/\/+$/, "") || providerCredentials.baseUrl,
  };

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
      // Copilot 供应商默认使用 Copilot 模板
      if (provider.meta?.providerType === PROVIDER_TYPES.GITHUB_COPILOT) {
        return TEMPLATE_TYPES.GITHUB_COPILOT;
      }
      // 优先使用保存的 templateType
      if (
        existingScript?.templateType &&
        (!isOfficialSubscription ||
          existingScript.templateType === TEMPLATE_TYPES.OFFICIAL_SUBSCRIPTION)
      ) {
        return existingScript.templateType as string;
      }
      // 官方 CLI/OAuth 供应商默认使用官方订阅额度模板，但开关默认关闭
      if (isOfficialSubscription) {
        return TEMPLATE_TYPES.OFFICIAL_SUBSCRIPTION;
      }
      // 向后兼容：根据字段推断模板类型
      // 检测 NEW_API 模板（有 accessToken 或 userId）
      if (existingScript?.accessToken || existingScript?.userId) {
        return TEMPLATE_TYPES.NEW_API;
      }
      // 检测 GENERAL 模板（有 apiKey 或 baseUrl）
      if (existingScript?.apiKey || existingScript?.baseUrl) {
        return TEMPLATE_TYPES.GENERAL;
      }
      // 新配置：如果 URL 匹配 Coding Plan 供应商，自动选择 Coding Plan 模板
      if (detectCodingPlanProvider(providerCredentials.baseUrl)) {
        return TEMPLATE_TYPES.TOKEN_PLAN;
      }
      // 新配置：如果 URL 匹配官方余额查询供应商，自动选择 Balance 模板
      if (detectBalanceProvider(providerCredentials.baseUrl)) {
        return TEMPLATE_TYPES.BALANCE;
      }
      // 默认使用 GENERAL（与默认代码模板一致）
      return TEMPLATE_TYPES.GENERAL;
    },
  );

  const [showApiKey, setShowApiKey] = useState(false);
  const [showAccessToken, setShowAccessToken] = useState(false);

  const handleEnableToggle = (checked: boolean) => {
    if (checked && !settingsData?.usageConfirmed) {
      setShowUsageConfirm(true);
    } else {
      setScript({ ...script, enabled: checked });
    }
  };

  const handleUsageConfirm = async () => {
    setShowUsageConfirm(false);
    try {
      if (settingsData) {
        const { webdavSync: _, ...rest } = settingsData;
        await settingsApi.save({ ...rest, usageConfirmed: true });
        await queryClient.invalidateQueries({ queryKey: ["settings"] });
      }
    } catch (error) {
      console.error("Failed to save usage confirmed:", error);
    }
    setScript({ ...script, enabled: true });
  };

  const handleSave = () => {
    // 专用模板不需要脚本验证
    if (!NATIVE_USAGE_TEMPLATES.has(selectedTemplate || "")) {
      if (script.enabled && !script.code.trim()) {
        toast.error(t("usageScript.scriptEmpty"));
        return;
      }
      if (script.enabled && !script.code.includes("return")) {
        toast.error(t("usageScript.mustHaveReturn"), { duration: 5000 });
        return;
      }
    }
    // 保存时记录当前选择的模板类型
    const scriptWithTemplate = {
      ...script,
      templateType: selectedTemplate as
        | "custom"
        | "general"
        | "newapi"
        | "github_copilot"
        | "token_plan"
        | "balance"
        | "official_subscription"
        | undefined,
    };
    onSave(scriptWithTemplate);
    onClose();
  };

  const handleTest = async () => {
    setTesting(true);
    try {
      // 官方订阅额度模板使用 CLI/OAuth 凭据和官方 API
      if (selectedTemplate === TEMPLATE_TYPES.OFFICIAL_SUBSCRIPTION) {
        const { subscriptionApi } = await import("@/lib/api/subscription");
        const quota = await subscriptionApi.getQuota(appId);
        if (quota.success && quota.tiers.length > 0) {
          const summary = quota.tiers
            .map((tier) => `${tier.name}: ${Math.round(tier.utilization)}%`)
            .join(", ");
          toast.success(`${t("usageScript.testSuccess")}${summary}`, {
            duration: 3000,
            closeButton: true,
          });
          queryClient.setQueryData(["subscription", "quota", appId], quota);
        } else {
          toast.error(
            `${t("usageScript.testFailed")}: ${quota.error || t("endpointTest.noResult")}`,
            { duration: 5000 },
          );
        }
        return;
      }

      // 官方余额查询模板使用专用 API
      if (selectedTemplate === TEMPLATE_TYPES.BALANCE) {
        const baseUrl = providerCredentials.baseUrl ?? "";
        const apiKey = providerCredentials.apiKey ?? "";
        const { subscriptionApi } = await import("@/lib/api/subscription");
        const result = await subscriptionApi.getBalance(baseUrl, apiKey);
        if (result.success && result.data && result.data.length > 0) {
          const summary = result.data
            .map((d) =>
              formatUsageDataSummary(d, {
                invalid: t("usage.invalid"),
                remaining: t("usage.remaining"),
                used: t("usage.used"),
              }),
            )
            .join(", ");
          toast.success(`${t("usageScript.testSuccess")}${summary}`, {
            duration: 3000,
            closeButton: true,
          });
          queryClient.setQueryData(["usage", provider.id, appId], result);
        } else {
          toast.error(
            `${t("usageScript.testFailed")}: ${result.error || t("endpointTest.noResult")}`,
            { duration: 5000 },
          );
        }
        return;
      }

      // Coding Plan 模板使用专用 API
      if (selectedTemplate === TEMPLATE_TYPES.TOKEN_PLAN) {
        // ZenMux 手填 baseUrl/apiKey；火山是 native 供应商，baseUrl 走推理配置，
        // 另用账号 AK/SK 签名查询控制面用量。
        const isZenMux = script.codingPlanProvider === "zenmux";
        const isVolcengine = script.codingPlanProvider === "volcengine";
        const baseUrl = isZenMux
          ? (script.baseUrl ?? "")
          : (providerCredentials.baseUrl ?? "");
        const apiKey = isZenMux
          ? (script.apiKey ?? "")
          : (providerCredentials.apiKey ?? "");
        const { subscriptionApi } = await import("@/lib/api/subscription");
        const quota = await subscriptionApi.getCodingPlanQuota(
          baseUrl,
          apiKey,
          isVolcengine ? script.accessKeyId : undefined,
          isVolcengine ? script.secretAccessKey : undefined,
        );
        if (quota.success && quota.tiers.length > 0) {
          const summary = quota.tiers
            .map((tier) => `${tier.name}: ${Math.round(tier.utilization)}%`)
            .join(", ");
          toast.success(`${t("usageScript.testSuccess")}${summary}`, {
            duration: 3000,
            closeButton: true,
          });
          // 将结果转换为 UsageResult 格式更新缓存
          const usageData = quota.tiers.map((tier) => ({
            planName: tier.name,
            remaining: 100 - tier.utilization,
            total: 100,
            used: tier.utilization,
            unit: "%",
          }));
          queryClient.setQueryData(["usage", provider.id, appId], {
            success: true,
            data: usageData,
          });
        } else {
          toast.error(
            `${t("usageScript.testFailed")}: ${quota.error || t("endpointTest.noResult")}`,
            { duration: 5000 },
          );
        }
        return;
      }

      // Copilot 模板使用专用 API
      if (selectedTemplate === TEMPLATE_TYPES.GITHUB_COPILOT) {
        const accountId = resolveManagedAccountId(
          provider.meta,
          PROVIDER_TYPES.GITHUB_COPILOT,
        );
        const usage = accountId
          ? await copilotGetUsageForAccount(accountId)
          : await copilotGetUsage();
        const premium = usage.quota_snapshots.premium_interactions;
        const used = premium.entitlement - premium.remaining;
        const summary = `[${usage.copilot_plan}] ${t("usage.remaining")} ${premium.remaining}/${premium.entitlement} (${t("usageScript.resetDate")}: ${usage.quota_reset_date})`;
        toast.success(`${t("usageScript.testSuccess")}${summary}`, {
          duration: 3000,
          closeButton: true,
        });
        // 更新缓存
        queryClient.setQueryData(["usage", provider.id, appId], {
          success: true,
          data: [
            {
              planName: usage.copilot_plan,
              remaining: premium.remaining,
              total: premium.entitlement,
              used: used,
              unit: t("usageScript.premiumRequests"),
            },
          ],
        });
        return;
      }

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
          .map((plan: UsageData) =>
            formatUsageDataSummary(plan, {
              invalid: t("usage.invalid"),
              remaining: t("usage.remaining"),
              used: t("usage.used"),
            }),
          )
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
      // 后端命令 Err(String) 时 invoke 以裸字符串 reject（如瞬时网络失败），
      // 直接读 .message 会得到 undefined。
      toast.error(
        `${t("usageScript.testFailed")}: ${extractErrorMessage(error) || t("common.unknown")}`,
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
    if (preset !== undefined) {
      if (presetName === TEMPLATE_TYPES.CUSTOM) {
        // 自定义模板没有凭证输入框：清空显式覆盖值后，测试与真实查询
        // 都会在后端回退到供应商配置（Provider::resolve_usage_credentials），
        // 与下方“支持的变量”区域展示的 {{apiKey}}/{{baseUrl}} 取值一致。
        setScript({
          ...script,
          code: preset,
          apiKey: undefined,
          baseUrl: undefined,
          accessToken: undefined,
          userId: undefined,
        });
      } else if (presetName === TEMPLATE_TYPES.GENERAL) {
        setScript({
          ...script,
          code: preset,
          accessToken: undefined,
          userId: undefined,
        });
      } else if (presetName === TEMPLATE_TYPES.NEW_API) {
        setScript({
          ...script,
          code: preset,
          apiKey: undefined,
        });
      } else if (presetName === TEMPLATE_TYPES.GITHUB_COPILOT) {
        // Copilot 模板不需要脚本和凭证，使用专用 API
        setScript({
          ...script,
          code: "",
          apiKey: undefined,
          baseUrl: undefined,
          accessToken: undefined,
          userId: undefined,
        });
      } else if (presetName === TEMPLATE_TYPES.TOKEN_PLAN) {
        // Coding Plan 模板不需要脚本，使用 Rust 原生查询
        const autoDetected = detectCodingPlanProvider(
          providerCredentials.baseUrl,
        );
        const provider = script.codingPlanProvider || autoDetected || "kimi";
        // ZenMux 保留手填 baseUrl/apiKey；火山保留账号 AK/SK；其余清除。
        const isZenMux = provider === "zenmux";
        const isVolcengine = provider === "volcengine";
        setScript({
          ...script,
          code: "",
          apiKey: isZenMux ? script.apiKey : undefined,
          baseUrl: isZenMux ? script.baseUrl : undefined,
          accessToken: undefined,
          userId: undefined,
          accessKeyId: isVolcengine ? script.accessKeyId : undefined,
          secretAccessKey: isVolcengine ? script.secretAccessKey : undefined,
          codingPlanProvider: provider,
        });
      } else if (presetName === TEMPLATE_TYPES.BALANCE) {
        // 官方余额查询模板不需要脚本，使用 Rust 原生查询
        setScript({
          ...script,
          code: "",
          apiKey: undefined,
          baseUrl: undefined,
          accessToken: undefined,
          userId: undefined,
        });
      } else if (presetName === TEMPLATE_TYPES.OFFICIAL_SUBSCRIPTION) {
        // 官方订阅额度查询不需要脚本，使用 CLI/OAuth 凭据
        setScript({
          ...script,
          code: "",
          apiKey: undefined,
          baseUrl: undefined,
          accessToken: undefined,
          userId: undefined,
        });
      }
      setSelectedTemplate(presetName);
    }
  };

  const shouldShowCredentialsConfig =
    selectedTemplate === TEMPLATE_TYPES.GENERAL ||
    selectedTemplate === TEMPLATE_TYPES.NEW_API ||
    (selectedTemplate === TEMPLATE_TYPES.TOKEN_PLAN &&
      script.codingPlanProvider === "zenmux");

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
          disabled={
            !script.enabled ||
            NATIVE_USAGE_TEMPLATES.has(selectedTemplate || "")
          }
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
          onCheckedChange={handleEnableToggle}
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
              {Object.keys(PRESET_TEMPLATES)
                .filter((name) => {
                  const isCopilotProvider =
                    provider.meta?.providerType === "github_copilot";
                  // Copilot 供应商只显示 copilot 模板
                  if (isCopilotProvider) {
                    return name === TEMPLATE_TYPES.GITHUB_COPILOT;
                  }
                  // 官方 CLI/OAuth 供应商只显示官方订阅额度模板
                  if (isOfficialSubscription) {
                    return name === TEMPLATE_TYPES.OFFICIAL_SUBSCRIPTION;
                  }
                  // 非 Copilot 供应商不显示 copilot 模板
                  return (
                    name !== TEMPLATE_TYPES.GITHUB_COPILOT &&
                    name !== TEMPLATE_TYPES.OFFICIAL_SUBSCRIPTION
                  );
                })
                .map((name) => {
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
            {selectedTemplate === TEMPLATE_TYPES.CUSTOM && (
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
                    {effectiveScriptCredentials.baseUrl ? (
                      <code className="text-foreground/70 break-all font-mono">
                        {effectiveScriptCredentials.baseUrl}
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
                    {effectiveScriptCredentials.apiKey ? (
                      <>
                        {showApiKey ? (
                          <code className="text-foreground/70 break-all font-mono">
                            {effectiveScriptCredentials.apiKey}
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

            {/* Copilot 模式：自动认证提示 */}
            {selectedTemplate === TEMPLATE_TYPES.GITHUB_COPILOT && (
              <div className="space-y-2 border-t border-white/10 pt-3">
                <p className="text-sm text-muted-foreground">
                  {t("usageScript.copilotAutoAuth")}
                </p>
              </div>
            )}

            {/* 官方余额查询模式：自动提示 */}
            {selectedTemplate === TEMPLATE_TYPES.BALANCE && (
              <div className="space-y-3 border-t border-white/10 pt-3">
                <p className="text-sm text-muted-foreground">
                  {t("usageScript.balanceHint")}
                </p>
                <div className="flex gap-2 flex-wrap">
                  {BALANCE_PROVIDERS.filter((bp) =>
                    bp.pattern.test(providerCredentials.baseUrl || ""),
                  ).map((bp) => (
                    <span
                      key={bp.id}
                      className="inline-flex items-center px-2.5 py-1 rounded-md bg-primary/10 text-primary text-xs font-medium"
                    >
                      {bp.label}
                    </span>
                  ))}
                </div>
              </div>
            )}

            {/* 官方订阅额度模式：自动提示 */}
            {selectedTemplate === TEMPLATE_TYPES.OFFICIAL_SUBSCRIPTION && (
              <div className="space-y-2 border-t border-white/10 pt-3">
                <p className="text-sm text-muted-foreground">
                  {t("usageScript.officialSubscriptionHint")}
                </p>
              </div>
            )}

            {/* Coding Plan 模式：供应商选择 */}
            {selectedTemplate === TEMPLATE_TYPES.TOKEN_PLAN && (
              <div className="space-y-3 border-t border-white/10 pt-3">
                <p className="text-sm text-muted-foreground">
                  {t("usageScript.tokenPlanHint")}
                </p>
                <div className="flex gap-2 flex-wrap">
                  {CODING_PLAN_PROVIDERS.map((cp) => (
                    <Button
                      key={cp.id}
                      type="button"
                      variant={
                        script.codingPlanProvider === cp.id
                          ? "default"
                          : "outline"
                      }
                      size="sm"
                      className={cn(
                        "rounded-lg border",
                        script.codingPlanProvider === cp.id
                          ? "shadow-sm"
                          : "bg-background text-muted-foreground hover:bg-accent hover:text-accent-foreground",
                      )}
                      onClick={() =>
                        setScript({
                          ...script,
                          codingPlanProvider: cp.id,
                        })
                      }
                    >
                      {cp.label}
                    </Button>
                  ))}
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
                  {selectedTemplate === TEMPLATE_TYPES.GENERAL && (
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

                  {selectedTemplate === TEMPLATE_TYPES.NEW_API && (
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

                  {selectedTemplate === TEMPLATE_TYPES.TOKEN_PLAN &&
                    script.codingPlanProvider === "zenmux" && (
                      <>
                        <div className="space-y-2">
                          <Label htmlFor="usage-zenmux-base-url">
                            {t("usageScript.baseUrl")}
                          </Label>
                          <Input
                            id="usage-zenmux-base-url"
                            type="text"
                            value={script.baseUrl || ""}
                            onChange={(e) =>
                              setScript({ ...script, baseUrl: e.target.value })
                            }
                            placeholder="https://api.zenmux.com/v1/..."
                            autoComplete="off"
                            className="border-white/10"
                          />
                        </div>

                        <div className="space-y-2">
                          <Label htmlFor="usage-zenmux-api-key">API Key</Label>
                          <div className="relative">
                            <Input
                              id="usage-zenmux-api-key"
                              type={showApiKey ? "text" : "password"}
                              value={script.apiKey || ""}
                              onChange={(e) =>
                                setScript({
                                  ...script,
                                  apiKey: e.target.value,
                                })
                              }
                              placeholder="sk-..."
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
                      </>
                    )}
                </div>
              </div>
            )}

            {/* 火山方舟：控制面用量查询需账号 AK/SK（与推理 Key 是两套凭据） */}
            {selectedTemplate === TEMPLATE_TYPES.TOKEN_PLAN &&
              script.codingPlanProvider === "volcengine" && (
                <div className="space-y-4">
                  <div>
                    <h4 className="text-sm font-medium text-foreground">
                      {t("usageScript.credentialsConfig")}
                    </h4>
                    <p className="text-xs text-muted-foreground mt-1 leading-relaxed">
                      {t("usageScript.volcengineAkSkHint")}
                    </p>
                    <p className="text-xs text-muted-foreground mt-1.5">
                      {t("usageScript.volcengineKeyConsoleLink")}{" "}
                      <button
                        type="button"
                        onClick={() =>
                          settingsApi.openExternal(VOLCENGINE_KEY_CONSOLE_URL)
                        }
                        className="inline-flex items-center gap-1 text-blue-400 dark:text-blue-500 hover:text-blue-500 dark:hover:text-blue-400 transition-colors break-all align-baseline underline-offset-2 hover:underline"
                      >
                        {VOLCENGINE_KEY_CONSOLE_URL}
                        <ExternalLink size={12} className="shrink-0" />
                      </button>
                    </p>
                  </div>

                  <div className="grid gap-4 md:grid-cols-2">
                    <div className="space-y-2">
                      <Label htmlFor="usage-volcengine-ak">
                        {t("usageScript.accessKeyId")}
                      </Label>
                      <Input
                        id="usage-volcengine-ak"
                        type="text"
                        value={script.accessKeyId || ""}
                        onChange={(e) =>
                          setScript({
                            ...script,
                            accessKeyId: e.target.value,
                          })
                        }
                        placeholder="AKLT..."
                        autoComplete="off"
                        className="border-white/10"
                      />
                    </div>

                    <div className="space-y-2">
                      <Label htmlFor="usage-volcengine-sk">
                        {t("usageScript.secretAccessKey")}
                      </Label>
                      <div className="relative">
                        <Input
                          id="usage-volcengine-sk"
                          type={showApiKey ? "text" : "password"}
                          value={script.secretAccessKey || ""}
                          onChange={(e) =>
                            setScript({
                              ...script,
                              secretAccessKey: e.target.value,
                            })
                          }
                          placeholder="••••••••"
                          autoComplete="off"
                          className="border-white/10"
                        />
                        {script.secretAccessKey && (
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
                      timeout:
                        e.target.value === ""
                          ? ("" as unknown as number)
                          : Number(e.target.value),
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
                    script.autoQueryInterval ?? script.autoIntervalMinutes ?? 5
                  }
                  onChange={(e) =>
                    setScript({
                      ...script,
                      autoQueryInterval:
                        e.target.value === ""
                          ? ("" as unknown as number)
                          : Number(e.target.value),
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

          {/* 提取器代码 - 专用模板不需要 */}
          {!NATIVE_USAGE_TEMPLATES.has(selectedTemplate || "") && (
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
                onChange={(value) =>
                  setScript((prev) => ({ ...prev, code: value }))
                }
                height={480}
                language="javascript"
                showMinimap={false}
                darkMode={isDarkMode}
              />
            </div>
          )}

          {/* 帮助信息 - 专用模板不需要 */}
          {!NATIVE_USAGE_TEMPLATES.has(selectedTemplate || "") && (
            <div className="glass rounded-xl border border-white/10 p-6 text-sm text-foreground/90">
              <h4 className="font-medium mb-2">
                {t("usageScript.scriptHelp")}
              </h4>
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
          )}
        </div>
      )}

      <ConfirmDialog
        isOpen={showUsageConfirm}
        variant="info"
        title={t("confirm.usage.title")}
        message={t("confirm.usage.message")}
        confirmText={t("confirm.usage.confirm")}
        onConfirm={() => void handleUsageConfirm()}
        onCancel={() => setShowUsageConfirm(false)}
      />
    </FullScreenPanel>
  );
};

export default UsageScriptModal;
