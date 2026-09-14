import { useEffect, useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

interface StructuredOptionsEditorProps {
  id: string;
  title: string;
  hint: string;
  addLabel: string;
  emptyLabel: string;
  keyLabel: string;
  valueLabel: string;
  keyPlaceholder: string;
  valuePlaceholder: string;
  removeLabel: string;
  options: Record<string, unknown>;
  onOptionsChange: (options: Record<string, unknown>) => void;
  className?: string;
}

interface DraftOption {
  id: string;
  key: string;
  value: string;
}

function parseOptionValue(value: string): unknown {
  try {
    return JSON.parse(value);
  } catch {
    return value;
  }
}

function formatOptionValue(value: unknown): string {
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

function OptionKeyInput({
  optionKey,
  onChange,
  ariaLabel,
  placeholder,
}: {
  optionKey: string;
  onChange: (newKey: string) => boolean;
  ariaLabel: string;
  placeholder: string;
}) {
  const [localValue, setLocalValue] = useState(optionKey);

  useEffect(() => {
    setLocalValue(optionKey);
  }, [optionKey]);

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
        const nextKey = localValue.trim();
        if (!nextKey || nextKey === optionKey) {
          setLocalValue(optionKey);
          return;
        }
        if (!onChange(nextKey)) setLocalValue(optionKey);
      }}
      aria-label={ariaLabel}
      placeholder={placeholder}
      className="min-w-0 flex-1"
    />
  );
}

export function StructuredOptionsEditor({
  id,
  title,
  hint,
  addLabel,
  emptyLabel,
  keyLabel,
  valueLabel,
  keyPlaceholder,
  valuePlaceholder,
  removeLabel,
  options,
  onOptionsChange,
  className,
}: StructuredOptionsEditorProps) {
  const [drafts, setDrafts] = useState<DraftOption[]>([]);

  const addOption = () => {
    setDrafts((current) => [
      ...current,
      {
        id: crypto.randomUUID(),
        key: "",
        value: "",
      },
    ]);
  };

  const removeOption = (key: string) => {
    const next = { ...options };
    delete next[key];
    onOptionsChange(next);
  };

  const renameOption = (oldKey: string, newKey: string): boolean => {
    if (oldKey === newKey) return true;
    if (Object.prototype.hasOwnProperty.call(options, newKey)) return false;

    const next: Record<string, unknown> = {};
    for (const [key, value] of Object.entries(options)) {
      next[key === oldKey ? newKey : key] = value;
    }
    onOptionsChange(next);
    return true;
  };

  const updateOption = (key: string, value: string) => {
    onOptionsChange({
      ...options,
      [key]: parseOptionValue(value),
    });
  };

  const updateDraft = (id: string, update: Partial<DraftOption>) => {
    setDrafts((current) =>
      current.map((draft) =>
        draft.id === id ? { ...draft, ...update } : draft,
      ),
    );
  };

  const removeDraft = (id: string) => {
    setDrafts((current) => current.filter((draft) => draft.id !== id));
  };

  const commitDraft = (draft: DraftOption): boolean => {
    const key = draft.key.trim();
    if (!key || Object.prototype.hasOwnProperty.call(options, key))
      return false;
    onOptionsChange({
      ...options,
      [key]: parseOptionValue(draft.value),
    });
    removeDraft(draft.id);
    return true;
  };

  const hasRows = Object.keys(options).length > 0 || drafts.length > 0;

  return (
    <div
      id={id}
      className={cn("space-y-2 border-l border-border-default pl-3", className)}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0 max-w-3xl flex-1 space-y-1">
          <span className="block text-sm font-medium text-foreground">
            {title}
          </span>
          <p className="text-xs text-muted-foreground">{hint}</p>
        </div>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={addOption}
          aria-label={addLabel}
          className="h-7 shrink-0 gap-1"
        >
          <Plus className="h-3.5 w-3.5" />
          {addLabel}
        </Button>
      </div>

      <div className="max-w-3xl space-y-2">
        {!hasRows ? (
          <p className="py-1 text-sm text-muted-foreground">{emptyLabel}</p>
        ) : (
          <div className="space-y-2">
            <div className="mb-1 flex items-center gap-2 px-1 text-xs text-muted-foreground">
              <span className="flex-1">{keyLabel}</span>
              <span className="flex-1">{valueLabel}</span>
              <span className="w-9" />
            </div>

            {Object.entries(options).map(([key, value]) => (
              <div key={key} className="flex items-center gap-2">
                <OptionKeyInput
                  optionKey={key}
                  onChange={(nextKey) => renameOption(key, nextKey)}
                  ariaLabel={keyLabel}
                  placeholder={keyPlaceholder}
                />
                <Input
                  value={formatOptionValue(value)}
                  onChange={(event) => updateOption(key, event.target.value)}
                  aria-label={valueLabel}
                  placeholder={valuePlaceholder}
                  className="min-w-0 flex-1 font-mono"
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  onClick={() => removeOption(key)}
                  aria-label={removeLabel}
                  className="h-9 w-9 shrink-0 text-muted-foreground hover:text-destructive"
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </div>
            ))}

            {drafts.map((draft) => (
              <div
                key={draft.id}
                className="flex items-center gap-2"
                onBlurCapture={(event) => {
                  const nextTarget = event.relatedTarget;
                  if (
                    nextTarget instanceof Node &&
                    event.currentTarget.contains(nextTarget)
                  ) {
                    return;
                  }
                  commitDraft(draft);
                }}
              >
                <Input
                  value={draft.key}
                  onChange={(event) =>
                    updateDraft(draft.id, { key: event.target.value })
                  }
                  onKeyDown={(event) => {
                    if (event.key !== "Enter") return;
                    event.preventDefault();
                    const valueInput = event.currentTarget.nextElementSibling;
                    if (valueInput instanceof HTMLElement) valueInput.focus();
                  }}
                  aria-label={keyLabel}
                  placeholder={keyPlaceholder}
                  autoFocus
                  className="min-w-0 flex-1"
                />
                <Input
                  value={draft.value}
                  onChange={(event) =>
                    updateDraft(draft.id, { value: event.target.value })
                  }
                  onKeyDown={(event) => {
                    if (event.key !== "Enter") return;
                    event.preventDefault();
                    commitDraft(draft);
                  }}
                  aria-label={valueLabel}
                  placeholder={valuePlaceholder}
                  className="min-w-0 flex-1 font-mono"
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  onClick={() => removeDraft(draft.id)}
                  aria-label={removeLabel}
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
