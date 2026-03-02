import { useState, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { Info, Upload, CheckCircle2, XCircle, Key, FileKey2 } from "lucide-react";
import { toast } from "sonner";
import EndpointSpeedTest from "./EndpointSpeedTest";
import { ApiKeySection, EndpointField } from "./shared";
import type { ProviderCategory } from "@/types";

export type VertexAuthMode = "api-key" | "service-account";

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

  // Vertex
  vertexAuthMode?: VertexAuthMode;
  onVertexAuthModeChange?: (mode: VertexAuthMode) => void;
  vertexRegion?: string;
  onVertexRegionChange?: (region: string) => void;
  vertexServiceAccountJson?: string;
  onVertexServiceAccountJsonChange?: (json: string) => void;
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
  vertexAuthMode,
  onVertexAuthModeChange,
  vertexRegion,
  onVertexRegionChange,
  vertexServiceAccountJson,
  onVertexServiceAccountJsonChange,
}: GeminiFormFieldsProps) {
  const { t } = useTranslation();

  const isGoogleOfficial =
    partnerPromotionKey?.toLowerCase() === "google-official";
  const isVertex =
    partnerPromotionKey?.toLowerCase() === "google-vertex";

  // 服务账号 JSON 验证状态
  const [jsonValidation, setJsonValidation] = useState<{
    isValid: boolean;
    projectId?: string;
    error?: string;
  }>({ isValid: false });

  const validateServiceAccountJson = useCallback((jsonStr: string) => {
    if (!jsonStr.trim()) {
      setJsonValidation({ isValid: false });
      return;
    }
    try {
      const json = JSON.parse(jsonStr);
      const requiredFields = [
        "type",
        "project_id",
        "private_key_id",
        "private_key",
        "client_email",
        "client_id",
      ];
      const missingFields = requiredFields.filter((field) => !json[field]);
      if (missingFields.length > 0) {
        setJsonValidation({
          isValid: false,
          error: `缺少必要字段: ${missingFields.join(", ")}`,
        });
        return;
      }
      if (json.type !== "service_account") {
        setJsonValidation({
          isValid: false,
          error: "type 字段必须为 service_account",
        });
        return;
      }
      setJsonValidation({ isValid: true, projectId: json.project_id });
    } catch (error) {
      setJsonValidation({
        isValid: false,
        error: error instanceof Error ? error.message : "JSON 格式错误",
      });
    }
  }, []);

  // 编辑模式下自动验证已有的服务账号 JSON
  useEffect(() => {
    if (vertexServiceAccountJson && vertexServiceAccountJson.trim()) {
      validateServiceAccountJson(vertexServiceAccountJson);
    }
  }, [vertexServiceAccountJson, validateServiceAccountJson]);

  const handleFileUpload = async (
    event: React.ChangeEvent<HTMLInputElement>,
  ) => {
    const file = event.target.files?.[0];
    if (!file) return;
    try {
      const text = await file.text();
      onVertexServiceAccountJsonChange?.(text);
      validateServiceAccountJson(text);
      toast.success(
        t("provider.form.vertex.fileUploaded", {
          defaultValue: "服务账号文件已上传",
        }),
      );
    } catch {
      toast.error(
        t("provider.form.vertex.fileError", {
          defaultValue: "读取文件失败",
        }),
      );
    }
    event.target.value = "";
  };

  const handleJsonChange = (value: string) => {
    onVertexServiceAccountJsonChange?.(value);
    validateServiceAccountJson(value);
  };

  return (
    <>
      {/* Google OAuth 提示 */}
      {isGoogleOfficial && (
        <div className="rounded-lg border border-blue-200 bg-blue-50 p-4 dark:border-blue-800 dark:bg-blue-950">
          <div className="flex gap-3">
            <Info className="h-5 w-5 flex-shrink-0 text-blue-600 dark:text-blue-400" />
            <div className="space-y-1">
              <p className="text-sm font-medium text-blue-900 dark:text-blue-100">
                {t("provider.form.gemini.oauthTitle", {
                  defaultValue: "OAuth 认证模式",
                })}
              </p>
              <p className="text-sm text-blue-700 dark:text-blue-300">
                {t("provider.form.gemini.oauthHint", {
                  defaultValue:
                    "Google 官方使用 OAuth 个人认证，无需填写 API Key。首次使用时会自动打开浏览器进行登录。",
                })}
              </p>
            </div>
          </div>
        </div>
      )}

      {/* Vertex AI 模式 */}
      {isVertex && (
        <div className="space-y-4">
          {/* Region */}
          <div className="space-y-2">
            <FormLabel>
              {t("provider.form.vertex.region", { defaultValue: "Region" })}
            </FormLabel>
            <Input
              value={vertexRegion ?? "global"}
              onChange={(e) => onVertexRegionChange?.(e.target.value)}
              placeholder="global"
            />
            <p className="text-sm text-muted-foreground">
              {t("provider.form.vertex.regionHint", {
                defaultValue:
                  "GCP 区域，默认为 global。常用: us-central1, us-east5, europe-west1",
              })}
            </p>
          </div>

          {/* 授权模式切换 */}
          <div className="space-y-2">
            <FormLabel>
              {t("provider.form.vertex.authMode", {
                defaultValue: "授权模式",
              })}
            </FormLabel>
            <div className="grid grid-cols-2 gap-2">
              <button
                type="button"
                onClick={() => onVertexAuthModeChange?.("api-key")}
                className={`flex items-center gap-2 rounded-lg border p-3 text-left text-sm transition-colors ${
                  vertexAuthMode === "api-key"
                    ? "border-blue-500 bg-blue-50 text-blue-700 dark:border-blue-400 dark:bg-blue-950 dark:text-blue-300"
                    : "border-border hover:bg-muted"
                }`}
              >
                <Key className="h-4 w-4 flex-shrink-0" />
                <div>
                  <p className="font-medium">API Key</p>
                  <p className="text-xs text-muted-foreground">
                    {t("provider.form.vertex.apiKeyDesc", {
                      defaultValue: "Fast 模式，使用 API Key 认证",
                    })}
                  </p>
                </div>
              </button>
              <button
                type="button"
                onClick={() => onVertexAuthModeChange?.("service-account")}
                className={`flex items-center gap-2 rounded-lg border p-3 text-left text-sm transition-colors ${
                  vertexAuthMode === "service-account"
                    ? "border-blue-500 bg-blue-50 text-blue-700 dark:border-blue-400 dark:bg-blue-950 dark:text-blue-300"
                    : "border-border hover:bg-muted"
                }`}
              >
                <FileKey2 className="h-4 w-4 flex-shrink-0" />
                <div>
                  <p className="font-medium">
                    {t("provider.form.vertex.serviceAccount", {
                      defaultValue: "服务账号",
                    })}
                  </p>
                  <p className="text-xs text-muted-foreground">
                    {t("provider.form.vertex.serviceAccountDesc", {
                      defaultValue: "支持 Gemini、Claude、开源模型",
                    })}
                  </p>
                </div>
              </button>
            </div>
          </div>

          {/* API Key 模式 */}
          {vertexAuthMode === "api-key" && (
            <ApiKeySection
              value={apiKey}
              onChange={onApiKeyChange}
              category={category}
              placeholder={{
                official: t("providerForm.apiKeyAutoFill", {
                  defaultValue: "输入 Vertex API Key",
                }),
                thirdParty: t("providerForm.apiKeyAutoFill", {
                  defaultValue: "输入 Vertex API Key",
                }),
              }}
              shouldShowLink={shouldShowApiKeyLink}
              websiteUrl={websiteUrl}
              isPartner={isPartner}
              partnerPromotionKey={partnerPromotionKey}
            />
          )}

          {/* 服务账号模式 */}
          {vertexAuthMode === "service-account" && (
            <>
              <div className="rounded-lg border border-green-200 bg-green-50 p-3 dark:border-green-800 dark:bg-green-950">
                <div className="flex gap-2">
                  <Info className="h-4 w-4 flex-shrink-0 text-green-600 dark:text-green-400 mt-0.5" />
                  <p className="text-sm text-green-700 dark:text-green-300">
                    {t("provider.form.vertex.serviceAccountHint", {
                      defaultValue:
                        "服务账号模式支持完整的 Vertex AI 功能，包括 Gemini、Claude 和开源模型。",
                    })}
                  </p>
                </div>
              </div>

              {/* 文件上传 */}
              <div className="space-y-2">
                <FormLabel>
                  {t("provider.form.vertex.uploadJson", {
                    defaultValue: "上传服务账号 JSON 文件",
                  })}
                </FormLabel>
                <Button
                  type="button"
                  variant="outline"
                  onClick={() =>
                    document.getElementById("vertex-sa-file")?.click()
                  }
                  className="w-full"
                >
                  <Upload className="mr-2 h-4 w-4" />
                  {t("provider.form.vertex.chooseFile", {
                    defaultValue: "选择文件",
                  })}
                </Button>
                <input
                  id="vertex-sa-file"
                  type="file"
                  accept=".json"
                  className="hidden"
                  onChange={handleFileUpload}
                />
              </div>

              {/* JSON 文本输入 */}
              <div className="space-y-2">
                <FormLabel>
                  {t("provider.form.vertex.pasteJson", {
                    defaultValue: "或直接粘贴 JSON 内容",
                  })}
                </FormLabel>
                <Textarea
                  value={vertexServiceAccountJson ?? ""}
                  onChange={(e) => handleJsonChange(e.target.value)}
                  placeholder={`{
  "type": "service_account",
  "project_id": "your-project-id",
  ...
}`}
                  className="font-mono text-sm min-h-[160px]"
                />
              </div>

              {/* 验证状态 */}
              {vertexServiceAccountJson && (
                <div
                  className={`rounded-lg border p-3 ${
                    jsonValidation.isValid
                      ? "border-green-200 bg-green-50 dark:border-green-800 dark:bg-green-950"
                      : "border-red-200 bg-red-50 dark:border-red-800 dark:bg-red-950"
                  }`}
                >
                  <div className="flex gap-2">
                    {jsonValidation.isValid ? (
                      <>
                        <CheckCircle2 className="h-4 w-4 flex-shrink-0 text-green-600 dark:text-green-400 mt-0.5" />
                        <div className="text-sm text-green-700 dark:text-green-300">
                          <p className="font-medium">
                            {t("provider.form.vertex.validationSuccess", {
                              defaultValue: "验证成功",
                            })}
                          </p>
                          {jsonValidation.projectId && (
                            <p className="mt-1">
                              Project ID: {jsonValidation.projectId}
                            </p>
                          )}
                        </div>
                      </>
                    ) : (
                      <>
                        <XCircle className="h-4 w-4 flex-shrink-0 text-red-600 dark:text-red-400 mt-0.5" />
                        <div className="text-sm text-red-700 dark:text-red-300">
                          <p className="font-medium">
                            {t("provider.form.vertex.validationFailed", {
                              defaultValue: "验证失败",
                            })}
                          </p>
                          {jsonValidation.error && (
                            <p className="mt-1">{jsonValidation.error}</p>
                          )}
                        </div>
                      </>
                    )}
                  </div>
                </div>
              )}
            </>
          )}
        </div>
      )}

      {/* 非 Vertex 的 API Key 输入框 */}
      {!isVertex && shouldShowApiKey && !isGoogleOfficial && (
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

      {/* Base URL 输入框 */}
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
