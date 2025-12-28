import { useTranslation } from "react-i18next";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { ApiKeySection, EndpointField } from "./shared";
import type { ProviderCategory } from "@/types";

interface DroidFormFieldsProps {
  // API Key
  shouldShowApiKey: boolean;
  apiKey: string;
  onApiKeyChange: (key: string) => void;
  category?: ProviderCategory;

  // Base URL
  baseUrl: string;
  onBaseUrlChange: (url: string) => void;

  // Model
  model: string;
  onModelChange: (value: string) => void;

  // Provider Type
  provider: string;
  onProviderChange: (value: string) => void;
}

export function DroidFormFields({
  shouldShowApiKey,
  apiKey,
  onApiKeyChange,
  category,
  baseUrl,
  onBaseUrlChange,
  model,
  onModelChange,
  provider,
  onProviderChange,
}: DroidFormFieldsProps) {
  const { t } = useTranslation();

  return (
    <>
      {/* API Key 输入框 */}
      {shouldShowApiKey && (
        <ApiKeySection
          value={apiKey}
          onChange={onApiKeyChange}
          category={category}
          shouldShowLink={false}
          websiteUrl=""
        />
      )}

      {/* Base URL 输入框 */}
      <EndpointField
        id="droidBaseUrl"
        label={t("providerForm.apiEndpoint", { defaultValue: "API 端点" })}
        value={baseUrl}
        onChange={onBaseUrlChange}
        placeholder="https://api.example.com"
      />

      {/* Model 输入框 */}
      <div>
        <FormLabel htmlFor="droid-model">
          {t("provider.model", { defaultValue: "模型" })}
        </FormLabel>
        <Input
          id="droid-model"
          value={model}
          onChange={(e) => onModelChange(e.target.value)}
          placeholder="claude-sonnet-4-5-20250929"
        />
      </div>

      {/* Provider Type 下拉选择 */}
      <div>
        <FormLabel htmlFor="droid-provider">
          {t("provider.providerType", { defaultValue: "Provider Type" })}
        </FormLabel>
        <Select value={provider} onValueChange={onProviderChange}>
          <SelectTrigger id="droid-provider">
            <SelectValue
              placeholder={t("provider.selectProviderType", {
                defaultValue: "Select provider type",
              })}
            />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="anthropic">Anthropic</SelectItem>
            <SelectItem value="generic-chat-completion-api">
              Generic Chat Completion API
            </SelectItem>
          </SelectContent>
        </Select>
      </div>
    </>
  );
}
