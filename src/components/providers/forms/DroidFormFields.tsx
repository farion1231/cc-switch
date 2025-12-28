import { useTranslation } from "react-i18next";
import { FormField, FormItem, FormLabel, FormControl } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { UseFormReturn } from "react-hook-form";
import ApiKeyInput from "./ApiKeyInput";

interface DroidFormFieldsProps {
  form: UseFormReturn<any>;
  apiKey: string;
  setApiKey: (value: string) => void;
  baseUrl: string;
  setBaseUrl: (value: string) => void;
  model: string;
  setModel: (value: string) => void;
  provider: string;
  setProvider: (value: string) => void;
}

export function DroidFormFields({
  form,
  apiKey,
  setApiKey,
  baseUrl,
  setBaseUrl,
  model,
  setModel,
  provider,
  setProvider,
}: DroidFormFieldsProps) {
  const { t } = useTranslation();

  return (
    <div className="space-y-4">
      {/* API Key */}
      <FormField
        control={form.control}
        name="droidApiKey"
        render={() => (
          <FormItem>
            <FormLabel>{t("provider.apiKey")}</FormLabel>
            <FormControl>
              <ApiKeyInput
                value={apiKey}
                onChange={setApiKey}
                placeholder="your-secret-api-key-here"
              />
            </FormControl>
          </FormItem>
        )}
      />

      {/* Base URL */}
      <FormField
        control={form.control}
        name="droidBaseUrl"
        render={() => (
          <FormItem>
            <FormLabel>{t("provider.baseUrl", { defaultValue: "Base URL" })}</FormLabel>
            <FormControl>
              <Input
                value={baseUrl}
                onChange={(e) => setBaseUrl(e.target.value)}
                placeholder="https://api.example.com"
              />
            </FormControl>
          </FormItem>
        )}
      />

      {/* Model */}
      <FormField
        control={form.control}
        name="droidModel"
        render={() => (
          <FormItem>
            <FormLabel>{t("provider.model", { defaultValue: "Model" })}</FormLabel>
            <FormControl>
              <Input
                value={model}
                onChange={(e) => setModel(e.target.value)}
                placeholder="claude-sonnet-4-5-20250929"
              />
            </FormControl>
          </FormItem>
        )}
      />

      {/* Provider Type */}
      <FormField
        control={form.control}
        name="droidProvider"
        render={() => (
          <FormItem>
            <FormLabel>{t("provider.providerType", { defaultValue: "Provider Type" })}</FormLabel>
            <FormControl>
              <Select value={provider} onValueChange={setProvider}>
                <SelectTrigger>
                  <SelectValue placeholder={t("provider.selectProviderType", { defaultValue: "Select provider type" })} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="anthropic">Anthropic</SelectItem>
                  <SelectItem value="generic-chat-completion-api">Generic Chat Completion API</SelectItem>
                </SelectContent>
              </Select>
            </FormControl>
          </FormItem>
        )}
      />
    </div>
  );
}
