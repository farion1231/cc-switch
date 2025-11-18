import React, { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Save, Plus, AlertCircle, ChevronDown, ChevronUp, Wand2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import type { AppId } from "@/lib/api/types";
import { McpServer, McpServerSpec } from "@/types";
import { mcpPresets, getMcpPresetWithDescription } from "@/config/mcpPresets";
import McpWizardModal from "./McpWizardModal";
import {
  extractErrorMessage,
  translateMcpBackendError,
} from "@/utils/errorUtils";
import {
  tomlToMcpServer,
  extractIdFromToml,
  mcpServerToToml,
} from "@/utils/tomlUtils";
import { normalizeTomlText } from "@/utils/textNormalization";
import { formatJSON, parseSmartMcpJson } from "@/utils/formatters";
import { useMcpValidation } from "./useMcpValidation";
import { useUpsertMcpServer } from "@/hooks/useMcp";

interface McpFormModalProps {
  editingId?: string;
  initialData?: McpServer;
  onSave: () => Promise<void>; // v3.7.0: 简化为仅用于关闭表单的回调
  onClose: () => void;
  existingIds?: string[];
  defaultFormat?: "json" | "toml"; // 默认配置格式（可选，默认为 JSON）
  defaultEnabledApps?: AppId[]; // 默认启用到哪些应用（可选，默认为全部应用）
}

/**
 * MCP 表单模态框组件（v3.7.0 完整重构版）
 * - 支持 JSON 和 TOML 两种格式
 * - 统一管理，通过复选框选择启用到哪些应用
 */
const McpFormModal: React.FC<McpFormModalProps> = ({
  editingId,
  initialData,
  onSave,
  onClose,
  existingIds = [],
  defaultFormat = "json",
  defaultEnabledApps = ["claude", "codex", "gemini"],
}) => {
  const { t } = useTranslation();
  const { formatTomlError, validateTomlConfig, validateJsonConfig } =
    useMcpValidation();

  const upsertMutation = useUpsertMcpServer();

  const [formId, setFormId] = useState(
    () => editingId || initialData?.id || "",
  );
  const [formName, setFormName] = useState(initialData?.name || "");
  const [formDescription, setFormDescription] = useState(
    initialData?.description || "",
  );
  const [formHomepage, setFormHomepage] = useState(initialData?.homepage || "");
  const [formDocs, setFormDocs] = useState(initialData?.docs || "");
  const [formTags, setFormTags] = useState(initialData?.tags?.join(", ") || "");

  // 启用状态：编辑模式使用现有值，新增模式使用默认值
  const [enabledApps, setEnabledApps] = useState<{
    claude: boolean;
    codex: boolean;
    gemini: boolean;
  }>(() => {
    if (initialData?.apps) {
      return { ...initialData.apps };
    }
    // 新增模式：根据 defaultEnabledApps 设置初始值
    return {
      claude: defaultEnabledApps.includes("claude"),
      codex: defaultEnabledApps.includes("codex"),
      gemini: defaultEnabledApps.includes("gemini"),
    };
  });

  // 编辑模式下禁止修改 ID
  const isEditing = !!editingId;

  // 判断是否在编辑模式下有附加信息
  const hasAdditionalInfo = !!(
    initialData?.description ||
    initialData?.tags?.length ||
    initialData?.homepage ||
    initialData?.docs
  );

  // 附加信息展开状态（编辑模式下有值时默认展开）
  const [showMetadata, setShowMetadata] = useState(
    isEditing ? hasAdditionalInfo : false,
  );

  // 配置格式：优先使用 defaultFormat，编辑模式下可从现有数据推断
  const useTomlFormat = useMemo(() => {
    if (initialData?.server) {
      // 编辑模式：尝试从现有数据推断格式（这里简化处理，默认 JSON）
      return defaultFormat === "toml";
    }
    return defaultFormat === "toml";
  }, [defaultFormat, initialData]);

  // 根据格式决定初始配置
  const [formConfig, setFormConfig] = useState(() => {
    const spec = initialData?.server;
    if (!spec) return "";
    if (useTomlFormat) {
      return mcpServerToToml(spec);
    }
    return JSON.stringify(spec, null, 2);
  });

  const [configError, setConfigError] = useState("");
  const [saving, setSaving] = useState(false);
  const [isWizardOpen, setIsWizardOpen] = useState(false);
  const [idError, setIdError] = useState("");

  // 判断是否使用 TOML 格式（向后兼容，后续可扩展为格式切换按钮）
  const useToml = useTomlFormat;

  const wizardInitialSpec = useMemo(() => {
    const fallback = initialData?.server;
    if (!formConfig.trim()) {
      return fallback;
    }

    if (useToml) {
      try {
        return tomlToMcpServer(formConfig);
      } catch {
        return fallback;
      }
    }

    try {
      const parsed = JSON.parse(formConfig);
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        return parsed as McpServerSpec;
      }
      return fallback;
    } catch {
      return fallback;
    }
  }, [formConfig, initialData, useToml]);

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
    const preset = mcpPresets[index];
    const presetWithDesc = getMcpPresetWithDescription(preset, t);

    const id = ensureUniqueId(presetWithDesc.id);
    setFormId(id);
    setFormName(presetWithDesc.name || presetWithDesc.id);
    setFormDescription(presetWithDesc.description || "");
    setFormHomepage(presetWithDesc.homepage || "");
    setFormDocs(presetWithDesc.docs || "");
    setFormTags(presetWithDesc.tags?.join(", ") || "");

    // 根据格式转换配置
    if (useToml) {
      const toml = mcpServerToToml(presetWithDesc.server);
      setFormConfig(toml);
      setConfigError(validateTomlConfig(toml));
    } else {
      const json = JSON.stringify(presetWithDesc.server, null, 2);
      setFormConfig(json);
      setConfigError(validateJsonConfig(json));
    }
    setSelectedPreset(index);
  };

  // 切回自定义
  const applyCustom = () => {
    setSelectedPreset(-1);
    // 恢复到空白模板
    setFormId("");
    setFormName("");
    setFormDescription("");
    setFormHomepage("");
    setFormDocs("");
    setFormTags("");
    setFormConfig("");
    setConfigError("");
  };

  const handleConfigChange = (value: string) => {
    // 若为 TOML 模式，先做引号归一化，避免中文输入法导致的格式错误
    const nextValue = useToml ? normalizeTomlText(value) : value;
    setFormConfig(nextValue);

    if (useToml) {
      // TOML validation (use hook's complete validation)
      const err = validateTomlConfig(nextValue);
      if (err) {
        setConfigError(err);
        return;
      }

      // Try to extract ID (if user hasn't filled it yet)
      if (nextValue.trim() && !formId.trim()) {
        const extractedId = extractIdFromToml(nextValue);
        if (extractedId) {
          setFormId(extractedId);
        }
      }
    } else {
      // JSON validation with smart parsing
      try {
        const result = parseSmartMcpJson(value);

        // 验证解析后的配置对象
        const configJson = JSON.stringify(result.config);
        const validationErr = validateJsonConfig(configJson);

        if (validationErr) {
          setConfigError(validationErr);
          return;
        }

        // 自动填充提取的 id（仅当表单 id 为空且不在编辑模式时）
        if (result.id && !formId.trim() && !isEditing) {
          const uniqueId = ensureUniqueId(result.id);
          setFormId(uniqueId);

          // 如果 name 也为空，同时填充 name
          if (!formName.trim()) {
            setFormName(result.id);
          }
        }

        // 不在输入时自动格式化，保持用户输入的原样
        // 格式清理将在提交时进行

        setConfigError("");
      } catch (err: any) {
        const errorMessage = err?.message || String(err);
        setConfigError(t("mcp.error.jsonInvalid") + ": " + errorMessage);
      }
    }
  };

  const handleFormatJson = () => {
    if (!formConfig.trim()) return;

    try {
      const formatted = formatJSON(formConfig);
      setFormConfig(formatted);
      toast.success(t("common.formatSuccess"));
    } catch (error) {
      const errorMessage =
        error instanceof Error ? error.message : String(error);
      toast.error(
        t("common.formatError", {
          error: errorMessage,
        }),
      );
    }
  };

  const handleWizardApply = (title: string, json: string) => {
    setFormId(title);
    if (!formName.trim()) {
      setFormName(title);
    }
    // Wizard returns JSON, convert based on format if needed
    if (useToml) {
      try {
        const server = JSON.parse(json) as McpServerSpec;
        const toml = mcpServerToToml(server);
        setFormConfig(toml);
        setConfigError(validateTomlConfig(toml));
      } catch (e: any) {
        setConfigError(t("mcp.error.jsonInvalid"));
      }
    } else {
      setFormConfig(json);
      setConfigError(validateJsonConfig(json));
    }
  };

  const handleSubmit = async () => {
    const trimmedId = formId.trim();
    if (!trimmedId) {
      toast.error(t("mcp.error.idRequired"), { duration: 3000 });
      return;
    }

    // 新增模式：阻止提交重名 ID
    if (!isEditing && existingIds.includes(trimmedId)) {
      setIdError(t("mcp.error.idExists"));
      return;
    }

    // Validate configuration format
    let serverSpec: McpServerSpec;

    if (useToml) {
      // TOML mode
      const tomlError = validateTomlConfig(formConfig);
      setConfigError(tomlError);
      if (tomlError) {
        toast.error(t("mcp.error.tomlInvalid"), { duration: 3000 });
        return;
      }

      if (!formConfig.trim()) {
        // Empty configuration
        serverSpec = {
          type: "stdio",
          command: "",
          args: [],
        };
      } else {
        try {
          serverSpec = tomlToMcpServer(formConfig);
        } catch (e: any) {
          const msg = e?.message || String(e);
          setConfigError(formatTomlError(msg));
          toast.error(t("mcp.error.tomlInvalid"), { duration: 4000 });
          return;
        }
      }
    } else {
      // JSON mode
      if (!formConfig.trim()) {
        // Empty configuration
        serverSpec = {
          type: "stdio",
          command: "",
          args: [],
        };
      } else {
        try {
          // 使用智能解析器，支持带外层键的格式
          const result = parseSmartMcpJson(formConfig);
          serverSpec = result.config as McpServerSpec;
        } catch (e: any) {
          const errorMessage = e?.message || String(e);
          setConfigError(t("mcp.error.jsonInvalid") + ": " + errorMessage);
          toast.error(t("mcp.error.jsonInvalid"), { duration: 4000 });
          return;
        }
      }
    }

    // 前置必填校验
    if (serverSpec?.type === "stdio" && !serverSpec?.command?.trim()) {
      toast.error(t("mcp.error.commandRequired"), { duration: 3000 });
      return;
    }
    if (
      (serverSpec?.type === "http" || serverSpec?.type === "sse") &&
      !serverSpec?.url?.trim()
    ) {
      toast.error(t("mcp.wizard.urlRequired"), { duration: 3000 });
      return;
    }

    setSaving(true);
    try {
      // 先处理 name 字段（必填）
      const nameTrimmed = (formName || trimmedId).trim();
      const finalName = nameTrimmed || trimmedId;

      const entry: McpServer = {
        ...(initialData ? { ...initialData } : {}),
        id: trimmedId,
        name: finalName,
        server: serverSpec,
        // 使用表单中的启用状态（v3.7.0 完整重构）
        apps: enabledApps,
      };

      const descriptionTrimmed = formDescription.trim();
      if (descriptionTrimmed) {
        entry.description = descriptionTrimmed;
      } else {
        delete entry.description;
      }

      const homepageTrimmed = formHomepage.trim();
      if (homepageTrimmed) {
        entry.homepage = homepageTrimmed;
      } else {
        delete entry.homepage;
      }

      const docsTrimmed = formDocs.trim();
      if (docsTrimmed) {
        entry.docs = docsTrimmed;
      } else {
        delete entry.docs;
      }

      const parsedTags = formTags
        .split(",")
        .map((tag) => tag.trim())
        .filter((tag) => tag.length > 0);
      if (parsedTags.length > 0) {
        entry.tags = parsedTags;
      } else {
        delete entry.tags;
      }

      // 保存到统一配置
      await upsertMutation.mutateAsync(entry);
      toast.success(t("common.success"));
      await onSave(); // 通知父组件关闭表单
    } catch (error: any) {
      const detail = extractErrorMessage(error);
      const mapped = translateMcpBackendError(detail, t);
      const msg = mapped || detail || t("mcp.error.saveFailed");
      toast.error(msg, { duration: mapped || detail ? 6000 : 4000 });
    } finally {
      setSaving(false);
    }
  };

  const getFormTitle = () => {
    return isEditing ? t("mcp.editServer") : t("mcp.addServer");
  };

  return (
    <>
      <Dialog open={true} onOpenChange={(open) => !open && onClose()}>
        <DialogContent className="max-w-3xl max-h-[90vh] flex flex-col">
          <DialogHeader>
            <DialogTitle>{getFormTitle()}</DialogTitle>
          </DialogHeader>

          {/* Content - Scrollable */}
          <div className="flex-1 overflow-y-auto px-6 py-4 space-y-4">
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
                  {mcpPresets.map((preset, idx) => {
                    const descriptionKey = `mcp.presets.${preset.id}.description`;
                    return (
                      <button
                        key={preset.id}
                        type="button"
                        onClick={() => applyPreset(idx)}
                        className={`inline-flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-colors ${
                          selectedPreset === idx
                            ? "bg-emerald-500 text-white dark:bg-emerald-600"
                            : "bg-gray-100 dark:bg-gray-800 text-gray-500 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-gray-700"
                        }`}
                        title={t(descriptionKey)}
                      >
                        {preset.id}
                      </button>
                    );
                  })}
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
              <Input
                type="text"
                placeholder={t("mcp.form.titlePlaceholder")}
                value={formId}
                onChange={(e) => handleIdChange(e.target.value)}
                disabled={isEditing}
              />
            </div>

            {/* Name */}
            <div>
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
                {t("mcp.form.name")}
              </label>
              <Input
                type="text"
                placeholder={t("mcp.form.namePlaceholder")}
                value={formName}
                onChange={(e) => setFormName(e.target.value)}
              />
            </div>

            {/* 启用到哪些应用（v3.7.0 新增） */}
            <div>
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-3">
                {t("mcp.form.enabledApps")}
              </label>
              <div className="flex flex-wrap gap-4">
                <div className="flex items-center gap-2">
                  <Checkbox
                    id="enable-claude"
                    checked={enabledApps.claude}
                    onCheckedChange={(checked: boolean) =>
                      setEnabledApps({ ...enabledApps, claude: checked })
                    }
                  />
                  <label
                    htmlFor="enable-claude"
                    className="text-sm text-gray-700 dark:text-gray-300 cursor-pointer select-none"
                  >
                    {t("mcp.unifiedPanel.apps.claude")}
                  </label>
                </div>

                <div className="flex items-center gap-2">
                  <Checkbox
                    id="enable-codex"
                    checked={enabledApps.codex}
                    onCheckedChange={(checked: boolean) =>
                      setEnabledApps({ ...enabledApps, codex: checked })
                    }
                  />
                  <label
                    htmlFor="enable-codex"
                    className="text-sm text-gray-700 dark:text-gray-300 cursor-pointer select-none"
                  >
                    {t("mcp.unifiedPanel.apps.codex")}
                  </label>
                </div>

                <div className="flex items-center gap-2">
                  <Checkbox
                    id="enable-gemini"
                    checked={enabledApps.gemini}
                    onCheckedChange={(checked: boolean) =>
                      setEnabledApps({ ...enabledApps, gemini: checked })
                    }
                  />
                  <label
                    htmlFor="enable-gemini"
                    className="text-sm text-gray-700 dark:text-gray-300 cursor-pointer select-none"
                  >
                    {t("mcp.unifiedPanel.apps.gemini")}
                  </label>
                </div>
              </div>
            </div>

            {/* 可折叠的附加信息按钮 */}
            <div>
              <button
                type="button"
                onClick={() => setShowMetadata(!showMetadata)}
                className="flex items-center gap-2 text-sm font-medium text-gray-700 dark:text-gray-300 hover:text-gray-900 dark:hover:text-gray-100 transition-colors"
              >
                {showMetadata ? (
                  <ChevronUp size={16} />
                ) : (
                  <ChevronDown size={16} />
                )}
                {t("mcp.form.additionalInfo")}
              </button>
            </div>

            {/* 附加信息区域（可折叠） */}
            {showMetadata && (
              <>
                {/* Description (描述) */}
                <div>
                  <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
                    {t("mcp.form.description")}
                  </label>
                  <Input
                    type="text"
                    placeholder={t("mcp.form.descriptionPlaceholder")}
                    value={formDescription}
                    onChange={(e) => setFormDescription(e.target.value)}
                  />
                </div>

                {/* Tags */}
                <div>
                  <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
                    {t("mcp.form.tags")}
                  </label>
                  <Input
                    type="text"
                    placeholder={t("mcp.form.tagsPlaceholder")}
                    value={formTags}
                    onChange={(e) => setFormTags(e.target.value)}
                  />
                </div>

                {/* Homepage */}
                <div>
                  <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
                    {t("mcp.form.homepage")}
                  </label>
                  <Input
                    type="text"
                    placeholder={t("mcp.form.homepagePlaceholder")}
                    value={formHomepage}
                    onChange={(e) => setFormHomepage(e.target.value)}
                  />
                </div>

                {/* Docs */}
                <div>
                  <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
                    {t("mcp.form.docs")}
                  </label>
                  <Input
                    type="text"
                    placeholder={t("mcp.form.docsPlaceholder")}
                    value={formDocs}
                    onChange={(e) => setFormDocs(e.target.value)}
                  />
                </div>
              </>
            )}

            {/* 配置输入框（根据格式显示 JSON 或 TOML） */}
            <div>
              <div className="flex items-center justify-between mb-2">
                <label className="text-sm font-medium text-gray-700 dark:text-gray-300">
                  {useToml
                    ? t("mcp.form.tomlConfig")
                    : t("mcp.form.jsonConfig")}
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
              <Textarea
                className="h-48 resize-none font-mono text-xs"
                placeholder={
                  useToml
                    ? t("mcp.form.tomlPlaceholder")
                    : t("mcp.form.jsonPlaceholder")
                }
                value={formConfig}
                onChange={(e) => handleConfigChange(e.target.value)}
              />
              {/* 格式化按钮（仅 JSON 模式） */}
              {!useToml && (
                <div className="flex items-center justify-between mt-2">
                  <button
                    type="button"
                    onClick={handleFormatJson}
                    className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-gray-700 dark:text-gray-300 hover:text-blue-600 dark:hover:text-blue-400 transition-colors"
                  >
                    <Wand2 className="w-3.5 h-3.5" />
                    {t("common.format")}
                  </button>
                </div>
              )}
              {configError && (
                <div className="flex items-center gap-2 mt-2 text-red-500 dark:text-red-400 text-sm">
                  <AlertCircle size={16} />
                  <span>{configError}</span>
                </div>
              )}
            </div>
          </div>

          {/* Footer */}
          <DialogFooter className="flex justify-end gap-3 pt-4">
            {/* 操作按钮 */}
            <Button type="button" variant="ghost" onClick={onClose}>
              {t("common.cancel")}
            </Button>
            <Button
              type="button"
              onClick={handleSubmit}
              disabled={saving || (!isEditing && !!idError)}
              variant="mcp"
            >
              {isEditing ? <Save size={16} /> : <Plus size={16} />}
              {saving
                ? t("common.saving")
                : isEditing
                  ? t("common.save")
                  : t("common.add")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Wizard Modal */}
      <McpWizardModal
        isOpen={isWizardOpen}
        onClose={() => setIsWizardOpen(false)}
        onApply={handleWizardApply}
        initialTitle={formId}
        initialServer={wizardInitialSpec}
      />
    </>
  );
};

export default McpFormModal;
