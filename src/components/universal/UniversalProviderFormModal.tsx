import { useState, useLayoutEffect, useCallback, useMemo } from "react";
import { useDarkMode } from "@/hooks/useDarkMode";
import { useTranslation } from "react-i18next";
import {
  ChevronDown,
  Download,
  Eye,
  EyeOff,
  Loader2,
  RefreshCw,
  Wand2,
} from "lucide-react";
import { toast } from "@/lib/toast";
import {
  fetchModelsForConfig,
  showFetchModelsError,
} from "@/lib/api/model-fetch";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ImeSafeInput } from "@/components/ui/ime-safe-input";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
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
import { deepClone } from "@/utils/deepClone";
import { ProviderPresetSelector } from "@/components/providers/forms/ProviderPresetSelector";
import { providerPresets } from "@/config/claudeProviderPresets";

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
  const isDarkMode = useDarkMode();
  const { t } = useTranslation();
  const isEditMode = !!editingProvider;

  // 表单状态
  const [selectedPreset, setSelectedPreset] =
    useState<UniversalProviderPreset | null>(null);
  const [name, setName] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [apiFormat, setApiFormat] =
    useState<UniversalProvider["apiFormat"]>("anthropic");
  const [showApiKey, setShowApiKey] = useState(false);
  const [websiteUrl, setWebsiteUrl] = useState("");
  const [notes, setNotes] = useState("");

  // 应用启用状态
  const [claudeEnabled, setClaudeEnabled] = useState(true);
  const [claudeDesktopEnabled, setClaudeDesktopEnabled] = useState(false);
  const [hermesEnabled, setHermesEnabled] = useState(false);
  const [codexEnabled, setCodexEnabled] = useState(true);
  const [geminiEnabled, setGeminiEnabled] = useState(true);

  // 模型配置
  const [models, setModels] = useState<UniversalProviderModels>({});

  // 保存并同步确认弹窗
  const [syncConfirmOpen, setSyncConfirmOpen] = useState(false);
  const [pendingProvider, setPendingProvider] =
    useState<UniversalProvider | null>(null);

  // Populate before paint so opening the panel never shows empty/stale fields.
  useLayoutEffect(() => {
    if (editingProvider) {
      // 编辑模式：加载现有数据
      setName(editingProvider.name);
      setBaseUrl(editingProvider.baseUrl);
      setApiKey(editingProvider.apiKey);
      setApiFormat(editingProvider.apiFormat ?? "anthropic");
      setIcon(editingProvider.icon);
      setIconColor(editingProvider.iconColor);
      setWebsiteUrl(editingProvider.websiteUrl || "");
      setNotes(editingProvider.notes || "");
      setClaudeEnabled(editingProvider.apps.claude);
      setClaudeDesktopEnabled(editingProvider.apps.claudeDesktop ?? false);
      setHermesEnabled(editingProvider.apps.hermes ?? false);
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
      setApiFormat("anthropic");
      setIcon(undefined);
      setIconColor(undefined);
      setWebsiteUrl(defaultPreset.websiteUrl || "");
      setNotes("");
      setClaudeEnabled(defaultPreset.defaultApps.claude);
      setCodexEnabled(defaultPreset.defaultApps.codex);
      setGeminiEnabled(defaultPreset.defaultApps.gemini);
      setModels(deepClone(defaultPreset.defaultModels));
    }
  }, [editingProvider, initialPreset, isOpen]);

  // 选择预设
  // 厂商标准库选择（ProviderPresetSelector 就地模式）
  const [selectedLibraryPresetId, setSelectedLibraryPresetId] = useState<
    string | null
  >(null);
  const libraryPresetEntries = useMemo(
    () =>
      providerPresets
        .map((preset, index) => ({ id: `claude-${index}`, preset }))
        .filter((entry) => !(entry.preset as { hidden?: boolean }).hidden),
    [],
  );
  const handleLibraryPresetChange = useCallback(
    (entryId: string) => {
      setSelectedLibraryPresetId(entryId);
      const entry = libraryPresetEntries.find((item) => item.id === entryId);
      if (!entry || entryId === "custom") return;
      const preset = entry.preset as (typeof providerPresets)[number];
      const env =
        (preset.settingsConfig as { env?: Record<string, unknown> }).env ?? {};
      const str = (v: unknown) =>
        typeof v === "string" && v.trim() ? v : undefined;
      setSelectedPreset(null); // 库预设优先于通用预设（NewAPI 等）
      setName(preset.name);
      setWebsiteUrl(preset.websiteUrl || "");
      if (preset.icon) {
        setIcon(preset.icon);
        setIconColor(preset.iconColor ?? undefined);
      }
      const base = str(env.ANTHROPIC_BASE_URL);
      if (base) setBaseUrl(base);
      setApiFormat(preset.apiFormat ?? "anthropic");
      const mainModel = str(env.ANTHROPIC_MODEL);
      if (mainModel) {
        setModels((prev) => ({
          ...prev,
          claude: {
            ...prev.claude,
            model: mainModel,
            haikuModel: str(env.ANTHROPIC_DEFAULT_HAIKU_MODEL) ?? mainModel,
            sonnetModel: str(env.ANTHROPIC_DEFAULT_SONNET_MODEL) ?? mainModel,
            opusModel: str(env.ANTHROPIC_DEFAULT_OPUS_MODEL) ?? mainModel,
          },
        }));
      }
    },
    [libraryPresetEntries],
  );

  // 更新模型配置
  // 模型列表获取（按当前连接的 baseUrl/apiKey/apiFormat）+ 一键设置
  const [fetchedModelIds, setFetchedModelIds] = useState<string[]>([]);
  const [fetchingModels, setFetchingModels] = useState(false);
  const [modelPickerOpen, setModelPickerOpen] = useState(false);
  const [icon, setIcon] = useState<string | undefined>(undefined);
  const [iconColor, setIconColor] = useState<string | undefined>(undefined);

  const handleFetchModels = useCallback(async () => {
    if (!baseUrl.trim() || !apiKey.trim()) {
      toast.error(
        t("providerForm.fetchModelsNeedConnection", {
          defaultValue: "请先填写 API 地址与密钥",
        }),
      );
      return;
    }
    setFetchingModels(true);
    try {
      const models = await fetchModelsForConfig(
        baseUrl.trim(),
        apiKey.trim(),
        undefined,
        undefined,
        undefined,
        { apiFormat: apiFormat ?? undefined },
      );
      setFetchedModelIds(models.map((m) => m.id));
      toast.success(
        t("providerForm.fetchModelsSuccess", {
          defaultValue: "已获取 {{count}} 个模型",
          count: models.length,
        }),
      );
    } catch (error) {
      showFetchModelsError(error, t, {
        hasApiKey: Boolean(apiKey.trim()),
        hasBaseUrl: Boolean(baseUrl.trim()),
      });
    } finally {
      setFetchingModels(false);
    }
  }, [apiFormat, apiKey, baseUrl, t]);

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
    const model = models.codex?.model || "gpt-5.6-sol";
    const reasoningEffort = models.codex?.reasoningEffort || "high";
    // 确保 base_url 以 /v1 结尾（Codex 使用 OpenAI 兼容 API）
    const codexBaseUrl = baseUrl.endsWith("/v1")
      ? baseUrl
      : `${baseUrl.replace(/\/+$/, "")}/v1`;
    const configToml = `model_provider = "custom"
model = "${model}"
model_reasoning_effort = "${reasoningEffort}"
disable_response_storage = true

[model_providers.custom]
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
          apiFormat,
          websiteUrl: websiteUrl.trim() || undefined,
          notes: notes.trim() || undefined,
          apps: {
            claude: claudeEnabled,
            claudeDesktop: claudeDesktopEnabled,
            codex: codexEnabled,
            gemini: geminiEnabled,
            hermes: hermesEnabled,
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

    // 库预设/编辑带来的图标状态覆盖（state 优先）
    provider.icon = icon ?? provider.icon;
    provider.iconColor = iconColor ?? provider.iconColor;

    // 如果是新建，更新应用启用状态和模型
    if (!editingProvider) {
      provider.apiFormat = apiFormat;
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
    apiFormat,
    websiteUrl,
    notes,
    icon,
    iconColor,
    claudeEnabled,
    claudeDesktopEnabled,
    codexEnabled,
    geminiEnabled,
    hermesEnabled,
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
            claudeDesktop: claudeDesktopEnabled,
            codex: codexEnabled,
            gemini: geminiEnabled,
            hermes: hermesEnabled,
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
      trackUnsavedChanges
      title={
        isEditMode
          ? t("universalProvider.edit", { defaultValue: "编辑统一供应商" })
          : t("universalProvider.add", { defaultValue: "添加统一供应商" })
      }
      onClose={onClose}
      footer={footer}
    >
      <div className="space-y-6">
        {/* 预设选择（仅新建模式）——复用添加供应商的完整选择器：
            常驻搜索 + 分类 + 国内/海外等多版本切换（VersionSwitch） */}
        {!isEditMode && (
          <ProviderPresetSelector
            selectedPresetId={selectedLibraryPresetId}
            presetEntries={libraryPresetEntries}
            onPresetChange={handleLibraryPresetChange}
          />
        )}

        {/* 基本信息——双列网格，与添加供应商表单同排版 */}
        <div className="space-y-4">
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="name">
                {t("universalProvider.name", { defaultValue: "名称" })}
              </Label>
              <ImeSafeInput
                id="name"
                value={name}
                onValueChange={setName}
                placeholder={t("universalProvider.namePlaceholder", {
                  defaultValue: "例如：我的 NewAPI",
                })}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="notes">
                {t("universalProvider.notes", { defaultValue: "备注" })}
              </Label>
              <ImeSafeInput
                id="notes"
                value={notes}
                onValueChange={setNotes}
                placeholder={t("universalProvider.notesPlaceholder", {
                  defaultValue: "可选：添加备注信息",
                })}
              />
            </div>
          </div>

          <div className="space-y-2">
            <Label htmlFor="baseUrl">
              {t("universalProvider.baseUrl", { defaultValue: "API 地址" })}
            </Label>
            <ImeSafeInput
              id="baseUrl"
              value={baseUrl}
              onValueChange={setBaseUrl}
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

          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="universalApiFormat">
                {t("endpoints.apiFormat", { defaultValue: "协议格式" })}
              </Label>
              <Select
                value={apiFormat ?? "anthropic"}
                onValueChange={(value) =>
                  setApiFormat(value as UniversalProvider["apiFormat"])
                }
              >
                <SelectTrigger id="universalApiFormat">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="anthropic">Anthropic Messages</SelectItem>
                  <SelectItem value="openai_chat">
                    OpenAI Chat Completions
                  </SelectItem>
                  <SelectItem value="openai_responses">
                    OpenAI Responses
                  </SelectItem>
                  <SelectItem value="gemini_native">Gemini Native</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-2">
              <Label htmlFor="websiteUrl">
                {t("universalProvider.websiteUrl", {
                  defaultValue: "官网地址",
                })}
              </Label>
              <ImeSafeInput
                id="websiteUrl"
                value={websiteUrl}
                onValueChange={setWebsiteUrl}
                placeholder={t("universalProvider.websiteUrlPlaceholder", {
                  defaultValue: "https://example.com（可选）",
                })}
              />
            </div>
          </div>
          <p className="text-xs text-fg-3">
            {t("endpoints.apiFormatHint", {
              defaultValue:
                "代理按此格式在 Claude / OpenAI / Gemini 协议间转换；大多数中转站选 Anthropic",
            })}
          </p>
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
                <ProviderIcon icon="claude" name="Claude Desktop" size={20} />
                <span className="font-medium">Claude Desktop</span>
              </div>
              <Switch
                checked={claudeDesktopEnabled}
                onCheckedChange={setClaudeDesktopEnabled}
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
            <div className="flex items-center justify-between rounded-lg border p-3">
              <div className="flex items-center gap-2">
                <ProviderIcon icon="hermes" name="Hermes" size={20} />
                <span className="font-medium">Hermes</span>
              </div>
              <Switch
                checked={hermesEnabled}
                onCheckedChange={setHermesEnabled}
              />
            </div>
          </div>
        </div>

        {/* 模型配置 */}
        <div className="space-y-4">
          <div className="flex items-center gap-2">
            <Label>
              {t("universalProvider.modelConfig", { defaultValue: "模型配置" })}
            </Label>
            <div className="ml-auto flex items-center gap-2">
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="h-7 gap-1"
                disabled={fetchingModels}
                onClick={handleFetchModels}
              >
                {fetchingModels ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <Download className="h-3.5 w-3.5" />
                )}
                {t("providerForm.fetchModels", {
                  defaultValue: "获取模型列表",
                })}
              </Button>
              {(claudeEnabled || claudeDesktopEnabled) && (
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="h-7 gap-1"
                  disabled={!models.claude?.model?.trim()}
                  onClick={() => {
                    const main = models.claude?.model?.trim();
                    if (!main) return;
                    setModels((prev) => ({
                      ...prev,
                      claude: {
                        ...prev.claude,
                        haikuModel: main,
                        sonnetModel: main,
                        opusModel: main,
                      },
                    }));
                  }}
                >
                  <Wand2 className="h-3.5 w-3.5" />
                  {t("providerForm.quickSetModels", {
                    defaultValue: "一键设置",
                  })}
                </Button>
              )}
            </div>
          </div>
          {fetchedModelIds.length > 0 && (
            <p className="text-xs text-fg-3">
              {t("universalProvider.fetchedHint", {
                defaultValue: `已获取 ${fetchedModelIds.length} 个模型，点击主模型旁的下拉选择`,
                count: fetchedModelIds.length,
              })}
            </p>
          )}

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
                  <div className="flex gap-1">
                    <Input
                      value={models.claude?.model || ""}
                      onChange={(e) =>
                        updateModel("claude", "model", e.target.value)
                      }
                      placeholder="claude-sonnet-4-20250514"
                    />
                    <Popover
                      open={modelPickerOpen}
                      onOpenChange={setModelPickerOpen}
                    >
                      <PopoverTrigger asChild>
                        <Button
                          type="button"
                          variant="outline"
                          size="icon"
                          className="h-9 w-9 shrink-0"
                          disabled={fetchedModelIds.length === 0}
                        >
                          <ChevronDown className="h-4 w-4" />
                        </Button>
                      </PopoverTrigger>
                      <PopoverContent className="w-72 p-0" align="end">
                        <ScrollArea className="h-56">
                          <div className="p-1">
                            {fetchedModelIds.map((model) => (
                              <button
                                key={model}
                                type="button"
                                className="w-full truncate rounded px-2 py-1.5 text-left font-mono text-xs hover:bg-subtle"
                                onClick={() => {
                                  updateModel("claude", "model", model);
                                  setModelPickerOpen(false);
                                }}
                              >
                                {model}
                              </button>
                            ))}
                          </div>
                        </ScrollArea>
                      </PopoverContent>
                    </Popover>
                  </div>
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
                    placeholder="gpt-5.6-sol"
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
            <p className="text-xs text-fg-2">
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
                  darkMode={isDarkMode}
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
                  darkMode={isDarkMode}
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
                  darkMode={isDarkMode}
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
