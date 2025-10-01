import React, { useState, useEffect, useRef } from "react";
import { Provider, ProviderCategory } from "../types";
import { AppType } from "../lib/tauri-api";
import {
  updateCommonConfigSnippet,
  hasCommonConfigSnippet,
  getApiKeyFromConfig,
  hasApiKeyField,
  setApiKeyInConfig,
  updateTomlCommonConfigSnippet,
  hasTomlCommonConfigSnippet,
  validateJsonConfig,
  applyTemplateValues,
} from "../utils/providerConfigUtils";
import { providerPresets } from "../config/providerPresets";
import type { TemplateValueConfig } from "../config/providerPresets";
import {
  codexProviderPresets,
  generateThirdPartyAuth,
  generateThirdPartyConfig,
} from "../config/codexProviderPresets";
import PresetSelector from "./ProviderForm/PresetSelector";
import ApiKeyInput from "./ProviderForm/ApiKeyInput";
import ClaudeConfigEditor from "./ProviderForm/ClaudeConfigEditor";
import CodexConfigEditor from "./ProviderForm/CodexConfigEditor";
import KimiModelSelector from "./ProviderForm/KimiModelSelector";
import { X, AlertCircle, Save } from "lucide-react";
import { isLinux } from "../lib/platform";
// 分类仅用于控制少量交互（如官方禁用 API Key），不显示介绍组件

type TemplateValueMap = Record<string, TemplateValueConfig>;

type TemplatePath = Array<string | number>;

const collectTemplatePaths = (
  source: unknown,
  templateKeys: string[],
  currentPath: TemplatePath = [],
  acc: TemplatePath[] = [],
): TemplatePath[] => {
  if (typeof source === "string") {
    const hasPlaceholder = templateKeys.some((key) =>
      source.includes(`\${${key}}`),
    );
    if (hasPlaceholder) {
      acc.push([...currentPath]);
    }
    return acc;
  }

  if (Array.isArray(source)) {
    source.forEach((item, index) =>
      collectTemplatePaths(item, templateKeys, [...currentPath, index], acc),
    );
    return acc;
  }

  if (source && typeof source === "object") {
    Object.entries(source).forEach(([key, value]) =>
      collectTemplatePaths(value, templateKeys, [...currentPath, key], acc),
    );
  }

  return acc;
};

const getValueAtPath = (source: any, path: TemplatePath) => {
  return path.reduce<any>((acc, key) => {
    if (acc === undefined || acc === null) {
      return undefined;
    }
    return acc[key as keyof typeof acc];
  }, source);
};

const setValueAtPath = (
  target: any,
  path: TemplatePath,
  value: unknown,
): any => {
  if (path.length === 0) {
    return value;
  }

  let current = target;

  for (let i = 0; i < path.length - 1; i++) {
    const key = path[i];
    const nextKey = path[i + 1];
    const isNextIndex = typeof nextKey === "number";

    if (current[key as keyof typeof current] === undefined) {
      current[key as keyof typeof current] = isNextIndex ? [] : {};
    } else {
      const currentValue = current[key as keyof typeof current];
      if (isNextIndex && !Array.isArray(currentValue)) {
        current[key as keyof typeof current] = [];
      } else if (
        !isNextIndex &&
        (typeof currentValue !== "object" || currentValue === null)
      ) {
        current[key as keyof typeof current] = {};
      }
    }

    current = current[key as keyof typeof current];
  }

  const finalKey = path[path.length - 1];
  current[finalKey as keyof typeof current] = value;
  return target;
};

const applyTemplateValuesToConfigString = (
  presetConfig: any,
  currentConfigString: string,
  values: TemplateValueMap,
) => {
  const replacedConfig = applyTemplateValues(presetConfig, values);
  const templateKeys = Object.keys(values);
  if (templateKeys.length === 0) {
    return JSON.stringify(replacedConfig, null, 2);
  }

  const placeholderPaths = collectTemplatePaths(presetConfig, templateKeys);

  try {
    const parsedConfig = currentConfigString.trim()
      ? JSON.parse(currentConfigString)
      : {};
    let targetConfig: any;
    if (Array.isArray(parsedConfig)) {
      targetConfig = [...parsedConfig];
    } else if (parsedConfig && typeof parsedConfig === "object") {
      targetConfig = JSON.parse(JSON.stringify(parsedConfig));
    } else {
      targetConfig = {};
    }

    if (placeholderPaths.length === 0) {
      return JSON.stringify(targetConfig, null, 2);
    }

    let mutatedConfig = targetConfig;

    for (const path of placeholderPaths) {
      const nextValue = getValueAtPath(replacedConfig, path);
      if (path.length === 0) {
        mutatedConfig = nextValue;
      } else {
        setValueAtPath(mutatedConfig, path, nextValue);
      }
    }

    return JSON.stringify(mutatedConfig, null, 2);
  } catch {
    return JSON.stringify(replacedConfig, null, 2);
  }
};

const COMMON_CONFIG_STORAGE_KEY = "cc-switch:common-config-snippet";
const CODEX_COMMON_CONFIG_STORAGE_KEY = "cc-switch:codex-common-config-snippet";
const DEFAULT_COMMON_CONFIG_SNIPPET = `{
  "includeCoAuthoredBy": false
}`;
const DEFAULT_CODEX_COMMON_CONFIG_SNIPPET = `# Common Codex config
# Add your common TOML configuration here`;

interface ProviderFormProps {
  appType?: AppType;
  title: string;
  submitText: string;
  initialData?: Provider;
  showPresets?: boolean;
  onSubmit: (data: Omit<Provider, "id">) => void;
  onClose: () => void;
}

const ProviderForm: React.FC<ProviderFormProps> = ({
  appType = "claude",
  title,
  submitText,
  initialData,
  showPresets = false,
  onSubmit,
  onClose,
}) => {
  // 对于 Codex，需要分离 auth 和 config
  const isCodex = appType === "codex";

  const [formData, setFormData] = useState({
    name: initialData?.name || "",
    websiteUrl: initialData?.websiteUrl || "",
    settingsConfig: initialData
      ? JSON.stringify(initialData.settingsConfig, null, 2)
      : "",
  });
  const [category, setCategory] = useState<ProviderCategory | undefined>(
    initialData?.category
  );

  // Claude 模型配置状态
  const [claudeModel, setClaudeModel] = useState("");
  const [claudeSmallFastModel, setClaudeSmallFastModel] = useState("");
  const [baseUrl, setBaseUrl] = useState(""); // 新增：基础 URL 状态
  // 模板变量状态
  const [templateValues, setTemplateValues] =
    useState<Record<string, TemplateValueConfig>>({});

  // Codex 特有的状态
  const [codexAuth, setCodexAuthState] = useState("");
  const [codexConfig, setCodexConfigState] = useState("");
  const [codexApiKey, setCodexApiKey] = useState("");
  const [isCodexTemplateModalOpen, setIsCodexTemplateModalOpen] =
    useState(false);
  // -1 表示自定义，null 表示未选择，>= 0 表示预设索引
  const [selectedCodexPreset, setSelectedCodexPreset] = useState<number | null>(
    showPresets && isCodex ? -1 : null
  );

  const setCodexAuth = (value: string) => {
    setCodexAuthState(value);
    setCodexAuthError(validateCodexAuth(value));
  };

  const setCodexConfig = (value: string) => {
    setCodexConfigState(value);
  };

  const setCodexCommonConfigSnippet = (value: string) => {
    setCodexCommonConfigSnippetState(value);
  };

  // 初始化 Codex 配置
  useEffect(() => {
    if (isCodex && initialData) {
      const config = initialData.settingsConfig;
      if (typeof config === "object" && config !== null) {
        setCodexAuth(JSON.stringify(config.auth || {}, null, 2));
        setCodexConfig(config.config || "");
        try {
          const auth = config.auth || {};
          if (auth && typeof auth.OPENAI_API_KEY === "string") {
            setCodexApiKey(auth.OPENAI_API_KEY);
          }
        } catch {
          // ignore
        }
      }
    }
  }, [isCodex, initialData]);

  const [error, setError] = useState("");
  const [useCommonConfig, setUseCommonConfig] = useState(false);
  const [commonConfigSnippet, setCommonConfigSnippet] = useState<string>(() => {
    if (typeof window === "undefined") {
      return DEFAULT_COMMON_CONFIG_SNIPPET;
    }
    try {
      const stored = window.localStorage.getItem(COMMON_CONFIG_STORAGE_KEY);
      if (stored && stored.trim()) {
        return stored;
      }
    } catch {
      // ignore localStorage 读取失败
    }
    return DEFAULT_COMMON_CONFIG_SNIPPET;
  });
  const [commonConfigError, setCommonConfigError] = useState("");
  const [settingsConfigError, setSettingsConfigError] = useState("");
  // 用于跟踪是否正在通过通用配置更新
  const isUpdatingFromCommonConfig = useRef(false);

  // Codex 通用配置状态
  const [useCodexCommonConfig, setUseCodexCommonConfig] = useState(false);
  const [codexCommonConfigSnippet, setCodexCommonConfigSnippetState] =
    useState<string>(() => {
      if (typeof window === "undefined") {
        return DEFAULT_CODEX_COMMON_CONFIG_SNIPPET.trim();
      }
      try {
        const stored = window.localStorage.getItem(
          CODEX_COMMON_CONFIG_STORAGE_KEY
        );
        if (stored && stored.trim()) {
          return stored.trim();
        }
      } catch {
        // ignore localStorage 读取失败
      }
      return DEFAULT_CODEX_COMMON_CONFIG_SNIPPET.trim();
    });
  const [codexCommonConfigError, setCodexCommonConfigError] = useState("");
  const isUpdatingFromCodexCommonConfig = useRef(false);

  // -1 表示自定义，null 表示未选择，>= 0 表示预设索引
  const [selectedPreset, setSelectedPreset] = useState<number | null>(
    showPresets ? -1 : null
  );
  const [apiKey, setApiKey] = useState("");
  const [codexAuthError, setCodexAuthError] = useState("");

  // Kimi 模型选择状态
  const [kimiAnthropicModel, setKimiAnthropicModel] = useState("");
  const [kimiAnthropicSmallFastModel, setKimiAnthropicSmallFastModel] =
    useState("");

  const validateSettingsConfig = (value: string): string => {
    return validateJsonConfig(value, "配置内容");
  };

  const validateCodexAuth = (value: string): string => {
    if (!value.trim()) {
      return "";
    }
    try {
      const parsed = JSON.parse(value);
      if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
        return "auth.json 必须是 JSON 对象";
      }
      return "";
    } catch {
      return "auth.json 格式错误，请检查JSON语法";
    }
  };

  const updateSettingsConfigValue = (value: string) => {
    setFormData((prev) => ({
      ...prev,
      settingsConfig: value,
    }));
    setSettingsConfigError(validateSettingsConfig(value));
  };

  // 初始化自定义模式的默认配置
  useEffect(() => {
    if (
      showPresets &&
      selectedPreset === -1 &&
      !initialData &&
      formData.settingsConfig === ""
    ) {
      // 设置自定义模板
      const customTemplate = {
        env: {
          ANTHROPIC_BASE_URL: "https://your-api-endpoint.com",
          ANTHROPIC_AUTH_TOKEN: "",
          // 可选配置
          // ANTHROPIC_MODEL: "your-model-name",
          // ANTHROPIC_SMALL_FAST_MODEL: "your-fast-model-name"
        },
      };
      const templateString = JSON.stringify(customTemplate, null, 2);

      updateSettingsConfigValue(templateString);
      setApiKey("");
    }
  }, []); // 只在组件挂载时执行一次

  // 初始化时检查通用配置片段
  useEffect(() => {
    if (initialData) {
      if (!isCodex) {
        const configString = JSON.stringify(
          initialData.settingsConfig,
          null,
          2
        );
        const hasCommon = hasCommonConfigSnippet(
          configString,
          commonConfigSnippet
        );
        setUseCommonConfig(hasCommon);
        setSettingsConfigError(validateSettingsConfig(configString));

        // 初始化模型配置（编辑模式）
        if (
          initialData.settingsConfig &&
          typeof initialData.settingsConfig === "object"
        ) {
          const config = initialData.settingsConfig as {
            env?: Record<string, any>;
          };
          if (config.env) {
            setClaudeModel(config.env.ANTHROPIC_MODEL || "");
            setClaudeSmallFastModel(
              config.env.ANTHROPIC_SMALL_FAST_MODEL || ""
            );
            setBaseUrl(config.env.ANTHROPIC_BASE_URL || ""); // 初始化基础 URL

            // 初始化 Kimi 模型选择
            setKimiAnthropicModel(config.env.ANTHROPIC_MODEL || "");
            setKimiAnthropicSmallFastModel(
              config.env.ANTHROPIC_SMALL_FAST_MODEL || ""
            );
          }
        }
      } else {
        // Codex 初始化时检查 TOML 通用配置
        const hasCommon = hasTomlCommonConfigSnippet(
          codexConfig,
          codexCommonConfigSnippet
        );
        setUseCodexCommonConfig(hasCommon);
      }
    }
  }, [
    initialData,
    commonConfigSnippet,
    codexCommonConfigSnippet,
    isCodex,
    codexConfig,
  ]);

  // 当选择预设变化时，同步类别
  useEffect(() => {
    if (!showPresets) return;
    if (!isCodex) {
      if (selectedPreset !== null && selectedPreset >= 0) {
        const preset = providerPresets[selectedPreset];
        setCategory(
          preset?.category || (preset?.isOfficial ? "official" : undefined)
        );
      } else if (selectedPreset === -1) {
        setCategory("custom");
      }
    } else {
      if (selectedCodexPreset !== null && selectedCodexPreset >= 0) {
        const preset = codexProviderPresets[selectedCodexPreset];
        setCategory(
          preset?.category || (preset?.isOfficial ? "official" : undefined)
        );
      } else if (selectedCodexPreset === -1) {
        setCategory("custom");
      }
    }
  }, [showPresets, isCodex, selectedPreset, selectedCodexPreset]);

  // 同步本地存储的通用配置片段
  useEffect(() => {
    if (typeof window === "undefined") return;
    try {
      if (commonConfigSnippet.trim()) {
        window.localStorage.setItem(
          COMMON_CONFIG_STORAGE_KEY,
          commonConfigSnippet
        );
      } else {
        window.localStorage.removeItem(COMMON_CONFIG_STORAGE_KEY);
      }
    } catch {
      // ignore
    }
  }, [commonConfigSnippet]);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    setError("");

    if (!formData.name) {
      setError("请填写供应商名称");
      return;
    }

    let settingsConfig: Record<string, any>;

    if (isCodex) {
      const currentAuthError = validateCodexAuth(codexAuth);
      setCodexAuthError(currentAuthError);
      if (currentAuthError) {
        setError(currentAuthError);
        return;
      }
      // Codex: 仅要求 auth.json 必填；config.toml 可为空
      if (!codexAuth.trim()) {
        setError("请填写 auth.json 配置");
        return;
      }

      try {
        const authJson = JSON.parse(codexAuth);

        // 非官方预设强制要求 OPENAI_API_KEY
        if (selectedCodexPreset !== null) {
          const preset = codexProviderPresets[selectedCodexPreset];
          const isOfficial = Boolean(preset?.isOfficial);
          if (!isOfficial) {
            const key =
              typeof authJson.OPENAI_API_KEY === "string"
                ? authJson.OPENAI_API_KEY.trim()
                : "";
            if (!key) {
              setError("请填写 OPENAI_API_KEY");
              return;
            }
          }
        }

        settingsConfig = {
          auth: authJson,
          config: codexConfig ?? "",
        };
      } catch (err) {
        setError("auth.json 格式错误，请检查JSON语法");
        return;
      }
    } else {
      const currentSettingsError = validateSettingsConfig(
        formData.settingsConfig
      );
      setSettingsConfigError(currentSettingsError);
      if (currentSettingsError) {
        setError(currentSettingsError);
        return;
      }

      if (selectedTemplatePreset && templateValueEntries.length > 0) {
        for (const [key, config] of templateValueEntries) {
          const entry = templateValues[key];
          const resolvedValue = (
            entry?.editorValue ??
            entry?.defaultValue ??
            config.defaultValue ??
            ""
          ).trim();
          if (!resolvedValue) {
            setError(`请填写 ${config.label}`);
            return;
          }
        }
      }
      // Claude: 原有逻辑
      if (!formData.settingsConfig.trim()) {
        setError("请填写配置内容");
        return;
      }

      try {
        settingsConfig = JSON.parse(formData.settingsConfig);
      } catch (err) {
        setError("配置JSON格式错误，请检查语法");
        return;
      }
    }

    onSubmit({
      name: formData.name,
      websiteUrl: formData.websiteUrl,
      settingsConfig,
      // 仅在用户选择了预设或手动选择“自定义”时持久化分类
      ...(category ? { category } : {}),
    });
  };

  const handleChange = (
    e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>
  ) => {
    const { name, value } = e.target;

    if (name === "settingsConfig") {
      // 只有在不是通过通用配置更新时，才检查并同步选择框状态
      if (!isUpdatingFromCommonConfig.current) {
        const hasCommon = hasCommonConfigSnippet(value, commonConfigSnippet);
        setUseCommonConfig(hasCommon);
      }

      // 同步 API Key 输入框显示与值
      const parsedKey = getApiKeyFromConfig(value);
      setApiKey(parsedKey);

      // 不再从 JSON 自动提取或覆盖官网地址，只更新配置内容
      updateSettingsConfigValue(value);
    } else {
      setFormData((prev) => ({
        ...prev,
        [name]: value,
      }));
    }
  };

  // 处理通用配置开关
  const handleCommonConfigToggle = (checked: boolean) => {
    const { updatedConfig, error: snippetError } = updateCommonConfigSnippet(
      formData.settingsConfig,
      commonConfigSnippet,
      checked
    );

    if (snippetError) {
      setCommonConfigError(snippetError);
      if (snippetError.includes("配置 JSON 解析失败")) {
        setSettingsConfigError("配置JSON格式错误，请检查语法");
      }
      setUseCommonConfig(false);
      return;
    }

    setCommonConfigError("");
    setUseCommonConfig(checked);
    // 标记正在通过通用配置更新
    isUpdatingFromCommonConfig.current = true;
    updateSettingsConfigValue(updatedConfig);
    // 在下一个事件循环中重置标记
    setTimeout(() => {
      isUpdatingFromCommonConfig.current = false;
    }, 0);
  };

  const handleCommonConfigSnippetChange = (value: string) => {
    const previousSnippet = commonConfigSnippet;
    setCommonConfigSnippet(value);

    if (!value.trim()) {
      setCommonConfigError("");
      if (useCommonConfig) {
        const { updatedConfig } = updateCommonConfigSnippet(
          formData.settingsConfig,
          previousSnippet,
          false
        );
        // 直接更新 formData，不通过 handleChange
        updateSettingsConfigValue(updatedConfig);
        setUseCommonConfig(false);
      }
      return;
    }

    // 验证JSON格式
    const validationError = validateJsonConfig(value, "通用配置片段");
    if (validationError) {
      setCommonConfigError(validationError);
    } else {
      setCommonConfigError("");
    }

    // 若当前启用通用配置且格式正确，需要替换为最新片段
    if (useCommonConfig && !validationError) {
      const removeResult = updateCommonConfigSnippet(
        formData.settingsConfig,
        previousSnippet,
        false
      );
      if (removeResult.error) {
        setCommonConfigError(removeResult.error);
        if (removeResult.error.includes("配置 JSON 解析失败")) {
          setSettingsConfigError("配置JSON格式错误，请检查语法");
        }
        return;
      }
      const addResult = updateCommonConfigSnippet(
        removeResult.updatedConfig,
        value,
        true
      );

      if (addResult.error) {
        setCommonConfigError(addResult.error);
        if (addResult.error.includes("配置 JSON 解析失败")) {
          setSettingsConfigError("配置JSON格式错误，请检查语法");
        }
        return;
      }

      // 标记正在通过通用配置更新，避免触发状态检查
      isUpdatingFromCommonConfig.current = true;
      updateSettingsConfigValue(addResult.updatedConfig);
      // 在下一个事件循环中重置标记
      setTimeout(() => {
        isUpdatingFromCommonConfig.current = false;
      }, 0);
    }

    // 保存通用配置到 localStorage
    if (!validationError && typeof window !== "undefined") {
      try {
        window.localStorage.setItem(COMMON_CONFIG_STORAGE_KEY, value);
      } catch {
        // ignore localStorage 写入失败
      }
    }
  };

  const applyPreset = (preset: (typeof providerPresets)[0], index: number) => {
    let appliedSettingsConfig = preset.settingsConfig;
    let initialTemplateValues: TemplateValueMap = {};

    if (preset.templateValues) {
      initialTemplateValues = Object.fromEntries(
        Object.entries(preset.templateValues).map(([key, config]) => [
          key,
          {
            ...config,
            editorValue: config.editorValue
              ? config.editorValue
              : config.defaultValue ?? "",
          },
        ])
      );
      appliedSettingsConfig = applyTemplateValues(
        preset.settingsConfig,
        initialTemplateValues
      );
    }

    setTemplateValues(initialTemplateValues);

    const configString = JSON.stringify(appliedSettingsConfig, null, 2);

    setFormData({
      name: preset.name,
      websiteUrl: preset.websiteUrl,
      settingsConfig: configString,
    });
    setSettingsConfigError(validateSettingsConfig(configString));
    setCategory(
      preset.category || (preset.isOfficial ? "official" : undefined)
    );

    // 设置选中的预设
    setSelectedPreset(index);

    // 清空 API Key 输入框，让用户重新输入
    setApiKey("");
    setBaseUrl(""); // 清空基础 URL

    // 同步通用配置状态
    const hasCommon = hasCommonConfigSnippet(configString, commonConfigSnippet);
    setUseCommonConfig(hasCommon);
    setCommonConfigError("");

    // 如果预设包含模型配置，初始化模型输入框
    if (appliedSettingsConfig && typeof appliedSettingsConfig === "object") {
      const config = appliedSettingsConfig as { env?: Record<string, any> };
      if (config.env) {
        setClaudeModel(config.env.ANTHROPIC_MODEL || "");
        setClaudeSmallFastModel(config.env.ANTHROPIC_SMALL_FAST_MODEL || "");

        // 如果是 Kimi 预设，同步 Kimi 模型选择
        if (preset.name?.includes("Kimi")) {
          setKimiAnthropicModel(config.env.ANTHROPIC_MODEL || "");
          setKimiAnthropicSmallFastModel(
            config.env.ANTHROPIC_SMALL_FAST_MODEL || ""
          );
        }
      } else {
        setClaudeModel("");
        setClaudeSmallFastModel("");
      }
    }
  };

  // 处理点击自定义按钮
  const handleCustomClick = () => {
    setSelectedPreset(-1);
    setTemplateValues({});

    // 设置自定义模板
    const customTemplate = {
      env: {
        ANTHROPIC_BASE_URL: "https://your-api-endpoint.com",
        ANTHROPIC_AUTH_TOKEN: "",
        // 可选配置
        // ANTHROPIC_MODEL: "your-model-name",
        // ANTHROPIC_SMALL_FAST_MODEL: "your-fast-model-name"
      },
    };
    const templateString = JSON.stringify(customTemplate, null, 2);

    setFormData({
      name: "",
      websiteUrl: "",
      settingsConfig: templateString,
    });
    setSettingsConfigError(validateSettingsConfig(templateString));
    setApiKey("");
    setBaseUrl("https://your-api-endpoint.com"); // 设置默认的基础 URL
    setUseCommonConfig(false);
    setCommonConfigError("");
    setClaudeModel("");
    setClaudeSmallFastModel("");
    setKimiAnthropicModel("");
    setKimiAnthropicSmallFastModel("");
    setCategory("custom");
  };

  // Codex: 应用预设
  const applyCodexPreset = (
    preset: (typeof codexProviderPresets)[0],
    index: number
  ) => {
    const authString = JSON.stringify(preset.auth || {}, null, 2);
    setCodexAuth(authString);
    setCodexConfig(preset.config || "");

    setFormData((prev) => ({
      ...prev,
      name: preset.name,
      websiteUrl: preset.websiteUrl,
    }));

    setSelectedCodexPreset(index);
    setCategory(
      preset.category || (preset.isOfficial ? "official" : undefined)
    );

    // 清空 API Key，让用户重新输入
    setCodexApiKey("");
  };

  // Codex: 处理点击自定义按钮
  const handleCodexCustomClick = () => {
    setSelectedCodexPreset(-1);

    // 设置自定义模板
    const customAuth = generateThirdPartyAuth("");
    const customConfig = generateThirdPartyConfig(
      "custom",
      "https://your-api-endpoint.com/v1",
      "gpt-5-codex"
    );

    setFormData({
      name: "",
      websiteUrl: "",
      settingsConfig: "",
    });
    setSettingsConfigError(validateSettingsConfig(""));
    setCodexAuth(JSON.stringify(customAuth, null, 2));
    setCodexConfig(customConfig);
    setCodexApiKey("");
    setCategory("custom");
  };

  // 处理 API Key 输入并自动更新配置
  const handleApiKeyChange = (key: string) => {
    setApiKey(key);

    const configString = setApiKeyInConfig(
      formData.settingsConfig,
      key.trim(),
      { createIfMissing: selectedPreset !== null && selectedPreset !== -1 }
    );

    // 更新表单配置
    updateSettingsConfigValue(configString);

    // 同步通用配置开关
    const hasCommon = hasCommonConfigSnippet(configString, commonConfigSnippet);
    setUseCommonConfig(hasCommon);
  };

  // 处理基础 URL 变化
  const handleBaseUrlChange = (url: string) => {
    setBaseUrl(url);

    try {
      const config = JSON.parse(formData.settingsConfig || "{}");
      if (!config.env) {
        config.env = {};
      }
      config.env.ANTHROPIC_BASE_URL = url.trim();

      updateSettingsConfigValue(JSON.stringify(config, null, 2));
    } catch {
      // ignore
    }
  };

  // Codex: 处理 API Key 输入并写回 auth.json
  const handleCodexApiKeyChange = (key: string) => {
    setCodexApiKey(key);
    try {
      const auth = JSON.parse(codexAuth || "{}");
      auth.OPENAI_API_KEY = key.trim();
      setCodexAuth(JSON.stringify(auth, null, 2));
    } catch {
      // ignore
    }
  };

  // Codex: 处理通用配置开关
  const handleCodexCommonConfigToggle = (checked: boolean) => {
    const snippet = codexCommonConfigSnippet.trim();
    const { updatedConfig, error: snippetError } =
      updateTomlCommonConfigSnippet(codexConfig, snippet, checked);

    if (snippetError) {
      setCodexCommonConfigError(snippetError);
      setUseCodexCommonConfig(false);
      return;
    }

    setCodexCommonConfigError("");
    setUseCodexCommonConfig(checked);
    // 标记正在通过通用配置更新
    isUpdatingFromCodexCommonConfig.current = true;
    setCodexConfig(updatedConfig);
    // 在下一个事件循环中重置标记
    setTimeout(() => {
      isUpdatingFromCodexCommonConfig.current = false;
    }, 0);
  };

  // Codex: 处理通用配置片段变化
  const handleCodexCommonConfigSnippetChange = (value: string) => {
    const previousSnippet = codexCommonConfigSnippet.trim();
    const sanitizedValue = value.trim();
    setCodexCommonConfigSnippet(value);

    if (!sanitizedValue) {
      setCodexCommonConfigError("");
      if (useCodexCommonConfig) {
        const { updatedConfig } = updateTomlCommonConfigSnippet(
          codexConfig,
          previousSnippet,
          false
        );
        setCodexConfig(updatedConfig);
        setUseCodexCommonConfig(false);
      }
      return;
    }

    // TOML 不需要验证 JSON 格式，直接更新
    if (useCodexCommonConfig) {
      const removeResult = updateTomlCommonConfigSnippet(
        codexConfig,
        previousSnippet,
        false
      );
      const addResult = updateTomlCommonConfigSnippet(
        removeResult.updatedConfig,
        sanitizedValue,
        true
      );

      if (addResult.error) {
        setCodexCommonConfigError(addResult.error);
        return;
      }

      // 标记正在通过通用配置更新
      isUpdatingFromCodexCommonConfig.current = true;
      setCodexConfig(addResult.updatedConfig);
      // 在下一个事件循环中重置标记
      setTimeout(() => {
        isUpdatingFromCodexCommonConfig.current = false;
      }, 0);
    }

    // 保存 Codex 通用配置到 localStorage
    if (typeof window !== "undefined") {
      try {
        window.localStorage.setItem(
          CODEX_COMMON_CONFIG_STORAGE_KEY,
          sanitizedValue
        );
      } catch {
        // ignore localStorage 写入失败
      }
    }
  };

  // Codex: 处理 config 变化
  const handleCodexConfigChange = (value: string) => {
    if (!isUpdatingFromCodexCommonConfig.current) {
      const hasCommon = hasTomlCommonConfigSnippet(
        value,
        codexCommonConfigSnippet
      );
      setUseCodexCommonConfig(hasCommon);
    }
    setCodexConfig(value);
  };

  // 根据当前配置决定是否展示 API Key 输入框
  // 自定义模式(-1)也需要显示 API Key 输入框
  const showApiKey =
    selectedPreset !== null ||
    (!showPresets && hasApiKeyField(formData.settingsConfig));

  const selectedTemplatePreset =
    !isCodex &&
    selectedPreset !== null &&
    selectedPreset >= 0 &&
    selectedPreset < providerPresets.length
      ? providerPresets[selectedPreset]
      : null;

  const templateValueEntries: Array<[string, TemplateValueConfig]> =
    selectedTemplatePreset?.templateValues
      ? (Object.entries(
          selectedTemplatePreset.templateValues
        ) as Array<[string, TemplateValueConfig]>)
      : [];

  // 判断当前选中的预设是否是官方
  const isOfficialPreset =
    (selectedPreset !== null &&
      selectedPreset >= 0 &&
      (providerPresets[selectedPreset]?.isOfficial === true ||
        providerPresets[selectedPreset]?.category === "official")) ||
    category === "official";

  // 判断当前选中的预设是否是 Kimi
  const isKimiPreset =
    selectedPreset !== null &&
    selectedPreset >= 0 &&
    providerPresets[selectedPreset]?.name?.includes("Kimi");

  // 判断当前编辑的是否是 Kimi 提供商（通过名称或配置判断）
  const isEditingKimi =
    initialData &&
    (formData.name.includes("Kimi") ||
      formData.name.includes("kimi") ||
      (formData.settingsConfig.includes("api.moonshot.cn") &&
        formData.settingsConfig.includes("ANTHROPIC_MODEL")));

  // 综合判断是否应该显示 Kimi 模型选择器
  const shouldShowKimiSelector = isKimiPreset || isEditingKimi;

  // 判断是否显示基础 URL 输入框（仅自定义模式显示）
  const showBaseUrlInput = selectedPreset === -1 && !isCodex;

  // 判断是否显示"获取 API Key"链接（国产官方、聚合站和第三方显示）
  const shouldShowApiKeyLink =
    !isCodex &&
    !isOfficialPreset &&
    (category === "cn_official" ||
      category === "aggregator" ||
      category === "third_party" ||
      (selectedPreset !== null &&
        selectedPreset >= 0 &&
        (providerPresets[selectedPreset]?.category === "cn_official" ||
          providerPresets[selectedPreset]?.category === "aggregator" ||
          providerPresets[selectedPreset]?.category === "third_party")));

  // 获取当前供应商的网址
  const getCurrentWebsiteUrl = () => {
    if (selectedPreset !== null && selectedPreset >= 0) {
      const preset = providerPresets[selectedPreset];
      if (!preset) return "";
      // 仅第三方供应商使用专用 apiKeyUrl，其余使用官网地址
      return preset.category === "third_party"
        ? preset.apiKeyUrl || preset.websiteUrl || ""
        : preset.websiteUrl || "";
    }
    return formData.websiteUrl || "";
  };

  // 获取 Codex 当前供应商的网址
  const getCurrentCodexWebsiteUrl = () => {
    if (selectedCodexPreset !== null && selectedCodexPreset >= 0) {
      const preset = codexProviderPresets[selectedCodexPreset];
      if (!preset) return "";
      // 仅第三方供应商使用专用 apiKeyUrl，其余使用官网地址
      return preset.category === "third_party"
        ? preset.apiKeyUrl || preset.websiteUrl || ""
        : preset.websiteUrl || "";
    }
    return formData.websiteUrl || "";
  };

  // Codex: 控制显示 API Key 与官方标记
  const getCodexAuthApiKey = (authString: string): string => {
    try {
      const auth = JSON.parse(authString || "{}");
      return typeof auth.OPENAI_API_KEY === "string" ? auth.OPENAI_API_KEY : "";
    } catch {
      return "";
    }
  };

  // 自定义模式(-1)不显示独立的 API Key 输入框
  const showCodexApiKey =
    (selectedCodexPreset !== null && selectedCodexPreset !== -1) ||
    (!showPresets && getCodexAuthApiKey(codexAuth) !== "");

  // 不再渲染分类介绍组件，避免造成干扰

  const isCodexOfficialPreset =
    (selectedCodexPreset !== null &&
      selectedCodexPreset >= 0 &&
      (codexProviderPresets[selectedCodexPreset]?.isOfficial === true ||
        codexProviderPresets[selectedCodexPreset]?.category === "official")) ||
    category === "official";

  // 判断是否显示 Codex 的"获取 API Key"链接（国产官方、聚合站和第三方显示）
  const shouldShowCodexApiKeyLink =
    isCodex &&
    !isCodexOfficialPreset &&
    (category === "cn_official" ||
      category === "aggregator" ||
      category === "third_party" ||
      (selectedCodexPreset !== null &&
        selectedCodexPreset >= 0 &&
        (codexProviderPresets[selectedCodexPreset]?.category ===
          "cn_official" ||
          codexProviderPresets[selectedCodexPreset]?.category ===
            "aggregator" ||
          codexProviderPresets[selectedCodexPreset]?.category ===
            "third_party")));

  // 处理模型输入变化，自动更新 JSON 配置
  const handleModelChange = (
    field: "ANTHROPIC_MODEL" | "ANTHROPIC_SMALL_FAST_MODEL",
    value: string
  ) => {
    if (field === "ANTHROPIC_MODEL") {
      setClaudeModel(value);
    } else {
      setClaudeSmallFastModel(value);
    }

    // 更新 JSON 配置
    try {
      const currentConfig = formData.settingsConfig
        ? JSON.parse(formData.settingsConfig)
        : { env: {} };
      if (!currentConfig.env) currentConfig.env = {};

      if (value.trim()) {
        currentConfig.env[field] = value.trim();
      } else {
        delete currentConfig.env[field];
      }

      updateSettingsConfigValue(JSON.stringify(currentConfig, null, 2));
    } catch (err) {
      // 如果 JSON 解析失败，不做处理
    }
  };

  // Kimi 模型选择处理函数
  const handleKimiModelChange = (
    field: "ANTHROPIC_MODEL" | "ANTHROPIC_SMALL_FAST_MODEL",
    value: string
  ) => {
    if (field === "ANTHROPIC_MODEL") {
      setKimiAnthropicModel(value);
    } else {
      setKimiAnthropicSmallFastModel(value);
    }

    // 更新配置 JSON
    try {
      const currentConfig = JSON.parse(formData.settingsConfig || "{}");
      if (!currentConfig.env) currentConfig.env = {};
      currentConfig.env[field] = value;

      const updatedConfigString = JSON.stringify(currentConfig, null, 2);
      updateSettingsConfigValue(updatedConfigString);
    } catch (err) {
      console.error("更新 Kimi 模型配置失败:", err);
    }
  };

  // 初始时从配置中同步 API Key（编辑模式）
  useEffect(() => {
    if (!initialData) return;
    const parsedKey = getApiKeyFromConfig(
      JSON.stringify(initialData.settingsConfig)
    );
    if (parsedKey) setApiKey(parsedKey);
  }, [initialData]);

  // 支持按下 ESC 关闭弹窗
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      {/* Backdrop */}
      <div
        className={`absolute inset-0 bg-black/50 dark:bg-black/70${
          isLinux() ? "" : " backdrop-blur-sm"
        }`}
      />

      {/* Modal */}
      <div className="relative bg-white dark:bg-gray-900 rounded-xl shadow-lg max-w-3xl w-full mx-4 max-h-[90vh] overflow-hidden flex flex-col">
        {/* Header */}
        <div className="flex items-center justify-between p-6 border-b border-gray-200 dark:border-gray-800">
          <h2 className="text-xl font-semibold text-gray-900 dark:text-gray-100">
            {title}
          </h2>
          <button
            type="button"
            onClick={onClose}
            className="p-1 text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-100 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-md transition-colors"
            aria-label="关闭"
          >
            <X size={18} />
          </button>
        </div>

        <form onSubmit={handleSubmit} className="flex flex-col flex-1 min-h-0">
          <div className="flex-1 overflow-auto p-6 space-y-6">
            {error && (
              <div className="flex items-center gap-3 p-4 bg-red-100 dark:bg-red-900/20 border border-red-500/20 dark:border-red-500/30 rounded-lg">
                <AlertCircle
                  size={20}
                  className="text-red-500 dark:text-red-400 flex-shrink-0"
                />
                <p className="text-red-500 dark:text-red-400 text-sm font-medium">
                  {error}
                </p>
              </div>
            )}

            {showPresets && !isCodex && (
              <PresetSelector
                presets={providerPresets}
                selectedIndex={selectedPreset}
                onSelectPreset={(index) =>
                  applyPreset(providerPresets[index], index)
                }
                onCustomClick={handleCustomClick}
              />
            )}

            {showPresets && isCodex && (
              <PresetSelector
                presets={codexProviderPresets}
                selectedIndex={selectedCodexPreset}
                onSelectPreset={(index) =>
                  applyCodexPreset(codexProviderPresets[index], index)
                }
                onCustomClick={handleCodexCustomClick}
                renderCustomDescription={() => (
                  <>
                    手动配置供应商，需要填写完整的配置信息，或者
                    <button
                      type="button"
                      onClick={() => setIsCodexTemplateModalOpen(true)}
                      className="text-blue-400 dark:text-blue-500 hover:text-blue-500 dark:hover:text-blue-400 transition-colors ml-1"
                    >
                      使用配置向导
                    </button>
                  </>
                )}
              />
            )}

            <div className="space-y-2">
              <label
                htmlFor="name"
                className="block text-sm font-medium text-gray-900 dark:text-gray-100"
              >
                供应商名称 *
              </label>
              <input
                type="text"
                id="name"
                name="name"
                value={formData.name}
                onChange={handleChange}
                placeholder="例如：Anthropic 官方"
                required
                autoComplete="off"
                className="w-full px-3 py-2 border border-gray-200 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100 rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-blue-500/20 dark:focus:ring-blue-400/20 focus:border-blue-500 dark:focus:border-blue-400 transition-colors"
              />
            </div>

            <div className="space-y-2">
              <label
                htmlFor="websiteUrl"
                className="block text-sm font-medium text-gray-900 dark:text-gray-100"
              >
                官网地址
              </label>
              <input
                type="url"
                id="websiteUrl"
                name="websiteUrl"
                value={formData.websiteUrl}
                onChange={handleChange}
                placeholder="https://example.com（可选）"
                autoComplete="off"
                className="w-full px-3 py-2 border border-gray-200 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100 rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-blue-500/20 dark:focus:ring-blue-400/20 focus:border-blue-500 dark:focus:border-blue-400 transition-colors"
              />
            </div>

            {!isCodex && showApiKey && (
              <div className="space-y-1">
                <ApiKeyInput
                  value={apiKey}
                  onChange={handleApiKeyChange}
                  required={!isOfficialPreset}
                  placeholder={
                    isOfficialPreset
                      ? "官方登录无需填写 API Key，直接保存即可"
                      : shouldShowKimiSelector
                        ? "填写后可获取模型列表"
                        : "只需要填这里，下方配置会自动填充"
                  }
                  disabled={isOfficialPreset}
                />
                {shouldShowApiKeyLink && getCurrentWebsiteUrl() && (
                  <div className="-mt-1 pl-1">
                    <a
                      href={getCurrentWebsiteUrl()}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="text-xs text-blue-400 dark:text-blue-500 hover:text-blue-500 dark:hover:text-blue-400 transition-colors"
                    >
                      获取 API Key
                    </a>
                  </div>
                )}
              </div>
            )}

            {!isCodex && selectedTemplatePreset && templateValueEntries.length > 0 && (
              <div className="space-y-3">
                <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100">
                  参数配置 - {selectedTemplatePreset.name.trim()} *
                </h3>
                <div className="space-y-4">
                  {templateValueEntries.map(([key, config]) => (
                    <div key={key} className="space-y-2">
                      <label className="sr-only" htmlFor={`template-${key}`}>
                        {config.label}
                      </label>
                      <input
                        id={`template-${key}`}
                        type="text"
                        required
                        placeholder={`${config.label} *`}
                        value={
                          templateValues[key]?.editorValue ??
                          config.editorValue ??
                          config.defaultValue ??
                          ""
                        }
                        onChange={(e) => {
                          const newValue = e.target.value;
                          setTemplateValues((prev) => {
                            const prevEntry = prev[key];
                            const nextEntry: TemplateValueConfig = {
                              ...config,
                              ...(prevEntry ?? {}),
                              editorValue: newValue,
                            };
                            const nextValues: TemplateValueMap = {
                              ...prev,
                              [key]: nextEntry,
                            };

                            if (selectedTemplatePreset) {
                              try {
                                const configString = applyTemplateValuesToConfigString(
                                  selectedTemplatePreset.settingsConfig,
                                  formData.settingsConfig,
                                  nextValues
                                );
                                setFormData((prevForm) => ({
                                  ...prevForm,
                                  settingsConfig: configString,
                                }));
                                setSettingsConfigError(
                                  validateSettingsConfig(configString)
                                );
                              } catch (err) {
                                console.error("更新模板值失败:", err);
                              }
                            }

                            return nextValues;
                          });
                        }}
                        aria-label={config.label}
                        autoComplete="off"
                        className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md bg-white dark:bg-gray-800 text-gray-900 dark:text-gray-100 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
                      />
                    </div>
                  ))}
                </div>
              </div>
            )}

            {/* 基础 URL 输入框 - 仅在自定义模式下显示 */}
            {!isCodex && showBaseUrlInput && (
              <div className="space-y-2">
                <label
                  htmlFor="baseUrl"
                  className="block text-sm font-medium text-gray-900 dark:text-gray-100"
                >
                  请求地址
                </label>
                <input
                  type="url"
                  id="baseUrl"
                  value={baseUrl}
                  onChange={(e) => handleBaseUrlChange(e.target.value)}
                  placeholder="https://your-api-endpoint.com"
                  autoComplete="off"
                  className="w-full px-3 py-2 border border-gray-200 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100 rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-blue-500/20 dark:focus:ring-blue-400/20 focus:border-blue-500 dark:focus:border-blue-400 transition-colors"
                />
                <div className="p-3 bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-700 rounded-lg">
                  <p className="text-xs text-amber-600 dark:text-amber-400">
                    💡 填写兼容 Claude API 的服务端点地址
                  </p>
                </div>
              </div>
            )}

            {!isCodex && shouldShowKimiSelector && (
              <KimiModelSelector
                apiKey={apiKey}
                anthropicModel={kimiAnthropicModel}
                anthropicSmallFastModel={kimiAnthropicSmallFastModel}
                onModelChange={handleKimiModelChange}
                disabled={isOfficialPreset}
              />
            )}

            {isCodex && showCodexApiKey && (
              <div className="space-y-1">
                <ApiKeyInput
                  id="codexApiKey"
                  label="API Key"
                  value={codexApiKey}
                  onChange={handleCodexApiKeyChange}
                  placeholder={
                    isCodexOfficialPreset
                      ? "官方无需填写 API Key，直接保存即可"
                      : "只需要填这里，下方 auth.json 会自动填充"
                  }
                  disabled={isCodexOfficialPreset}
                  required={
                    selectedCodexPreset !== null &&
                    selectedCodexPreset >= 0 &&
                    !isCodexOfficialPreset
                  }
                />
                {shouldShowCodexApiKeyLink && getCurrentCodexWebsiteUrl() && (
                  <div className="-mt-1 pl-1">
                    <a
                      href={getCurrentCodexWebsiteUrl()}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="text-xs text-blue-400 dark:text-blue-500 hover:text-blue-500 dark:hover:text-blue-400 transition-colors"
                    >
                      获取 API Key
                    </a>
                  </div>
                )}
              </div>
            )}

            {/* Claude 或 Codex 的配置部分 */}
            {isCodex ? (
              <CodexConfigEditor
                authValue={codexAuth}
                configValue={codexConfig}
                onAuthChange={setCodexAuth}
                onConfigChange={handleCodexConfigChange}
                onAuthBlur={() => {
                  try {
                    const auth = JSON.parse(codexAuth || "{}");
                    const key =
                      typeof auth.OPENAI_API_KEY === "string"
                        ? auth.OPENAI_API_KEY
                        : "";
                    setCodexApiKey(key);
                  } catch {
                    // ignore
                  }
                }}
                useCommonConfig={useCodexCommonConfig}
                onCommonConfigToggle={handleCodexCommonConfigToggle}
                commonConfigSnippet={codexCommonConfigSnippet}
                onCommonConfigSnippetChange={
                  handleCodexCommonConfigSnippetChange
                }
                commonConfigError={codexCommonConfigError}
                authError={codexAuthError}
                isCustomMode={selectedCodexPreset === -1}
                onWebsiteUrlChange={(url) => {
                  setFormData((prev) => ({
                    ...prev,
                    websiteUrl: url,
                  }));
                }}
                onNameChange={(name) => {
                  setFormData((prev) => ({
                    ...prev,
                    name,
                  }));
                }}
                isTemplateModalOpen={isCodexTemplateModalOpen}
                setIsTemplateModalOpen={setIsCodexTemplateModalOpen}
              />
            ) : (
              <>
                {/* 可选的模型配置输入框 - 仅在非官方且非 Kimi 时显示 */}
                {!isOfficialPreset && !shouldShowKimiSelector && (
                  <div className="space-y-4">
                    <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                      <div className="space-y-2">
                        <label
                          htmlFor="anthropicModel"
                          className="block text-sm font-medium text-gray-900 dark:text-gray-100"
                        >
                          主模型 (可选)
                        </label>
                        <input
                          type="text"
                          id="anthropicModel"
                          value={claudeModel}
                          onChange={(e) =>
                            handleModelChange("ANTHROPIC_MODEL", e.target.value)
                          }
                          placeholder="例如: GLM-4.5"
                          autoComplete="off"
                          className="w-full px-3 py-2 border border-gray-200 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100 rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-blue-500/20 dark:focus:ring-blue-400/20 focus:border-blue-500 dark:focus:border-blue-400 transition-colors"
                        />
                      </div>

                      <div className="space-y-2">
                        <label
                          htmlFor="anthropicSmallFastModel"
                          className="block text-sm font-medium text-gray-900 dark:text-gray-100"
                        >
                          快速模型 (可选)
                        </label>
                        <input
                          type="text"
                          id="anthropicSmallFastModel"
                          value={claudeSmallFastModel}
                          onChange={(e) =>
                            handleModelChange(
                              "ANTHROPIC_SMALL_FAST_MODEL",
                              e.target.value
                            )
                          }
                          placeholder="例如: GLM-4.5-Air"
                          autoComplete="off"
                          className="w-full px-3 py-2 border border-gray-200 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100 rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-blue-500/20 dark:focus:ring-blue-400/20 focus:border-blue-500 dark:focus:border-blue-400 transition-colors"
                        />
                      </div>
                    </div>

                    <div className="p-3 bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-700 rounded-lg">
                      <p className="text-xs text-amber-600 dark:text-amber-400">
                        💡 留空将使用供应商的默认模型
                      </p>
                    </div>
                  </div>
                )}

                <ClaudeConfigEditor
                  value={formData.settingsConfig}
                  onChange={(value) =>
                    handleChange({
                      target: { name: "settingsConfig", value },
                    } as React.ChangeEvent<HTMLTextAreaElement>)
                  }
                  useCommonConfig={useCommonConfig}
                  onCommonConfigToggle={handleCommonConfigToggle}
                  commonConfigSnippet={commonConfigSnippet}
                  onCommonConfigSnippetChange={handleCommonConfigSnippetChange}
                  commonConfigError={commonConfigError}
                  configError={settingsConfigError}
                />
              </>
            )}
          </div>

          {/* Footer */}
          <div className="flex items-center justify-end gap-3 p-6 border-t border-gray-200 dark:border-gray-800 bg-gray-100 dark:bg-gray-800">
            <button
              type="button"
              onClick={onClose}
              className="px-4 py-2 text-sm font-medium text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-100 hover:bg-white dark:hover:bg-gray-700 rounded-lg transition-colors"
            >
              取消
            </button>
            <button
              type="submit"
              className="px-4 py-2 bg-blue-500 dark:bg-blue-600 text-white rounded-lg hover:bg-blue-600 dark:hover:bg-blue-700 transition-colors text-sm font-medium flex items-center gap-2"
            >
              <Save className="w-4 h-4" />
              {submitText}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
};

export default ProviderForm;
