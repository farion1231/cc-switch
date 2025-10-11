import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { X, Save, AlertCircle } from "lucide-react";
import { McpServer } from "../../types";
import { mcpPresets } from "../../config/mcpPresets";
import { buttonStyles, inputStyles } from "../../lib/styles";
import McpWizardModal from "./McpWizardModal";
import {
  extractErrorMessage,
  translateMcpBackendError,
} from "../../utils/errorUtils";
import { AppType } from "../../lib/tauri-api";
import {
  validateToml,
  tomlToMcpServer,
  extractIdFromToml,
  mcpServerToToml,
} from "../../utils/tomlUtils";

interface McpFormModalProps {
  appType: AppType;
  editingId?: string;
  initialData?: McpServer;
  onSave: (id: string, server: McpServer) => Promise<void>;
  onClose: () => void;
  existingIds?: string[];
  onNotify?: (
    message: string,
    type: "success" | "error",
    duration?: number,
  ) => void;
}

/**
 * MCP 表单模态框组件（简化版）
 * Claude: 使用 JSON 格式
 * Codex: 使用 TOML 格式
 */
const McpFormModal: React.FC<McpFormModalProps> = ({
  appType,
  editingId,
  initialData,
  onSave,
  onClose,
  existingIds = [],
  onNotify,
}) => {
  const { t } = useTranslation();

  // JSON 基本校验（返回 i18n 文案）
  const validateJson = (text: string): string => {
    if (!text.trim()) return "";
    try {
      const parsed = JSON.parse(text);
      if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
        return t("mcp.error.jsonInvalid");
      }
      return "";
    } catch {
      return t("mcp.error.jsonInvalid");
    }
  };

  // 统一格式化 TOML 错误（本地化 + 详情）
  const formatTomlError = (err: string): string => {
    if (!err) return "";
    if (err === "mustBeObject" || err === "parseError") {
      return t("mcp.error.tomlInvalid");
    }
    return `${t("mcp.error.tomlInvalid")}: ${err}`;
  };
  const [formId, setFormId] = useState(editingId || "");
  const [formDescription, setFormDescription] = useState(
    (initialData as any)?.description || "",
  );

  // 根据 appType 决定初始格式
  const [formConfig, setFormConfig] = useState(() => {
    if (!initialData) return "";
    if (appType === "codex") {
      return mcpServerToToml(initialData);
    } else {
      return JSON.stringify(initialData, null, 2);
    }
  });

  const [configError, setConfigError] = useState("");
  const [saving, setSaving] = useState(false);
  const [isWizardOpen, setIsWizardOpen] = useState(false);
  const [idError, setIdError] = useState("");

  // 编辑模式下禁止修改 ID
  const isEditing = !!editingId;

  // 判断是否使用 TOML 格式
  const useToml = appType === "codex";

  // 预设选择状态（仅新增模式显示；-1 表示自定义）
  const [selectedPreset, setSelectedPreset] = useState<number | null>(
    isEditing ? null : -1,
  );

  const handleIdChange = (value: string) => {
    setFormId(value);
    if (!isEditing) {
      const exists = existingIds.includes(value.trim());
      setIdError(exists ? t("mcp.error.idExists") : "");
    }
  };

  const ensureUniqueId = (base: string): string => {
    let candidate = base.trim();
    if (!candidate) candidate = "mcp-server";
    if (!existingIds.includes(candidate)) return candidate;
    let i = 1;
    while (existingIds.includes(`${candidate}-${i}`)) i++;
    return `${candidate}-${i}`;
  };

  // 应用预设（写入表单但不落库）
  const applyPreset = (index: number) => {
    if (index < 0 || index >= mcpPresets.length) return;
    const p = mcpPresets[index];
    const id = ensureUniqueId(p.id);
    setFormId(id);
    setFormDescription(p.description || "");

    // 根据格式转换配置
    if (useToml) {
      const toml = mcpServerToToml(p.server);
      setFormConfig(toml);
      {
        const err = validateToml(toml);
        setConfigError(formatTomlError(err));
      }
    } else {
      const json = JSON.stringify(p.server, null, 2);
      setFormConfig(json);
      setConfigError(validateJson(json));
    }
    setSelectedPreset(index);
  };

  // 切回自定义
  const applyCustom = () => {
    setSelectedPreset(-1);
    // 恢复到空白模板
    setFormId("");
    setFormDescription("");
    setFormConfig("");
    setConfigError("");
  };

  const handleConfigChange = (value: string) => {
    setFormConfig(value);

    if (useToml) {
      // TOML 校验
      const err = validateToml(value);
      if (err) {
        setConfigError(formatTomlError(err));
        return;
      }

      // 尝试解析并做必填字段提示
      if (value.trim()) {
        try {
          const server = tomlToMcpServer(value);
          if (server.type === "stdio" && !server.command?.trim()) {
            setConfigError(t("mcp.error.commandRequired"));
            return;
          }
          if (server.type === "http" && !server.url?.trim()) {
            setConfigError(t("mcp.wizard.urlRequired"));
            return;
          }

          // 尝试提取 ID（如果用户还没有填写）
          if (!formId.trim()) {
            const extractedId = extractIdFromToml(value);
            if (extractedId) {
              setFormId(extractedId);
            }
          }
        } catch (e: any) {
          const msg = e?.message || String(e);
          setConfigError(formatTomlError(msg));
          return;
        }
      }
    } else {
      // JSON 校验
      const baseErr = validateJson(value);
      if (baseErr) {
        setConfigError(baseErr);
        return;
      }

      // 进一步结构校验
      if (value.trim()) {
        try {
          const obj = JSON.parse(value);
          if (obj && typeof obj === "object") {
            if (Object.prototype.hasOwnProperty.call(obj, "mcpServers")) {
              setConfigError(t("mcp.error.singleServerObjectRequired"));
              return;
            }

            const typ = (obj as any)?.type;
            if (typ === "stdio" && !(obj as any)?.command?.trim()) {
              setConfigError(t("mcp.error.commandRequired"));
              return;
            }
            if (typ === "http" && !(obj as any)?.url?.trim()) {
              setConfigError(t("mcp.wizard.urlRequired"));
              return;
            }
          }
        } catch {
          // 解析异常已在基础校验覆盖
        }
      }
    }

    setConfigError("");
  };

  const handleWizardApply = (title: string, json: string) => {
    setFormId(title);
    // Wizard 返回的是 JSON，根据格式决定是否需要转换
    if (useToml) {
      try {
        const server = JSON.parse(json) as McpServer;
        const toml = mcpServerToToml(server);
        setFormConfig(toml);
        const err = validateToml(toml);
        setConfigError(formatTomlError(err));
      } catch (e: any) {
        setConfigError(t("mcp.error.jsonInvalid"));
      }
    } else {
      setFormConfig(json);
      setConfigError(validateJson(json));
    }
  };

  const handleSubmit = async () => {
    if (!formId.trim()) {
      onNotify?.(t("mcp.error.idRequired"), "error", 3000);
      return;
    }

    // 新增模式：阻止提交重名 ID
    if (!isEditing && existingIds.includes(formId.trim())) {
      setIdError(t("mcp.error.idExists"));
      return;
    }

    // 验证配置格式
    let server: McpServer;

    if (useToml) {
      // TOML 模式
      const tomlError = validateToml(formConfig);
      setConfigError(formatTomlError(tomlError));
      if (tomlError) {
        onNotify?.(t("mcp.error.tomlInvalid"), "error", 3000);
        return;
      }

      if (!formConfig.trim()) {
        // 空配置
        server = {
          type: "stdio",
          command: "",
          args: [],
        };
      } else {
        try {
          server = tomlToMcpServer(formConfig);
        } catch (e: any) {
          const msg = e?.message || String(e);
          setConfigError(formatTomlError(msg));
          onNotify?.(t("mcp.error.tomlInvalid"), "error", 4000);
          return;
        }
      }
    } else {
      // JSON 模式
      const jsonError = validateJson(formConfig);
      setConfigError(jsonError);
      if (jsonError) {
        onNotify?.(t("mcp.error.jsonInvalid"), "error", 3000);
        return;
      }

      if (!formConfig.trim()) {
        // 空配置
        server = {
          type: "stdio",
          command: "",
          args: [],
        };
      } else {
        try {
          server = JSON.parse(formConfig) as McpServer;
        } catch (e: any) {
          setConfigError(t("mcp.error.jsonInvalid"));
          onNotify?.(t("mcp.error.jsonInvalid"), "error", 4000);
          return;
        }
      }
    }

    // 前置必填校验
    if (server?.type === "stdio" && !server?.command?.trim()) {
      onNotify?.(t("mcp.error.commandRequired"), "error", 3000);
      return;
    }
    if (server?.type === "http" && !server?.url?.trim()) {
      onNotify?.(t("mcp.wizard.urlRequired"), "error", 3000);
      return;
    }

    setSaving(true);
    try {
      // 保留原有的 enabled 状态
      if (initialData?.enabled !== undefined) {
        server.enabled = initialData.enabled;
      }

      // 保存 description 到 server 对象
      if (formDescription.trim()) {
        (server as any).description = formDescription.trim();
      }

      // 显式等待父组件保存流程
      await onSave(formId.trim(), server);
    } catch (error: any) {
      const detail = extractErrorMessage(error);
      const mapped = translateMcpBackendError(detail, t);
      const msg = mapped || detail || t("mcp.error.saveFailed");
      onNotify?.(msg, "error", mapped || detail ? 6000 : 4000);
    } finally {
      setSaving(false);
    }
  };

  const getFormTitle = () => {
    if (appType === "claude") {
      return isEditing ? t("mcp.editClaudeServer") : t("mcp.addClaudeServer");
    } else {
      return isEditing ? t("mcp.editCodexServer") : t("mcp.addCodexServer");
    }
  };

  return (
    <div className="fixed inset-0 z-[60] flex items-center justify-center">
      {/* Backdrop */}
      <div
        className="absolute inset-0 bg-black/50 backdrop-blur-sm"
        onClick={onClose}
      />

      {/* Modal */}
      <div className="relative bg-white dark:bg-gray-900 rounded-xl shadow-lg max-w-3xl w-full mx-4 max-h-[90vh] overflow-hidden flex flex-col">
        {/* Header */}
        <div className="flex-shrink-0 flex items-center justify-between p-6 border-b border-gray-200 dark:border-gray-800">
          <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
            {getFormTitle()}
          </h3>
          <button
            onClick={onClose}
            className="p-1 text-gray-500 hover:text-gray-900 hover:bg-gray-100 dark:text-gray-400 dark:hover:text-gray-100 dark:hover:bg-gray-800 rounded-md transition-colors"
          >
            <X size={18} />
          </button>
        </div>

        {/* Content - Scrollable */}
        <div className="flex-1 overflow-y-auto p-6 space-y-4">
          {/* 预设选择（仅新增时展示） */}
          {!isEditing && (
            <div>
              <label className="block text-sm font-medium text-gray-900 dark:text-gray-100 mb-3">
                {t("mcp.presets.title")}
              </label>
              <div className="flex flex-wrap gap-2">
                <button
                  type="button"
                  onClick={applyCustom}
                  className={`inline-flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-colors ${
                    selectedPreset === -1
                      ? "bg-emerald-500 text-white dark:bg-emerald-600"
                      : "bg-gray-100 dark:bg-gray-800 text-gray-500 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-gray-700"
                  }`}
                >
                  {t("presetSelector.custom")}
                </button>
                {mcpPresets.map((p, idx) => (
                  <button
                    key={p.id}
                    type="button"
                    onClick={() => applyPreset(idx)}
                    className={`inline-flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-colors ${
                      selectedPreset === idx
                        ? "bg-emerald-500 text-white dark:bg-emerald-600"
                        : "bg-gray-100 dark:bg-gray-800 text-gray-500 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-gray-700"
                    }`}
                    title={p.description}
                  >
                    {p.name || p.id}
                  </button>
                ))}
              </div>
            </div>
          )}
          {/* ID (标题) */}
          <div>
            <div className="flex items-center justify-between mb-2">
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300">
                {t("mcp.form.title")} <span className="text-red-500">*</span>
              </label>
              {!isEditing && idError && (
                <span className="text-xs text-red-500 dark:text-red-400">
                  {idError}
                </span>
              )}
            </div>
            <input
              className={inputStyles.text}
              placeholder={t("mcp.form.titlePlaceholder")}
              value={formId}
              onChange={(e) => handleIdChange(e.target.value)}
              disabled={isEditing}
            />
          </div>

          {/* Description (描述) */}
          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
              {t("mcp.form.description")}
            </label>
            <input
              className={inputStyles.text}
              placeholder={t("mcp.form.descriptionPlaceholder")}
              value={formDescription}
              onChange={(e) => setFormDescription(e.target.value)}
            />
          </div>

          {/* 配置输入框（根据格式显示 JSON 或 TOML） */}
          <div>
            <div className="flex items-center justify-between mb-2">
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300">
                {useToml ? t("mcp.form.tomlConfig") : t("mcp.form.jsonConfig")}
              </label>
              {(isEditing || selectedPreset === -1) && (
                <button
                  type="button"
                  onClick={() => setIsWizardOpen(true)}
                  className="text-sm text-blue-500 dark:text-blue-400 hover:text-blue-600 dark:hover:text-blue-300 transition-colors"
                >
                  {t("mcp.form.useWizard")}
                </button>
              )}
            </div>
            <textarea
              className={`${inputStyles.text} h-48 resize-none font-mono text-xs`}
              placeholder={
                useToml
                  ? t("mcp.form.tomlPlaceholder")
                  : t("mcp.form.jsonPlaceholder")
              }
              value={formConfig}
              onChange={(e) => handleConfigChange(e.target.value)}
            />
            {configError && (
              <div className="flex items-center gap-2 mt-2 text-red-500 dark:text-red-400 text-sm">
                <AlertCircle size={16} />
                <span>{configError}</span>
              </div>
            )}
          </div>
        </div>

        {/* Footer */}
        <div className="flex-shrink-0 flex items-center justify-end gap-3 p-6 border-t border-gray-200 dark:border-gray-800 bg-gray-100 dark:bg-gray-800">
          <button
            onClick={onClose}
            className="px-4 py-2 text-gray-500 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-gray-700 hover:text-gray-900 dark:hover:text-gray-200 rounded-lg transition-colors text-sm font-medium"
          >
            {t("common.cancel")}
          </button>
          <button
            onClick={handleSubmit}
            disabled={saving || (!isEditing && !!idError)}
            className={`inline-flex items-center gap-2 ${buttonStyles.mcp}`}
          >
            <Save size={16} />
            {saving
              ? t("common.saving")
              : isEditing
                ? t("common.save")
                : t("common.add")}
          </button>
        </div>
      </div>

      {/* Wizard Modal */}
      <McpWizardModal
        isOpen={isWizardOpen}
        onClose={() => setIsWizardOpen(false)}
        onApply={handleWizardApply}
        onNotify={onNotify}
      />
    </div>
  );
};

export default McpFormModal;
