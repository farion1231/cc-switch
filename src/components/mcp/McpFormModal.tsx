import React, { useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import { Check, Eye, EyeOff, Plus, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { HelpTip } from "@/components/ui/help-tip";
import { SegmentedControl } from "@/components/ui/segmented-control";
import {
  Sheet,
  SheetBody,
  SheetContent,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { HoverTip } from "@/components/ui/hover-tip";
import { AppGlyph, APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import type { McpServer, McpServerSpec } from "@/types";
import type { McpAppId } from "@/config/appConfig";
import { MCP_APP_IDS } from "@/config/appConfig";
import { mcpPresets, getMcpPresetWithDescription } from "@/config/mcpPresets";
import {
  extractErrorMessage,
  translateMcpBackendError,
} from "@/utils/errorUtils";
import { useUpsertMcpServer } from "@/hooks/useMcp";
import { cn } from "@/lib/utils";
import { NeutralBadge } from "./AppMatrix";
import {
  CHECKBOX_CLASS,
  DisclosureButton,
  FIELD_CLASS,
  FieldError,
  LABEL_CLASS,
  MiniSegmented,
  MONO_FIELD_CLASS,
  RequiredMark,
} from "./formBits";
import {
  connectionOf,
  jsonTextOf,
  parseJsonText,
  recognizePaste,
  secretCountOf,
  specOf,
  uniqueId,
  validateDraft,
  type JsonParseError,
  type KeyValueRow,
  type McpDraftConnection,
  type McpDraftErrors,
  type McpTransport,
  type PasteFormat,
  type PastedServer,
} from "./mcpDraft";

type AppsState = Record<McpAppId, boolean>;

interface MetaDraft {
  name: string;
  description: string;
  tags: string;
  homepage: string;
  docs: string;
}

interface BatchItem extends PastedServer {
  exists: boolean;
  include: boolean;
}

interface McpFormModalProps {
  /** 编辑时传：服务器的键名（不能改） */
  editingId?: string;
  initialData?: McpServer;
  /** 现有全部服务器（查重、批量覆盖用） */
  existingServers?: Record<string, McpServer>;
  /** 「写入到」里列出的应用（应用页里设为显示的） */
  visibleAppIds?: McpAppId[];
  onSave: () => void | Promise<void>;
  onClose: () => void;
  /** 编辑时底栏左侧「删除…」 */
  onRequestDelete?: (id: string) => void;
}

const emptyMeta = (): MetaDraft => ({
  name: "",
  description: "",
  tags: "",
  homepage: "",
  docs: "",
});

function metaOf(server?: McpServer): MetaDraft {
  if (!server) return emptyMeta();
  return {
    // 显示名和键名一样时不必重复填
    name: server.name && server.name !== server.id ? server.name : "",
    description: server.description ?? "",
    tags: (server.tags ?? []).join(", "),
    homepage: server.homepage ?? "",
    docs: server.docs ?? "",
  };
}

function appsOf(
  server: McpServer | undefined,
  defaults: readonly McpAppId[],
): AppsState {
  const out = {} as AppsState;
  for (const app of MCP_APP_IDS) {
    out[app] = server ? Boolean(server.apps?.[app]) : defaults.includes(app);
  }
  return out;
}

/**
 * MCP 添加 / 编辑抽屉（宽 560）：模板、粘贴识别、表单 ⇄ JSON、写入到、说明与链接。
 * 旧版的「配置向导」并进了表单。
 */
const McpFormModal: React.FC<McpFormModalProps> = ({
  editingId,
  initialData,
  existingServers = {},
  visibleAppIds = MCP_APP_IDS,
  onSave,
  onClose,
  onRequestDelete,
}) => {
  const { t } = useTranslation();
  const upsertMutation = useUpsertMcpServer();
  const isEdit = Boolean(editingId);
  const existingIds = useMemo(
    () => Object.keys(existingServers),
    [existingServers],
  );

  const [name, setName] = useState(editingId ?? "");
  const [conn, setConn] = useState<McpDraftConnection>(() =>
    connectionOf(initialData?.server ?? { type: "stdio" }),
  );
  const [apps, setApps] = useState<AppsState>(() =>
    appsOf(initialData, visibleAppIds),
  );
  const [meta, setMeta] = useState<MetaDraft>(() => metaOf(initialData));
  const [metaOpen, setMetaOpen] = useState(false);
  const [tab, setTab] = useState<"form" | "json">("form");
  const [jsonText, setJsonText] = useState("");
  const [jsonError, setJsonError] = useState<JsonParseError | null>(null);
  const [revealJson, setRevealJson] = useState(false);
  const [revealed, setRevealed] = useState<Record<string, boolean>>({});
  const [attempted, setAttempted] = useState(false);
  const [template, setTemplate] = useState<string | null>(null);
  const [pasteText, setPasteText] = useState("");
  const [paste, setPaste] = useState<
    | { ok: true; format: PasteFormat; name: string; count: number }
    | { ok: false }
    | null
  >(null);
  const [batch, setBatch] = useState<{
    format: PasteFormat;
    items: BatchItem[];
  } | null>(null);
  const [batchError, setBatchError] = useState(false);
  const [saving, setSaving] = useState(false);
  const savingRef = useRef(false);

  const errors: McpDraftErrors = useMemo(
    () =>
      validateDraft(name, conn, {
        checkName: !isEdit,
        existingIds,
      }),
    [conn, existingIds, isEdit, name],
  );
  // 重名即时提示（R6）；其他错误点了「添加 / 保存」才出现
  const visibleErrors: McpDraftErrors = attempted
    ? errors
    : errors.name === "nameExists"
      ? { name: errors.name }
      : {};

  const fieldErrorText = (code?: string) => {
    if (!code) return "";
    return t(`mcpPage.drawer.errors.${code}`, { name: name.trim() });
  };

  const formatLabel = (format: PasteFormat) =>
    t(`mcpPage.drawer.pasteFormats.${format}`);

  // ─── 模板 / 粘贴 ─────────────────────────────────────────────────────
  const pickTemplate = (id: string) => {
    const preset = mcpPresets.find((item) => item.id === id);
    if (!preset) return;
    const withDesc = getMcpPresetWithDescription(preset, t);
    setTemplate(id);
    setName(uniqueId(preset.id, existingIds));
    setConn(connectionOf(withDesc.server));
    setMeta({
      name: withDesc.name && withDesc.name !== preset.id ? withDesc.name : "",
      description: withDesc.description ?? "",
      tags: (withDesc.tags ?? []).join(", "),
      homepage: withDesc.homepage ?? "",
      docs: withDesc.docs ?? "",
    });
    setBatch(null);
    setPaste(null);
    setTab("form");
    setJsonError(null);
    setAttempted(false);
  };

  const handlePaste = (text: string) => {
    const result = recognizePaste(text);
    if (!result) {
      setPasteText(text);
      setPaste(null);
      return;
    }
    if (!result.ok) {
      setPasteText(text);
      setPaste({ ok: false });
      return;
    }
    // 识别成功就清空输入框：凭据不留在明文框里，内容进表单或批量列表
    setPasteText("");
    setTemplate(null);
    if (result.items.length === 1) {
      const [item] = result.items;
      setName(item.name);
      setConn(connectionOf(item.spec));
      setMeta(emptyMeta());
      setBatch(null);
      setTab("form");
      setJsonError(null);
      setAttempted(false);
      setRevealed({});
      setPaste({
        ok: true,
        format: result.format,
        name: item.name,
        count: 1,
      });
      return;
    }
    setBatch({
      format: result.format,
      items: result.items.map((item) => {
        const exists = existingIds.includes(item.name);
        return { ...item, exists, include: !exists };
      }),
    });
    setBatchError(false);
    setPaste({
      ok: true,
      format: result.format,
      name: "",
      count: result.items.length,
    });
  };

  // ─── 表单 ⇄ JSON ─────────────────────────────────────────────────────
  const switchTab = (next: "form" | "json") => {
    if (next === tab) return;
    if (next === "json") {
      setJsonText(jsonTextOf(conn, revealJson));
      setJsonError(null);
      setTab("json");
      return;
    }
    const parsed = parseJsonText(jsonText, conn);
    if (!parsed.ok) {
      setJsonError(parsed.error);
      return;
    }
    setConn(connectionOf(parsed.spec));
    setJsonError(null);
    setTab("form");
  };

  const handleJsonChange = (text: string) => {
    setJsonText(text);
    const parsed = parseJsonText(text, conn);
    setJsonError(parsed.ok ? null : parsed.error);
  };

  const toggleRevealJson = () => {
    // 先把当前文字里的改动收进草稿，再换显示方式
    const parsed = parseJsonText(jsonText, conn);
    const base = parsed.ok ? connectionOf(parsed.spec) : conn;
    if (parsed.ok) setConn(base);
    const next = !revealJson;
    setRevealJson(next);
    setJsonText(jsonTextOf(base, next));
  };

  const jsonErrorText = (error: JsonParseError) =>
    error.kind === "notObject"
      ? t("mcpPage.drawer.jsonNotObject")
      : error.kind === "maskedUnknown"
        ? t("mcpPage.drawer.jsonMaskedUnknown", { key: error.key })
        : error.line
          ? t("mcpPage.drawer.jsonSyntaxLine", { line: error.line })
          : t("mcpPage.drawer.jsonSyntax");

  // ─── 连接字段编辑 ───────────────────────────────────────────────────
  const patchConn = (patch: Partial<McpDraftConnection>) =>
    setConn((prev) => ({ ...prev, ...patch }));

  const updateRows = (
    field: "env" | "headers",
    updater: (rows: KeyValueRow[]) => KeyValueRow[],
  ) => setConn((prev) => ({ ...prev, [field]: updater(prev[field]) }));

  // ─── 提交 ───────────────────────────────────────────────────────────
  const focusSoon = (id: string) =>
    window.setTimeout(() => document.getElementById(id)?.focus(), 0);

  const buildEntry = (
    id: string,
    spec: McpServerSpec,
    base?: McpServer,
    metaDraft?: MetaDraft,
  ): McpServer => {
    // 只用「写入到」里能看到的勾选覆盖原值：隐藏只影响界面，覆盖已有服务器时隐藏
    // 应用的开关原样带回，否则会被当成关掉、从那些应用的配置里删掉；不支持 MCP 的
    // 应用（OpenClaw 等）沿用原值，新建时为 false
    const visibleApps = Object.fromEntries(
      visibleAppIds.map((app) => [app, apps[app]]),
    );
    const entry: McpServer = {
      ...(base ? { ...base } : {}),
      id,
      name: id,
      server: spec,
      apps: {
        openclaw: false,
        ...(base?.apps ?? appsOf(undefined, [])),
        ...visibleApps,
      } as McpServer["apps"],
    };
    if (!metaDraft) {
      entry.name = base?.name || id;
      return entry;
    }
    entry.name = metaDraft.name.trim() || id;
    const assign = (key: "description" | "homepage" | "docs") => {
      const value = metaDraft[key].trim();
      if (value) entry[key] = value;
      else delete entry[key];
    };
    assign("description");
    assign("homepage");
    assign("docs");
    const tags = metaDraft.tags
      .split(/[,，]/)
      .map((tag) => tag.trim())
      .filter(Boolean);
    if (tags.length) entry.tags = tags;
    else delete entry.tags;
    return entry;
  };

  const reportSaveError = (error: unknown) => {
    const detail = extractErrorMessage(error);
    const mapped = translateMcpBackendError(detail, t);
    toast.error(mapped || detail || t("mcp.error.saveFailed"), {
      duration: 6000,
    });
  };

  const submitBatch = async () => {
    if (!batch) return;
    const chosen = batch.items.filter((item) => item.include);
    if (chosen.length === 0) {
      setBatchError(true);
      return;
    }
    savingRef.current = true;
    setSaving(true);
    let added = 0;
    let replaced = 0;
    const failed: string[] = [];
    try {
      for (const item of chosen) {
        const base = item.exists ? existingServers[item.name] : undefined;
        try {
          await upsertMutation.mutateAsync(
            buildEntry(item.name, item.spec, base),
          );
          if (item.exists) replaced += 1;
          else added += 1;
        } catch (error) {
          failed.push(item.name);
          reportSaveError(error);
        }
      }
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
    if (added + replaced > 0) {
      toast.success(
        replaced
          ? t("mcpPage.toast.batchAddedReplaced", {
              count: added,
              replaced,
            })
          : t("mcpPage.toast.batchAdded", { count: added }),
        { closeButton: true },
      );
    }
    if (failed.length === 0) await onSave();
  };

  const submitSingle = async () => {
    let current = conn;
    if (tab === "json") {
      const parsed = parseJsonText(jsonText, conn);
      if (!parsed.ok) {
        setJsonError(parsed.error);
        focusSoon("mcp-json");
        return;
      }
      current = connectionOf(parsed.spec);
      setConn(current);
    }
    const errs = validateDraft(name, current, {
      checkName: !isEdit,
      existingIds,
    });
    setAttempted(true);
    if (errs.name || errs.command || errs.url) {
      if (tab === "json" && !errs.name) {
        setJsonError(null);
        setTab("form");
      }
      focusSoon(errs.name ? "mcp-name" : errs.command ? "mcp-cmd" : "mcp-url");
      return;
    }
    const id = isEdit ? (editingId as string) : name.trim();
    savingRef.current = true;
    setSaving(true);
    try {
      await upsertMutation.mutateAsync(
        buildEntry(id, specOf(current), initialData, meta),
      );
      toast.success(
        isEdit
          ? t("mcpPage.toast.saved", { id })
          : t("mcpPage.toast.added", { id }),
        { closeButton: true },
      );
      await onSave();
    } catch (error) {
      reportSaveError(error);
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  };

  const handleSubmit = () => {
    if (savingRef.current) return;
    void (batch ? submitBatch() : submitSingle());
  };

  // ─── 渲染 ───────────────────────────────────────────────────────────
  const title = isEdit
    ? t("mcpPage.drawer.editTitle", { id: editingId })
    : t("mcpPage.drawer.addTitle");

  const batchChosen = batch?.items.filter((item) => item.include) ?? [];
  const batchNew = batchChosen.filter((item) => !item.exists).length;
  const batchOver = batchChosen.length - batchNew;
  const submitLabel = batch
    ? batchChosen.length === 0
      ? t("common.add")
      : batchOver
        ? t("mcpPage.drawer.addManyReplace", {
            count: batchNew,
            replaced: batchOver,
          })
        : t("mcpPage.drawer.addMany", { count: batchNew })
    : isEdit
      ? t("common.save")
      : t("common.add");

  const renderKvRows = (field: "env" | "headers", label: string) => {
    const rows = conn[field];
    return (
      <div
        role="group"
        aria-labelledby={`mcp-${field}-label`}
        className="flex flex-col gap-1.5"
      >
        <span id={`mcp-${field}-label`} className={LABEL_CLASS}>
          {label}
        </span>
        {rows.map((row, index) => {
          const revealKey = `${field}:${index}`;
          const shown = Boolean(revealed[revealKey]);
          const rowName = row.key.trim() || String(index + 1);
          return (
            <div key={index} className="flex items-center gap-1">
              <input
                type="text"
                className={cn(MONO_FIELD_CLASS, "w-[188px] shrink-0")}
                aria-label={t("mcpPage.drawer.kvKeyAria", {
                  label,
                  index: index + 1,
                })}
                placeholder={t("mcpPage.drawer.kvKeyPlaceholder")}
                value={row.key}
                autoComplete="off"
                spellCheck={false}
                onChange={(event) =>
                  updateRows(field, (list) =>
                    list.map((item, i) =>
                      i === index ? { ...item, key: event.target.value } : item,
                    ),
                  )
                }
              />
              <input
                type={shown ? "text" : "password"}
                className={cn(MONO_FIELD_CLASS, "flex-1")}
                aria-label={t("mcpPage.drawer.kvValueAria", {
                  label,
                  index: index + 1,
                })}
                placeholder={t("mcpPage.drawer.kvValuePlaceholder")}
                value={row.value}
                autoComplete="off"
                spellCheck={false}
                onChange={(event) =>
                  updateRows(field, (list) =>
                    list.map((item, i) =>
                      i === index
                        ? { ...item, value: event.target.value }
                        : item,
                    ),
                  )
                }
              />
              <HoverTip
                content={
                  shown
                    ? t("mcpPage.drawer.hideValueTip")
                    : t("mcpPage.drawer.showValueTip")
                }
              >
                <Button
                  type="button"
                  variant="quiet"
                  size="icon-compact"
                  aria-pressed={shown}
                  aria-label={t("mcpPage.drawer.revealValue", {
                    name: rowName,
                  })}
                  onClick={() =>
                    setRevealed((prev) => ({ ...prev, [revealKey]: !shown }))
                  }
                >
                  {shown ? (
                    <EyeOff className="h-4 w-4" strokeWidth={1.5} />
                  ) : (
                    <Eye className="h-4 w-4" strokeWidth={1.5} />
                  )}
                </Button>
              </HoverTip>
              <HoverTip content={t("common.delete")}>
                <Button
                  type="button"
                  variant="quiet"
                  size="icon-compact"
                  aria-label={t("mcpPage.drawer.removeRow", {
                    label,
                    name: rowName,
                  })}
                  onClick={() => {
                    updateRows(field, (list) =>
                      list.filter((_, i) => i !== index),
                    );
                    setRevealed({});
                  }}
                >
                  <X className="h-3.5 w-3.5" strokeWidth={1.5} />
                </Button>
              </HoverTip>
            </div>
          );
        })}
        <AddRowButton
          onClick={() =>
            updateRows(field, (list) => [...list, { key: "", value: "" }])
          }
        >
          {field === "env"
            ? t("mcpPage.drawer.addEnv")
            : t("mcpPage.drawer.addHeader")}
        </AddRowButton>
      </div>
    );
  };

  const extraKeys = Object.keys(conn.extra);
  const hasSecrets = secretCountOf(conn) > 0;

  return (
    <Sheet
      open
      onOpenChange={(open) => {
        if (!open && !savingRef.current) onClose();
      }}
    >
      <SheetContent
        width={560}
        closeLabel={t("common.close")}
        aria-describedby={undefined}
      >
        <SheetHeader className="flex h-[52px] items-center space-y-0 border-b border-border pe-12 ps-6 pt-0">
          <SheetTitle className="truncate text-section" title={title}>
            {title}
          </SheetTitle>
        </SheetHeader>

        <SheetBody className="flex flex-col gap-5 px-6 pb-6 pt-5">
          {!isEdit && (
            <>
              <div className="flex flex-col gap-2">
                <span id="mcp-tpl-label" className={LABEL_CLASS}>
                  {t("mcpPage.drawer.templates")}
                </span>
                <div
                  role="group"
                  aria-labelledby="mcp-tpl-label"
                  className="flex flex-wrap gap-2"
                >
                  {mcpPresets.map((preset) => (
                    <HoverTip
                      content={t(`mcp.presets.${preset.id}.description`)}
                    >
                      <button
                        key={preset.id}
                        type="button"
                        aria-pressed={template === preset.id}
                        onClick={() => pickTemplate(preset.id)}
                        className="h-7 whitespace-nowrap rounded-full border border-border-strong bg-surface px-3 text-body transition-colors hover:bg-subtle aria-pressed:bg-selected aria-pressed:font-medium"
                      >
                        {t(`mcpPage.templateLabels.${preset.id}`, {
                          defaultValue: preset.id,
                        })}
                      </button>
                    </HoverTip>
                  ))}
                </div>
              </div>
              <div className="flex flex-col gap-1.5">
                <label htmlFor="mcp-paste" className={LABEL_CLASS}>
                  {t("mcpPage.drawer.paste")}
                </label>
                <textarea
                  id="mcp-paste"
                  value={pasteText}
                  onChange={(event) => handlePaste(event.target.value)}
                  placeholder={t("mcpPage.drawer.pastePlaceholder")}
                  aria-invalid={paste?.ok === false}
                  aria-describedby={paste ? "mcp-paste-hint" : undefined}
                  spellCheck={false}
                  rows={3}
                  className={cn(
                    MONO_FIELD_CLASS,
                    "h-16 resize-none py-2 leading-[18px] placeholder:font-sans placeholder:text-body",
                  )}
                />
                {paste?.ok === true && (
                  <span
                    id="mcp-paste-hint"
                    className="flex items-start gap-1 text-caption text-success-text"
                  >
                    <Check
                      aria-hidden="true"
                      className="mt-0.5 h-3.5 w-3.5 shrink-0"
                      strokeWidth={1.5}
                    />
                    <span>
                      {paste.count > 1
                        ? t("mcpPage.drawer.pasteManyOk")
                        : paste.name
                          ? t("mcpPage.drawer.pasteOneOk", {
                              format: formatLabel(paste.format),
                              name: paste.name,
                            })
                          : t("mcpPage.drawer.pasteOneNoName", {
                              format: formatLabel(paste.format),
                            })}
                    </span>
                  </span>
                )}
                {paste?.ok === false && (
                  <FieldError id="mcp-paste-hint">
                    {t("mcpPage.drawer.pasteHelp")}
                  </FieldError>
                )}
              </div>
              <div aria-hidden="true" className="h-px shrink-0 bg-border" />
            </>
          )}

          {batch ? (
            <section
              aria-labelledby="mcp-batch-title"
              className="flex flex-col gap-2"
            >
              <div className="flex items-center gap-3">
                <h3
                  id="mcp-batch-title"
                  className="m-0 flex-1 text-body font-semibold text-fg-1"
                >
                  {t("mcpPage.drawer.batchTitle", {
                    count: batch.items.length,
                    format: formatLabel(batch.format),
                  })}
                </h3>
                <button
                  type="button"
                  onClick={() => {
                    setBatch(null);
                    setPaste(null);
                    setBatchError(false);
                  }}
                  className="text-caption font-medium text-fg-2 underline underline-offset-[3px] hover:text-fg-1"
                >
                  {t("mcpPage.drawer.batchClear")}
                </button>
              </div>
              <ul className="m-0 list-none rounded-panel border border-border p-0">
                {batch.items.map((item, index) => {
                  const inputId = `mcp-batch-${index}`;
                  const statusId = `${inputId}-status`;
                  const setItem = (patch: Partial<BatchItem>) =>
                    setBatch((prev) =>
                      prev
                        ? {
                            ...prev,
                            items: prev.items.map((it, i) =>
                              i === index ? { ...it, ...patch } : it,
                            ),
                          }
                        : prev,
                    );
                  return (
                    <li
                      key={`${item.name}-${index}`}
                      className={cn(
                        "flex min-h-12 items-center gap-2.5 py-1.5 pe-2.5 ps-3.5",
                        index > 0 && "border-t border-border",
                      )}
                    >
                      <input
                        id={inputId}
                        type="checkbox"
                        className={CHECKBOX_CLASS}
                        checked={item.include}
                        aria-describedby={statusId}
                        onChange={(event) => {
                          setItem({ include: event.target.checked });
                          setBatchError(false);
                        }}
                      />
                      <label
                        htmlFor={inputId}
                        className="min-w-0 truncate text-body font-medium"
                      >
                        {item.name}
                      </label>
                      <NeutralBadge mono>
                        {item.spec.type ?? "stdio"}
                      </NeutralBadge>
                      <span className="flex-1" />
                      <span
                        id={statusId}
                        className="shrink-0 whitespace-nowrap text-caption text-fg-2"
                      >
                        {item.exists
                          ? t("mcpPage.drawer.batchExists")
                          : t("mcpPage.drawer.batchNew")}
                      </span>
                      {item.exists && (
                        <MiniSegmented
                          label={t("mcpPage.drawer.batchExistsGroup", {
                            name: item.name,
                          })}
                          items={[
                            {
                              value: "skip",
                              label: t("mcpPage.drawer.batchSkip"),
                            },
                            {
                              value: "over",
                              label: t("mcpPage.drawer.batchOverwrite"),
                            },
                          ]}
                          value={item.include ? "over" : "skip"}
                          onValueChange={(value) => {
                            setItem({ include: value === "over" });
                            setBatchError(false);
                          }}
                        />
                      )}
                    </li>
                  );
                })}
              </ul>
              {batchError && (
                <FieldError>{t("mcpPage.drawer.batchNone")}</FieldError>
              )}
            </section>
          ) : (
            <>
              <div className="flex flex-col gap-1.5">
                <div className="flex items-center gap-0.5">
                  <label htmlFor="mcp-name" className={LABEL_CLASS}>
                    {t("mcpPage.drawer.name")}
                    <RequiredMark srText={t("mcpPage.drawer.required")} />
                  </label>
                  <HelpTip
                    title={
                      isEdit
                        ? t("mcpPage.drawer.nameHelpEditTitle")
                        : t("mcpPage.drawer.nameHelpTitle")
                    }
                  >
                    {isEdit
                      ? t("mcpPage.drawer.nameHelpEdit")
                      : t("mcpPage.drawer.nameHelp")}
                  </HelpTip>
                </div>
                <input
                  id="mcp-name"
                  type="text"
                  className={MONO_FIELD_CLASS}
                  value={name}
                  readOnly={isEdit}
                  placeholder={t("mcpPage.drawer.namePlaceholder")}
                  aria-required={!isEdit}
                  aria-invalid={Boolean(visibleErrors.name)}
                  aria-describedby={
                    visibleErrors.name ? "mcp-name-hint" : undefined
                  }
                  autoComplete="off"
                  spellCheck={false}
                  onChange={(event) => setName(event.target.value)}
                />
                {visibleErrors.name && (
                  <FieldError id="mcp-name-hint">
                    {fieldErrorText(visibleErrors.name)}
                  </FieldError>
                )}
              </div>

              <section
                aria-labelledby="mcp-conn-title"
                className="flex flex-col gap-4"
              >
                <div className="flex items-center gap-3">
                  <h3
                    id="mcp-conn-title"
                    className="m-0 flex-1 text-body font-semibold text-fg-1"
                  >
                    {t("mcpPage.drawer.connection")}
                  </h3>
                  <SegmentedControl
                    aria-label={t("mcpPage.drawer.editMode")}
                    className="h-8 rounded-[8px]"
                    value={tab}
                    onValueChange={switchTab}
                    items={[
                      {
                        value: "form",
                        label: t("mcpPage.drawer.tabForm"),
                        disabled: tab === "json" && jsonError !== null,
                        className: "min-w-[60px] rounded-[5px]",
                      },
                      {
                        value: "json",
                        label: "JSON",
                        className: "min-w-[60px] rounded-[5px]",
                      },
                    ]}
                  />
                </div>

                {tab === "form" ? (
                  <>
                    <div className="flex flex-col gap-1.5">
                      <span id="mcp-tr-label" className={LABEL_CLASS}>
                        {t("mcpPage.drawer.transport")}
                      </span>
                      <SegmentedControl<McpTransport>
                        aria-label={t("mcpPage.drawer.transport")}
                        className="h-8 self-start rounded-[8px]"
                        value={conn.transport}
                        onValueChange={(value) =>
                          patchConn({ transport: value })
                        }
                        items={[
                          {
                            value: "stdio",
                            label: t("mcpPage.drawer.transportStdio"),
                            className: "rounded-[5px] px-3",
                          },
                          {
                            value: "http",
                            label: "HTTP",
                            className: "rounded-[5px] px-3",
                          },
                          {
                            value: "sse",
                            label: "SSE",
                            className: "rounded-[5px] px-3",
                          },
                        ]}
                      />
                    </div>

                    {conn.transport === "stdio" ? (
                      <>
                        <div className="flex flex-col gap-1.5">
                          <label htmlFor="mcp-cmd" className={LABEL_CLASS}>
                            {t("mcpPage.drawer.command")}
                            <RequiredMark
                              srText={t("mcpPage.drawer.required")}
                            />
                          </label>
                          <input
                            id="mcp-cmd"
                            type="text"
                            className={cn(
                              MONO_FIELD_CLASS,
                              "placeholder:font-sans placeholder:text-body",
                            )}
                            value={conn.command}
                            placeholder={t("mcpPage.drawer.commandPlaceholder")}
                            aria-required="true"
                            aria-invalid={Boolean(visibleErrors.command)}
                            aria-describedby={
                              visibleErrors.command ? "mcp-cmd-hint" : undefined
                            }
                            autoComplete="off"
                            spellCheck={false}
                            onChange={(event) =>
                              patchConn({ command: event.target.value })
                            }
                          />
                          {visibleErrors.command && (
                            <FieldError id="mcp-cmd-hint">
                              {fieldErrorText(visibleErrors.command)}
                            </FieldError>
                          )}
                        </div>

                        <div
                          role="group"
                          aria-labelledby="mcp-args-label"
                          className="flex flex-col gap-1.5"
                        >
                          <span id="mcp-args-label" className={LABEL_CLASS}>
                            {t("mcpPage.drawer.args")}
                          </span>
                          {conn.args.map((arg, index) => (
                            <div
                              key={index}
                              className="flex items-center gap-1"
                            >
                              <input
                                type="text"
                                className={cn(MONO_FIELD_CLASS, "flex-1")}
                                aria-label={t("mcpPage.drawer.argAria", {
                                  index: index + 1,
                                })}
                                value={arg}
                                autoComplete="off"
                                spellCheck={false}
                                onChange={(event) =>
                                  patchConn({
                                    args: conn.args.map((item, i) =>
                                      i === index ? event.target.value : item,
                                    ),
                                  })
                                }
                              />
                              <HoverTip content={t("common.delete")}>
                                <Button
                                  type="button"
                                  variant="quiet"
                                  size="icon-compact"
                                  aria-label={t("mcpPage.drawer.removeArg", {
                                    index: index + 1,
                                  })}
                                  onClick={() =>
                                    patchConn({
                                      args: conn.args.filter(
                                        (_, i) => i !== index,
                                      ),
                                    })
                                  }
                                >
                                  <X
                                    className="h-3.5 w-3.5"
                                    strokeWidth={1.5}
                                  />
                                </Button>
                              </HoverTip>
                            </div>
                          ))}
                          <AddRowButton
                            onClick={() =>
                              patchConn({ args: [...conn.args, ""] })
                            }
                          >
                            {t("mcpPage.drawer.addArg")}
                          </AddRowButton>
                        </div>

                        {renderKvRows("env", t("mcpPage.drawer.env"))}

                        <div className="flex flex-col gap-1.5">
                          <label htmlFor="mcp-cwd" className={LABEL_CLASS}>
                            {t("mcpPage.drawer.cwd")}
                            <span className="ms-1.5 text-caption font-normal text-fg-2">
                              {t("mcpPage.drawer.optional")}
                            </span>
                          </label>
                          <input
                            id="mcp-cwd"
                            type="text"
                            className={cn(
                              MONO_FIELD_CLASS,
                              "placeholder:font-sans placeholder:text-body",
                            )}
                            value={conn.cwd}
                            placeholder={t("mcpPage.drawer.cwdPlaceholder")}
                            autoComplete="off"
                            spellCheck={false}
                            onChange={(event) =>
                              patchConn({ cwd: event.target.value })
                            }
                          />
                        </div>
                      </>
                    ) : (
                      <>
                        <div className="flex flex-col gap-1.5">
                          <label htmlFor="mcp-url" className={LABEL_CLASS}>
                            URL
                            <RequiredMark
                              srText={t("mcpPage.drawer.required")}
                            />
                          </label>
                          <input
                            id="mcp-url"
                            type="text"
                            inputMode="url"
                            className={MONO_FIELD_CLASS}
                            value={conn.url}
                            placeholder="https://mcp.example.com/mcp"
                            aria-required="true"
                            aria-invalid={Boolean(visibleErrors.url)}
                            aria-describedby={
                              visibleErrors.url ? "mcp-url-hint" : undefined
                            }
                            autoComplete="off"
                            spellCheck={false}
                            onChange={(event) =>
                              patchConn({ url: event.target.value })
                            }
                          />
                          {visibleErrors.url && (
                            <FieldError id="mcp-url-hint">
                              {fieldErrorText(visibleErrors.url)}
                            </FieldError>
                          )}
                        </div>
                        {renderKvRows("headers", t("mcpPage.drawer.headers"))}
                      </>
                    )}

                    {extraKeys.length > 0 && (
                      <div className="flex flex-wrap items-center gap-2">
                        <span className={LABEL_CLASS}>
                          {t("mcpPage.drawer.extraFields", {
                            count: extraKeys.length,
                          })}
                        </span>
                        {extraKeys.map((key) => (
                          <code
                            key={key}
                            className="h-[22px] whitespace-nowrap rounded-control bg-subtle px-2 font-mono text-caption leading-[22px]"
                          >
                            {key}
                          </code>
                        ))}
                        <button
                          type="button"
                          onClick={() => switchTab("json")}
                          className="text-body font-medium text-fg-1 underline underline-offset-[3px] hover:text-fg-2"
                        >
                          {t("mcpPage.drawer.editInJson")}
                        </button>
                      </div>
                    )}
                  </>
                ) : (
                  <div className="flex flex-col gap-1.5">
                    <div className="flex min-h-7 items-center gap-3">
                      <div className="flex flex-1 items-center gap-0.5">
                        <label htmlFor="mcp-json" className={LABEL_CLASS}>
                          {t("mcpPage.drawer.jsonLabel")}
                        </label>
                        <HelpTip title={t("mcpPage.drawer.jsonHelpTitle")}>
                          {t("mcpPage.drawer.jsonHelp")}
                        </HelpTip>
                      </div>
                      {hasSecrets && (
                        <button
                          type="button"
                          aria-pressed={revealJson}
                          onClick={toggleRevealJson}
                          className="inline-flex h-7 items-center gap-1 rounded-control pe-2 ps-1.5 text-body font-medium text-fg-2 transition-colors hover:bg-subtle hover:text-fg-1"
                        >
                          {revealJson ? (
                            <EyeOff className="h-3.5 w-3.5" strokeWidth={2} />
                          ) : (
                            <Eye className="h-3.5 w-3.5" strokeWidth={2} />
                          )}
                          {t("mcpPage.drawer.showValues")}
                        </button>
                      )}
                    </div>
                    <textarea
                      id="mcp-json"
                      value={jsonText}
                      onChange={(event) => handleJsonChange(event.target.value)}
                      aria-invalid={jsonError !== null}
                      aria-describedby={jsonError ? "mcp-json-hint" : undefined}
                      spellCheck={false}
                      wrap="off"
                      className={cn(
                        MONO_FIELD_CLASS,
                        "h-[300px] resize-y overflow-auto whitespace-pre px-3 py-2.5 leading-[18px]",
                      )}
                    />
                    {jsonError && (
                      <FieldError id="mcp-json-hint">
                        {jsonErrorText(jsonError)}
                        {jsonError.kind === "syntax" &&
                          ` ${t("mcpPage.drawer.jsonBlocksForm")}`}
                      </FieldError>
                    )}
                    {!jsonError &&
                      attempted &&
                      (errors.command || errors.url) && (
                        <FieldError>
                          {fieldErrorText(errors.command ?? errors.url)}
                        </FieldError>
                      )}
                  </div>
                )}
              </section>
            </>
          )}

          <div
            role="group"
            aria-labelledby="mcp-apps-label"
            className="flex min-w-0 flex-col gap-2"
          >
            <div className="flex items-center gap-0.5">
              <span id="mcp-apps-label" className={LABEL_CLASS}>
                {t("mcpPage.drawer.writeTo")}
              </span>
              <HelpTip title={t("mcpPage.drawer.writeToHelpTitle")}>
                {t("mcpPage.drawer.writeToHelp")}
              </HelpTip>
            </div>
            <div className="grid grid-cols-2 gap-x-4 gap-y-2">
              {visibleAppIds.map((app) => (
                <label
                  key={app}
                  className="flex h-6 cursor-pointer items-center gap-2 text-body"
                >
                  <input
                    type="checkbox"
                    className={CHECKBOX_CLASS}
                    checked={apps[app]}
                    onChange={(event) =>
                      setApps((prev) => ({
                        ...prev,
                        [app]: event.target.checked,
                      }))
                    }
                  />
                  <AppGlyph app={app} size={16} badgeClassName="bg-surface" />
                  <span className="whitespace-nowrap">
                    {APP_DISPLAY_NAME[app]}
                  </span>
                </label>
              ))}
            </div>
          </div>

          {!batch && (
            <div className="flex flex-col gap-3">
              <DisclosureButton
                open={metaOpen}
                controls="mcp-meta"
                onToggle={() => setMetaOpen((value) => !value)}
                hint={t("mcpPage.drawer.metaHint")}
              >
                {t("mcpPage.drawer.meta")}
              </DisclosureButton>
              {metaOpen && (
                <div id="mcp-meta" className="flex flex-col gap-3.5">
                  <div className="flex flex-col gap-1.5">
                    <div className="flex items-center gap-0.5">
                      <label htmlFor="mcp-meta-name" className={LABEL_CLASS}>
                        {t("mcpPage.drawer.displayName")}
                      </label>
                      <HelpTip title={t("mcpPage.drawer.displayNameHelpTitle")}>
                        {t("mcpPage.drawer.displayNameHelp")}
                      </HelpTip>
                    </div>
                    <input
                      id="mcp-meta-name"
                      type="text"
                      className={FIELD_CLASS}
                      value={meta.name}
                      autoComplete="off"
                      onChange={(event) =>
                        setMeta((prev) => ({
                          ...prev,
                          name: event.target.value,
                        }))
                      }
                    />
                  </div>
                  <div className="flex flex-col gap-1.5">
                    <label htmlFor="mcp-meta-desc" className={LABEL_CLASS}>
                      {t("mcpPage.drawer.description")}
                    </label>
                    <textarea
                      id="mcp-meta-desc"
                      rows={2}
                      className={cn(FIELD_CLASS, "h-auto py-2")}
                      value={meta.description}
                      onChange={(event) =>
                        setMeta((prev) => ({
                          ...prev,
                          description: event.target.value,
                        }))
                      }
                    />
                  </div>
                  <div className="flex flex-col gap-1.5">
                    <label htmlFor="mcp-meta-tags" className={LABEL_CLASS}>
                      {t("mcpPage.drawer.tags")}
                    </label>
                    <input
                      id="mcp-meta-tags"
                      type="text"
                      className={FIELD_CLASS}
                      value={meta.tags}
                      placeholder={t("mcpPage.drawer.tagsPlaceholder")}
                      autoComplete="off"
                      onChange={(event) =>
                        setMeta((prev) => ({
                          ...prev,
                          tags: event.target.value,
                        }))
                      }
                    />
                  </div>
                  <div className="flex flex-col gap-1.5">
                    <label htmlFor="mcp-meta-home" className={LABEL_CLASS}>
                      {t("mcpPage.drawer.homepage")}
                    </label>
                    <input
                      id="mcp-meta-home"
                      type="text"
                      inputMode="url"
                      className={FIELD_CLASS}
                      value={meta.homepage}
                      placeholder="https://"
                      autoComplete="off"
                      spellCheck={false}
                      onChange={(event) =>
                        setMeta((prev) => ({
                          ...prev,
                          homepage: event.target.value,
                        }))
                      }
                    />
                  </div>
                  <div className="flex flex-col gap-1.5">
                    <label htmlFor="mcp-meta-docs" className={LABEL_CLASS}>
                      {t("mcpPage.drawer.docs")}
                    </label>
                    <input
                      id="mcp-meta-docs"
                      type="text"
                      inputMode="url"
                      className={FIELD_CLASS}
                      value={meta.docs}
                      placeholder="https://"
                      autoComplete="off"
                      spellCheck={false}
                      onChange={(event) =>
                        setMeta((prev) => ({
                          ...prev,
                          docs: event.target.value,
                        }))
                      }
                    />
                  </div>
                </div>
              )}
            </div>
          )}
        </SheetBody>

        <SheetFooter className="h-14 px-6 py-0">
          {isEdit && onRequestDelete && (
            <Button
              type="button"
              variant="quiet"
              size="regular"
              className="-ms-2.5 text-danger-text hover:text-danger-text"
              disabled={saving}
              onClick={() => onRequestDelete(editingId as string)}
            >
              {t("mcpPage.drawer.delete")}
            </Button>
          )}
          <div className="flex-1" />
          <Button
            type="button"
            variant="neutral"
            size="regular"
            disabled={saving}
            onClick={onClose}
          >
            {t("common.cancel")}
          </Button>
          <Button
            type="button"
            variant="solid"
            size="regular"
            disabled={saving}
            onClick={handleSubmit}
          >
            {saving ? t("common.saving") : submitLabel}
          </Button>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  );
};

function AddRowButton({
  onClick,
  children,
}: {
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="-ms-1.5 inline-flex h-7 items-center gap-1 self-start whitespace-nowrap rounded-control pe-2 ps-1.5 text-body font-medium text-fg-2 transition-colors hover:bg-subtle hover:text-fg-1"
    >
      <Plus aria-hidden="true" className="h-3.5 w-3.5" strokeWidth={2} />
      {children}
    </button>
  );
}

export default McpFormModal;
