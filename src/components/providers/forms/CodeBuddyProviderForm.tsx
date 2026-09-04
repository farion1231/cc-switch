import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import type { ProviderFormProps, ProviderFormValues } from "./ProviderForm";

type CodeBuddyFormProps = Omit<ProviderFormProps, "appId">;

interface EntryShape {
  id?: string;
  name?: string;
  vendor?: string;
  url?: string;
  apiKey?: string;
  supportsToolCall?: boolean;
  supportsImages?: boolean;
  supportsReasoning?: boolean;
}

/**
 * CodeBuddy 模型端点表单。
 * 一个 CC Switch 供应商 = models.json 里的一条模型端点（id/url/apiKey + 能力开关）。
 * - 新建：模型 key（id）可编辑，作为 providerKey/DB id；
 * - 编辑：模型 key 只读（后端禁止改名，改名视为删除+新建）。
 */
export function CodeBuddyProviderForm({
  providerId,
  submitLabel,
  onSubmit,
  onCancel,
  onSubmittingChange,
  onSubmitReadyChange,
  initialData,
  showButtons = true,
}: CodeBuddyFormProps) {
  const { t } = useTranslation();
  const isEdit = Boolean(initialData);

  const cfg = (initialData?.settingsConfig ?? {}) as EntryShape;
  const [name, setName] = useState(cfg.name ?? initialData?.name ?? "");
  const [modelKey, setModelKey] = useState(
    typeof cfg.id === "string" ? cfg.id : (providerId ?? ""),
  );
  const [url, setUrl] = useState(cfg.url ?? "");
  const [apiKey, setApiKey] = useState(cfg.apiKey ?? "");
  const [supportsToolCall, setSupportsToolCall] = useState(
    cfg.supportsToolCall !== false,
  );
  const [supportsImages, setSupportsImages] = useState(
    cfg.supportsImages === true,
  );
  const [supportsReasoning, setSupportsReasoning] = useState(
    cfg.supportsReasoning === true,
  );
  const [submitting, setSubmitting] = useState(false);

  const isReady =
    name.trim().length > 0 &&
    modelKey.trim().length > 0 &&
    url.trim().length > 0 &&
    apiKey.trim().length > 0;

  useEffect(() => {
    onSubmitReadyChange?.(isReady);
  }, [isReady, onSubmitReadyChange]);

  const handleSubmit = useCallback(
    async (event: React.FormEvent) => {
      event.preventDefault();
      if (!isReady || submitting) return;

      const entry: EntryShape = {
        id: modelKey.trim(),
        name: name.trim(),
        vendor: "cc-switch",
        url: url.trim(),
        apiKey: apiKey.trim(),
        supportsToolCall,
        supportsImages,
        supportsReasoning,
      };

      const values: ProviderFormValues = {
        name: name.trim(),
        settingsConfig: JSON.stringify(entry, null, 2),
        providerKey: modelKey.trim(),
        presetCategory: "custom",
      };

      setSubmitting(true);
      onSubmittingChange?.(true);
      try {
        await onSubmit(values);
      } finally {
        setSubmitting(false);
        onSubmittingChange?.(false);
      }
    },
    [
      isReady,
      submitting,
      modelKey,
      name,
      url,
      apiKey,
      supportsToolCall,
      supportsImages,
      supportsReasoning,
      onSubmit,
      onSubmittingChange,
    ],
  );

  return (
    <form id="provider-form" onSubmit={handleSubmit} className="space-y-4">
      <div className="space-y-1.5">
        <Label htmlFor="codebuddy-name">{t("codebuddy.form.name")}</Label>
        <Input
          id="codebuddy-name"
          value={name}
          onChange={(event) => setName(event.target.value)}
          placeholder={t("codebuddy.form.namePlaceholder")}
        />
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="codebuddy-key">{t("codebuddy.form.modelKey")}</Label>
        <Input
          id="codebuddy-key"
          value={modelKey}
          disabled={isEdit}
          onChange={(event) => setModelKey(event.target.value)}
          placeholder="deepseek-chat"
          className={isEdit ? "opacity-60" : undefined}
        />
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="codebuddy-url">{t("codebuddy.form.url")}</Label>
        <Input
          id="codebuddy-url"
          value={url}
          onChange={(event) => setUrl(event.target.value)}
          placeholder="https://api.deepseek.com/chat/completions"
        />
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="codebuddy-api-key">{t("codebuddy.form.apiKey")}</Label>
        <Input
          id="codebuddy-api-key"
          type="password"
          value={apiKey}
          onChange={(event) => setApiKey(event.target.value)}
          placeholder="sk-..."
        />
      </div>

      <div className="space-y-3 rounded-lg border p-3">
        <div className="flex items-center justify-between gap-4">
          <Label htmlFor="codebuddy-tool">
            {t("codebuddy.form.supportsToolCall")}
          </Label>
          <Switch
            id="codebuddy-tool"
            checked={supportsToolCall}
            onCheckedChange={setSupportsToolCall}
          />
        </div>
        <div className="flex items-center justify-between gap-4">
          <Label htmlFor="codebuddy-images">
            {t("codebuddy.form.supportsImages")}
          </Label>
          <Switch
            id="codebuddy-images"
            checked={supportsImages}
            onCheckedChange={setSupportsImages}
          />
        </div>
        <div className="flex items-center justify-between gap-4">
          <Label htmlFor="codebuddy-reasoning">
            {t("codebuddy.form.supportsReasoning")}
          </Label>
          <Switch
            id="codebuddy-reasoning"
            checked={supportsReasoning}
            onCheckedChange={setSupportsReasoning}
          />
        </div>
      </div>

      {showButtons ? (
        <div className="flex justify-end gap-2 pt-2">
          {onCancel ? (
            <Button
              type="button"
              variant="outline"
              onClick={onCancel}
              disabled={submitting}
            >
              {t("common.cancel", { defaultValue: "取消" })}
            </Button>
          ) : null}
          <Button type="submit" disabled={!isReady || submitting}>
            {submitting ? "…" : submitLabel}
          </Button>
        </div>
      ) : null}
    </form>
  );
}
