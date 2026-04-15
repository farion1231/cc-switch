import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Save } from "lucide-react";
import { toast } from "sonner";
import {
  useHermesModelConfig,
  useSaveHermesModelConfig,
} from "@/hooks/useHermes";
import { extractErrorMessage } from "@/utils/errorUtils";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { HermesModelConfig } from "@/types";

const ModelPanel: React.FC = () => {
  const { t } = useTranslation();
  const { data: modelData, isLoading } = useHermesModelConfig(true);
  const saveModelMutation = useSaveHermesModelConfig();

  const [defaultModel, setDefaultModel] = useState("");
  const [provider, setProvider] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [contextLength, setContextLength] = useState("");
  const [maxTokens, setMaxTokens] = useState("");

  // Preserve unknown fields from the original config
  const [extra, setExtra] = useState<Record<string, unknown>>({});

  useEffect(() => {
    if (modelData === undefined) return;
    if (modelData) {
      setDefaultModel(modelData.default ?? "");
      setProvider(modelData.provider ?? "");
      setBaseUrl(modelData.base_url ?? "");
      setContextLength(
        modelData.context_length != null
          ? String(modelData.context_length)
          : "",
      );
      setMaxTokens(
        modelData.max_tokens != null ? String(modelData.max_tokens) : "",
      );
      // Collect unknown fields
      const {
        default: _d,
        provider: _p,
        base_url: _b,
        context_length: _c,
        max_tokens: _m,
        ...rest
      } = modelData;
      setExtra(rest);
    } else {
      setDefaultModel("");
      setProvider("");
      setBaseUrl("");
      setContextLength("");
      setMaxTokens("");
      setExtra({});
    }
  }, [modelData]);

  const handleSave = async () => {
    try {
      const config: HermesModelConfig = {
        ...extra,
      };
      if (defaultModel.trim()) config.default = defaultModel.trim();
      if (provider.trim()) config.provider = provider.trim();
      if (baseUrl.trim()) config.base_url = baseUrl.trim();

      const cl = parseInt(contextLength);
      if (!isNaN(cl) && cl > 0) config.context_length = cl;

      const mt = parseInt(maxTokens);
      if (!isNaN(mt) && mt > 0) config.max_tokens = mt;

      await saveModelMutation.mutateAsync(config);
      toast.success(t("hermes.model.saveSuccess"));
    } catch (error) {
      toast.error(t("hermes.model.saveFailed"), {
        description: extractErrorMessage(error),
      });
    }
  };

  if (isLoading) {
    return (
      <div className="px-6 pt-4 pb-8 flex items-center justify-center min-h-[200px]">
        <div className="text-sm text-muted-foreground">
          {t("common.loading")}
        </div>
      </div>
    );
  }

  return (
    <div className="px-6 pt-4 pb-8">
      <p className="text-sm text-muted-foreground mb-4">
        {t("hermes.model.description")}
      </p>

      <div className="rounded-xl border border-border bg-card p-5 mb-4">
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <div className="space-y-2 sm:col-span-2">
            <Label htmlFor="hermes-model-default">
              {t("hermes.model.default", { defaultValue: "Default Model" })}
            </Label>
            <Input
              id="hermes-model-default"
              value={defaultModel}
              onChange={(e) => setDefaultModel(e.target.value)}
              placeholder="anthropic/claude-opus-4-6"
            />
            <p className="text-xs text-muted-foreground">
              {t("hermes.model.defaultHint", {
                defaultValue:
                  "The default model to use, e.g. anthropic/claude-opus-4-6",
              })}
            </p>
          </div>

          <div className="space-y-2">
            <Label htmlFor="hermes-model-provider">
              {t("hermes.model.provider", { defaultValue: "Provider" })}
            </Label>
            <Input
              id="hermes-model-provider"
              value={provider}
              onChange={(e) => setProvider(e.target.value)}
              placeholder="openrouter"
            />
            <p className="text-xs text-muted-foreground">
              {t("hermes.model.providerHint", {
                defaultValue:
                  "Provider name for model routing (e.g. openrouter, anthropic)",
              })}
            </p>
          </div>

          <div className="space-y-2">
            <Label htmlFor="hermes-model-baseurl">
              {t("hermes.model.baseUrl", { defaultValue: "Base URL" })}
            </Label>
            <Input
              id="hermes-model-baseurl"
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
              placeholder="https://api.example.com/v1"
            />
            <p className="text-xs text-muted-foreground">
              {t("hermes.model.baseUrlHint", {
                defaultValue: "Override the API endpoint URL for this model",
              })}
            </p>
          </div>

          <div className="space-y-2">
            <Label htmlFor="hermes-model-context">
              {t("hermes.model.contextLength", {
                defaultValue: "Context Length",
              })}
            </Label>
            <Input
              id="hermes-model-context"
              type="number"
              value={contextLength}
              onChange={(e) => setContextLength(e.target.value)}
              placeholder="200000"
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="hermes-model-maxtokens">
              {t("hermes.model.maxTokens", { defaultValue: "Max Tokens" })}
            </Label>
            <Input
              id="hermes-model-maxtokens"
              type="number"
              value={maxTokens}
              onChange={(e) => setMaxTokens(e.target.value)}
              placeholder="16384"
            />
          </div>
        </div>
      </div>

      <div className="flex justify-end">
        <Button
          size="sm"
          onClick={handleSave}
          disabled={saveModelMutation.isPending}
        >
          <Save className="w-4 h-4 mr-1" />
          {saveModelMutation.isPending ? t("common.saving") : t("common.save")}
        </Button>
      </div>
    </div>
  );
};

export default ModelPanel;
