import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Plus, AlertCircle } from "lucide-react";
import { PiProviderCard } from "./PiProviderCard";
import { usePiProviders } from "@/hooks/usePiProviders";
import type { PiProviderConfig } from "@/lib/api/pi";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Alert, AlertDescription } from "@/components/ui/alert";

const API_OPTIONS = [
  { value: "openai-completions", label: "OpenAI Compatible" },
  { value: "openai-responses", label: "OpenAI Responses" },
  { value: "anthropic-messages", label: "Anthropic Messages" },
  { value: "google-generative-ai", label: "Google Generative AI" },
] as const;

const PRESET_PROVIDERS = [
  {
    id: "anthropic",
    name: "Anthropic (API Key)",
    api: "anthropic-messages" as const,
    baseUrl: "https://api.anthropic.com",
  },
  {
    id: "openai",
    name: "OpenAI",
    api: "openai-completions" as const,
    baseUrl: "https://api.openai.com/v1",
  },
  {
    id: "google",
    name: "Google Gemini",
    api: "google-generative-ai" as const,
    baseUrl: "https://generativelanguage.googleapis.com/v1beta",
  },
  {
    id: "deepseek",
    name: "DeepSeek",
    api: "openai-completions" as const,
    baseUrl: "https://api.deepseek.com",
  },
  {
    id: "openrouter",
    name: "OpenRouter",
    api: "openai-completions" as const,
    baseUrl: "https://openrouter.ai/api/v1",
  },
  {
    id: "groq",
    name: "Groq",
    api: "openai-completions" as const,
    baseUrl: "https://api.groq.com/openai/v1",
  },
  {
    id: "custom",
    name: "Custom",
    api: "openai-completions" as const,
    baseUrl: "",
  },
];

const DEFAULT_MODEL = {
  id: "",
  name: "",
  reasoning: true,
  input: ["text"],
  contextWindow: 128000,
  maxTokens: 16384,
  cost: { input: 3.0, output: 15.0, cacheRead: 0.3, cacheWrite: 3.75 },
};

export function PiPage() {
  const { t } = useTranslation();
  const {
    providers,
    providersMap,
    isLoading,
    addProvider,
    updateProvider,
    deleteProvider,
    setActive,
    refetch,
  } = usePiProviders();

  const [isDialogOpen, setIsDialogOpen] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [formData, setFormData] = useState({
    providerId: "",
    preset: "custom" as string,
    api: "openai-completions" as string,
    baseUrl: "",
    apiKey: "",
    modelId: "",
    modelName: "",
  });

  const hasProviders = providers.length > 0;

  const openAddDialog = () => {
    setEditingId(null);
    setFormData({
      providerId: "",
      preset: "custom",
      api: "openai-completions",
      baseUrl: "",
      apiKey: "",
      modelId: "",
      modelName: "",
    });
    setIsDialogOpen(true);
  };

  const openEditDialog = (id: string) => {
    const config = providersMap[id];
    if (!config) return;
    setEditingId(id);
    const displayId = id.replace("cc-switch-", "");
    setFormData({
      providerId: displayId,
      preset: "custom",
      api: config.api ?? "openai-completions",
      baseUrl: config.baseUrl ?? "",
      apiKey: config.apiKey ?? "",
      modelId: config.models?.[0]?.id ?? "",
      modelName: config.models?.[0]?.name ?? "",
    });
    setIsDialogOpen(true);
  };

  const handlePresetChange = (presetId: string) => {
    const preset = PRESET_PROVIDERS.find((p) => p.id === presetId);
    if (!preset) return;
    setFormData((prev) => ({
      ...prev,
      preset: presetId,
      api: preset.api,
      baseUrl: preset.baseUrl,
      providerId: preset.id === "custom" ? prev.providerId : preset.id,
    }));
  };

  const handleSave = async () => {
    const providerId = formData.providerId || formData.preset;
    if (!providerId) return;

    const model = formData.modelId
      ? [
          {
            ...DEFAULT_MODEL,
            id: formData.modelId,
            name: formData.modelName || formData.modelId,
          },
        ]
      : [];

    const config: PiProviderConfig = {
      baseUrl: formData.baseUrl,
      api: formData.api,
      apiKey: formData.apiKey,
      authHeader: true,
      models: model,
    };

    if (editingId) {
      await updateProvider.mutateAsync({ id: providerId, config });
    } else {
      await addProvider.mutateAsync({ id: providerId, config });
    }
    setIsDialogOpen(false);
    refetch();
  };

  const handleDelete = async (id: string) => {
    if (window.confirm(t("confirm.deleteProvider"))) {
      await deleteProvider.mutateAsync(id);
    }
  };

  const handleSetActive = async (id: string) => {
    const config = providersMap[id];
    const modelId = config?.models?.[0]?.id;
    await setActive.mutateAsync({ providerId: id, modelId });
  };

  if (isLoading) {
    return <div className="p-6 text-muted-foreground">Loading...</div>;
  }

  return (
    <div className="space-y-6 p-6">
      {/* Pi CLI detection */}
      <Alert>
        <AlertCircle className="w-4 h-4" />
        <AlertDescription>
          {t("pi.notInstalled")}
        </AlertDescription>
      </Alert>

      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-lg font-semibold">Pi</h2>
          <p className="text-sm text-muted-foreground">
            {hasProviders
              ? `${providers.length} provider(s) configured`
              : t("pi.noProviders")}
          </p>
        </div>
        <Button onClick={openAddDialog} size="sm">
          <Plus className="w-4 h-4 mr-2" />
          {t("pi.addProvider")}
        </Button>
      </div>

      {/* Provider List */}
      {hasProviders ? (
        <div className="grid gap-4 grid-cols-1 md:grid-cols-2">
          {providers.map((provider) => (
            <PiProviderCard
              key={provider.id}
              id={provider.id}
              config={
                {
                  baseUrl: provider.baseUrl,
                  api: provider.api,
                  apiKey: provider.apiKey,
                  models: provider.models,
                } as PiProviderConfig
              }
              isActive={providersMap[provider.id]?.baseUrl !== undefined}
              onEdit={openEditDialog}
              onDelete={handleDelete}
              onSetActive={handleSetActive}
            />
          ))}
        </div>
      ) : (
        <div className="text-center py-12 text-muted-foreground border rounded-lg border-dashed">
          <p>{t("pi.noProviders")}</p>
          <Button
            onClick={openAddDialog}
            variant="outline"
            size="sm"
            className="mt-4"
          >
            <Plus className="w-4 h-4 mr-2" />
            {t("pi.addProvider")}
          </Button>
        </div>
      )}

      {/* Add/Edit Dialog */}
      <Dialog open={isDialogOpen} onOpenChange={setIsDialogOpen}>
        <DialogContent className="max-w-lg">
          <DialogHeader>
            <DialogTitle>
              {editingId ? t("pi.editProvider") : t("pi.addProvider")}
            </DialogTitle>
          </DialogHeader>
          <div className="space-y-4">
            {/* Preset selector */}
            <div className="space-y-2">
              <Label>{t("provider")}</Label>
              <Select
                value={formData.preset}
                onValueChange={handlePresetChange}
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {PRESET_PROVIDERS.map((p) => (
                    <SelectItem key={p.id} value={p.id}>
                      {p.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            {/* Provider ID (for custom) */}
            {formData.preset === "custom" && (
              <div className="space-y-2">
                <Label>Provider ID</Label>
                <Input
                  value={formData.providerId}
                  onChange={(e) =>
                    setFormData((prev) => ({
                      ...prev,
                      providerId: e.target.value,
                    }))
                  }
                  placeholder="my-provider"
                />
              </div>
            )}

            {/* API Type */}
            <div className="space-y-2">
              <Label>{t("pi.apiType")}</Label>
              <Select
                value={formData.api}
                onValueChange={(v) =>
                  setFormData((prev) => ({ ...prev, api: v }))
                }
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {API_OPTIONS.map((opt) => (
                    <SelectItem key={opt.value} value={opt.value}>
                      {opt.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            {/* Base URL */}
            <div className="space-y-2">
              <Label>{t("pi.baseUrl")}</Label>
              <Input
                value={formData.baseUrl}
                onChange={(e) =>
                  setFormData((prev) => ({
                    ...prev,
                    baseUrl: e.target.value,
                  }))
                }
                placeholder="https://api.example.com/v1"
              />
            </div>

            {/* API Key */}
            <div className="space-y-2">
              <Label>{t("pi.apiKey")}</Label>
              <Input
                value={formData.apiKey}
                onChange={(e) =>
                  setFormData((prev) => ({
                    ...prev,
                    apiKey: e.target.value,
                  }))
                }
                type="password"
                placeholder="sk-..."
              />
            </div>

            {/* Model ID */}
            <div className="space-y-2">
              <Label>{t("pi.modelId")}</Label>
              <Input
                value={formData.modelId}
                onChange={(e) =>
                  setFormData((prev) => ({
                    ...prev,
                    modelId: e.target.value,
                  }))
                }
                placeholder="claude-sonnet-4-20250514"
              />
            </div>

            {/* Model Name */}
            <div className="space-y-2">
              <Label>{t("pi.modelName")}</Label>
              <Input
                value={formData.modelName}
                onChange={(e) =>
                  setFormData((prev) => ({
                    ...prev,
                    modelName: e.target.value,
                  }))
                }
                placeholder="Claude Sonnet 4"
              />
            </div>

            {/* Actions */}
            <div className="flex justify-end gap-2 pt-4">
              <Button
                variant="outline"
                onClick={() => setIsDialogOpen(false)}
              >
                {t("cancel")}
              </Button>
              <Button onClick={handleSave} disabled={addProvider.isPending || updateProvider.isPending}>
                {editingId ? t("save") : t("add")}
              </Button>
            </div>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}
