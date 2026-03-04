import { useTranslation } from "react-i18next";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Info, LogIn, Copy, XCircle, Timer } from "lucide-react";
import { Button } from "@/components/ui/button";
import EndpointSpeedTest from "./EndpointSpeedTest";
import { ApiKeySection, EndpointField } from "./shared";
import type { ProviderCategory } from "@/types";

interface EndpointCandidate {
  url: string;
}

interface GeminiFormFieldsProps {
  providerId?: string;
  // API Key
  shouldShowApiKey: boolean;
  apiKey: string;
  onApiKeyChange: (key: string) => void;
  category?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  websiteUrl: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;

  // Base URL
  shouldShowSpeedTest: boolean;
  baseUrl: string;
  onBaseUrlChange: (url: string) => void;
  isEndpointModalOpen: boolean;
  onEndpointModalToggle: (open: boolean) => void;
  onCustomEndpointsChange: (endpoints: string[]) => void;
  autoSelect: boolean;
  onAutoSelectChange: (checked: boolean) => void;

  // Model
  shouldShowModelField: boolean;
  model: string;
  onModelChange: (value: string) => void;

  // Speed Test Endpoints
  speedTestEndpoints: EndpointCandidate[];
  onStartCliLogin?: () => void;
  cliLoginLoading?: boolean;
  canStartCliLogin?: boolean;
  cliLoginPanel?: {
    authUrl?: string;
    expectedFilesDir?: string;
    userCode?: string;
    message?: string;
    remainingSeconds: number;
    statusText: string;
    errorText?: string;
    isPolling: boolean;
    isFinished: boolean;
  } | null;
  onCopyCliLoginUrl?: () => void;
  onCancelCliLogin?: () => void;
  cliLoginCancelLoading?: boolean;
}

export function GeminiFormFields({
  providerId,
  shouldShowApiKey,
  apiKey,
  onApiKeyChange,
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  isPartner,
  partnerPromotionKey,
  shouldShowSpeedTest,
  baseUrl,
  onBaseUrlChange,
  isEndpointModalOpen,
  onEndpointModalToggle,
  onCustomEndpointsChange,
  autoSelect,
  onAutoSelectChange,
  shouldShowModelField,
  model,
  onModelChange,
  speedTestEndpoints,
  onStartCliLogin,
  cliLoginLoading = false,
  canStartCliLogin = false,
  cliLoginPanel,
  onCopyCliLoginUrl,
  onCancelCliLogin,
  cliLoginCancelLoading = false,
}: GeminiFormFieldsProps) {
  const { t } = useTranslation();

  // 检测是否为 Google 官方预设
  const isGoogleOfficial =
    partnerPromotionKey?.toLowerCase() === "google-official";

  return (
    <>
      {/* 官方登录：文件凭据导入流程 */}
      {category === "official" && (
        <div className="space-y-3 rounded-lg border border-blue-200 bg-blue-50 p-4 dark:border-blue-800 dark:bg-blue-950">
          <div className="flex gap-3">
            <Info className="h-5 w-5 flex-shrink-0 text-blue-600 dark:text-blue-400" />
            <div className="space-y-1">
              <p className="text-sm font-medium text-blue-900 dark:text-blue-100">
                {t("provider.form.gemini.fileLoginTitle", {
                  defaultValue: "文件凭据导入登录",
                })}
              </p>
              <p className="text-sm text-blue-700 dark:text-blue-300">
                {t("provider.form.gemini.fileLoginHint", {
                  defaultValue:
                    "点击后按面板提示将 oauth_creds.json 与 google_accounts.json 放入隔离目录，等待状态变为已授权后完成绑定。",
                })}
              </p>
              {isGoogleOfficial && (
                <p className="text-xs text-blue-700/80 dark:text-blue-300/80">
                  Google Official
                </p>
              )}
            </div>
          </div>

          {onStartCliLogin && (
            <div className="space-y-2 rounded-md border border-blue-200/70 bg-white/60 p-3 dark:border-blue-800/70 dark:bg-blue-900/20">
              <Button
                type="button"
                onClick={onStartCliLogin}
                disabled={!canStartCliLogin || cliLoginLoading}
                className="w-full sm:w-auto"
              >
                <LogIn className="h-4 w-4 mr-2" />
                Login with Gemini
              </Button>

              {cliLoginPanel && (
                <div className="space-y-2 rounded-md border border-border-default bg-background px-3 py-2">
                  <div className="flex items-center justify-between gap-2">
                    <span className="text-sm font-medium">授权状态</span>
                    <span className="text-xs text-muted-foreground">
                      {cliLoginPanel.statusText}
                    </span>
                  </div>
                  <div className="text-xs text-muted-foreground break-all">
                    authUrl: {cliLoginPanel.authUrl ?? "--"}
                  </div>
                  <div className="text-xs text-muted-foreground break-all">
                    expectedFilesDir: {cliLoginPanel.expectedFilesDir ?? "--"}
                  </div>
                  <p className="text-xs text-muted-foreground">
                    请将 `oauth_creds.json` 和 `google_accounts.json` 放入上方隔离目录后继续轮询。
                  </p>
                  {cliLoginPanel.message && (
                    <p className="text-xs text-muted-foreground break-all">
                      {cliLoginPanel.message}
                    </p>
                  )}
                  <div className="flex flex-wrap items-center gap-2">
                    {onCopyCliLoginUrl && (
                      <Button
                        type="button"
                        size="sm"
                        variant="secondary"
                        onClick={onCopyCliLoginUrl}
                        disabled={!cliLoginPanel.authUrl}
                      >
                        <Copy className="h-3.5 w-3.5 mr-1.5" />
                        {t("common.copy", { defaultValue: "复制" })}
                      </Button>
                    )}
                    {cliLoginPanel.userCode && (
                      <code className="inline-flex items-center rounded bg-muted px-2 py-1 text-sm font-semibold tracking-wider">
                        {cliLoginPanel.userCode}
                      </code>
                    )}
                  </div>
                  <div className="flex flex-wrap items-center gap-3 text-xs text-muted-foreground">
                    <span className="inline-flex items-center gap-1">
                      <Timer className="h-3.5 w-3.5" />
                      {t("provider.geminiLoginCountdown", {
                        defaultValue: "剩余 {{seconds}} 秒",
                        seconds: Math.max(0, cliLoginPanel.remainingSeconds),
                      })}
                    </span>
                    {cliLoginPanel.isPolling && !cliLoginPanel.isFinished && (
                      <span>正在检查导入状态...</span>
                    )}
                  </div>
                  {cliLoginPanel.errorText && (
                    <p className="text-xs text-destructive">
                      {cliLoginPanel.errorText}
                    </p>
                  )}
                  {onCancelCliLogin && !cliLoginPanel.isFinished && (
                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      onClick={onCancelCliLogin}
                      disabled={cliLoginCancelLoading}
                    >
                      <XCircle className="h-3.5 w-3.5 mr-1.5" />
                      {t("common.cancel", { defaultValue: "取消" })}
                    </Button>
                  )}
                </div>
              )}
            </div>
          )}
        </div>
      )}

      {/* API Key 输入框 */}
      {shouldShowApiKey && !isGoogleOfficial && (
        <ApiKeySection
          value={apiKey}
          onChange={onApiKeyChange}
          category={category}
          shouldShowLink={shouldShowApiKeyLink}
          websiteUrl={websiteUrl}
          isPartner={isPartner}
          partnerPromotionKey={partnerPromotionKey}
        />
      )}

      {/* Base URL 输入框（统一使用与 Codex 相同的样式与交互） */}
      {shouldShowSpeedTest && (
        <EndpointField
          id="baseUrl"
          label={t("providerForm.apiEndpoint", { defaultValue: "API 端点" })}
          value={baseUrl}
          onChange={onBaseUrlChange}
          placeholder={t("providerForm.apiEndpointPlaceholder", {
            defaultValue: "https://your-api-endpoint.com/",
          })}
          onManageClick={() => onEndpointModalToggle(true)}
        />
      )}

      {/* Model 输入框 */}
      {shouldShowModelField && (
        <div>
          <FormLabel htmlFor="gemini-model">
            {t("provider.form.gemini.model", { defaultValue: "模型" })}
          </FormLabel>
          <Input
            id="gemini-model"
            value={model}
            onChange={(e) => onModelChange(e.target.value)}
            placeholder="gemini-3-pro-preview"
          />
        </div>
      )}

      {/* 端点测速弹窗 */}
      {shouldShowSpeedTest && isEndpointModalOpen && (
        <EndpointSpeedTest
          appId="gemini"
          providerId={providerId}
          value={baseUrl}
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
