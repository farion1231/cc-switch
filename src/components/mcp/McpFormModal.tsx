import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { X, Save, AlertCircle } from "lucide-react";
import { McpServer } from "../../types";
import { mcpPresets } from "../../config/mcpPresets";
import { buttonStyles, inputStyles } from "../../lib/styles";
import McpWizardModal from "./McpWizardModal";
import { extractErrorMessage } from "../../utils/errorUtils";
import { AppType } from "../../lib/tauri-api";

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
 * 验证 JSON 格式
 */
const validateJson = (text: string): string => {
  if (!text.trim()) return "";
  try {
    const parsed = JSON.parse(text);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return "JSON 必须是对象";
    }
    return "";
  } catch {
    return "JSON 格式错误";
  }
};

/**
 * MCP 表单模态框组件（简化版）
 * 仅包含：标题（必填）、描述（可选）、JSON 配置（可选，带格式校验）
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
  const [formId, setFormId] = useState(editingId || "");
  const [formDescription, setFormDescription] = useState(
    (initialData as any)?.description || "",
  );
  const [formJson, setFormJson] = useState(
    initialData ? JSON.stringify(initialData, null, 2) : "",
  );
  const [jsonError, setJsonError] = useState("");
  const [saving, setSaving] = useState(false);
  const [isWizardOpen, setIsWizardOpen] = useState(false);
  const [idError, setIdError] = useState("");

  // 编辑模式下禁止修改 ID
  const isEditing = !!editingId;

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
    const json = JSON.stringify(p.server, null, 2);
    setFormJson(json);
    // 触发一次校验
    setJsonError(validateJson(json));
    setSelectedPreset(index);
  };

  // 切回自定义
  const applyCustom = () => {
    setSelectedPreset(-1);
    // 恢复到空白模板
    setFormId("");
    setFormDescription("");
    setFormJson("");
    setJsonError("");
  };

  const handleJsonChange = (value: string) => {
    setFormJson(value);

    // 基础 JSON 校验
    const baseErr = validateJson(value);
    if (baseErr) {
      setJsonError(baseErr);
      return;
    }

    // 进一步结构校验：仅允许单个服务器对象，禁止整份配置
    if (value.trim()) {
      try {
        const obj = JSON.parse(value);
        if (obj && typeof obj === "object") {
          if (Object.prototype.hasOwnProperty.call(obj, "mcpServers")) {
            setJsonError(t("mcp.error.singleServerObjectRequired"));
            return;
          }

          // 若带有类型，做必填字段提示（不阻止输入，仅给出即时反馈）
          const typ = (obj as any)?.type;
          if (typ === "stdio" && !(obj as any)?.command?.trim()) {
            setJsonError(t("mcp.error.commandRequired"));
            return;
          }
          if (typ === "http" && !(obj as any)?.url?.trim()) {
            setJsonError(t("mcp.wizard.urlRequired"));
            return;
          }
        }
      } catch {
        // 解析异常已在基础校验覆盖
      }
    }

    setJsonError("");
  };

  const handleWizardApply = (json: string) => {
    setFormJson(json);
    setJsonError(validateJson(json));
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

    // 验证 JSON
    const currentJsonError = validateJson(formJson);
    setJsonError(currentJsonError);
    if (currentJsonError) {
      onNotify?.(t("mcp.error.jsonInvalid"), "error", 3000);
      return;
    }

    setSaving(true);
    try {
      let server: McpServer;
      if (formJson.trim()) {
        // 解析 JSON 配置
        server = JSON.parse(formJson) as McpServer;

        // 前置必填校验，避免后端拒绝后才提示
        if (server?.type === "stdio" && !server?.command?.trim()) {
          onNotify?.(t("mcp.error.commandRequired"), "error", 3000);
          return;
        }
        if (server?.type === "http" && !server?.url?.trim()) {
          onNotify?.(t("mcp.wizard.urlRequired"), "error", 3000);
          return;
        }
      } else {
        // 空 JSON 时提供默认值（注意：后端会校验 stdio 需要非空 command / http 需要 url）
        server = {
          type: "stdio",
          command: "",
          args: [],
        };
      }

      // 保留原有的 enabled 状态
      if (initialData?.enabled !== undefined) {
        server.enabled = initialData.enabled;
      }

      // 保存 description 到 server 对象
      if (formDescription.trim()) {
        (server as any).description = formDescription.trim();
      }

      // 显式等待父组件保存流程，以便正确处理成功/失败
      await onSave(formId.trim(), server);
    } catch (error: any) {
      // 提取后端错误信息（支持 string / {message} / tauri payload）
      const detail = extractErrorMessage(error);
      const msg = detail || t("mcp.error.saveFailed");
      onNotify?.(msg, "error", detail ? 6000 : 4000);
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
            <div className="space-y-2">
              <div className="text-sm font-medium text-gray-900 dark:text-gray-100">
                {t("mcp.presets.title")}
              </div>
              <div className="flex flex-wrap gap-2">
                <button
                  type="button"
                  onClick={applyCustom}
                  className={`${
                    selectedPreset === -1 ? "bg-gray-900 text-white dark:bg-gray-100 dark:text-gray-900" : "bg-gray-100 text-gray-700 dark:bg-gray-800 dark:text-gray-200"
                  } px-3 py-1.5 rounded-md text-xs font-medium transition-colors`}
                >
                  {t("presetSelector.custom")}
                </button>
                {mcpPresets.map((p, idx) => (
                  <button
                    key={p.id}
                    type="button"
                    onClick={() => applyPreset(idx)}
                    className={`${
                      selectedPreset === idx
                        ? "bg-emerald-500 text-white"
                        : "bg-emerald-50 text-emerald-700 dark:bg-emerald-900/20 dark:text-emerald-300"
                    } px-3 py-1.5 rounded-md text-xs font-medium transition-colors`}
                    title={p.description}
                  >
                    {p.name || p.id}
                  </button>
                ))}
              </div>
              {/* 无需环境变量提示：已移除 */}
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

          {/* JSON 配置 */}
          <div>
            <div className="flex items-center justify-between mb-2">
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300">
                {t("mcp.form.jsonConfig")}
              </label>
              <button
                type="button"
                onClick={() => setIsWizardOpen(true)}
                className="text-sm text-blue-500 dark:text-blue-400 hover:text-blue-600 dark:hover:text-blue-300 transition-colors"
              >
                {t("mcp.form.useWizard")}
              </button>
            </div>
            <textarea
              className={`${inputStyles.text} h-48 resize-none font-mono text-xs`}
              placeholder={t("mcp.form.jsonPlaceholder")}
              value={formJson}
              onChange={(e) => handleJsonChange(e.target.value)}
            />
            {jsonError && (
              <div className="flex items-center gap-2 mt-2 text-red-500 dark:text-red-400 text-sm">
                <AlertCircle size={16} />
                <span>{jsonError}</span>
              </div>
            )}
          </div>
        </div>

        {/* Footer */}
        <div className="flex-shrink-0 flex items-center justify-end gap-3 p-6 border-t border-gray-200 dark:border-gray-800 bg-gray-100 dark:bg-gray-800">
          <button onClick={onClose} className={buttonStyles.secondary}>
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
