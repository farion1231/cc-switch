import { useId } from "react";
import { useTranslation } from "react-i18next";
import { Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ImeSafeInput } from "@/components/ui/ime-safe-input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { ModelApiFormat } from "@/types";
import type { FetchedModel } from "@/lib/api/model-fetch";
import { ModelDropdown } from "./shared";

export interface ModelApiFormatRow {
  rowId: number;
  model: string;
  format: ModelApiFormat | "";
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
}
export type ModelApiFormatsEditor = Omit<ModelApiFormatsFieldProps, "models">;

export function ModelApiFormatsField({
  rows,
  onChange,
  isFullUrl,
  models,
}: ModelApiFormatsFieldProps) {
  const { t } = useTranslation();
  const id = useId();
  const update = (rowId: number, patch: Partial<ModelApiFormatRow>) =>
    onChange(
      rows.map((row) => (row.rowId === rowId ? { ...row, ...patch } : row)),
    );
  const error = buildModelApiFormats(rows).error;
  return (
    <details
      className="space-y-2 rounded-md border border-border-default p-3"
      open={rows.length > 0 || undefined}
    >
      <summary className="cursor-pointer text-sm font-medium">
        {t("providerForm.modelApiFormats.title")}
      </summary>
      <p className="text-xs text-fg-2">
        {t("providerForm.modelApiFormats.hint")}
      </p>
      {isFullUrl && (
        <p role="alert" className="text-xs text-destructive">
          {t("providerForm.modelApiFormats.fullUrl")}
        </p>
      )}
      {rows.map((row, index) => (
        <div key={row.rowId} className="flex flex-wrap items-center gap-2">
          <div className="flex min-w-40 flex-1 gap-1">
            <ImeSafeInput
              id={`${id}-model-${row.rowId}`}
              aria-label={`${t("providerForm.modelApiFormats.model")} ${index + 1}`}
              value={row.model}
              placeholder={t("providerForm.modelApiFormats.model")}
              onValueChange={(model) => update(row.rowId, { model })}
            />
            {models.length > 0 && (
              <ModelDropdown
                models={models}
                onSelect={(model) => update(row.rowId, { model })}
              />
            )}
          </div>
          <Select
            value={row.format}
            onValueChange={(format) =>
              update(row.rowId, { format: format as ModelApiFormat })
            }
          >
            <SelectTrigger
              className="w-52"
              aria-label={`${t("providerForm.modelApiFormats.protocol")} ${index + 1}`}
            >
              <SelectValue
                placeholder={t("providerForm.modelApiFormats.protocol")}
              />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="anthropic">Anthropic Messages</SelectItem>
              <SelectItem value="openai_responses">OpenAI Responses</SelectItem>
              <SelectItem value="openai_chat">
                OpenAI Chat Completions
              </SelectItem>
            </SelectContent>
          </Select>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            aria-label={`${t("providerForm.modelApiFormats.remove")} ${index + 1}`}
            onClick={() =>
              onChange(rows.filter((entry) => entry.rowId !== row.rowId))
            }
          >
            <Trash2 className="h-4 w-4" />
          </Button>
        </div>
      ))}
      {error && (
        <p role="alert" className="text-xs text-destructive">
          {t(`providerForm.modelApiFormats.${error}`)}
        </p>
      )}
      <Button
        type="button"
        variant="outline"
        size="sm"
        disabled={isFullUrl}
        onClick={() =>
          onChange([...rows, { rowId: ++nextRowId, model: "", format: "" }])
        }
      >
        <Plus className="mr-1 h-4 w-4" />
        {t("providerForm.modelApiFormats.add")}
      </Button>
    </details>
  );
}
