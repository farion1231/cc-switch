import { useTranslation } from "react-i18next";
import { KeyRound, LogIn, Copy, XCircle, Timer } from "lucide-react";
import { Button } from "@/components/ui/button";
import EndpointSpeedTest from "./EndpointSpeedTest";
import { ApiKeySection, EndpointField } from "./shared";
import type { ProviderCategory } from "@/types";

interface EndpointCandidate {
  url: string;
}

interface CodexFormFieldsProps {
  providerId?: string;
  // API Key
  codexApiKey: string;
  onApiKeyChange: (key: string) => void;
  category?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  websiteUrl: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;

  // Base URL
  shouldShowSpeedTest: boolean;
  codexBaseUrl: string;
  onBaseUrlChange: (url: string) => void;
  isEndpointModalOpen: boolean;
  onEndpointModalToggle: (open: boolean) => void;
  onCustomEndpointsChange?: (endpoints: string[]) => void;
  autoSelect: boolean;
  onAutoSelectChange: (checked: boolean) => void;

  // Model Name
  shouldShowModelField?: boolean;
  modelName?: string;
  onModelNameChange?: (model: string) => void;

  // Speed Test Endpoints
  speedTestEndpoints: EndpointCandidate[];
  onQuickBindAuth?: () => void;
  quickBindLoading?: boolean;
  canQuickBind?: boolean;
  onStartDeviceLogin?: () => void;
  deviceLoginLoading?: boolean;
  deviceLoginPanel?: {
    userCode: string;
    verificationUrl: string;
    remainingSeconds: number;
    statusText: string;
    errorText?: string;
    isPolling: boolean;
    isFinished: boolean;
  } | null;
  onCopyDeviceCode?: () => void;
  onCancelDeviceLogin?: () => void;
  deviceLoginCancelLoading?: boolean;
}

export function CodexFormFields({
  providerId,
  codexApiKey,
  onApiKeyChange,
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  isPartner,
  partnerPromotionKey,
  shouldShowSpeedTest,
  codexBaseUrl,
  onBaseUrlChange,
  isEndpointModalOpen,
  onEndpointModalToggle,
  onCustomEndpointsChange,
  autoSelect,
  onAutoSelectChange,
  shouldShowModelField = true,
  modelName = "",
  onModelNameChange,
  speedTestEndpoints,
  onQuickBindAuth,
  quickBindLoading = false,
  canQuickBind = false,
  onStartDeviceLogin,
  deviceLoginLoading = false,
  deviceLoginPanel,
  onCopyDeviceCode,
  onCancelDeviceLogin,
  deviceLoginCancelLoading = false,
}: CodexFormFieldsProps) {
  const { t } = useTranslation();

  return (
    <>
      {/* Codex API Key 输入框 */}
      <ApiKeySection
        id="codexApiKey"
        label="API Key"
        value={codexApiKey}
        onChange={onApiKeyChange}
        category={category}
        shouldShowLink={shouldShowApiKeyLink}
        websiteUrl={websiteUrl}
        isPartner={isPartner}
        partnerPromotionKey={partnerPromotionKey}
        placeholder={{
          official: t("providerForm.codexOfficialNoApiKey", {
            defaultValue: "官方供应商无需 API Key",
          }),
          thirdParty: t("providerForm.codexApiKeyAutoFill", {
            defaultValue: "输入 API Key，将自动填充到配置",
          }),
        }}
      />

      {category === "official" && onStartDeviceLogin && (
        <div className="space-y-2 rounded-lg border border-border-default/80 bg-muted/20 p-3">
          <div className="flex flex-wrap items-center gap-2">
            <Button
              type="button"
              onClick={onStartDeviceLogin}
              disabled={!canQuickBind || deviceLoginLoading}
              className="w-full sm:w-auto"
            >
              <LogIn className="h-4 w-4 mr-2" />
              {t("provider.codexDeviceLogin", {
                defaultValue: "Login with ChatGPT",
              })}
            </Button>
            {onQuickBindAuth && (
              <Button
                type="button"
                variant="outline"
                onClick={onQuickBindAuth}
                disabled={!canQuickBind || quickBindLoading}
                className="w-full sm:w-auto"
              >
                <KeyRound className="h-4 w-4 mr-2" />
                {t("provider.codexBindLogin", {
                  defaultValue: "一键绑定当前登录态",
                })}
              </Button>
            )}
          </div>
          <p className="text-xs text-muted-foreground">
            {t("provider.codexDeviceLoginHint", {
              defaultValue:
                "点击后将自动打开 ChatGPT 验证页面，请输入授权码并等待授权完成。",
            })}
          </p>
          {deviceLoginPanel && (
            <div className="space-y-2 rounded-md border border-border-default bg-background px-3 py-2">
              <div className="flex items-center justify-between gap-2">
                <span className="text-sm font-medium">
                  {t("provider.codexDeviceLoginPanelTitle", {
                    defaultValue: "授权状态",
                  })}
                </span>
                <span className="text-xs text-muted-foreground">
                  {deviceLoginPanel.statusText}
                </span>
              </div>
              <div className="text-xs text-muted-foreground break-all">
                {deviceLoginPanel.verificationUrl}
              </div>
              <div className="flex items-center gap-2">
                <code className="inline-flex items-center rounded bg-muted px-2 py-1 text-sm font-semibold tracking-wider">
                  {deviceLoginPanel.userCode}
                </code>
                {onCopyDeviceCode && (
                  <Button
                    type="button"
                    size="sm"
                    variant="secondary"
                    onClick={onCopyDeviceCode}
                  >
                    <Copy className="h-3.5 w-3.5 mr-1.5" />
                    {t("common.copy", { defaultValue: "复制" })}
                  </Button>
                )}
              </div>
              <div className="flex flex-wrap items-center gap-3 text-xs text-muted-foreground">
                <span className="inline-flex items-center gap-1">
                  <Timer className="h-3.5 w-3.5" />
                  {t("provider.codexDeviceLoginCountdown", {
                    defaultValue: "剩余 {{seconds}} 秒",
                    seconds: Math.max(0, deviceLoginPanel.remainingSeconds),
                  })}
                </span>
                {deviceLoginPanel.isPolling && !deviceLoginPanel.isFinished && (
                  <span>{t("provider.codexDeviceLoginPolling", { defaultValue: "正在检查授权状态..." })}</span>
                )}
              </div>
              {deviceLoginPanel.errorText && (
                <p className="text-xs text-destructive">
                  {deviceLoginPanel.errorText}
                </p>
              )}
              {onCancelDeviceLogin && !deviceLoginPanel.isFinished && (
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  onClick={onCancelDeviceLogin}
                  disabled={deviceLoginCancelLoading}
                >
                  <XCircle className="h-3.5 w-3.5 mr-1.5" />
                  {t("common.cancel", { defaultValue: "取消" })}
                </Button>
              )}
            </div>
          )}
        </div>
      )}

      {category !== "official" && onQuickBindAuth && (
        <div className="space-y-2">
          <Button
            type="button"
            variant="outline"
            onClick={onQuickBindAuth}
            disabled={!canQuickBind || quickBindLoading}
            className="w-full sm:w-auto"
          >
            <KeyRound className="h-4 w-4 mr-2" />
            {t("provider.codexBindLogin", {
              defaultValue: "一键绑定当前登录态",
            })}
          </Button>
          <p className="text-xs text-muted-foreground">
            {t("provider.codexBindLoginHint", {
              defaultValue:
                "从该 Provider 已保存的 ChatGPT 登录信息自动绑定账号，用于额度轮询与调度。",
            })}
          </p>
        </div>
      )}

      {/* Codex Base URL 输入框 */}
      {shouldShowSpeedTest && (
        <EndpointField
          id="codexBaseUrl"
          label={t("codexConfig.apiUrlLabel")}
          value={codexBaseUrl}
          onChange={onBaseUrlChange}
          placeholder={t("providerForm.codexApiEndpointPlaceholder")}
          hint={t("providerForm.codexApiHint")}
          onManageClick={() => onEndpointModalToggle(true)}
        />
      )}

      {/* Codex Model Name 输入框 */}
      {shouldShowModelField && onModelNameChange && (
        <div className="space-y-2">
          <label
            htmlFor="codexModelName"
            className="block text-sm font-medium text-foreground"
          >
            {t("codexConfig.modelName", { defaultValue: "模型名称" })}
          </label>
          <input
            id="codexModelName"
            type="text"
            value={modelName}
            onChange={(e) => onModelNameChange(e.target.value)}
            placeholder={t("codexConfig.modelNamePlaceholder", {
              defaultValue: "例如: gpt-5-codex",
            })}
            className="w-full px-3 py-2 border border-border-default bg-background text-foreground rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-blue-500/20 dark:focus:ring-blue-400/20 transition-colors"
          />
          <p className="text-xs text-muted-foreground">
            {t("codexConfig.modelNameHint", {
              defaultValue: "指定使用的模型，将自动更新到 config.toml 中",
            })}
          </p>
        </div>
      )}

      {/* 端点测速弹窗 - Codex */}
      {shouldShowSpeedTest && isEndpointModalOpen && (
        <EndpointSpeedTest
          appId="codex"
          providerId={providerId}
          value={codexBaseUrl}
          onChange={onBaseUrlChange}
          initialEndpoints={speedTestEndpoints}
          visible={isEndpointModalOpen}
          onClose={() => onEndpointModalToggle(false)}
          autoSelect={autoSelect}
          onAutoSelectChange={onAutoSelectChange}
          onCustomEndpointsChange={onCustomEndpointsChange}
        />
      )}
    </>
  );
}
