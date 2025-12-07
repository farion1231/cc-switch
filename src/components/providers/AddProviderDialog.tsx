import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { Plus, ArrowLeft } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import type { Provider, CustomEndpoint, UniversalProvider } from "@/types";
import type { AppId } from "@/lib/api";
import { universalProvidersApi } from "@/lib/api";
import {
  ProviderForm,
  type ProviderFormValues,
} from "@/components/providers/forms/ProviderForm";
import { UniversalProviderFormModal } from "@/components/universal/UniversalProviderFormModal";
import { UniversalProviderPanel } from "@/components/universal";
import { providerPresets } from "@/config/claudeProviderPresets";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { geminiProviderPresets } from "@/config/geminiProviderPresets";
import type { UniversalProviderPreset } from "@/config/universalProviderPresets";

interface AddProviderDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  appId: AppId;
  onSubmit: (provider: Omit<Provider, "id">) => Promise<void> | void;
}

export function AddProviderDialog({
  open,
  onOpenChange,
  appId,
  onSubmit,
}: AddProviderDialogProps) {
  const { t } = useTranslation();
  const [universalFormOpen, setUniversalFormOpen] = useState(false);
  const [universalPanelOpen, setUniversalPanelOpen] = useState(false);
  const [selectedUniversalPreset, setSelectedUniversalPreset] =
    useState<UniversalProviderPreset | null>(null);

  // Handle manage universal providers
  const handleManageUniversalProviders = useCallback(() => {
    setUniversalPanelOpen(true);
  }, []);

  // Close universal panel and return to main dialog
  const handleUniversalPanelClose = useCallback(() => {
    setUniversalPanelOpen(false);
  }, []);

  // Handle universal preset selection
  const handleUniversalPresetSelect = useCallback(
    (preset: UniversalProviderPreset) => {
      setSelectedUniversalPreset(preset);
      setUniversalFormOpen(true);
    },
    [],
  );

  // Handle universal provider save
  const handleUniversalProviderSave = useCallback(
    async (provider: UniversalProvider) => {
      try {
        await universalProvidersApi.upsert(provider);
        toast.success(
          t("universalProvider.addSuccess", {
            defaultValue: "统一供应商添加成功",
          }),
        );
        setUniversalFormOpen(false);
        setSelectedUniversalPreset(null);
        onOpenChange(false);
      } catch (error) {
        console.error("[AddProviderDialog] Failed to save universal provider", error);
        toast.error(
          t("universalProvider.addFailed", {
            defaultValue: "统一供应商添加失败",
          }),
        );
      }
    },
    [t, onOpenChange],
  );

  // Close universal form and return to main dialog
  const handleUniversalFormClose = useCallback(() => {
    setUniversalFormOpen(false);
    setSelectedUniversalPreset(null);
  }, []);

  const handleSubmit = useCallback(
    async (values: ProviderFormValues) => {
      const parsedConfig = JSON.parse(values.settingsConfig) as Record<
        string,
        unknown
      >;

      // 构造基础提交数据
      const providerData: Omit<Provider, "id"> = {
        name: values.name.trim(),
        notes: values.notes?.trim() || undefined,
        websiteUrl: values.websiteUrl?.trim() || undefined,
        settingsConfig: parsedConfig,
        icon: values.icon?.trim() || undefined,
        iconColor: values.iconColor?.trim() || undefined,
        ...(values.presetCategory ? { category: values.presetCategory } : {}),
        ...(values.meta ? { meta: values.meta } : {}),
      };

      const hasCustomEndpoints =
        providerData.meta?.custom_endpoints &&
        Object.keys(providerData.meta.custom_endpoints).length > 0;

      if (!hasCustomEndpoints) {
        // 收集端点候选（仅在缺少自定义端点时兜底）
        const urlSet = new Set<string>();

        const addUrl = (rawUrl?: string) => {
          const url = (rawUrl || "").trim().replace(/\/+$/, "");
          if (url && url.startsWith("http")) {
            urlSet.add(url);
          }
        };

        if (values.presetId) {
          if (appId === "claude") {
            const presets = providerPresets;
            const presetIndex = parseInt(
              values.presetId.replace("claude-", ""),
            );
            if (
              !isNaN(presetIndex) &&
              presetIndex >= 0 &&
              presetIndex < presets.length
            ) {
              const preset = presets[presetIndex];
              if (preset?.endpointCandidates) {
                preset.endpointCandidates.forEach(addUrl);
              }
            }
          } else if (appId === "codex") {
            const presets = codexProviderPresets;
            const presetIndex = parseInt(values.presetId.replace("codex-", ""));
            if (
              !isNaN(presetIndex) &&
              presetIndex >= 0 &&
              presetIndex < presets.length
            ) {
              const preset = presets[presetIndex];
              if (Array.isArray(preset.endpointCandidates)) {
                preset.endpointCandidates.forEach(addUrl);
              }
            }
          } else if (appId === "gemini") {
            const presets = geminiProviderPresets;
            const presetIndex = parseInt(
              values.presetId.replace("gemini-", ""),
            );
            if (
              !isNaN(presetIndex) &&
              presetIndex >= 0 &&
              presetIndex < presets.length
            ) {
              const preset = presets[presetIndex];
              if (Array.isArray(preset.endpointCandidates)) {
                preset.endpointCandidates.forEach(addUrl);
              }
            }
          }
        }

        if (appId === "claude") {
          const env = parsedConfig.env as Record<string, any> | undefined;
          if (env?.ANTHROPIC_BASE_URL) {
            addUrl(env.ANTHROPIC_BASE_URL);
          }
        } else if (appId === "codex") {
          const config = parsedConfig.config as string | undefined;
          if (config) {
            const baseUrlMatch = config.match(
              /base_url\s*=\s*["']([^"']+)["']/,
            );
            if (baseUrlMatch?.[1]) {
              addUrl(baseUrlMatch[1]);
            }
          }
        } else if (appId === "gemini") {
          const env = parsedConfig.env as Record<string, any> | undefined;
          if (env?.GOOGLE_GEMINI_BASE_URL) {
            addUrl(env.GOOGLE_GEMINI_BASE_URL);
          }
        }

        const urls = Array.from(urlSet);
        if (urls.length > 0) {
          const now = Date.now();
          const customEndpoints: Record<string, CustomEndpoint> = {};
          urls.forEach((url) => {
            customEndpoints[url] = {
              url,
              addedAt: now,
              lastUsed: undefined,
            };
          });

          providerData.meta = {
            ...(providerData.meta ?? {}),
            custom_endpoints: customEndpoints,
          };
        }
      }

      await onSubmit(providerData);
      onOpenChange(false);
    },
    [appId, onSubmit, onOpenChange],
  );

  const submitLabel =
    appId === "claude"
      ? t("provider.addClaudeProvider")
      : appId === "codex"
        ? t("provider.addCodexProvider")
        : t("provider.addGeminiProvider");

  const footer = (
    <>
      <Button
        variant="outline"
        onClick={() => onOpenChange(false)}
        className="border-border/20 hover:bg-accent hover:text-accent-foreground"
      >
        {t("common.cancel")}
      </Button>
      <Button
        type="submit"
        form="provider-form"
        className="bg-primary text-primary-foreground hover:bg-primary/90"
      >
        <Plus className="h-4 w-4 mr-2" />
        {t("common.add")}
      </Button>
    </>
  );

  return (
    <FullScreenPanel
      isOpen={open}
      title={submitLabel}
      onClose={() => onOpenChange(false)}
      footer={footer}
    >
      <ProviderForm
        appId={appId}
        submitLabel={t("common.add")}
        onSubmit={handleSubmit}
        onCancel={() => onOpenChange(false)}
        onUniversalPresetSelect={handleUniversalPresetSelect}
        onManageUniversalProviders={handleManageUniversalProviders}
        showButtons={false}
      />

      {/* Universal Provider Form Modal */}
      <UniversalProviderFormModal
        isOpen={universalFormOpen}
        onClose={handleUniversalFormClose}
        onSave={handleUniversalProviderSave}
        initialPreset={selectedUniversalPreset}
      />

      {/* Universal Provider Management Panel */}
      <FullScreenPanel
        isOpen={universalPanelOpen}
        title={t("universalProvider.title", { defaultValue: "统一供应商" })}
        onClose={handleUniversalPanelClose}
        footer={
          <Button variant="outline" onClick={handleUniversalPanelClose}>
            <ArrowLeft className="h-4 w-4 mr-2" />
            {t("common.back", { defaultValue: "返回" })}
          </Button>
        }
      >
        <UniversalProviderPanel />
      </FullScreenPanel>
    </FullScreenPanel>
  );
}
