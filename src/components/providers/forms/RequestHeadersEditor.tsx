import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ImeSafeInput } from "@/components/ui/ime-safe-input";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";
import { REQUEST_HEADER_DRAFT_PREFIX } from "./helpers/requestHeaders";

interface RequestHeadersEditorProps {
  headers: Record<string, string>;
  onHeadersChange: (headers: Record<string, string>) => void;
  className?: string;
}

function HeaderNameInput({
  headerName,
  onChange,
  ariaLabel,
  placeholder,
}: {
  headerName: string;
  onChange: (newName: string) => boolean;
  ariaLabel: string;
  placeholder: string;
}) {
  const isDraft = headerName.startsWith(REQUEST_HEADER_DRAFT_PREFIX);
  const displayValue = isDraft ? "" : headerName;
  const [localValue, setLocalValue] = useState(displayValue);

  useEffect(() => {
    setLocalValue(isDraft ? "" : headerName);
  }, [headerName, isDraft]);

  return (
    <Input
      value={localValue}
      onChange={(event) => setLocalValue(event.target.value)}
      onKeyDown={(event) => {
        if (event.key !== "Enter") return;
        event.preventDefault();
        event.currentTarget.blur();
      }}
      onBlur={() => {
        const trimmed = localValue.trim();
        if (!trimmed) {
          setLocalValue(displayValue);
          return;
        }
        if (trimmed === headerName) return;
        if (!onChange(trimmed)) setLocalValue(displayValue);
      }}
      aria-label={ariaLabel}
      placeholder={placeholder}
      className="min-w-0 flex-1"
    />
  );
}

function nextDraftKey(headers: Record<string, string>): string {
  let suffix = Date.now();
  while (`${REQUEST_HEADER_DRAFT_PREFIX}${suffix}` in headers) suffix += 1;
  return `${REQUEST_HEADER_DRAFT_PREFIX}${suffix}`;
}

export function RequestHeadersEditor({
  headers,
  onHeadersChange,
  className,
}: RequestHeadersEditorProps) {
  const { t } = useTranslation();

  const addHeader = () => {
    onHeadersChange({
      ...headers,
      [nextDraftKey(headers)]: "",
    });
  };

  const removeHeader = (key: string) => {
    const next = { ...headers };
    delete next[key];
    onHeadersChange(next);
  };

  const renameHeader = (oldKey: string, newKey: string): boolean => {
    const normalizedKey = newKey.toLowerCase();
    if (
      Object.keys(headers).some(
        (key) => key !== oldKey && key.toLowerCase() === normalizedKey,
      )
    ) {
      return false;
    }

    const next: Record<string, string> = {};
    for (const [key, value] of Object.entries(headers)) {
      next[key === oldKey ? newKey : key] = value;
    }
    onHeadersChange(next);
    return true;
  };

  const updateHeader = (key: string, value: string) => {
    onHeadersChange({ ...headers, [key]: value });
  };

  return (
    <div
      className={cn("space-y-2 border-l border-border-default pl-3", className)}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="max-w-3xl space-y-1">
          <Label>{t("opencode.headers", { defaultValue: "Headers" })}</Label>
          <p className="text-xs text-muted-foreground">
            {t("opencode.headersHint", {
              defaultValue:
                "Optional HTTP headers sent with provider requests, such as HTTP-Referer or X-Title.",
            })}
          </p>
        </div>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={addHeader}
          aria-label={t("opencode.addHeader", {
            defaultValue: "Add header",
          })}
          className="h-7 shrink-0 gap-1"
        >
          <Plus className="h-3.5 w-3.5" />
          {t("opencode.addHeader", { defaultValue: "Add" })}
        </Button>
      </div>

      <div className="max-w-3xl" aria-live="polite">
        {Object.keys(headers).length === 0 ? (
          <p className="py-1 text-sm text-muted-foreground">
            {t("opencode.noHeaders", {
              defaultValue: "No custom headers configured",
            })}
          </p>
        ) : (
          <div className="space-y-2">
            <div className="mb-1 flex items-center gap-2 px-1 text-xs text-muted-foreground">
              <span className="flex-1">
                {t("opencode.headerName", { defaultValue: "Header" })}
              </span>
              <span className="flex-1">
                {t("opencode.headerValue", { defaultValue: "Value" })}
              </span>
              <span className="w-9" />
            </div>
            {Object.entries(headers).map(([key, value]) => (
              <div key={key} className="flex items-center gap-2">
                <HeaderNameInput
                  headerName={key}
                  onChange={(newKey) => renameHeader(key, newKey)}
                  ariaLabel={t("opencode.headerName", {
                    defaultValue: "Header",
                  })}
                  placeholder={t("opencode.headerNamePlaceholder", {
                    defaultValue: "X-Title",
                  })}
                />
                <ImeSafeInput
                  value={value}
                  onValueChange={(nextValue) => updateHeader(key, nextValue)}
                  aria-label={t("opencode.headerValue", {
                    defaultValue: "Value",
                  })}
                  placeholder={t("opencode.headerValuePlaceholder", {
                    defaultValue: "CC Switch",
                  })}
                  className="min-w-0 flex-1"
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  onClick={() => removeHeader(key)}
                  aria-label={t("opencode.removeHeader", {
                    defaultValue: "Remove header",
                  })}
                  className="h-9 w-9 shrink-0 text-muted-foreground hover:text-destructive"
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
