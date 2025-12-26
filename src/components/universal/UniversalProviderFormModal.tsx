import { useState, useEffect, useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Eye, EyeOff, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { ProviderIcon } from "@/components/ProviderIcon";
import JsonEditor from "@/components/JsonEditor";
import type { UniversalProvider, UniversalProviderModels } from "@/types";
import {
  universalProviderPresets,
  createUniversalProviderFromPreset,
  type UniversalProviderPreset,
} from "@/config/universalProviderPresets";

interface UniversalProviderFormModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSave: (provider: UniversalProvider) => void;
  onSaveAndSync?: (provider: UniversalProvider) => void;
  editingProvider?: UniversalProvider | null;
  initialPreset?: UniversalProviderPreset | null;
}

export function UniversalProviderFormModal({
  isOpen,
  onClose,
  onSave,
  onSaveAndSync,
  editingProvider,
  initialPreset,
}: UniversalProviderFormModalProps) {
  const { t } = useTranslation();
  const isEditMode = !!editingProvider;

  // 表单状态
  const [selectedPreset, setSelectedPreset] =
    useState<UniversalProviderPreset | null>(null);
  const [name, setName] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [showApiKey, setShowApiKey] = useState(false);
  const [websiteUrl, setWebsiteUrl] = useState("");
  const [notes, setNotes] = useState("");

  // 应用启用状态
  const [claudeEnabled, setClaudeEnabled] = useState(true);
  const [codexEnabled, setCodexEnabled] = useState(true);
  const [geminiEnabled, setGeminiEnabled] = useState(true);

  // 模型配置
  const [models, setModels] = useState<UniversalProviderModels>({});

  // 保存并同步确认弹窗
  const [syncConfirmOpen, setSyncConfirmOpen] = useState(false);
  const [pendingProvider, setPendingProvider] =
    useState<UniversalProvider | null>(null);

  // 初始化表单
  useEffect(() => {
    if (editingProvider) {
      // 编辑模式：加载现有数据
      setName(editingProvider.name);
      setBaseUrl(editingProvider.baseUrl);
      setApiKey(editingProvider.apiKey);
      setWebsiteUrl(editingProvider.websiteUrl || "");
      setNotes(editingProvider.notes || "");
      setClaudeEnabled(editingProvider.apps.claude);
      setCodexEnabled(editingProvider.apps.codex);
      setGeminiEnabled(editingProvider.apps.gemini);
      setModels(editingProvider.models || {});

      // 尝试匹配预设
      const preset = universalProviderPresets.find(
        (p) => p.providerType === editingProvider.providerType,
      );
      setSelectedPreset(preset || null);
    } else {
      // 新建模式：使用传入的预设或默认选择第一个预设
      const defaultPreset = initialPreset || universalProviderPresets[0];
      setSelectedPreset(defaultPreset);
      setName(defaultPreset.name);
      setBaseUrl("");
      setApiKey("");
      setWebsiteUrl(defaultPreset.websiteUrl || "");
      setNotes("");
      setClaudeEnabled(defaultPreset.defaultApps.claude);
      setCodexEnabled(defaultPreset.defaultApps.codex);
      setGeminiEnabled(defaultPreset.defaultApps.gemini);
      setModels(JSON.parse(JSON.stringify(defaultPreset.defaultModels)));
    }
  }, [editingProvider, initialPreset, isOpen]);

  // 选择预设
  const handlePresetSelect = useCallback(
    (preset: UniversalProviderPreset) => {
      setSelectedPreset(preset);
      if (!isEditMode) {
        setName(preset.name);
        setClaudeEnabled(preset.defaultApps.claude);
        setCodexEnabled(preset.defaultApps.codex);
        setGeminiEnabled(preset.defaultApps.gemini);
        setModels(JSON.parse(JSON.stringify(preset.defaultModels)));
      }
    },
    [isEditMode],
  );

  // 更新模型配置
  const updateModel = useCallback(
    (app: "claude" | "codex" | "gemini", field: string, value: string) => {
      setModels((prev) => ({
        ...prev,
        [app]: {
          ...(prev[app] || {}),
          [field]: value,
        },
      }));
    },
    [],
  );

  // 计算 Claude 配置 JSON 预览
  const claudeConfigJson = useMemo(() => {
    if (!claudeEnabled) return null;
    const model = models.claude?.model || "claude-sonnet-4-20250514";
    const haiku = models.claude?.haikuModel || "claude-haiku-4-20250514";
    const sonnet = models.claude?.sonnetModel || "claude-sonnet-4-20250514";
    const opus = models.claude?.opusModel || "claude-sonnet-4-20250514";
    return {
      env: {
        ANTHROPIC_BASE_URL: baseUrl,
        ANTHROPIC_AUTH_TOKEN: apiKey,
        ANTHROPIC_MODEL: model,
        ANTHROPIC_DEFAULT_HAIKU_MODEL: haiku,
        ANTHROPIC_DEFAULT_SONNET_MODEL: sonnet,
        ANTHROPIC_DEFAULT_OPUS_MODEL: opus,
      },
    };
  }, [claudeEnabled, baseUrl, apiKey, models.claude]);

  // 计算 Codex 配置 JSON 预览
  const codexConfigJson = useMemo(() => {
    if (!codexEnabled) return null;
    const model = models.codex?.model || "gpt-4o";
    const reasoningEffort = models.codex?.reasoningEffort || "high";
    // 确保 base_url 以 /v1 结尾（Codex 使用 OpenAI 兼容 API）
    const codexBaseUrl = baseUrl.endsWith("/v1")
      ? baseUrl
      : `${baseUrl.replace(/\/+$/, "")}/v1`;
    const configToml = `model_provider = "newapi"
model = "${model}"
model_reasoning_effort = "${reasoningEffort}"
disable_response_storage = true

[model_providers.newapi]
name = "NewAPI"
base_url = "${codexBaseUrl}"
wire_api = "responses"
requires_openai_auth = true`;
    return {
      auth: {
        OPENAI_API_KEY: apiKey,
      },
      config: configToml,
    };
  }, [codexEnabled, baseUrl, apiKey, models.codex]);

  // 计算 Gemini 配置 JSON 预览
  const geminiConfigJson = useMemo(() => {
    if (!geminiEnabled) return null;
    const model = models.gemini?.model || "gemini-2.5-pro";
    return {
      env: {
        GOOGLE_GEMINI_BASE_URL: baseUrl,
        GEMINI_API_KEY: apiKey,
        GEMINI_MODEL: model,
      },
    };
  }, [geminiEnabled, baseUrl, apiKey, models.gemini]);

  // 提交表单
  const handleSubmit = useCallback(() => {
    if (!name.trim() || !baseUrl.trim() || !apiKey.trim()) {
      return;
    }

    const provider: UniversalProvider = editingProvider
      ? {
          ...editingProvider,
          name: name.trim(),
          baseUrl: baseUrl.trim(),
          apiKey: apiKey.trim(),
          websiteUrl: websiteUrl.trim() || undefined,
          notes: notes.trim() || undefined,
          apps: {
            claude: claudeEnabled,
            codex: codexEnabled,
            gemini: geminiEnabled,
          },
          models,
        }
      : createUniversalProviderFromPreset(
          selectedPreset || universalProviderPresets[0],
          crypto.randomUUID(),
          baseUrl.trim(),
          apiKey.trim(),
          name.trim(),
        );

    // 如果是新建，更新应用启用状态和模型
    if (!editingProvider) {
      provider.apps = {
        claude: claudeEnabled,
        codex: codexEnabled,
        gemini: geminiEnabled,
      };
      provider.models = models;
      provider.websiteUrl = websiteUrl.trim() || undefined;
      provider.notes = notes.trim() || undefined;
    }

    onSave(provider);
    onClose();
  }, [
    editingProvider,
    name,
    baseUrl,
    apiKey,
    websiteUrl,
    notes,
    claudeEnabled,
    codexEnabled,
    geminiEnabled,
    models,
    selectedPreset,
    onSave,
    onClose,
  ]);

  // 构建 provider 对象的辅助函数
  const buildProvider = useCallback((): UniversalProvider | null => {
    if (!name.trim() || !baseUrl.trim() || !apiKey.trim()) {
      return null;
    }

    const provider: UniversalProvider = editingProvider
      ? {
          ...editingProvider,
          name: name.trim(),
          baseUrl: baseUrl.trim(),
          apiKey: apiKey.trim(),
          websiteUrl: websiteUrl.trim() || undefined,
          notes: notes.trim() || undefined,
          apps: {
            claude: claudeEnabled,
            codex: codexEnabled,
            gemini: geminiEnabled,
          },
          models,
        }
      : createUniversalProviderFromPreset(
          selectedPreset || universalProviderPresets[0],
          crypto.randomUUID(),
          baseUrl.trim(),
          apiKey.trim(),
          name.trim(),
        );

    // 如果是新建，更新应用启用状态和模型
    if (!editingProvider) {
      provider.apps = {
        claude: claudeEnabled,
        codex: codexEnabled,
        gemini: geminiEnabled,
      };
      provider.models = models;
      provider.websiteUrl = websiteUrl.trim() || undefined;
      provider.notes = notes.trim() || undefined;
    }

    return provider;
  }, [
    editingProvider,
    name,
    baseUrl,
    apiKey,
    websiteUrl,
    notes,
    claudeEnabled,
    codexEnabled,
    geminiEnabled,
    models,
    selectedPreset,
  ]);

  // 打开保存并同步确认弹窗
  const handleSaveAndSyncClick = useCallback(() => {
    const provider = buildProvider();
    if (!provider || !onSaveAndSync) return;

    setPendingProvider(provider);
    setSyncConfirmOpen(true);
  }, [buildProvider, onSaveAndSync]);

  // 确认保存并同步
  const confirmSaveAndSync = useCallback(() => {
    if (!pendingProvider || !onSaveAndSync) return;

    onSaveAndSync(pendingProvider);
    setSyncConfirmOpen(false);
    setPendingProvider(null);
    onClose();
  }, [pendingProvider, onSaveAndSync, onClose]);

  const footer = (
    <>
      <Button variant="outline" onClick={onClose}>
        {t("common.cancel", { defaultValue: "取消" })}
      </Button>
      {isEditMode && onSaveAndSync ? (
        <Button
          onClick={handleSaveAndSyncClick}
          disabled={!name.trim() || !baseUrl.trim() || !apiKey.trim()}
        >
          <RefreshCw className="mr-1.5 h-4 w-4" />
          {t("universalProvider.saveAndSync", { defaultValue: "保存并同步" })}
        </Button>
      ) : (
        <Button
          onClick={handleSubmit}
          disabled={!name.trim() || !baseUrl.trim() || !apiKey.trim()}
        >
          {t("common.add", { defaultValue: "添加" })}
        </Button>
      )}
    </>
  );

  return (
    <FullScreenPanel
      isOpen={isOpen}
      title={
        isEditMode
          ? t("universalProvider.edit", { defaultValue: "编辑统一供应商" })
          : t("universalProvider.add", { defaultValue: "添加统一供应商" })
      }
      onClose={onClose}
      footer={footer}
    >
      <div className="space-y-6">
        {/* 预设选择（仅新建模式） */}
        {!isEditMode && (
          <div className="space-y-3">
            <Label>
              {t("universalProvider.selectPreset", {
                defaultValue: "选择预设类型",
              })}
            </Label>
            <div className="flex flex-wrap gap-2">
              {universalProviderPresets.map((preset) => (
                <button
                  key={preset.providerType}
                  type="button"
                  onClick={() => handlePresetSelect(preset)}
                  className={`inline-flex items-center gap-2 rounded-lg px-4 py-2 text-sm font-medium transition-colors ${
                    selectedPreset?.providerType === preset.providerType
                      ? "bg-primary text-primary-foreground"
                      : "bg-accent text-muted-foreground hover:bg-accent/80"
                  }`}
                >
                  <ProviderIcon
                    icon={preset.icon}
                    name={preset.name}
                    size={16}
                  />
                  {preset.name}
                </button>
              ))}
            </div>
            {selectedPreset?.description && (
              <p className="text-xs text-muted-foreground">
                {selectedPreset.description}
              </p>
            )}
          </div>
        )}

        {/* 基本信息 */}
        <div className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="name">
              {t("universalProvider.name", { defaultValue: "名称" })}
            </Label>
            <Input
              id="name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t("universalProvider.namePlaceholder", {
                defaultValue: "例如：我的 NewAPI",
              })}
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="baseUrl">
              {t("universalProvider.baseUrl", { defaultValue: "API 地址" })}
            </Label>
            <Input
              id="baseUrl"
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
              placeholder="https://api.example.com"
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="apiKey">
              {t("universalProvider.apiKey", { defaultValue: "API Key" })}
            </Label>
            <div className="relative">
              <Input
                id="apiKey"
                type={showApiKey ? "text" : "password"}
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                placeholder="sk-..."
                className="pr-10"
              />
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className="absolute right-0 top-0 h-full px-3"
                onClick={() => setShowApiKey(!showApiKey)}
              >
                {showApiKey ? (
                  <EyeOff className="h-4 w-4" />
                ) : (
                  <Eye className="h-4 w-4" />
                )}
              </Button>
            </div>
          </div>

          <div className="space-y-2">
            <Label htmlFor="websiteUrl">
              {t("universalProvider.websiteUrl", { defaultValue: "官网地址" })}
            </Label>
            <Input
              id="websiteUrl"
              value={websiteUrl}
              onChange={(e) => setWebsiteUrl(e.target.value)}
              placeholder={t("universalProvider.websiteUrlPlaceholder", {
                defaultValue: "https://example.com（可选，用于在列表中显示）",
              })}
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="notes">
              {t("universalProvider.notes", { defaultValue: "备注" })}
            </Label>
            <Input
              id="notes"
              value={notes}
              onChange={(e) => setNotes(e.target.value)}
              placeholder={t("universalProvider.notesPlaceholder", {
                defaultValue: "可选：添加备注信息",
              })}
            />
          </div>
        </div>

        {/* 应用启用 */}
        <div className="space-y-3">
          <Label>
            {t("universalProvider.enabledApps", { defaultValue: "启用的应用" })}
          </Label>
          <div className="flex flex-col gap-3">
            <div className="flex items-center justify-between rounded-lg border p-3">
              <div className="flex items-center gap-2">
                <ProviderIcon icon="claude" name="Claude" size={20} />
                <span className="font-medium">Claude Code</span>
              </div>
              <Switch
                checked={claudeEnabled}
                onCheckedChange={setClaudeEnabled}
              />
            </div>
            <div className="flex items-center justify-between rounded-lg border p-3">
              <div className="flex items-center gap-2">
                <ProviderIcon icon="openai" name="Codex" size={20} />
                <span className="font-medium">OpenAI Codex</span>
              </div>
              <Switch
                checked={codexEnabled}
                onCheckedChange={setCodexEnabled}
              />
            </div>
            <div className="flex items-center justify-between rounded-lg border p-3">
              <div className="flex items-center gap-2">
                <ProviderIcon icon="gemini" name="Gemini" size={20} />
                <span className="font-medium">Gemini CLI</span>
              </div>
              <Switch
                checked={geminiEnabled}
                onCheckedChange={setGeminiEnabled}
              />
            </div>
          </div>
        </div>

        {/* 模型配置 */}
        <div className="space-y-4">
          <Label>
            {t("universalProvider.modelConfig", { defaultValue: "模型配置" })}
          </Label>

          {/* Claude 模型 */}
          {claudeEnabled && (
            <div className="space-y-3 rounded-lg border p-4">
              <div className="flex items-center gap-2 font-medium">
                <ProviderIcon icon="claude" name="Claude" size={16} />
                Claude
              </div>
              <div className="grid gap-3 sm:grid-cols-2">
                <div className="space-y-1">
                  <Label className="text-xs">
                    {t("universalProvider.model", { defaultValue: "主模型" })}
                  </Label>
                  <Input
                    value={models.claude?.model || ""}
                    onChange={(e) =>
                      updateModel("claude", "model", e.target.value)
                    }
                    placeholder="claude-sonnet-4-20250514"
                  />
                </div>
                <div className="space-y-1">
                  <Label className="text-xs">Haiku</Label>
                  <Input
                    value={models.claude?.haikuModel || ""}
                    onChange={(e) =>
                      updateModel("claude", "haikuModel", e.target.value)
                    }
                    placeholder="claude-haiku-4-20250514"
                  />
                </div>
                <div className="space-y-1">
                  <Label className="text-xs">Sonnet</Label>
                  <Input
                    value={models.claude?.sonnetModel || ""}
                    onChange={(e) =>
                      updateModel("claude", "sonnetModel", e.target.value)
                    }
                    placeholder="claude-sonnet-4-20250514"
                  />
                </div>
                <div className="space-y-1">
                  <Label className="text-xs">Opus</Label>
                  <Input
                    value={models.claude?.opusModel || ""}
                    onChange={(e) =>
                      updateModel("claude", "opusModel", e.target.value)
                    }
                    placeholder="claude-sonnet-4-20250514"
                  />
                </div>
              </div>
            </div>
          )}

          {/* Codex 模型 */}
          {codexEnabled && (
            <div className="space-y-3 rounded-lg border p-4">
              <div className="flex items-center gap-2 font-medium">
                <ProviderIcon icon="openai" name="Codex" size={16} />
                Codex
              </div>
              <div className="grid gap-3 sm:grid-cols-2">
                <div className="space-y-1">
                  <Label className="text-xs">
                    {t("universalProvider.model", { defaultValue: "模型" })}
                  </Label>
                  <Input
                    value={models.codex?.model || ""}
                    onChange={(e) =>
                      updateModel("codex", "model", e.target.value)
                    }
                    placeholder="gpt-4o"
                  />
                </div>
                <div className="space-y-1">
                  <Label className="text-xs">Reasoning Effort</Label>
                  <Input
                    value={models.codex?.reasoningEffort || ""}
                    onChange={(e) =>
                      updateModel("codex", "reasoningEffort", e.target.value)
                    }
                    placeholder="high"
                  />
                </div>
              </div>
            </div>
          )}

          {/* Gemini 模型 */}
          {geminiEnabled && (
            <div className="space-y-3 rounded-lg border p-4">
              <div className="flex items-center gap-2 font-medium">
                <ProviderIcon icon="gemini" name="Gemini" size={16} />
                Gemini
              </div>
              <div className="space-y-1">
                <Label className="text-xs">
                  {t("universalProvider.model", { defaultValue: "模型" })}
                </Label>
                <Input
                  value={models.gemini?.model || ""}
                  onChange={(e) =>
                    updateModel("gemini", "model", e.target.value)
                  }
                  placeholder="gemini-2.5-pro"
                />
              </div>
            </div>
          )}
        </div>

        {/* 配置 JSON 预览 */}
        {isEditMode && (claudeEnabled || codexEnabled || geminiEnabled) && (
          <div className="space-y-4">
            <Label>
              {t("universalProvider.configJsonPreview", {
                defaultValue: "配置 JSON 预览",
              })}
            </Label>
            <p className="text-xs text-muted-foreground">
              {t("universalProvider.configJsonPreviewHint", {
                defaultValue:
                  "以下是将要同步到各应用的配置内容（仅覆盖显示的字段，保留其他自定义配置）",
              })}
            </p>

            {/* Claude JSON */}
            {claudeConfigJson && (
              <div className="space-y-2">
                <div className="flex items-center gap-2 text-sm font-medium">
                  <ProviderIcon icon="claude" name="Claude" size={16} />
                  Claude
                </div>
                <JsonEditor
                  value={JSON.stringify(claudeConfigJson, null, 2)}
                  onChange={() => {}}
                  height={180}
                />
              </div>
            )}

            {/* Codex JSON */}
            {codexConfigJson && (
              <div className="space-y-2">
                <div className="flex items-center gap-2 text-sm font-medium">
                  <ProviderIcon icon="openai" name="Codex" size={16} />
                  Codex
                </div>
                <JsonEditor
                  value={JSON.stringify(codexConfigJson, null, 2)}
                  onChange={() => {}}
                  height={280}
                />
              </div>
            )}

            {/* Gemini JSON */}
            {geminiConfigJson && (
              <div className="space-y-2">
                <div className="flex items-center gap-2 text-sm font-medium">
                  <ProviderIcon icon="gemini" name="Gemini" size={16} />
                  Gemini
                </div>
                <JsonEditor
                  value={JSON.stringify(geminiConfigJson, null, 2)}
                  onChange={() => {}}
                  height={140}
                />
              </div>
            )}
          </div>
        )}
      </div>

      {/* 保存并同步确认弹窗 */}
      <ConfirmDialog
        isOpen={syncConfirmOpen}
        title={t("universalProvider.syncConfirmTitle", {
          defaultValue: "同步统一供应商",
        })}
        message={t("universalProvider.syncConfirmDescription", {
          defaultValue: `同步 "${name}" 将会覆盖 Claude、Codex 和 Gemini 中关联的供应商配置。确定要继续吗？`,
          name: name,
        })}
        confirmText={t("universalProvider.saveAndSync", {
          defaultValue: "保存并同步",
        })}
        onConfirm={confirmSaveAndSync}
        onCancel={() => {
          setSyncConfirmOpen(false);
          setPendingProvider(null);
        }}
      />
    </FullScreenPanel>
  );
}
