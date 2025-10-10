import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { X, Save, AlertCircle } from "lucide-react";
import { McpServer } from "../../types";
import { buttonStyles, inputStyles } from "../../lib/styles";
import McpWizardModal from "./McpWizardModal";
import { extractErrorMessage } from "../../utils/errorUtils";

interface McpFormModalProps {
  editingId?: string;
  initialData?: McpServer;
  onSave: (id: string, server: McpServer) => Promise<void>;
  onClose: () => void;
  existingIds?: string[];
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
  editingId,
  initialData,
  onSave,
  onClose,
  existingIds = [],
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

  const handleIdChange = (value: string) => {
    setFormId(value);
    if (!isEditing) {
      const exists = existingIds.includes(value.trim());
      setIdError(exists ? t("mcp.error.idExists") : "");
    }
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
      alert(t("mcp.error.idRequired"));
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
      alert(t("mcp.error.jsonInvalid"));
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
          alert(t("mcp.error.commandRequired"));
          return;
        }
        if (server?.type === "http" && !server?.url?.trim()) {
          alert(t("mcp.wizard.urlRequired"));
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
      alert(msg);
    } finally {
      setSaving(false);
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
      <div className="relative bg-white dark:bg-gray-900 rounded-xl shadow-lg max-w-2xl w-full mx-4 overflow-hidden">
        {/* Header */}
        <div className="flex items-center justify-between p-6 border-b border-gray-200 dark:border-gray-800">
          <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
            {isEditing ? t("mcp.editServer") : t("mcp.addServer")}
          </h3>
          <button
            onClick={onClose}
            className="p-1 text-gray-500 hover:text-gray-900 hover:bg-gray-100 dark:text-gray-400 dark:hover:text-gray-100 dark:hover:bg-gray-800 rounded-md transition-colors"
          >
            <X size={18} />
          </button>
        </div>

        {/* Content */}
        <div className="p-6 space-y-4">
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
        <div className="flex items-center justify-end gap-3 p-6 border-t border-gray-200 dark:border-gray-800">
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
      />
    </div>
  );
};

export default McpFormModal;
