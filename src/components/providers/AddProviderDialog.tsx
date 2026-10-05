import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import { toast } from "@/lib/toast";
import { Button } from "@/components/ui/button";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import {
  PresetStepContext,
  type PresetStepState,
} from "@/components/providers/forms/presetStep";
import type { Provider, CustomEndpoint, UniversalProvider } from "@/types";
import type { AppId } from "@/lib/api";
import { providersApi, universalProvidersApi } from "@/lib/api";
import type {
  EditorConflictPolicy,
  ProviderEditorSave,
} from "@/lib/api/providers";
import { useLiveEditConflict } from "@/components/providers/LiveEditConflictDialog";
import { toastEditorViewFailed } from "@/components/providers/forms/hooks/useDraftEditorProjection";
import { usesEditorView } from "@/config/appConfig";
import {
  ProviderForm,
  type ProviderFormValues,
} from "@/components/providers/forms/ProviderForm";
import { AuthSettingsPanel } from "@/components/providers/AuthSettingsPanel";
import { UniversalProviderFormModal } from "@/components/universal/UniversalProviderFormModal";
import { UniversalProviderPanel } from "@/components/universal";
import { providerPresets } from "@/config/claudeProviderPresets";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { geminiProviderPresets } from "@/config/geminiProviderPresets";
import { claudeDesktopProviderPresets } from "@/config/claudeDesktopProviderPresets";
import { extractCodexBaseUrl } from "@/utils/providerConfigUtils";
import { extractGrokBuildBaseUrl } from "@/utils/grokBuildConfig";
import { GROKBUILD_OFFICIAL_PROVIDER_ID } from "@/utils/providerCapabilities";
import type { OpenClawSuggestedDefaults } from "@/config/openclawProviderPresets";
import type { UniversalProviderPreset } from "@/config/universalProviderPresets";
import type { ManagedAuthProvider } from "@/lib/api";
import type { AppMode } from "@/types/proxy";

interface AddProviderDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  appId: AppId;
  onSubmit: (
    provider: Omit<Provider, "id"> & {
      providerKey?: string;
      suggestedDefaults?: OpenClawSuggestedDefaults;
      ensureClaudeDesktopOfficialSeed?: boolean;
      ensureGrokBuildOfficialSeed?: boolean;
      editorSave?: ProviderEditorSave;
    },
  ) => Promise<void> | void;
  /** 从供应商页哪一格打开的，见 ProviderForm 的同名参数 */
  modeView?: AppMode;
}

export function AddProviderDialog({
  open,
  onOpenChange,
  appId,
  onSubmit,
  modeView,
}: AddProviderDialogProps) {
  const { t } = useTranslation();
  // OpenCode and OpenClaw don't support universal providers
  const showUniversalTab =
    appId !== "opencode" &&
    appId !== "openclaw" &&
    appId !== "hermes" &&
    appId !== "pi" &&
    appId !== "mcode" &&
    appId !== "grokbuild" &&
    appId !== "claude-desktop";
  // 两步：先选预设，再填写（每次打开都从第 1 步开始）
  const [step, setStep] = useState<"pick" | "form">("pick");
  const [pickerHost, setPickerHost] = useState<HTMLDivElement | null>(null);
  const [manageUniversalOpen, setManageUniversalOpen] = useState(false);
  useEffect(() => {
    if (open) setStep("pick");
  }, [open, appId]);
  const selectorCount = useRef(0);
  const registerSelector = useCallback(() => {
    selectorCount.current += 1;
    return () => {
      selectorCount.current -= 1;
    };
  }, []);
  const stepState = useMemo<PresetStepState>(
    () => ({ appId, step, setStep, host: pickerHost, registerSelector }),
    [appId, step, pickerHost, registerSelector],
  );
  const [universalFormOpen, setUniversalFormOpen] = useState(false);
  const [selectedUniversalPreset, setSelectedUniversalPreset] =
    useState<UniversalProviderPreset | null>(null);
  const [isFormSubmitting, setIsFormSubmitting] = useState(false);
  // 表单用了聚合的简化布局：页头应用名后标「聚合模式」
  const [stackLayout, setStackLayout] = useState(false);
  const [authSettingsTarget, setAuthSettingsTarget] =
    useState<ManagedAuthProvider | null>(null);

  useEffect(() => {
    setAuthSettingsTarget(null);
  }, [appId, open]);

  // Claude：预设的关键字段套在当前 live 上显示（去掉当前供应商的关键字段），保存时
  // 其余部分的改动写进 live，这份底也用来三方比较。
  const [claudeLiveBase, setClaudeLiveBase] = useState<Record<
    string,
    unknown
  > | null>(null);
  const [claudeBaseLoaded, setClaudeBaseLoaded] = useState(false);
  // Codex、Gemini CLI、Grok Build：表单把预设投影到当前配置文件上显示，投影结果就是保存时
  // 三方比较的底（投影进行中或失败时为 null，保存只存供应商）。投影成它的草稿一起留着，
  // 后端按草稿分开预设带的字段和从 live 带进来的字段。
  const [draftEditorBase, setDraftEditorBase] = useState<{
    base: Record<string, unknown>;
    draft?: Record<string, unknown>;
  } | null>(null);
  const handleDraftEditorBase = useCallback(
    (base: Record<string, unknown> | null, draft?: Record<string, unknown>) =>
      setDraftEditorBase(base ? { base, draft } : null),
    [],
  );
  // 新增时表单自己把预设投影到配置文件上的应用（Claude Code 在对话框里取底，见上）。
  const projectsDraft = appId !== "claude" && usesEditorView(appId);
  const { submitWithConflictRetry, conflictDialog } = useLiveEditConflict();

  useEffect(() => {
    if (!open || appId !== "claude") {
      setClaudeLiveBase(null);
      setClaudeBaseLoaded(false);
      return;
    }
    let cancelled = false;
    providersApi
      .getEditorView(appId, {})
      .then((view) => {
        if (!cancelled) setClaudeLiveBase(view.settings);
      })
      .catch((error: unknown) => {
        // 读不了 settings.json：退回只显示预设。
        if (!cancelled) {
          setClaudeLiveBase(null);
          toastEditorViewFailed(t, error);
        }
      })
      .finally(() => {
        if (!cancelled) setClaudeBaseLoaded(true);
      });
    return () => {
      cancelled = true;
    };
  }, [open, appId, t]);

  const closeDialog = useCallback(() => {
    setAuthSettingsTarget(null);
    // 表单每次打开都会重新投影；这里清掉，免得下次打开时先用上一次的底。
    setDraftEditorBase(null);
    onOpenChange(false);
  }, [onOpenChange]);

  const handlePanelClose = useCallback(() => {
    if (authSettingsTarget) {
      setAuthSettingsTarget(null);
      return;
    }
    // 第 2 步的返回回到选预设
    if (step === "form") {
      setStep("pick");
      return;
    }
    closeDialog();
  }, [authSettingsTarget, closeDialog, step]);
  const formReadyToken = useMemo(
    () => Symbol("provider-form-ready"),
    [appId, open],
  );
  const currentFormReadyToken = useRef(formReadyToken);
  currentFormReadyToken.current = formReadyToken;
  const [formReadyState, setFormReadyState] = useState({
    token: formReadyToken,
    ready: appId !== "pi",
  });
  const isFormReady =
    formReadyState.token === formReadyToken
      ? formReadyState.ready
      : appId !== "pi";
  const handleSubmitReadyChange = useCallback(
    (ready: boolean) => {
      if (currentFormReadyToken.current === formReadyToken) {
        setFormReadyState({ token: formReadyToken, ready });
      }
    },
    [formReadyToken],
  );

  const handleUniversalProviderSave = useCallback(
    async (provider: UniversalProvider) => {
      try {
        await universalProvidersApi.upsert(provider);
      } catch (error) {
        console.error(
          "[AddProviderDialog] Failed to save universal provider",
          error,
        );
        toast.error(
          t("universalProvider.addFailed", {
            defaultValue: "统一供应商添加失败",
          }),
        );
        return;
      }

      try {
        await universalProvidersApi.sync(provider.id);
        toast.success(
          t("universalProvider.addedAndSynced", {
            defaultValue: "统一供应商已添加并同步",
          }),
        );
      } catch (error) {
        console.error(
          "[AddProviderDialog] Provider saved but sync failed",
          error,
        );
        toast.warning(
          t("universalProvider.addedButSyncFailed", {
            defaultValue: "统一供应商已添加，但同步失败",
          }),
        );
      }

      setUniversalFormOpen(false);
      setSelectedUniversalPreset(null);
      onOpenChange(false);
    },
    [t, onOpenChange],
  );

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
      const providerData: Omit<Provider, "id"> & {
        providerKey?: string;
        suggestedDefaults?: OpenClawSuggestedDefaults;
        ensureClaudeDesktopOfficialSeed?: boolean;
        ensureGrokBuildOfficialSeed?: boolean;
      } = {
        name: values.name.trim(),
        notes: values.notes?.trim() || undefined,
        websiteUrl: values.websiteUrl?.trim() || undefined,
        settingsConfig: parsedConfig,
        icon: values.icon?.trim() || undefined,
        iconColor: values.iconColor?.trim() || undefined,
        ...(values.presetCategory ? { category: values.presetCategory } : {}),
        ...(values.meta ? { meta: values.meta } : {}),
      };
      if (appId === "claude-desktop" && values.presetId) {
        const presetIndex = parseInt(
          values.presetId.replace("claude-desktop-", ""),
        );
        const preset = claudeDesktopProviderPresets[presetIndex];
        providerData.ensureClaudeDesktopOfficialSeed =
          values.presetCategory === "official" &&
          preset?.category === "official";
      }

      if (appId === "grokbuild" && values.presetId) {
        providerData.ensureGrokBuildOfficialSeed =
          values.presetCategory === "official" &&
          values.presetId === GROKBUILD_OFFICIAL_PROVIDER_ID;
      }

      // Apps whose native catalog has a stable provider key use it as the
      // managed provider identity.
      if (
        (appId === "opencode" ||
          appId === "openclaw" ||
          appId === "hermes" ||
          appId === "pi" ||
          appId === "mcode") &&
        values.providerKey
      ) {
        providerData.providerKey = values.providerKey;
      }
      const hasCustomEndpoints =
        providerData.meta?.custom_endpoints &&
        Object.keys(providerData.meta.custom_endpoints).length > 0;

      if (!hasCustomEndpoints && values.presetCategory !== "omo") {
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
          } else if (appId === "claude-desktop") {
            const presets = claudeDesktopProviderPresets;
            const presetIndex = parseInt(
              values.presetId.replace("claude-desktop-", ""),
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
              addUrl(preset.baseUrl);
            }
          }
        }

        if (appId === "claude") {
          const env = parsedConfig.env as Record<string, any> | undefined;
          if (env?.ANTHROPIC_BASE_URL) {
            addUrl(env.ANTHROPIC_BASE_URL);
          }
        } else if (appId === "claude-desktop") {
          const env = parsedConfig.env as Record<string, any> | undefined;
          if (env?.ANTHROPIC_BASE_URL) {
            addUrl(env.ANTHROPIC_BASE_URL);
          }
        } else if (appId === "codex") {
          const config = parsedConfig.config as string | undefined;
          if (config) {
            const extractedBaseUrl = extractCodexBaseUrl(config);
            if (extractedBaseUrl) {
              addUrl(extractedBaseUrl);
            }
          }
        } else if (appId === "gemini") {
          const env = parsedConfig.env as Record<string, any> | undefined;
          if (env?.GOOGLE_GEMINI_BASE_URL) {
            addUrl(env.GOOGLE_GEMINI_BASE_URL);
          }
        } else if (appId === "grokbuild") {
          const config = parsedConfig.config as string | undefined;
          if (config) {
            addUrl(extractGrokBuildBaseUrl(config));
          }
        } else if (appId === "opencode") {
          const options = parsedConfig.options as
            | Record<string, any>
            | undefined;
          if (options?.baseURL) {
            addUrl(options.baseURL);
          }
        } else if (appId === "openclaw") {
          // OpenClaw uses baseUrl directly
          if (parsedConfig.baseUrl) {
            addUrl(parsedConfig.baseUrl as string);
          }
        } else if (appId === "hermes") {
          if (parsedConfig.base_url) {
            addUrl(parsedConfig.base_url as string);
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

      // OpenClaw: pass suggestedDefaults for model registration
      if (appId === "openclaw" && values.suggestedDefaults) {
        providerData.suggestedDefaults = values.suggestedDefaults;
      }

      const editorBase =
        appId === "claude"
          ? claudeLiveBase && { base: claudeLiveBase }
          : projectsDraft
            ? draftEditorBase
            : null;
      const submit = async (onConflict: EditorConflictPolicy) => {
        await onSubmit({
          ...providerData,
          ...(editorBase ? { editorSave: { ...editorBase, onConflict } } : {}),
        });
        closeDialog();
      };
      await submitWithConflictRetry(submit);
    },
    [
      appId,
      onSubmit,
      closeDialog,
      claudeLiveBase,
      projectsDraft,
      draftEditorBase,
      submitWithConflictRetry,
    ],
  );

  const waitingForClaudeBase = appId === "claude" && !claudeBaseLoaded;

  // 表单已经挂上、却没有预设选择器（没有预设可选的表单）：直接进第 2 步。
  // 子组件的 effect 先于这里执行，选择器在同一次提交里已经登记过了。
  useEffect(() => {
    if (open && !waitingForClaudeBase && selectorCount.current === 0) {
      setStep("form");
    }
  }, [open, waitingForClaudeBase, appId]);

  const footer =
    step === "form" ? (
      <>
        <Button
          type="button"
          variant="neutral"
          size="regular"
          onClick={closeDialog}
        >
          {t("common.cancel")}
        </Button>
        <Button
          type="submit"
          form="provider-form"
          variant="solid"
          size="regular"
          disabled={isFormSubmitting || !isFormReady}
        >
          {isFormSubmitting && <Loader2 className="h-4 w-4 animate-spin" />}
          {t("common.add")}
        </Button>
      </>
    ) : null;

  const form = (
    <ProviderForm
      appId={appId}
      submitLabel={t("common.add")}
      modeView={modeView}
      onStackLayoutChange={setStackLayout}
      onSubmit={handleSubmit}
      onCancel={closeDialog}
      onManageAuthAccounts={setAuthSettingsTarget}
      onSubmittingChange={setIsFormSubmitting}
      onSubmitReadyChange={handleSubmitReadyChange}
      showButtons={false}
      claudeLiveBase={
        appId === "claude" ? (claudeLiveBase ?? undefined) : undefined
      }
      onEditorBaseChange={projectsDraft ? handleDraftEditorBase : undefined}
      onUniversalPresetSelect={
        showUniversalTab
          ? (preset) => {
              setSelectedUniversalPreset(preset);
              setUniversalFormOpen(true);
            }
          : undefined
      }
      onManageUniversalProviders={
        showUniversalTab ? () => setManageUniversalOpen(true) : undefined
      }
    />
  );

  return (
    <FullScreenPanel
      isOpen={open}
      title={t("provider.addNewProvider")}
      subtitle={
        stackLayout
          ? t("provider.formSubtitleStack", {
              app: APP_DISPLAY_NAME[appId],
              defaultValue: "{{app}}（聚合模式）",
            })
          : APP_DISPLAY_NAME[appId]
      }
      backLabel={
        step === "form"
          ? t("providerPreset.backToPick")
          : t("provider.backToList")
      }
      onClose={handlePanelClose}
      footer={footer}
      contentClassName={
        step === "pick"
          ? "flex h-full flex-col space-y-0 p-0"
          : appId === "pi"
            ? "mx-0 max-w-[1008px] pb-0 pt-4"
            : "mx-0 max-w-[1008px] pt-4"
      }
    >
      <PresetStepContext.Provider value={stepState}>
        {step === "pick" && (
          <div ref={setPickerHost} className="min-h-0 flex-1" />
        )}
        <div className={step === "pick" ? "hidden" : undefined}>
          {waitingForClaudeBase ? (
            <div className="py-12 text-center text-body text-fg-2">
              {t("common.loading")}
            </div>
          ) : (
            form
          )}
        </div>
      </PresetStepContext.Provider>

      {showUniversalTab && (
        <UniversalProviderFormModal
          isOpen={universalFormOpen}
          onClose={handleUniversalFormClose}
          onSave={handleUniversalProviderSave}
          initialPreset={selectedUniversalPreset}
        />
      )}

      {showUniversalTab && (
        <FullScreenPanel
          isOpen={manageUniversalOpen}
          title={t("universalProvider.manage")}
          onClose={() => setManageUniversalOpen(false)}
        >
          <UniversalProviderPanel />
        </FullScreenPanel>
      )}

      <AuthSettingsPanel
        target={authSettingsTarget}
        onClose={() => setAuthSettingsTarget(null)}
      />
      {conflictDialog}
    </FullScreenPanel>
  );
}
