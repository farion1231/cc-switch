import { useId, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Ban, Check } from "lucide-react";
import MarkdownEditor from "@/components/MarkdownEditor";
import { Button } from "@/components/ui/button";
import { Notice } from "@/components/ui/notice";
import { SegmentedControl } from "@/components/ui/segmented-control";
import {
  Sheet,
  SheetDescription,
  SheetPageContent,
} from "@/components/ui/sheet";
import { Switch } from "@/components/ui/switch";
import { fieldClass } from "@/components/ui/input";
import { FieldError } from "@/components/prompts/PromptFormPanel";
import { useDarkMode } from "@/hooks/useDarkMode";
import type {
  Subagent,
  SubagentErrorCode,
  SubagentInput,
} from "@/lib/api/subagents";
import { cn } from "@/lib/utils";
import {
  SUBAGENT_COLORS,
  SUBAGENT_NAME_RE,
  getSubagentBody,
  replaceSubagentBody,
  safeParseSubagentMarkdown,
  updateSubagentFrontmatter,
  type SubagentFieldsPatch,
} from "./frontmatter";
import { SubagentModelSelect } from "./SubagentModelSelect";
import { SubagentToolsSelect } from "./SubagentToolsSelect";
import { SUBAGENT_COLOR_HEX, subagentFilePath } from "./subagentUtils";

/** 保存的结果：saved 关编辑器；pending 是在等冲突确认（确认后由页面关）；failed 留在编辑器 */
export type SubagentSaveResult =
  | { status: "saved" }
  | { status: "pending" }
  | { status: "failed"; code?: SubagentErrorCode; message: string };

type EditorView = "form" | "source";

interface SubagentFormPanelProps {
  /** 编辑已有的；新建（含模板、复制为新 agent）时不传 */
  subagent?: Subagent;
  initialContent: string;
  initialEnabled: boolean;
  /** agents 目录的显示路径（~/.claude/agents） */
  dirDisplayPath?: string;
  /** 库里其它 agent 已用的名字，用来就地提示重名 */
  takenNames: Set<string>;
  onSave: (input: SubagentInput) => Promise<SubagentSaveResult>;
  onClose: () => void;
}

/**
 * 新建 / 编辑 subagent 的整页编辑器（同 PromptFormPanel 盖住内容区）。
 *
 * 整份 markdown 是唯一的数据源：表单的每次修改都改写 content 里的 frontmatter，
 * 源码模式直接编辑 content，所以两种模式之间切换不需要额外同步。
 */
export default function SubagentFormPanel({
  subagent,
  initialContent,
  initialEnabled,
  dirDisplayPath,
  takenNames,
  onSave,
  onClose,
}: SubagentFormPanelProps) {
  const { t } = useTranslation();
  const baseId = useId();
  const darkMode = useDarkMode();
  const [content, setContent] = useState(initialContent);
  const [enabled, setEnabled] = useState(initialEnabled);
  const [attempted, setAttempted] = useState(false);
  const [saving, setSaving] = useState(false);
  const savingRef = useRef(false);
  const [serverError, setServerError] = useState<{
    code?: SubagentErrorCode;
    message: string;
    /** 报错时的名字：用户改了名字就不再显示「已被占用」 */
    name?: string;
  } | null>(null);
  const nameRef = useRef<HTMLInputElement>(null);
  const descRef = useRef<HTMLTextAreaElement>(null);

  const parsed = useMemo(() => safeParseSubagentMarkdown(content), [content]);
  // 打开时 frontmatter 就坏了（手改出错的文件）：直接进源码模式修
  const [view, setView] = useState<EditorView>(() =>
    safeParseSubagentMarkdown(initialContent).ok ? "form" : "source",
  );
  const [switchError, setSwitchError] = useState(false);

  const fields = parsed.ok ? parsed.value.fields : {};
  const name = fields.name ?? "";
  const description = fields.description ?? "";
  const tools = fields.tools;
  const body = parsed.ok ? parsed.value.body : getSubagentBody(content);

  const patch = (next: SubagentFieldsPatch) => {
    setServerError((prev) =>
      prev?.code === "SUBAGENT_NAME_TAKEN" && !("name" in next) ? prev : null,
    );
    setContent((prev) => {
      try {
        return updateSubagentFrontmatter(prev, next);
      } catch {
        // 表单只在 frontmatter 合法时可用；万一不合法就不改，避免把原文冲掉
        return prev;
      }
    });
  };

  // ─── 校验 ──────────────────────────────────────────────────────────────
  const nameError = !name.trim()
    ? attempted
      ? t("subagents.form.nameRequired")
      : ""
    : !SUBAGENT_NAME_RE.test(name)
      ? t("subagents.form.nameInvalid")
      : takenNames.has(name)
        ? t("subagents.form.nameTaken", { name })
        : serverError?.code === "SUBAGENT_NAME_TAKEN" &&
            serverError.name === name
          ? t("subagents.form.nameTaken", { name })
          : "";
  const descError =
    attempted && !description.trim() ? t("subagents.form.descRequired") : "";
  const toolsError =
    attempted && tools !== undefined && tools.length === 0
      ? t("subagents.tools.emptyError")
      : "";
  const parseError = parsed.ok
    ? ""
    : t(`subagents.form.parseError.${parsed.code}`, {
        detail: parsed.message,
      });

  // 文件名原本就跟着 name 走（新建的都是）时改名一并改文件名；导入时保留了别的文件名的不动
  const followsName = !subagent || subagent.fileName === subagent.name;
  const nextFileName =
    followsName && SUBAGENT_NAME_RE.test(name) ? name : subagent?.fileName;
  const fileName = nextFileName ?? (name || "<name>");
  const targetPath = subagentFilePath(
    dirDisplayPath ?? "~/.claude/agents",
    fileName,
  );

  const changeView = (next: EditorView) => {
    if (next === view) return;
    if (next === "form" && !parsed.ok) {
      setSwitchError(true);
      return;
    }
    setSwitchError(false);
    setView(next);
  };

  const handleSave = async () => {
    if (savingRef.current) return;
    setAttempted(true);
    if (!parsed.ok) {
      setView("source");
      return;
    }
    const invalidName =
      !name.trim() || !SUBAGENT_NAME_RE.test(name) || takenNames.has(name);
    if (invalidName || !description.trim()) {
      if (view !== "form") setView("form");
      requestAnimationFrame(() =>
        (invalidName ? nameRef : descRef).current?.focus(),
      );
      return;
    }
    if (tools !== undefined && tools.length === 0) {
      if (view !== "form") setView("form");
      return;
    }

    savingRef.current = true;
    setSaving(true);
    setServerError(null);
    try {
      const result = await onSave({
        id: subagent?.id,
        fileName: subagent ? nextFileName : undefined,
        content,
        enabled,
      });
      if (result.status === "saved") {
        onClose();
      } else if (result.status === "failed") {
        setServerError({ code: result.code, message: result.message, name });
      }
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  };

  const handleClose = () => {
    if (!savingRef.current) onClose();
  };

  const title = subagent
    ? t("subagents.form.editTitle", { name: subagent.name })
    : t("subagents.form.newTitle");
  const submitLabel = enabled
    ? t("subagents.form.saveAndWrite")
    : t("common.save");

  const nameId = `${baseId}-name`;
  const nameHintId = `${baseId}-name-hint`;
  const descId = `${baseId}-desc`;
  const descHintId = `${baseId}-desc-hint`;
  const modelId = `${baseId}-model`;
  const toolsHintId = `${baseId}-tools-hint`;
  const labelClass = "text-body font-medium text-fg-1";
  const requiredMark = (
    <>
      <span aria-hidden="true" className="ms-0.5 text-danger-text">
        *
      </span>
      <span className="sr-only">{t("prompts.requiredMark")}</span>
    </>
  );

  const fieldsColumn = (
    <div className="flex min-w-0 flex-col gap-5">
      <div className="flex flex-col gap-1.5">
        <label htmlFor={nameId} className={labelClass}>
          {t("subagents.form.name")}
          {requiredMark}
        </label>
        <input
          ref={nameRef}
          id={nameId}
          type="text"
          value={name}
          onChange={(event) => patch({ name: event.target.value })}
          disabled={saving}
          placeholder="code-reviewer"
          aria-required="true"
          aria-invalid={Boolean(nameError)}
          aria-describedby={nameHintId}
          autoComplete="off"
          spellCheck={false}
          className={cn(fieldClass, "h-8 font-mono text-caption")}
        />
        {nameError ? (
          <FieldError id={`${nameHintId}-error`}>{nameError}</FieldError>
        ) : null}
        <span id={nameHintId} className="text-caption text-fg-2">
          {t("subagents.form.nameHint")}{" "}
          <code
            title={targetPath}
            className="break-all font-mono text-caption text-fg-1"
          >
            {targetPath}
          </code>
        </span>
      </div>

      <div className="flex flex-col gap-1.5">
        <label htmlFor={descId} className={labelClass}>
          {t("subagents.form.description")}
          {requiredMark}
        </label>
        <textarea
          ref={descRef}
          id={descId}
          value={description}
          rows={4}
          onChange={(event) => patch({ description: event.target.value })}
          disabled={saving}
          placeholder={t("subagents.form.descriptionPlaceholder")}
          aria-required="true"
          aria-invalid={Boolean(descError)}
          aria-describedby={descHintId}
          className={cn(
            fieldClass,
            "min-h-[88px] resize-y px-2.5 py-2 leading-[18px]",
          )}
        />
        {descError ? (
          <FieldError id={`${descHintId}-error`}>{descError}</FieldError>
        ) : null}
        <span id={descHintId} className="text-caption text-fg-2">
          {t("subagents.form.descriptionHint")}
        </span>
      </div>

      <div className="flex flex-col gap-1.5">
        <label htmlFor={modelId} className={labelClass}>
          {t("subagents.form.model")}
        </label>
        <SubagentModelSelect
          id={modelId}
          value={fields.model}
          disabled={saving}
          onChange={(value) => patch({ model: value || null })}
        />
      </div>

      <div className="flex flex-col gap-1.5">
        <span className={labelClass}>{t("subagents.form.tools")}</span>
        <SubagentToolsSelect
          tools={tools}
          disabled={saving}
          error={toolsError || undefined}
          errorId={toolsHintId}
          onChange={(next) => patch({ tools: next ?? null })}
        />
      </div>

      <div className="flex flex-col gap-1.5">
        <span className={labelClass}>{t("subagents.form.color")}</span>
        <div
          role="group"
          aria-label={t("subagents.form.color")}
          className="flex flex-wrap items-center gap-1.5"
        >
          <button
            type="button"
            aria-pressed={!fields.color}
            disabled={saving}
            onClick={() => patch({ color: null })}
            aria-label={t("subagents.form.colorNone")}
            title={t("subagents.form.colorNone")}
            className={cn(
              "inline-flex h-6 w-6 items-center justify-center rounded-full border border-border-strong text-fg-3 transition-shadow",
              !fields.color && "ring-2 ring-ring ring-offset-2 ring-offset-app",
            )}
          >
            <Ban aria-hidden="true" className="h-3.5 w-3.5" strokeWidth={1.5} />
          </button>
          {SUBAGENT_COLORS.map((color) => {
            const on = fields.color === color;
            return (
              <button
                key={color}
                type="button"
                aria-pressed={on}
                disabled={saving}
                onClick={() => patch({ color })}
                aria-label={t(`subagents.colors.${color}`)}
                title={t(`subagents.colors.${color}`)}
                style={{ backgroundColor: SUBAGENT_COLOR_HEX[color] }}
                className={cn(
                  "inline-flex h-6 w-6 items-center justify-center rounded-full text-white transition-shadow",
                  on && "ring-2 ring-ring ring-offset-2 ring-offset-app",
                )}
              >
                {on ? (
                  <Check aria-hidden="true" className="h-3.5 w-3.5" />
                ) : null}
              </button>
            );
          })}
          {fields.color &&
          !(SUBAGENT_COLORS as readonly string[]).includes(fields.color) ? (
            <span className="font-mono text-caption text-fg-2">
              {fields.color}
            </span>
          ) : null}
        </div>
      </div>
    </div>
  );

  return (
    <Sheet
      open
      modal={false}
      onOpenChange={(open) => {
        if (!open) handleClose();
      }}
    >
      <SheetPageContent
        // 子代理即时写盘，不属于外层供应商表单的改动：别让外层的未保存跟踪把它算进去
        data-unsaved-ignore=""
        title={title}
        closeLabel={t("common.back")}
        onEscapeKeyDown={(event) => {
          if (savingRef.current) event.preventDefault();
        }}
      >
        <SheetDescription className="sr-only">
          {t("subagents.form.sheetDescription", { path: targetPath })}
        </SheetDescription>

        <div className="flex h-12 shrink-0 items-center gap-3 border-b border-border px-6">
          {/* 只切换显示方式，不算修改 */}
          <div data-unsaved-ignore="">
            <SegmentedControl<EditorView>
              size="sm"
              aria-label={t("subagents.form.viewAria")}
              value={view}
              onValueChange={changeView}
              items={[
                { value: "form", label: t("subagents.form.viewForm") },
                { value: "source", label: t("subagents.form.viewSource") },
              ]}
            />
          </div>
          <label className="ms-2 flex items-center gap-2 text-body text-fg-1">
            <Switch
              checked={enabled}
              disabled={saving}
              onCheckedChange={setEnabled}
              aria-describedby={`${baseId}-enabled-hint`}
            />
            {t("subagents.form.enabled")}
          </label>
          <span
            id={`${baseId}-enabled-hint`}
            className="hidden min-w-0 truncate text-caption text-fg-2 md:inline"
          >
            {enabled
              ? t("subagents.form.enabledHint", { path: targetPath })
              : t("subagents.form.disabledHint")}
          </span>
          <div className="flex-1" />
          <Button
            type="button"
            variant="neutral"
            size="regular"
            onClick={handleClose}
            disabled={saving}
          >
            {t("common.cancel")}
          </Button>
          <Button
            type="button"
            variant="solid"
            size="regular"
            onClick={() => void handleSave()}
            disabled={saving}
          >
            {saving ? t("common.saving") : submitLabel}
          </Button>
        </div>

        {(switchError || (view === "source" && attempted)) && parseError ? (
          <div role="status" className="shrink-0 px-6 pt-3">
            <Notice tone="danger" title={t("subagents.form.fixSourceFirst")}>
              {parseError}
            </Notice>
          </div>
        ) : null}
        {serverError && serverError.code !== "SUBAGENT_NAME_TAKEN" ? (
          <div role="alert" className="shrink-0 px-6 pt-3">
            <Notice tone="danger" title={t("subagents.form.saveFailed")}>
              {serverError.message}
            </Notice>
          </div>
        ) : null}

        {view === "form" ? (
          <div className="grid min-h-0 flex-1 grid-cols-1 gap-6 overflow-y-auto scroll-stable overscroll-contain px-6 pb-6 pt-5 lg:grid-cols-[minmax(300px,400px)_minmax(0,1fr)] lg:grid-rows-[minmax(380px,1fr)]">
            {/* 宽屏两栏：左栏字段自己滚，右栏编辑器铺满高度。行高至少放得下编辑器，窗口再矮就整块滚动，
                不再 overflow-hidden 把编辑器底部裁掉 */}
            {/* 滚动容器会裁掉溢出边框的部分：选中颜色的 ring + offset（4px）、输入框聚焦光晕（3px）。
                内边距留出这 4px，负外边距抵消，字段仍和右栏对齐；pe-3 让滚动条别贴着输入框 */}
            <div className="min-w-0 lg:-m-1 lg:overflow-y-auto lg:overscroll-contain lg:p-1 lg:pe-3">
              {fieldsColumn}
            </div>
            <div className="flex min-h-[360px] min-w-0 flex-col gap-1.5">
              <span className={labelClass}>{t("subagents.form.body")}</span>
              <span className="text-caption text-fg-2">
                {t("subagents.form.bodyHint")}
              </span>
              <MarkdownEditor
                key="body"
                value={body}
                onChange={(next) =>
                  setContent((prev) => replaceSubagentBody(prev, next))
                }
                darkMode={darkMode}
                readOnly={saving}
                placeholder={t("subagents.form.bodyPlaceholder")}
                minHeight="320px"
                className="min-h-0 flex-1"
              />
            </div>
          </div>
        ) : (
          <div className="flex min-h-0 flex-1 flex-col gap-1.5 px-6 pb-6 pt-4">
            <span className="text-caption text-fg-2">
              {t("subagents.form.sourceHint")}
            </span>
            {!parsed.ok && !switchError && !attempted ? (
              <span className="text-caption text-danger-text">
                {parseError}
              </span>
            ) : null}
            <MarkdownEditor
              key="source"
              value={content}
              onChange={(next) => {
                setServerError(null);
                setContent(next);
              }}
              darkMode={darkMode}
              readOnly={saving}
              minHeight="360px"
              className="min-h-0 flex-1"
            />
          </div>
        )}
      </SheetPageContent>
    </Sheet>
  );
}
