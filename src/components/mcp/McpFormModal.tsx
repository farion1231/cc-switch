import React, { useMemo, useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Save, Plus, AlertCircle, ChevronDown, ChevronUp } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import JsonEditor from "@/components/JsonEditor";
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
import { parseSmartMcpJson } from "@/utils/formatters";
import { useMcpValidation } from "./useMcpValidation";
import { useUpsertMcpServer } from "@/hooks/useMcp";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";

interface McpFormModalProps {
  editingId?: string;
  initialData?: McpServer;
  onSave: () => Promise<void>;
  onClose: () => void;
  existingIds?: string[];
  defaultFormat?: "json" | "toml";
  defaultEnabledApps?: AppId[];
}

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

  const [enabledApps, setEnabledApps] = useState<{
    claude: boolean;
    codex: boolean;
    gemini: boolean;
  }>(() => {
    if (initialData?.apps) {
      return { ...initialData.apps };
    }
    return {
      claude: defaultEnabledApps.includes("claude"),
      codex: defaultEnabledApps.includes("codex"),
      gemini: defaultEnabledApps.includes("gemini"),
    };
  });

  const isEditing = !!editingId;

  const hasAdditionalInfo = !!(
    initialData?.description ||
    initialData?.tags?.length ||
    initialData?.homepage ||
    initialData?.docs
  );

  const [showMetadata, setShowMetadata] = useState(
    isEditing ? hasAdditionalInfo : false,
  );

  const useTomlFormat = useMemo(() => {
    if (initialData?.server) {
      return defaultFormat === "toml";
    }
    return defaultFormat === "toml";
  }, [defaultFormat, initialData]);

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
  const [isDarkMode, setIsDarkMode] = useState(false);

  useEffect(() => {
    setIsDarkMode(document.documentElement.classList.contains("dark"));

    const observer = new MutationObserver(() => {
      setIsDarkMode(document.documentElement.classList.contains("dark"));
    });

    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class"],
    });

    return () => observer.disconnect();
  }, []);

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

  const applyCustom = () => {
    setSelectedPreset(-1);
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
    const nextValue = useToml ? normalizeTomlText(value) : value;
    setFormConfig(nextValue);

    if (useToml) {
      const err = validateTomlConfig(nextValue);
      if (err) {
        setConfigError(err);
        return;
      }

      if (nextValue.trim() && !formId.trim()) {
        const extractedId = extractIdFromToml(nextValue);
        if (extractedId) {
          setFormId(extractedId);
        }
      }
    } else {
      try {
        const result = parseSmartMcpJson(value);
        const configJson = JSON.stringify(result.config);
        const validationErr = validateJsonConfig(configJson);

        if (validationErr) {
          setConfigError(validationErr);
          return;
        }

        if (result.id && !formId.trim() && !isEditing) {
          const uniqueId = ensureUniqueId(result.id);
          setFormId(uniqueId);

          if (!formName.trim()) {
            setFormName(result.id);
          }
        }

        setConfigError("");
      } catch (err: any) {
        const errorMessage = err?.message || String(err);
        setConfigError(t("mcp.error.jsonInvalid") + ": " + errorMessage);
      }
    }
  };

  const handleWizardApply = (title: string, json: string) => {
    setFormId(title);
    if (!formName.trim()) {
      setFormName(title);
    }
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

    if (!isEditing && existingIds.includes(trimmedId)) {
      setIdError(t("mcp.error.idExists"));
      return;
    }

    let serverSpec: McpServerSpec;

    if (useToml) {
      const tomlError = validateTomlConfig(formConfig);
      setConfigError(tomlError);
      if (tomlError) {
        toast.error(t("mcp.error.tomlInvalid"), { duration: 3000 });
        return;
      }

      if (!formConfig.trim()) {
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
      if (!formConfig.trim()) {
        serverSpec = {
          type: "stdio",
          command: "",
          args: [],
        };
      } else {
        try {
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
      const nameTrimmed = (formName || trimmedId).trim();
      const finalName = nameTrimmed || trimmedId;

      const entry: McpServer = {
        ...(initialData ? { ...initialData } : {}),
        id: trimmedId,
        name: finalName,
        server: serverSpec,
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

      await upsertMutation.mutateAsync(entry);
      toast.success(t("common.success"), { closeButton: true });
      await onSave();
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
      <FullScreenPanel
        isOpen={true}
        title={getFormTitle()}
        onClose={onClose}
        footer={
          <Button
            type="button"
            onClick={handleSubmit}
            disabled={saving || (!isEditing && !!idError)}
            className="bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {isEditing ? <Save size={16} /> : <Plus size={16} />}
            {saving
              ? t("common.saving")
              : isEditing
                ? t("common.save")
                : t("common.add")}
          </Button>
        }
      >
        <div className="flex flex-col h-full gap-6">
          {/* 上半部分：表单字段 */}
          <div className="glass rounded-xl p-6 border border-white/10 space-y-6 flex-shrink-0">
            {/* 预设选择（仅新增时展示） */}
            {!isEditing && (
              <div>
                <label className="block text-sm font-medium text-foreground mb-3">
                  {t("mcp.presets.title")}
                </label>
                <div className="flex flex-wrap gap-2">
                  <button
                    type="button"
                    onClick={applyCustom}
                    className={`inline-flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-colors ${
                      selectedPreset === -1
                        ? "bg-emerald-500 text-white dark:bg-emerald-600"
                        : "bg-accent text-muted-foreground hover:bg-accent/80"
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
                            : "bg-accent text-muted-foreground hover:bg-accent/80"
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
                <label className="block text-sm font-medium text-foreground">
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
              <label className="block text-sm font-medium text-foreground mb-2">
                {t("mcp.form.name")}
              </label>
              <Input
                type="text"
                placeholder={t("mcp.form.namePlaceholder")}
                value={formName}
                onChange={(e) => setFormName(e.target.value)}
              />
            </div>

            {/* 启用到哪些应用 */}
            <div>
              <label className="block text-sm font-medium text-foreground mb-3">
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
                    className="text-sm text-foreground cursor-pointer select-none"
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
                    className="text-sm text-foreground cursor-pointer select-none"
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
                    className="text-sm text-foreground cursor-pointer select-none"
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
                className="flex items-center gap-2 text-sm font-medium text-muted-foreground hover:text-foreground transition-colors"
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
                <div>
                  <label className="block text-sm font-medium text-foreground mb-2">
                    {t("mcp.form.description")}
                  </label>
                  <Input
                    type="text"
                    placeholder={t("mcp.form.descriptionPlaceholder")}
                    value={formDescription}
                    onChange={(e) => setFormDescription(e.target.value)}
                  />
                </div>

                <div>
                  <label className="block text-sm font-medium text-foreground mb-2">
                    {t("mcp.form.tags")}
                  </label>
                  <Input
                    type="text"
                    placeholder={t("mcp.form.tagsPlaceholder")}
                    value={formTags}
                    onChange={(e) => setFormTags(e.target.value)}
                  />
                </div>

                <div>
                  <label className="block text-sm font-medium text-foreground mb-2">
                    {t("mcp.form.homepage")}
                  </label>
                  <Input
                    type="text"
                    placeholder={t("mcp.form.homepagePlaceholder")}
                    value={formHomepage}
                    onChange={(e) => setFormHomepage(e.target.value)}
                  />
                </div>

                <div>
                  <label className="block text-sm font-medium text-foreground mb-2">
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
          </div>

          {/* 下半部分：JSON 配置编辑器 - 自适应剩余高度 */}
          <div className="glass rounded-xl p-6 border border-white/10 flex flex-col flex-1 min-h-0">
            <div className="flex items-center justify-between mb-4 flex-shrink-0">
              <label className="text-sm font-medium text-foreground">
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
            <div className="flex-1 min-h-0 flex flex-col">
              <div className="flex-1 min-h-0">
                <JsonEditor
                  value={formConfig}
                  onChange={handleConfigChange}
                  placeholder={
                    useToml
                      ? t("mcp.form.tomlPlaceholder")
                      : t("mcp.form.jsonPlaceholder")
                  }
                  darkMode={isDarkMode}
                  rows={12}
                  showValidation={!useToml}
                  language={useToml ? "javascript" : "json"}
                  height="100%"
                />
              </div>
              {configError && (
                <div className="flex items-center gap-2 mt-2 text-red-500 dark:text-red-400 text-sm flex-shrink-0">
                  <AlertCircle size={16} />
                  <span>{configError}</span>
                </div>
              )}
            </div>
          </div>
        </div>
      </FullScreenPanel>

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
