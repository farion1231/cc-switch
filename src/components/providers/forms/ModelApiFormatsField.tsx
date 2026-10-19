import { useId, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ImeSafeInput } from "@/components/ui/ime-safe-input";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import type { ModelApiFormat } from "@/types";
import type { FetchedModel } from "@/lib/api/model-fetch";
import { ModelDropdown } from "./shared";

export interface ModelApiFormatRow {
  rowId: number;
  model: string;
  format: ModelApiFormat | "";
  draftFormat?: ModelApiFormat;
}
let nextRowId = 0;
export function modelApiFormatRows(
  formats?: Record<string, ModelApiFormat>,
): ModelApiFormatRow[] {
  return Object.entries(formats ?? {}).map(([model, format]) => ({
    rowId: ++nextRowId,
    model,
    format,
  }));
}
export function buildModelApiFormats(rows: ModelApiFormatRow[]): {
  formats?: Record<string, ModelApiFormat>;
  error?: "missingModel" | "missingProtocol" | "duplicate";
} {
  const entries: Array<[string, ModelApiFormat]> = [];
  const seen = new Set<string>();
  for (const row of rows) {
    const model = row.model
      .trim()
      .replace(/\[1m\]$/i, "")
      .trim();
    if (!model && !row.format) continue;
    if (!model) return { error: "missingModel" };
    if (!row.format) return { error: "missingProtocol" };
    if (seen.has(model)) return { error: "duplicate" };
    seen.add(model);
    entries.push([model, row.format]);
  }
  return { formats: entries.length ? Object.fromEntries(entries) : undefined };
}
export interface ModelApiFormatsFieldProps {
  rows: ModelApiFormatRow[];
  onChange: (rows: ModelApiFormatRow[]) => void;
  isFullUrl: boolean;
  models: FetchedModel[];
  client?: "claude" | "codex";
  defaultFormat?: string;
  protocolSettings?: Partial<Record<ModelApiFormat, ReactNode>>;
}
export type ModelApiFormatsEditor = Omit<
  ModelApiFormatsFieldProps,
  "models" | "client" | "defaultFormat" | "protocolSettings"
>;

const protocols = [
  { format: "anthropic", label: "Anthropic Messages" },
  { format: "openai_chat", label: "Chat Completions" },
  { format: "openai_responses", label: "Responses" },
] as const;

export function ModelApiFormatsField({
  rows,
  onChange,
  isFullUrl,
  models,
  defaultFormat,
  client = "codex",
  protocolSettings,
}: ModelApiFormatsFieldProps) {
  const { t } = useTranslation();
  const id = useId();
  const [active, setActive] = useState(
    rows.find((row) => row.model.trim() && row.format)?.format ||
      (protocols.some((p) => p.format === defaultFormat)
        ? defaultFormat!
        : "anthropic"),
  );
  const protocolOptions = protocols.map((protocol) => ({
    ...protocol,
    label:
      client === "claude" && protocol.format === "openai_chat"
        ? "OpenAI Chat Completions"
        : client === "claude" && protocol.format === "openai_responses"
          ? "OpenAI Responses API"
          : protocol.label,
  }));
  const nativeFormat = client === "claude" ? "anthropic" : "openai_responses";
  const error = buildModelApiFormats(rows).error;
  return (
    <div className="space-y-3">
      <div className="space-y-1">
        <p className="text-sm font-medium">
          {t("providerForm.modelApiFormats.title")}
        </p>
        <p className="text-xs text-fg-2">
          {t("providerForm.modelApiFormats.hint")}
        </p>
      </div>
      {isFullUrl && (
        <p role="alert" className="text-xs text-destructive">
          {t("providerForm.modelApiFormats.fullUrl")}
        </p>
      )}
      {error && (
        <p role="alert" className="text-xs text-destructive">
          {t(`providerForm.modelApiFormats.${error}`)}
        </p>
      )}
      <Tabs value={active} onValueChange={setActive}>
        <TabsList
          className="grid w-full grid-cols-3 items-stretch"
          aria-label={t("providerForm.modelApiFormats.title")}
        >
          {protocolOptions.map(({ format, label }) => (
            <TabsTrigger
              key={format}
              value={format}
              className="min-w-0 flex-col gap-1 whitespace-normal px-1 py-2 text-xs data-[state=active]:bg-surface data-[state=active]:text-fg-1 sm:text-sm"
            >
              <span className="break-words text-center leading-tight">
                {label} ·{" "}
                {
                  rows.filter(
                    (row) => row.format === format && row.model.trim(),
                  ).length
                }
              </span>
              <span className="inline-flex rounded border border-border px-1.5 py-0.5 text-[11px] font-normal leading-none text-fg-2">
                {t(
                  `providerForm.modelApiFormats.${format === nativeFormat ? "native" : "conversionRequired"}`,
                )}
              </span>
            </TabsTrigger>
          ))}
        </TabsList>
        {protocolOptions.map(({ format, label }) => (
          <TabsContent key={format} value={format} className="space-y-3 pt-2">
            <p className="text-sm font-medium">
              {t("providerForm.modelApiFormats.models")}
            </p>
            {rows.map(
              (row, index) =>
                (row.format || row.draftFormat || "anthropic") === format && (
                  <div key={row.rowId} className="flex items-center gap-2">
                    <div className="flex min-w-0 flex-1 gap-1">
                      <ImeSafeInput
                        id={`${id}-model-${row.rowId}`}
                        aria-label={`${t("providerForm.modelApiFormats.model")} ${index + 1}`}
                        value={row.model}
                        placeholder={t("providerForm.modelApiFormats.model")}
                        onValueChange={(model) =>
                          onChange(
                            rows.map((entry) =>
                              entry.rowId === row.rowId
                                ? { ...entry, model, format }
                                : entry,
                            ),
                          )
                        }
                      />
                      {models.length > 0 && (
                        <ModelDropdown
                          models={models}
                          onSelect={(model) =>
                            onChange(
                              rows.map((entry) =>
                                entry.rowId === row.rowId
                                  ? { ...entry, model, format }
                                  : entry,
                              ),
                            )
                          }
                        />
                      )}
                    </div>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      aria-label={`${t("providerForm.modelApiFormats.remove")} ${index + 1}`}
                      onClick={() =>
                        onChange(
                          rows.filter((entry) => entry.rowId !== row.rowId),
                        )
                      }
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </div>
                ),
            )}
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={isFullUrl}
              onClick={() =>
                onChange([
                  ...rows,
                  {
                    rowId: ++nextRowId,
                    model: "",
                    format: "",
                    draftFormat: format,
                  },
                ])
              }
            >
              <Plus className="mr-1 h-4 w-4" />
              {t("providerForm.modelApiFormats.add")}
            </Button>
            <div
              className="space-y-3 border-t border-border pt-3"
              role="group"
              aria-label={label}
            >
              {protocolSettings?.[format] ?? (
                <p className="text-xs text-fg-2">
                  {t(
                    client === "claude"
                      ? `providerForm.protocolSettings.${format === "anthropic" ? "claudeMessages" : format === "openai_chat" ? "claudeChat" : "claudeResponses"}Hint`
                      : `providerForm.protocolSettings.${format === "anthropic" ? "messages" : format === "openai_chat" ? "chat" : "responses"}Hint`,
                  )}
                </p>
              )}
            </div>
          </TabsContent>
        ))}
      </Tabs>
    </div>
  );
}
