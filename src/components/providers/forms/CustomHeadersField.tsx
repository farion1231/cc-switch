import { Plus, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import type { ProviderCustomHeaderEntry } from "@/types";

const HEADER_PRESETS = [
  { key: "User-Agent", value: "claude-code/0.1.0" },
  { key: "x-api-key", value: "sk-xxx" },
  { key: "X-Custom-Header", value: "value" },
];

interface CustomHeadersFieldProps {
  value: ProviderCustomHeaderEntry[];
  onChange: (value: ProviderCustomHeaderEntry[]) => void;
}

export function CustomHeadersField({
  value,
  onChange,
}: CustomHeadersFieldProps) {
  const { t } = useTranslation();

  const updateRow = (
    index: number,
    patch: Partial<ProviderCustomHeaderEntry>,
  ) => {
    onChange(
      value.map((row, rowIndex) =>
        rowIndex === index ? { ...row, ...patch } : row,
      ),
    );
  };

  const addRow = (preset?: ProviderCustomHeaderEntry) => {
    onChange([...value, preset ?? { key: "", value: "" }]);
  };

  const removeRow = (index: number) => {
    onChange(value.filter((_, rowIndex) => rowIndex !== index));
  };

  return (
    <div className="space-y-3 rounded-lg border border-border/50 bg-muted/20 p-4">
      <div className="space-y-1">
        <div className="flex items-center justify-between gap-3">
          <FormLabel>
            {t("providerForm.customHeaders", {
              defaultValue: "Custom Headers",
            })}
          </FormLabel>
          <div className="flex items-center gap-2">
            {HEADER_PRESETS.map((preset) => (
              <Button
                key={`${preset.key}:${preset.value}`}
                type="button"
                variant="outline"
                size="sm"
                className="h-7"
                onClick={() => addRow(preset)}
              >
                {preset.key}
              </Button>
            ))}
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="h-7 gap-1"
              onClick={() => addRow()}
            >
              <Plus className="h-3.5 w-3.5" />
              {t("common.add", { defaultValue: "Add" })}
            </Button>
          </div>
        </div>
        <p className="text-xs text-muted-foreground">
          {t("providerForm.customHeadersHint", {
            defaultValue:
              "Only takes effect when CC Switch is routing or proxying the provider request upstream. Custom headers override built-in values; leave the value empty to remove that header upstream.",
          })}
        </p>
        <p className="text-xs text-amber-600 dark:text-amber-400">
          {t("providerForm.customHeadersSensitiveHint", {
            defaultValue:
              "Overriding Authorization, x-api-key, or User-Agent changes upstream authentication, client fingerprinting, and allowlist behavior.",
          })}
        </p>
      </div>

      <div className="space-y-2">
        {value.map((row, index) => (
          <div key={`${index}-${row.key}`} className="flex gap-2">
            <Input
              value={row.key}
              onChange={(event) =>
                updateRow(index, { key: event.target.value })
              }
              placeholder="Header name"
            />
            <Input
              value={row.value}
              onChange={(event) =>
                updateRow(index, { value: event.target.value })
              }
              placeholder="Header value"
            />
            <Button
              type="button"
              variant="ghost"
              size="icon"
              onClick={() => removeRow(index)}
              aria-label={t("common.delete", { defaultValue: "Delete" })}
            >
              <Trash2 className="h-4 w-4" />
            </Button>
          </div>
        ))}
      </div>
    </div>
  );
}
