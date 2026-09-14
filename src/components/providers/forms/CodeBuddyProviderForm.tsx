import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, ChevronUp } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
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
  [key: string]: unknown;
}

/**
 * CodeBuddy 模型端点表单（= models.json 里的一条端点）。
 * - 结构化快速字段：名称 / 模型 ID / API 地址 / API Key / 能力开关；
 * - 「配置 JSON」：可展开的完整端点 JSON 编辑器，与结构化字段双向同步，
 *   与 Claude/Codex 的 JSON 编辑体验一致，能改/保留 CodeBuddy 的未知字段。
 * - 模型 key 编辑态锁定（后端禁止改名，改名 = 删除 + 新建）。
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

  const initialCfg = (initialData?.settingsConfig ?? {}) as EntryShape;
  const [name, setName] = useState(initialCfg.name ?? initialData?.name ?? "");
  const [modelKey, setModelKey] = useState(
    typeof initialCfg.id === "string" ? initialCfg.id : (providerId ?? ""),
  );
  const [url, setUrl] = useState(initialCfg.url ?? "");
  const [apiKey, setApiKey] = useState(initialCfg.apiKey ?? "");
  const [supportsToolCall, setSupportsToolCall] = useState(
    initialCfg.supportsToolCall !== false,
  );
  const [supportsImages, setSupportsImages] = useState(
    initialCfg.supportsImages === true,
  );
  const [supportsReasoning, setSupportsReasoning] = useState(
    initialCfg.supportsReasoning === true,
  );
  const [submitting, setSubmitting] = useState(false);
  const [showJson, setShowJson] = useState(isEdit);
  const [jsonText, setJsonText] = useState(() =>
    JSON.stringify(initialCfg, null, 2),
  );
  const [jsonError, setJsonError] = useState<string | null>(null);
  // 用户在 JSON 编辑中时暂停结构化字段 → JSON 的覆写，避免光标跳动。
  const editingJsonRef = useRef(false);
  const initializedRef = useRef(false);

  const structuredEntry = useMemo<EntryShape>(
    () => ({
      ...initialCfg,
      id: modelKey.trim(),
      name: name.trim(),
      vendor: initialCfg.vendor ?? "cc-switch",
      url: url.trim(),
      apiKey: apiKey.trim(),
      supportsToolCall,
      supportsImages,
      supportsReasoning,
    }),
    [
      initialCfg,
      modelKey,
      name,
      url,
      apiKey,
      supportsToolCall,
      supportsImages,
      supportsReasoning,
    ],
  );

  // 结构化字段变化 → 重写 JSON 文本（用户不在编辑 JSON 时）。
  useEffect(() => {
    if (editingJsonRef.current) return;
    if (!initializedRef.current) {
      initializedRef.current = true;
      return;
    }
    setJsonError(null);
    setJsonText(JSON.stringify(structuredEntry, null, 2));
  }, [structuredEntry]);

  const isReady =
    name.trim().length > 0 &&
    modelKey.trim().length > 0 &&
    url.trim().length > 0 &&
    apiKey.trim().length > 0 &&
    !jsonError;

  useEffect(() => {
    onSubmitReadyChange?.(isReady);
  }, [isReady, onSubmitReadyChange]);

  const parseJsonText = useCallback((): EntryShape | null => {
    try {
      const parsed = JSON.parse(jsonText) as EntryShape;
      if (typeof parsed !== "object" || parsed === null) {
        throw new Error("not-an-object");
      }
      return parsed;
    } catch {
      return null;
    }
  }, [jsonText]);

  // 失焦时若 JSON 合法则回填结构化字段。
  const syncFromJson = useCallback(() => {
    editingJsonRef.current = false;
    const parsed = parseJsonText();
    if (!parsed) {
      setJsonError(
        t("provider.jsonInvalid", {
          defaultValue: "配置 JSON 格式无效",
        }),
      );
      return;
    }
    setJsonError(null);
    if (typeof parsed.name === "string") setName(parsed.name);
    if (typeof parsed.id === "string") setModelKey(parsed.id);
    if (typeof parsed.url === "string") setUrl(parsed.url);
    if (typeof parsed.apiKey === "string") setApiKey(parsed.apiKey);
    if (typeof parsed.supportsToolCall === "boolean")
      setSupportsToolCall(parsed.supportsToolCall);
    if (typeof parsed.supportsImages === "boolean")
      setSupportsImages(parsed.supportsImages);
    if (typeof parsed.supportsReasoning === "boolean")
      setSupportsReasoning(parsed.supportsReasoning);
  }, [parseJsonText, t]);

  const handleSubmit = useCallback(
    async (event: React.FormEvent) => {
      event.preventDefault();
      if (!isReady || submitting) return;

      let entry = structuredEntry;
      const parsed = parseJsonText();
      if (parsed) {
        entry = parsed;
      } else {
        setJsonError(
          t("provider.jsonInvalid", {
            defaultValue: "配置 JSON 格式无效",
          }),
        );
        return;
      }
      // 编辑态禁止改名：即便用户在 JSON 里改了 id 也不写入。
      if (isEdit) {
        entry = { ...entry, id: modelKey.trim() };
      }

      const values: ProviderFormValues = {
        name: (entry.name as string) || name.trim(),
        settingsConfig: JSON.stringify(entry, null, 2),
        providerKey: (entry.id as string) || modelKey.trim(),
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
      structuredEntry,
      parseJsonText,
      isEdit,
      modelKey,
      name,
      t,
      onSubmit,
      onSubmittingChange,
    ],
  );

  const handleJsonChange = (value: string) => {
    setJsonText(value);
    try {
      JSON.parse(value);
      setJsonError(null);
    } catch {
      setJsonError(
        t("provider.jsonInvalid", {
          defaultValue: "配置 JSON 格式无效",
        }),
      );
    }
  };

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
          placeholder="https://api.example.com/chat/completions"
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

      <div className="rounded-lg border">
        <button
          type="button"
          className="flex w-full items-center justify-between px-3 py-2 text-sm font-medium text-muted-foreground hover:bg-accent/50 rounded-t-lg"
          onClick={() => setShowJson((prev) => !prev)}
        >
          <span>
            {t("provider.rawJson", {
              defaultValue: "配置 JSON",
            })}
          </span>
          {showJson ? (
            <ChevronUp className="h-4 w-4" />
          ) : (
            <ChevronDown className="h-4 w-4" />
          )}
        </button>
        {showJson ? (
          <div className="border-t p-3">
            <Textarea
              value={jsonText}
              rows={10}
              spellCheck={false}
              className="font-mono text-xs"
              onFocus={() => {
                editingJsonRef.current = true;
              }}
              onBlur={syncFromJson}
              onChange={(event) => handleJsonChange(event.target.value)}
            />
            {jsonError ? (
              <p className="mt-1 text-xs text-destructive">{jsonError}</p>
            ) : null}
          </div>
        ) : null}
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
