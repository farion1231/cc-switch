import { useCallback } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import {
  piApi,
  providersApi,
  settingsApi,
  openclawApi,
  type AppId,
} from "@/lib/api";
import type {
  Provider,
  UsageScript,
  OpenClawProviderConfig,
  OpenClawDefaultModel,
} from "@/types";
import type { OpenClawSuggestedDefaults } from "@/config/openclawProviderPresets";
import { injectCodingPlanUsageScript } from "@/config/codingPlanProviders";
import {
  useAddProviderMutation,
  useUpdateProviderMutation,
  useDeleteProviderMutation,
  useSwitchProviderMutation,
} from "@/lib/query";
import { usageKeys } from "@/lib/query/usage";
import { extractErrorMessage } from "@/utils/errorUtils";
import { openclawKeys } from "@/hooks/useOpenClaw";
import {
  extractCodexWireApi,
  isCodexAnthropicWireApi,
  isCodexChatWireApi,
} from "@/utils/providerConfigUtils";
import {
  providerNeedsRouting,
  supportsOfficialProxyTakeover,
} from "@/utils/providerCapabilities";
import { isOAuthProviderType } from "@/config/constants";

/**
 * Hook for managing provider actions (add, update, delete, switch)
 * Extracts business logic from App.tsx
 */
export function useProviderActions(
  activeApp: AppId,
  isProxyRunning?: boolean,
  isProxyTakeover?: boolean,
) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const addProviderMutation = useAddProviderMutation(activeApp);
  const updateProviderMutation = useUpdateProviderMutation(activeApp);
  const deleteProviderMutation = useDeleteProviderMutation(activeApp);
  const switchProviderMutation = useSwitchProviderMutation(activeApp);

  // Claude 插件同步逻辑
  const syncClaudePlugin = useCallback(
    async (provider: Provider) => {
      if (activeApp !== "claude") return;

      try {
        const settings = await settingsApi.get();
        if (!settings?.enableClaudePluginIntegration) {
          return;
        }

        const isOfficial = provider.category === "official";
        await settingsApi.applyClaudePluginConfig({ official: isOfficial });

        // 静默执行，不显示成功通知
      } catch (error) {
        const detail =
          extractErrorMessage(error) ||
          t("notifications.syncClaudePluginFailed", {
            defaultValue: "同步 Claude 插件失败",
          });
        toast.error(detail, { duration: 4200 });
      }
    },
    [activeApp, t],
  );

  // 添加供应商
  const addProvider = useCallback(
    async (
      provider: Omit<Provider, "id"> & {
        providerKey?: string;
        suggestedDefaults?: OpenClawSuggestedDefaults;
        addToLive?: boolean;
        ensureClaudeDesktopOfficialSeed?: boolean;
        ensureGrokBuildOfficialSeed?: boolean;
      },
    ) => {
      const enhanced = injectCodingPlanUsageScript(activeApp, provider);
      await addProviderMutation.mutateAsync(enhanced);

      // OpenClaw: register models to allowlist after adding provider
      if (activeApp === "openclaw" && provider.suggestedDefaults) {
        const { model, modelCatalog } = provider.suggestedDefaults;
        let modelsRegistered = false;

        try {
          // 1. Merge model catalog (allowlist)
          if (modelCatalog && Object.keys(modelCatalog).length > 0) {
            const existingCatalog = (await openclawApi.getModelCatalog()) || {};
            const mergedCatalog = { ...existingCatalog, ...modelCatalog };
            await openclawApi.setModelCatalog(mergedCatalog);
            await queryClient.invalidateQueries({
              queryKey: openclawKeys.health,
            });
            modelsRegistered = true;
          }

          // 2. Set default model (only if not already set)
          if (model) {
            const existingDefault = await openclawApi.getDefaultModel();
            if (!existingDefault?.primary) {
              await openclawApi.setDefaultModel(model);
              await queryClient.invalidateQueries({
                queryKey: openclawKeys.health,
              });
            }
          }

          // Show success toast if models were registered
          if (modelsRegistered) {
            toast.success(
              t("notifications.openclawModelsRegistered", {
                defaultValue: "模型已注册到 /model 列表",
              }),
              { closeButton: true },
            );
          }
        } catch (error) {
          // Log warning but don't block main flow - provider config is already saved
          console.warn(
            "[OpenClaw] Failed to register models to allowlist:",
            error,
          );
        }
      }
    },
    [addProviderMutation, activeApp, queryClient, t],
  );

  // 更新供应商
  const updateProvider = useCallback(
    async (provider: Provider, originalId?: string) => {
      await updateProviderMutation.mutateAsync({
        provider,
        originalId,
      });

      // 更新托盘菜单（失败不影响主操作）
      try {
        await providersApi.updateTrayMenu();
      } catch (trayError) {
        console.error(
          "Failed to update tray menu after updating provider",
          trayError,
        );
      }
    },
    [updateProviderMutation],
  );

  // 切换供应商
  const switchProvider = useCallback(
    async (provider: Provider) => {
      const isCopilotProvider =
        activeApp === "claude" &&
        provider.meta?.providerType === "github_copilot";
      const isCodexChatFormat =
        (activeApp === "codex" || activeApp === "grokbuild") &&
        (provider.meta?.apiFormat === "openai_chat" ||
          (typeof (provider.settingsConfig as Record<string, any>)?.config ===
            "string" &&
            isCodexChatWireApi(
              extractCodexWireApi(
                (provider.settingsConfig as Record<string, any>).config,
              ),
            )));
      const isCodexAnthropicFormat =
        (activeApp === "codex" || activeApp === "grokbuild") &&
        (provider.meta?.apiFormat === "anthropic" ||
          (typeof (provider.settingsConfig as Record<string, any>)?.config ===
            "string" &&
            isCodexAnthropicWireApi(
              extractCodexWireApi(
                (provider.settingsConfig as Record<string, any>).config,
              ),
            )));

      // Claude Desktop 的路由开关就是代理进程本身；其余应用还必须开启当前
      // 应用的 takeover。不能只看全局进程，否则其它应用已接管时会漏判；也
      // 不能只看 takeover，否则 Desktop 在路由已运行时会持续误报。
      const routingReady =
        activeApp === "claude-desktop"
          ? isProxyRunning === true
          : isProxyTakeover === true;

      // Determine why this provider requires the proxy.
      let proxyRequiredReason: string | null = null;
      if (!routingReady && providerNeedsRouting(activeApp, provider)) {
        if (isCopilotProvider) {
          proxyRequiredReason = t("notifications.proxyReasonCopilot", {
            defaultValue: "使用 GitHub Copilot 作为 Claude 供应商",
          });
        } else if (isOAuthProviderType(provider.meta?.providerType)) {
          // 托管 OAuth（codex_oauth / xai_oauth 等）：凭据由本地代理注入，
          // 是否需路由由 providerType 权威决定，不看 apiFormat（后端亦无视，
          // 见 forwarder.rs）——避免 codex_oauth 被改成 anthropic / 旧数据缺省
          // apiFormat 时漏判。Claude 下的 Copilot 保留上面的专属文案。
          proxyRequiredReason = t("notifications.proxyReasonManagedOAuth", {
            defaultValue: "使用托管 OAuth 登录（令牌由本地路由注入）",
          });
        } else if (
          provider.meta?.apiFormat === "openai_chat" &&
          activeApp === "claude"
        ) {
          proxyRequiredReason = t("notifications.proxyReasonOpenAIChat", {
            defaultValue: "使用 OpenAI Chat 接口格式",
          });
        } else if (
          provider.meta?.apiFormat === "openai_responses" &&
          activeApp === "claude"
        ) {
          proxyRequiredReason = t("notifications.proxyReasonOpenAIResponses", {
            defaultValue: "使用 OpenAI Responses 接口格式",
          });
        } else if (isCodexChatFormat) {
          proxyRequiredReason = t("notifications.proxyReasonOpenAIChat", {
            defaultValue: "使用 OpenAI Chat 接口格式",
          });
        } else if (isCodexAnthropicFormat) {
          proxyRequiredReason = t(
            "notifications.proxyReasonAnthropicMessages",
            {
              defaultValue: "使用 Anthropic Messages 接口格式",
            },
          );
        } else if (
          activeApp === "claude-desktop" &&
          provider.meta?.claudeDesktopMode === "proxy"
        ) {
          proxyRequiredReason = t("notifications.proxyReasonClaudeDesktop", {
            defaultValue: "使用 Claude Desktop 本地路由模式",
          });
        } else if (
          provider.meta?.isFullUrl &&
          (activeApp === "claude" ||
            activeApp === "codex" ||
            activeApp === "grokbuild")
        ) {
          proxyRequiredReason = t("notifications.proxyReasonFullUrl", {
            defaultValue: "开启了完整 URL 连接模式",
          });
        } else {
          proxyRequiredReason = t("notifications.proxyReasonRoutingRequired", {
            defaultValue: "需要本地路由处理请求",
          });
        }
      }

      if (proxyRequiredReason) {
        toast.warning(
          t("notifications.proxyRequiredForSwitch", {
            reason: proxyRequiredReason,
            defaultValue:
              "此供应商{{reason}}，需要代理服务才能正常使用，请先启动代理",
          }),
        );
      }

      // Codex official account cards can reuse the active native ChatGPT login
      // through local routing. Other apps' official providers remain blocked.
      const officialSupportsTakeover = supportsOfficialProxyTakeover(
        activeApp,
        provider,
      );
      if (
        isProxyTakeover &&
        provider.category === "official" &&
        !officialSupportsTakeover
      ) {
        toast.error(
          t("notifications.officialBlockedByProxy", {
            defaultValue:
              "代理接管模式下不能切换到官方供应商，使用代理访问官方 API 可能导致账号被封禁",
          }),
          { duration: 6000 },
        );
        return;
      }

      try {
        const result = await switchProviderMutation.mutateAsync(provider.id);
        await syncClaudePlugin(provider);

        // Surface switch warnings by code — a generic "backfill failed"
        // message for an auth-cleanup warning would point the user at the
        // wrong problem entirely.
        if (result?.warnings?.length) {
          const authCleanupFailed = result.warnings.some((warning) =>
            warning.startsWith("codex_auth_cleanup_failed"),
          );
          const hasOtherWarnings = result.warnings.some(
            (warning) => !warning.startsWith("codex_auth_cleanup_failed"),
          );
          if (authCleanupFailed) {
            toast.warning(
              t("notifications.codexAuthCleanupFailed", {
                defaultValue:
                  "切换成功，但未能删除 auth.json，官方登录凭据仍留在磁盘上；如需彻底移除请手动删除 Codex 配置目录中的 auth.json",
              }),
              { duration: 6000 },
            );
          }
          if (hasOtherWarnings) {
            toast.warning(
              t("notifications.backfillWarning", {
                defaultValue:
                  "切换成功，但旧供应商配置回填失败，您手动修改的配置可能未保存",
              }),
              { duration: 5000 },
            );
          }
        }

        // 若已弹过 proxyRequired 警告则不再弹 success
        if (!proxyRequiredReason) {
          let messageKey = "notifications.switchSuccess";
          let defaultMessage = "切换成功！";
          // codex 分支会自己弹 success（带 CTA/无 CTA）并把该 flag 置 false
          // 跳过末尾的默认 toast，避免重复弹窗。
          let continueWithSwitchToast = true;
          if (activeApp === "codex") {
            // Codex 不像 Claude Desktop 那样需要 cc-switch 持续运行；切完配置
            // 写进 `~/.codex/config.toml` 即视为生效。Codex 运行中读 config 是
            // session 启动时一次加载，运行中不重新加载，所以**当前** session
            // 仍用旧配置；**新** session 立即生效。把 toast 拆成两态：
            //   - 未在运行 → 「下次启动生效」（无缝）
            //   - 正在运行 → 「当前会话仍用旧配置」+ 提供「立即重启」CTA
            // 探测失败（pgrep 不在 PATH、跨平台兼容问题）时保守退回到旧的
            // 「请重启客户端」措辞，避免给用户错误的「无缝」承诺。
            let codexRunning = false;
            let probeOk = false;
            try {
              const status = await providersApi.detectCodexRunning();
              codexRunning = status.running;
              probeOk = true;
            } catch (probeErr) {
              console.warn(
                "detect_codex_running 失败，保守走重启提示",
                probeErr,
              );
            }
            if (probeOk && !codexRunning) {
              messageKey = "notifications.codexSeamlessSwitched";
              defaultMessage = "切换成功。Codex 未在运行，下次启动即可生效";
              toast.success(t(messageKey, { defaultValue: defaultMessage }), {
                closeButton: true,
              });
              continueWithSwitchToast = false;
            } else if (probeOk && codexRunning) {
              messageKey = "notifications.codexSeamlessSwitchedRunning";
              defaultMessage =
                "切换成功。Codex 正在运行：当前会话仍用旧配置，新会话已用新配置；点此重启 Codex 让当前会话也生效";
              const cta = t("notifications.codexRestartCta", {
                defaultValue: "重启 Codex",
              });
              const ctaDone = t("notifications.codexRestartDone", {
                defaultValue: "Codex 已重启，请运行 codex 继续",
              });
              const ctaNoop = t("notifications.codexRestartNotRunning", {
                defaultValue: "Codex 未在运行，无需重启",
              });
              toast.success(t(messageKey, { defaultValue: defaultMessage }), {
                closeButton: true,
                action: {
                  label: cta,
                  // 真重启：调用后端 restart_codex_process 跨平台杀进程。
                  // 后端在检测到活跃 session（~/.codex/sessions/**.jsonl
                  // 30s 内被写过）时**拒绝**并回传原因，避免丢对话上下文。
                  //
                  // 只杀不拉起：Codex CLI 抢 TTY，从 GUI 里 fork 新的会抢焦点、
                  // 也拿不到用户的终端环境变量。用户在原终端敲 `codex` 即可。
                  onClick: async () => {
                    try {
                      const result = await providersApi.restartCodexProcess();
                      if (result.refused) {
                        toast.warning(
                          t("notifications.codexRestartRefused", {
                            defaultValue:
                              result.refusal_reason ??
                              "检测到活跃 Codex session，为避免丢上下文已取消自动重启；请先在 Codex 里 /exit，再点重启。",
                          }),
                          { duration: 9000 },
                        );
                        return;
                      }
                      toast.success(result.running_before ? ctaDone : ctaNoop, {
                        duration: 4000,
                      });
                    } catch (restartErr) {
                      console.error("restart_codex_process 失败", restartErr);
                      toast.warning(
                        t("notifications.codexRestartFailed", {
                          defaultValue:
                            "自动重启 Codex 失败，请在终端手动执行 pkill -f codex 后重试",
                        }),
                        { duration: 6000 },
                      );
                    }
                  },
                },
              });
              continueWithSwitchToast = false;
            } else {
              messageKey = "notifications.codexRestartRequired";
              defaultMessage = "切换成功，请重启客户端以生效";
            }
          } else if (activeApp === "grokbuild") {
            messageKey = "notifications.grokBuildRestartRequired";
            defaultMessage = "切换成功，请重启 Grok Build 以生效";
          } else if (activeApp === "claude-desktop") {
            if (provider.meta?.claudeDesktopMode === "proxy") {
              messageKey = "notifications.claudeDesktopProxyRestartRequired";
              defaultMessage =
                "切换成功，请保持 CC Switch 运行，并重启 Claude Desktop 后生效";
            } else {
              messageKey = "notifications.claudeDesktopRestartRequired";
              defaultMessage = "切换成功，重启 Claude Desktop 后生效";
            }
          } else if (
            activeApp === "opencode" ||
            activeApp === "openclaw" ||
            activeApp === "mcode"
          ) {
            messageKey = "notifications.addToConfigSuccess";
            defaultMessage = "已添加到配置";
          }
          if (continueWithSwitchToast) {
            toast.success(t(messageKey, { defaultValue: defaultMessage }), {
              closeButton: true,
            });
          }
        }
      } catch {
        // 错误提示由 mutation 处理
      }
    },
    [
      switchProviderMutation,
      syncClaudePlugin,
      activeApp,
      isProxyRunning,
      isProxyTakeover,
      t,
    ],
  );

  // 删除供应商
  const deleteProvider = useCallback(
    async (id: string) => {
      await deleteProviderMutation.mutateAsync(id);
    },
    [deleteProviderMutation],
  );

  // 保存用量脚本
  const saveUsageScript = useCallback(
    async (provider: Provider, script: UsageScript) => {
      try {
        const updatedProvider: Provider = {
          ...provider,
          meta: {
            ...provider.meta,
            usage_script: script,
          },
        };

        if (activeApp === "pi") {
          await piApi.updateProviderUsageScript(provider.id, script);
        } else {
          await providersApi.update(updatedProvider, activeApp);
        }
        await queryClient.invalidateQueries({
          queryKey: ["providers", activeApp],
        });
        // 🔧 保存用量脚本后，也应该失效该 provider 的用量查询缓存
        // 这样主页列表会使用新配置重新查询，而不是使用测试时的缓存
        await queryClient.invalidateQueries({
          queryKey: usageKeys.script(provider.id, activeApp),
        });
        await queryClient.invalidateQueries({
          queryKey: ["subscription", "quota", activeApp],
        });
        toast.success(
          t("provider.usageSaved", {
            defaultValue: "用量查询配置已保存",
          }),
          { closeButton: true },
        );
      } catch (error) {
        const detail =
          extractErrorMessage(error) ||
          t("provider.usageSaveFailed", {
            defaultValue: "用量查询配置保存失败",
          });
        toast.error(detail);
      }
    },
    [activeApp, queryClient, t],
  );

  // Set provider as default model (OpenClaw only)
  const setAsDefaultModel = useCallback(
    async (provider: Provider, modelId?: string) => {
      const config = provider.settingsConfig as OpenClawProviderConfig;
      if (!config.models || config.models.length === 0) {
        toast.error(
          t("notifications.openclawNoModels", {
            defaultValue: "该供应商没有配置模型",
          }),
        );
        return;
      }

      const selectedModel = modelId
        ? config.models.find((model) => model.id === modelId)
        : config.models[0];
      if (!selectedModel) {
        toast.error(
          t("notifications.openclawModelNotFound", {
            defaultValue: "所选模型已不存在，请刷新后重试",
          }),
        );
        return;
      }

      try {
        const primary = `${provider.id}/${selectedModel.id}`;
        const existingDefault = await openclawApi.getDefaultModel();
        const model: OpenClawDefaultModel = {
          ...(existingDefault ?? {}),
          primary,
        };
        if (existingDefault?.fallbacks) {
          model.fallbacks = existingDefault.fallbacks.filter(
            (fallback) => fallback !== primary,
          );
        }

        await openclawApi.setDefaultModel(model);
        await queryClient.invalidateQueries({
          queryKey: openclawKeys.defaultModel,
        });
        await queryClient.invalidateQueries({
          queryKey: openclawKeys.health,
        });
        toast.success(
          t("notifications.openclawDefaultModelSet", {
            defaultValue: "已设为默认模型",
          }),
          { closeButton: true },
        );
      } catch (error) {
        const detail =
          extractErrorMessage(error) ||
          t("notifications.openclawDefaultModelSetFailed", {
            defaultValue: "设置默认模型失败",
          });
        toast.error(detail);
      }
    },
    [queryClient, t],
  );

  return {
    addProvider,
    updateProvider,
    switchProvider,
    deleteProvider,
    saveUsageScript,
    setAsDefaultModel,
    isLoading:
      addProviderMutation.isPending ||
      updateProviderMutation.isPending ||
      deleteProviderMutation.isPending ||
      switchProviderMutation.isPending,
  };
}
