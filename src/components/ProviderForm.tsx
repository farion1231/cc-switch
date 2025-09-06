import React, { useState, useEffect } from "react";
import { Provider } from "../types";
import { AppType } from "../lib/tauri-api";
import {
  updateCoAuthoredSetting,
  checkCoAuthoredSetting,
  getApiKeyFromConfig,
  hasApiKeyField,
  setApiKeyInConfig,
} from "../utils/providerConfigUtils";
import { providerPresets } from "../config/providerPresets";
import { codexProviderPresets } from "../config/codexProviderPresets";
import JsonEditor from "./JsonEditor";
import { X, AlertCircle, Save, Zap } from "lucide-react";

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

  // Codex 特有的状态
  const [codexAuth, setCodexAuth] = useState("");
  const [codexConfig, setCodexConfig] = useState("");
  const [codexApiKey, setCodexApiKey] = useState("");
  // -1 表示自定义，null 表示未选择，>= 0 表示预设索引
  const [selectedCodexPreset, setSelectedCodexPreset] = useState<number | null>(
    showPresets && isCodex ? -1 : null
  );

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
  const [disableCoAuthored, setDisableCoAuthored] = useState(false);
  // -1 表示自定义，null 表示未选择，>= 0 表示预设索引
  const [selectedPreset, setSelectedPreset] = useState<number | null>(
    showPresets ? -1 : null
  );
  const [apiKey, setApiKey] = useState("");

  // 初始化时检查禁用签名状态
  useEffect(() => {
    if (initialData) {
      const configString = JSON.stringify(initialData.settingsConfig, null, 2);
      const hasCoAuthoredDisabled = checkCoAuthoredSetting(configString);
      setDisableCoAuthored(hasCoAuthoredDisabled);
    }
  }, [initialData]);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    setError("");

    if (!formData.name) {
      setError("请填写供应商名称");
      return;
    }

    let settingsConfig: Record<string, any>;

    if (isCodex) {
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
    });
  };

  const handleChange = (
    e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>
  ) => {
    const { name, value } = e.target;

    if (name === "settingsConfig") {
      // 同时检查并同步选择框状态
      const hasCoAuthoredDisabled = checkCoAuthoredSetting(value);
      setDisableCoAuthored(hasCoAuthoredDisabled);

      // 同步 API Key 输入框显示与值
      const parsedKey = getApiKeyFromConfig(value);
      setApiKey(parsedKey);

      // 不再从 JSON 自动提取或覆盖官网地址，只更新配置内容
      setFormData((prev) => ({
        ...prev,
        [name]: value,
      }));
    } else {
      setFormData({
        ...formData,
        [name]: value,
      });
    }
  };

  // 处理选择框变化
  const handleCoAuthoredToggle = (checked: boolean) => {
    setDisableCoAuthored(checked);

    // 更新JSON配置
    const updatedConfig = updateCoAuthoredSetting(
      formData.settingsConfig,
      checked
    );
    setFormData({
      ...formData,
      settingsConfig: updatedConfig,
    });
  };

  const applyPreset = (preset: (typeof providerPresets)[0], index: number) => {
    const configString = JSON.stringify(preset.settingsConfig, null, 2);

    setFormData({
      name: preset.name,
      websiteUrl: preset.websiteUrl,
      settingsConfig: configString,
    });

    // 设置选中的预设
    setSelectedPreset(index);

    // 清空 API Key 输入框，让用户重新输入
    setApiKey("");

    // 同步选择框状态
    const hasCoAuthoredDisabled = checkCoAuthoredSetting(configString);
    setDisableCoAuthored(hasCoAuthoredDisabled);
  };

  // 处理点击自定义按钮
  const handleCustomClick = () => {
    setSelectedPreset(-1);
    setFormData({
      name: "",
      websiteUrl: "",
      settingsConfig: "",
    });
    setApiKey("");
    setDisableCoAuthored(false);
  };

  // Codex: 应用预设
  const applyCodexPreset = (
    preset: (typeof codexProviderPresets)[0],
    index: number
  ) => {
    const authString = JSON.stringify(preset.auth || {}, null, 2);
    setCodexAuth(authString);
    setCodexConfig(preset.config || "");

    setFormData({
      name: preset.name,
      websiteUrl: preset.websiteUrl,
      settingsConfig: formData.settingsConfig,
    });

    setSelectedCodexPreset(index);

    // 清空 API Key，让用户重新输入
    setCodexApiKey("");
  };

  // Codex: 处理点击自定义按钮
  const handleCodexCustomClick = () => {
    setSelectedCodexPreset(-1);
    setFormData({
      name: "",
      websiteUrl: "",
      settingsConfig: "",
    });
    setCodexAuth("");
    setCodexConfig("");
    setCodexApiKey("");
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
    setFormData((prev) => ({
      ...prev,
      settingsConfig: configString,
    }));

    // 同步选择框状态
    const hasCoAuthoredDisabled = checkCoAuthoredSetting(configString);
    setDisableCoAuthored(hasCoAuthoredDisabled);
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

  // 根据当前配置决定是否展示 API Key 输入框
  // 自定义模式(-1)不显示独立的 API Key 输入框
  const showApiKey =
    (selectedPreset !== null && selectedPreset !== -1) ||
    (!showPresets && hasApiKeyField(formData.settingsConfig));

  // 判断当前选中的预设是否是官方
  const isOfficialPreset =
    selectedPreset !== null &&
    selectedPreset >= 0 &&
    providerPresets[selectedPreset]?.isOfficial === true;

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
  const isCodexOfficialPreset =
    selectedCodexPreset !== null &&
    selectedCodexPreset >= 0 &&
    codexProviderPresets[selectedCodexPreset]?.isOfficial === true;

  // 初始时从配置中同步 API Key（编辑模式）
  useEffect(() => {
    if (initialData) {
      const parsedKey = getApiKeyFromConfig(
        JSON.stringify(initialData.settingsConfig)
      );
      if (parsedKey) setApiKey(parsedKey);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

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
      <div className="absolute inset-0 bg-black/50 backdrop-blur-sm" />

      {/* Modal */}
      <div className="relative bg-white rounded-xl shadow-lg max-w-3xl w-full mx-4 max-h-[90vh] overflow-hidden flex flex-col">
        {/* Header */}
        <div className="flex items-center justify-between p-6 border-b border-[var(--color-border)]">
          <h2 className="text-xl font-semibold text-[var(--color-text-primary)]">
            {title}
          </h2>
          <button
            type="button"
            onClick={onClose}
            className="p-1 text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)] hover:bg-[var(--color-bg-tertiary)] rounded-md transition-colors"
            aria-label="关闭"
          >
            <X size={18} />
          </button>
        </div>

        <form onSubmit={handleSubmit} className="flex flex-col flex-1 min-h-0">
          <div className="flex-1 overflow-auto p-6 space-y-6">
            {error && (
              <div className="flex items-center gap-3 p-4 bg-[var(--color-error-light)] border border-[var(--color-error)]/20 rounded-lg">
                <AlertCircle
                  size={20}
                  className="text-[var(--color-error)] flex-shrink-0"
                />
                <p className="text-[var(--color-error)] text-sm font-medium">
                  {error}
                </p>
              </div>
            )}

            {showPresets && !isCodex && (
              <div className="space-y-4">
                <div>
                  <label className="block text-sm font-medium text-[var(--color-text-primary)] mb-3">
                    选择配置类型
                  </label>
                  <div className="flex flex-wrap gap-2">
                    <button
                      type="button"
                      className={`inline-flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-colors ${
                        selectedPreset === -1
                          ? "bg-[var(--color-primary)] text-white"
                          : "bg-[var(--color-bg-tertiary)] text-[var(--color-text-secondary)] hover:bg-[var(--color-border)]"
                      }`}
                      onClick={handleCustomClick}
                    >
                      自定义
                    </button>
                    {providerPresets.map((preset, index) => (
                      <button
                        key={index}
                        type="button"
                        className={`inline-flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-colors ${
                          selectedPreset === index
                            ? preset.isOfficial
                              ? "bg-[var(--color-warning)] text-white"
                              : "bg-[var(--color-primary)] text-white"
                            : "bg-[var(--color-bg-tertiary)] text-[var(--color-text-secondary)] hover:bg-[var(--color-border)]"
                        }`}
                        onClick={() => applyPreset(preset, index)}
                      >
                        {preset.isOfficial && <Zap size={14} />}
                        {preset.name}
                      </button>
                    ))}
                  </div>
                </div>
                {selectedPreset === -1 && (
                  <p className="text-sm text-[var(--color-text-secondary)]">
                    手动配置供应商，需要填写完整的配置信息
                  </p>
                )}
                {selectedPreset !== -1 && selectedPreset !== null && (
                  <p className="text-sm text-[var(--color-text-secondary)]">
                    {isOfficialPreset
                      ? "Claude 官方登录，不需要填写 API Key"
                      : "使用预设配置，只需填写 API Key"}
                  </p>
                )}
              </div>
            )}

            {showPresets && isCodex && (
              <div className="space-y-4">
                <div>
                  <label className="block text-sm font-medium text-[var(--color-text-primary)] mb-3">
                    选择配置类型
                  </label>
                  <div className="flex flex-wrap gap-2">
                    <button
                      type="button"
                      className={`inline-flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-colors ${
                        selectedCodexPreset === -1
                          ? "bg-[var(--color-primary)] text-white"
                          : "bg-[var(--color-bg-tertiary)] text-[var(--color-text-secondary)] hover:bg-[var(--color-border)]"
                      }`}
                      onClick={handleCodexCustomClick}
                    >
                      自定义
                    </button>
                    {codexProviderPresets.map((preset, index) => (
                      <button
                        key={index}
                        type="button"
                        className={`inline-flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-colors ${
                          selectedCodexPreset === index
                            ? preset.isOfficial
                              ? "bg-[var(--color-warning)] text-white"
                              : "bg-[var(--color-primary)] text-white"
                            : "bg-[var(--color-bg-tertiary)] text-[var(--color-text-secondary)] hover:bg-[var(--color-border)]"
                        }`}
                        onClick={() => applyCodexPreset(preset, index)}
                      >
                        {preset.isOfficial && <Zap size={14} />}
                        {preset.name}
                      </button>
                    ))}
                  </div>
                </div>
                {selectedCodexPreset === -1 && (
                  <p className="text-sm text-[var(--color-text-secondary)]">
                    手动配置供应商，需要填写完整的配置信息
                  </p>
                )}
                {selectedCodexPreset !== -1 && selectedCodexPreset !== null && (
                  <p className="text-sm text-[var(--color-text-secondary)]">
                    {isCodexOfficialPreset
                      ? "Codex 官方登录，不需要填写 API Key"
                      : "使用预设配置，只需填写 API Key"}
                  </p>
                )}
              </div>
            )}

            <div className="space-y-2">
              <label
                htmlFor="name"
                className="block text-sm font-medium text-[var(--color-text-primary)]"
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
                className="w-full px-3 py-2 border border-[var(--color-border)] rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-[var(--color-primary)]/20 focus:border-[var(--color-primary)] transition-colors"
              />
            </div>

            {!isCodex && showApiKey && (
              <div className="space-y-2">
                <label
                  htmlFor="apiKey"
                  className="block text-sm font-medium text-[var(--color-text-primary)]"
                >
                  API Key *
                </label>
                <input
                  type="text"
                  id="apiKey"
                  value={apiKey}
                  onChange={(e) => handleApiKeyChange(e.target.value)}
                  placeholder={
                    isOfficialPreset
                      ? "官方登录无需填写 API Key，直接保存即可"
                      : "只需要填这里，下方配置会自动填充"
                  }
                  disabled={isOfficialPreset}
                  autoComplete="off"
                  className={`w-full px-3 py-2 border rounded-lg text-sm transition-colors ${
                    isOfficialPreset
                      ? "bg-[var(--color-bg-tertiary)] border-[var(--color-border)] text-[var(--color-text-tertiary)] cursor-not-allowed"
                      : "border-[var(--color-border)] focus:outline-none focus:ring-2 focus:ring-[var(--color-primary)]/20 focus:border-[var(--color-primary)]"
                  }`}
                />
              </div>
            )}

            {isCodex && showCodexApiKey && (
              <div className="space-y-2">
                <label
                  htmlFor="codexApiKey"
                  className="block text-sm font-medium text-[var(--color-text-primary)]"
                >
                  API Key *
                </label>
                <input
                  type="text"
                  id="codexApiKey"
                  value={codexApiKey}
                  onChange={(e) => handleCodexApiKeyChange(e.target.value)}
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
                  autoComplete="off"
                  className={`w-full px-3 py-2 border rounded-lg text-sm transition-colors ${
                    isCodexOfficialPreset
                      ? "bg-[var(--color-bg-tertiary)] border-[var(--color-border)] text-[var(--color-text-tertiary)] cursor-not-allowed"
                      : "border-[var(--color-border)] focus:outline-none focus:ring-2 focus:ring-[var(--color-primary)]/20 focus:border-[var(--color-primary)]"
                  }`}
                />
              </div>
            )}

            <div className="space-y-2">
              <label
                htmlFor="websiteUrl"
                className="block text-sm font-medium text-[var(--color-text-primary)]"
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
                className="w-full px-3 py-2 border border-[var(--color-border)] rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-[var(--color-primary)]/20 focus:border-[var(--color-primary)] transition-colors"
              />
            </div>

            {/* Claude 或 Codex 的配置部分 */}
            {isCodex ? (
              // Codex: 双编辑器
              <div className="space-y-6">
                <div className="space-y-2">
                  <label
                    htmlFor="codexAuth"
                    className="block text-sm font-medium text-[var(--color-text-primary)]"
                  >
                    auth.json (JSON) *
                  </label>
                  <textarea
                    id="codexAuth"
                    value={codexAuth}
                    onChange={(e) => {
                      const value = e.target.value;
                      setCodexAuth(value);
                      try {
                        const auth = JSON.parse(value || "{}");
                        const key =
                          typeof auth.OPENAI_API_KEY === "string"
                            ? auth.OPENAI_API_KEY
                            : "";
                        setCodexApiKey(key);
                      } catch {
                        // ignore
                      }
                    }}
                    placeholder={`{
  "OPENAI_API_KEY": "sk-your-api-key-here"
}`}
                    rows={6}
                    required
                    className="w-full px-3 py-2 border border-[var(--color-border)] rounded-lg text-sm font-mono focus:outline-none focus:ring-2 focus:ring-[var(--color-primary)]/20 focus:border-[var(--color-primary)] transition-colors resize-y min-h-[8rem]"
                  />
                  <p className="text-xs text-[var(--color-text-secondary)]">
                    Codex auth.json 配置内容
                  </p>
                </div>

                <div className="space-y-2">
                  <label
                    htmlFor="codexConfig"
                    className="block text-sm font-medium text-[var(--color-text-primary)]"
                  >
                    config.toml (TOML)
                  </label>
                  <textarea
                    id="codexConfig"
                    value={codexConfig}
                    onChange={(e) => setCodexConfig(e.target.value)}
                    placeholder=""
                    rows={8}
                    className="w-full px-3 py-2 border border-[var(--color-border)] rounded-lg text-sm font-mono focus:outline-none focus:ring-2 focus:ring-[var(--color-primary)]/20 focus:border-[var(--color-primary)] transition-colors resize-y min-h-[10rem]"
                  />
                  <p className="text-xs text-[var(--color-text-secondary)]">
                    Codex config.toml 配置内容
                  </p>
                </div>
              </div>
            ) : (
              // Claude: 原有的单编辑器
              <div className="space-y-2">
                <div className="flex items-center justify-between">
                  <label
                    htmlFor="settingsConfig"
                    className="block text-sm font-medium text-[var(--color-text-primary)]"
                  >
                    Claude Code 配置 (JSON) *
                  </label>
                  <label className="inline-flex items-center gap-2 text-sm text-[var(--color-text-secondary)] cursor-pointer">
                    <input
                      type="checkbox"
                      checked={disableCoAuthored}
                      onChange={(e) => handleCoAuthoredToggle(e.target.checked)}
                      className="w-4 h-4 text-[var(--color-primary)] bg-white border-[var(--color-border)] rounded focus:ring-[var(--color-primary)] focus:ring-2"
                    />
                    禁止 Claude Code 签名
                  </label>
                </div>
                <JsonEditor
                  value={formData.settingsConfig}
                  onChange={(value) =>
                    handleChange({
                      target: { name: "settingsConfig", value },
                    } as React.ChangeEvent<HTMLTextAreaElement>)
                  }
                  placeholder={`{
  "env": {
    "ANTHROPIC_BASE_URL": "https://api.anthropic.com",
    "ANTHROPIC_AUTH_TOKEN": "sk-your-api-key-here"
  }
}`}
                  rows={12}
                />
                <p className="text-xs text-[var(--color-text-secondary)]">
                  完整的 Claude Code settings.json 配置内容
                </p>
              </div>
            )}
          </div>

          {/* Footer */}
          <div className="flex items-center justify-end gap-3 p-6 border-t border-[var(--color-border)] bg-[var(--color-bg-tertiary)]">
            <button
              type="button"
              onClick={onClose}
              className="px-4 py-2 text-sm font-medium text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)] hover:bg-white rounded-lg transition-colors"
            >
              取消
            </button>
            <button
              type="submit"
              className="inline-flex items-center gap-2 px-4 py-2 bg-[var(--color-primary)] text-white rounded-lg hover:bg-[var(--color-primary-hover)] transition-colors text-sm font-medium"
            >
              <Save size={16} />
              {submitText}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
};

export default ProviderForm;
