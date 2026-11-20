import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Plus, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { cn } from "@/lib/utils";
import { splitToolsString } from "./frontmatter";
import { BUILTIN_TOOLS } from "./subagentUtils";

interface SubagentToolsSelectProps {
  /** undefined = 不写 tools（继承主会话的全部工具，含 MCP） */
  tools: string[] | undefined;
  onChange: (tools: string[] | undefined) => void;
  disabled?: boolean;
  /** 「仅允许」却一个都没选时的错误 */
  error?: string;
  errorId?: string;
}

const BUILTIN = new Set<string>(BUILTIN_TOOLS);

const chipClass =
  "inline-flex h-7 items-center gap-1 rounded-full border px-2.5 font-mono text-caption transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50";

/**
 * 工具：继承全部 / 仅允许。勾选只改列表里那一项，原来的顺序和不认识的条目
 * （mcp__server__tool、Bash(git:*) 这类）都留着。
 */
export function SubagentToolsSelect({
  tools,
  onChange,
  disabled = false,
  error,
  errorId,
}: SubagentToolsSelectProps) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState("");
  // 切回「继承全部」再切回来时，把刚才勾的那些还原
  const lastListRef = useRef<string[]>(tools ?? []);
  if (tools !== undefined) lastListRef.current = tools;

  const mode = tools === undefined ? "inherit" : "allow";
  const list = tools ?? [];
  const selected = new Set(list);
  const extras = list.filter((tool) => !BUILTIN.has(tool));

  const toggle = (tool: string) => {
    onChange(
      selected.has(tool)
        ? list.filter((item) => item !== tool)
        : [...list, tool],
    );
  };

  const addDraft = () => {
    const additions = splitToolsString(draft).filter(
      (tool) => !selected.has(tool),
    );
    setDraft("");
    if (additions.length) onChange([...list, ...additions]);
  };

  return (
    <div className="flex flex-col gap-2">
      <SegmentedControl
        size="sm"
        className="self-start"
        aria-label={t("subagents.tools.modeAria")}
        value={mode}
        onValueChange={(next) =>
          onChange(next === "inherit" ? undefined : [...lastListRef.current])
        }
        items={[
          {
            value: "inherit",
            label: t("subagents.tools.inherit"),
            disabled,
          },
          { value: "allow", label: t("subagents.tools.allow"), disabled },
        ]}
      />
      {mode === "inherit" ? (
        <p className="m-0 text-caption text-fg-2">
          {t("subagents.tools.inheritHint")}
        </p>
      ) : (
        <>
          <div
            role="group"
            aria-label={t("subagents.tools.builtinAria")}
            aria-describedby={error ? errorId : undefined}
            className="flex flex-wrap gap-1.5"
          >
            {BUILTIN_TOOLS.map((tool) => {
              const on = selected.has(tool);
              return (
                <button
                  key={tool}
                  type="button"
                  aria-pressed={on}
                  disabled={disabled}
                  onClick={() => toggle(tool)}
                  className={cn(
                    chipClass,
                    on
                      ? "border-transparent bg-selected text-fg-1"
                      : "border-border-strong bg-surface text-fg-2 hover:bg-subtle hover:text-fg-1",
                  )}
                >
                  {tool}
                </button>
              );
            })}
            {extras.map((tool) => (
              <span
                key={tool}
                className={cn(
                  chipClass,
                  "border-transparent bg-selected pe-1 text-fg-1",
                )}
              >
                <span className="max-w-[260px] truncate" title={tool}>
                  {tool}
                </span>
                <button
                  type="button"
                  disabled={disabled}
                  data-unsaved-edit=""
                  aria-label={t("subagents.tools.remove", { tool })}
                  onClick={() => toggle(tool)}
                  className="inline-flex h-5 w-5 items-center justify-center rounded-full text-fg-2 hover:bg-subtle hover:text-fg-1"
                >
                  <X aria-hidden="true" className="h-3 w-3" strokeWidth={2} />
                </button>
              </span>
            ))}
          </div>
          <div className="flex items-center gap-2">
            {/* 输入框里的草稿还没进列表，点「添加」才算改动 */}
            <Input
              data-unsaved-ignore=""
              value={draft}
              disabled={disabled}
              onChange={(event) => setDraft(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  addDraft();
                }
              }}
              placeholder={t("subagents.tools.customPlaceholder")}
              aria-label={t("subagents.tools.custom")}
              className="h-8 flex-1 font-mono text-caption"
            />
            <Button
              type="button"
              variant="neutral"
              size="regular"
              disabled={disabled || !draft.trim()}
              data-unsaved-edit=""
              onClick={addDraft}
            >
              <Plus className="h-3.5 w-3.5" />
              {t("subagents.tools.add")}
            </Button>
          </div>
          {error ? (
            <span id={errorId} className="text-caption text-danger-text">
              {error}
            </span>
          ) : null}
        </>
      )}
    </div>
  );
}
