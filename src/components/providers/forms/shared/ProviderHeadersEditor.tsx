import { useEffect, useState, type ReactNode } from "react";
import { Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

export const PROVIDER_HEADER_DRAFT_PREFIX = "draft-header:";

interface HeaderNameInputProps {
  name: string;
  onChange: (name: string) => boolean;
  placeholder: string;
}

function HeaderNameInput({
  name,
  onChange,
  placeholder,
}: HeaderNameInputProps) {
  const isDraft = name.startsWith(PROVIDER_HEADER_DRAFT_PREFIX);
  const visibleName = isDraft ? "" : name;
  const [value, setValue] = useState(visibleName);

  useEffect(() => {
    setValue(isDraft ? "" : name);
  }, [isDraft, name]);

  return (
    <Input
      value={value}
      onChange={(event) => setValue(event.target.value)}
      onBlur={() => {
        const nextName = value.trim();
        if (!nextName || nextName === name) {
          setValue(visibleName);
          return;
        }
        if (!onChange(nextName)) {
          setValue(visibleName);
        } else {
          setValue(nextName);
        }
      }}
      placeholder={placeholder}
      className="flex-1"
    />
  );
}

interface ProviderHeadersEditorProps {
  headers: Record<string, string>;
  onHeadersChange: (headers: Record<string, string>) => void;
  label: ReactNode;
  hint: ReactNode;
  emptyText: ReactNode;
  addLabel: ReactNode;
  addAriaLabel: string;
  nameLabel: ReactNode;
  valueLabel: ReactNode;
  namePlaceholder: string;
  valuePlaceholder: string;
  removeAriaLabel: string;
}

/** OpenCode-style key/value editor shared by provider configuration screens. */
export function ProviderHeadersEditor({
  headers,
  onHeadersChange,
  label,
  hint,
  emptyText,
  addLabel,
  addAriaLabel,
  nameLabel,
  valueLabel,
  namePlaceholder,
  valuePlaceholder,
  removeAriaLabel,
}: ProviderHeadersEditorProps) {
  const addHeader = () => {
    const draftName = `${PROVIDER_HEADER_DRAFT_PREFIX}${crypto.randomUUID()}`;
    onHeadersChange({ ...headers, [draftName]: "" });
  };

  const removeHeader = (name: string) => {
    const nextHeaders = { ...headers };
    delete nextHeaders[name];
    onHeadersChange(nextHeaders);
  };

  const renameHeader = (oldName: string, newName: string): boolean => {
    const normalizedName = newName.toLowerCase();
    const duplicate = Object.keys(headers).some(
      (name) => name !== oldName && name.toLowerCase() === normalizedName,
    );
    if (duplicate) return false;

    const nextHeaders: Record<string, string> = {};
    for (const [name, value] of Object.entries(headers)) {
      nextHeaders[name === oldName ? newName : name] = value;
    }
    onHeadersChange(nextHeaders);
    return true;
  };

  return (
    <div className="space-y-2 border-l border-border-default pl-3">
      <div className="flex items-start justify-between gap-3">
        <div className="max-w-3xl space-y-1">
          <Label>{label}</Label>
          <p className="text-xs text-muted-foreground">{hint}</p>
        </div>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={addHeader}
          aria-label={addAriaLabel}
          className="h-7 gap-1"
        >
          <Plus className="h-3.5 w-3.5" />
          {addLabel}
        </Button>
      </div>

      <div className="max-w-3xl">
        {Object.keys(headers).length === 0 ? (
          <p className="py-1 text-sm text-muted-foreground">{emptyText}</p>
        ) : (
          <div className="space-y-2">
            <div className="mb-1 flex items-center gap-2 px-1 text-xs text-muted-foreground">
              <span className="flex-1">{nameLabel}</span>
              <span className="flex-1">{valueLabel}</span>
              <span className="w-9" />
            </div>
            {Object.entries(headers).map(([name, value]) => (
              <div key={name} className="flex items-center gap-2">
                <HeaderNameInput
                  name={name}
                  onChange={(newName) => renameHeader(name, newName)}
                  placeholder={namePlaceholder}
                />
                <Input
                  value={value}
                  onChange={(event) =>
                    onHeadersChange({
                      ...headers,
                      [name]: event.target.value,
                    })
                  }
                  placeholder={valuePlaceholder}
                  className="flex-1"
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  onClick={() => removeHeader(name)}
                  aria-label={removeAriaLabel}
                  className="h-9 w-9 text-muted-foreground hover:text-destructive"
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
