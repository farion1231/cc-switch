import { toast } from "sonner";
import type { ProviderEditorInactiveField } from "@/lib/api/providers";

/** 点一个字段做什么：加入上方的编辑框，或复制它的值（可以照抄的 TOML）。 */
export type InactiveFieldAction =
  | {
      kind: "add";
      title: string;
      onAdd: (field: ProviderEditorInactiveField) => void;
    }
  | { kind: "copy"; copiedText: string };

interface InactiveFieldsPanelProps {
  fields: ProviderEditorInactiveField[];
  /** 面板上方的说明（已翻译）。 */
  hint: string;
  action: InactiveFieldAction;
}

/** 供应商行里保存着、但不随切换生效的字段。 */
export function InactiveFieldsPanel({
  fields,
  hint,
  action,
}: InactiveFieldsPanelProps) {
  if (fields.length === 0) return null;
  return (
    <div className="rounded-md border border-border-default bg-muted/40 p-3 space-y-2">
      <p className="text-xs text-muted-foreground">{hint}</p>
      <div className="flex flex-wrap gap-2">
        {fields.map((field) => {
          const name = field.path.join(".");
          return (
            <button
              key={name}
              type="button"
              title={action.kind === "add" ? action.title : String(field.value)}
              onClick={() => {
                if (action.kind === "add") {
                  action.onAdd(field);
                  return;
                }
                void navigator.clipboard
                  ?.writeText(String(field.value))
                  .then(() => toast.success(action.copiedText));
              }}
              className="rounded border border-border-default px-2 py-0.5 font-mono text-xs text-blue-500 hover:bg-blue-500/10 dark:text-blue-400"
            >
              {action.kind === "add" ? `+ ${name}` : name}
            </button>
          );
        })}
      </div>
    </div>
  );
}
