import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, Plus, RefreshCw, X } from "lucide-react";
import { toast } from "@/lib/toast";
import { Button } from "@/components/ui/button";
import {
  Sheet,
  SheetDescription,
  SheetPageContent,
} from "@/components/ui/sheet";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { skillsApi, type PiSkillsConfig } from "@/lib/api/skills";
import { cn } from "@/lib/utils";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import type { AppId } from "@/lib/api/types";

interface PiSharedConfigPanelProps {
  onClose: () => void;
  /** 保存成功后回调（调用方刷新技能列表，五态即时跟随） */
  onSaved: () => void;
}

/**
 * Pi 共享配置编辑器（settings.json 的 skills 数组）：
 * [表单 | JSON] 双模式（仿 McpFormModal 的切换与校验模式）。
 * 表单 = 结构化列表（分类徽章 + 警告 + 删除 + 添加 + 复选来源）；
 * JSON = 数组文本直编（必须解析为字符串数组，有错禁止切回/保存）。
 * 保存走 set_pi_skills_entries（双层 revision + 快照 + 原子写）。
 */
export function PiSharedConfigPanel({
  onClose,
  onSaved,
}: PiSharedConfigPanelProps) {
  const { t } = useTranslation();
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [config, setConfig] = useState<PiSkillsConfig | null>(null);
  const [entries, setEntries] = useState<string[]>([]);
  const [tab, setTab] = useState<"form" | "json">("form");
  const [jsonText, setJsonText] = useState("");
  const [jsonError, setJsonError] = useState<string | null>(null);
  const [addInput, setAddInput] = useState("");
  const [sourceOpen, setSourceOpen] = useState(false);
  const [revisionStale, setRevisionStale] = useState(false);
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  const savingRef = useRef(false);
  /** 装填基线（entries 序列化），关闭前比对判断未保存修改 */
  const baselineRef = useRef<string>("[]");

  const load = () => {
    setLoading(true);
    setRevisionStale(false);
    skillsApi
      .getPiSkillsConfig()
      .then((c) => {
        setConfig(c);
        setEntries(c.entries?.map((e) => e.raw) ?? []);
        baselineRef.current = JSON.stringify(
          c.entries?.map((e) => e.raw) ?? [],
        );
        setJsonText(
          JSON.stringify(c.entries?.map((e) => e.raw) ?? [], null, 2),
        );
        setJsonError(null);
      })
      .catch((e) => {
        toast.error(String(e));
        onClose();
      })
      .finally(() => setLoading(false));
  };

  useEffect(() => {
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (!config) return null;

  const parseError = config.parseError ?? null;

  /** 是否有未保存修改（表单比 entries，JSON 比解析后的语义值；解析失败视为有修改） */
  const isDirty = () => {
    if (tab === "json") {
      const parsed = parseJsonArray(jsonText);
      return !parsed.ok || JSON.stringify(parsed.value) !== baselineRef.current;
    }
    return JSON.stringify(entries) !== baselineRef.current;
  };
  /** 关闭请求：保存中不许关；有未保存修改先确认 */
  const requestClose = () => {
    if (savingRef.current) return;
    if (isDirty()) {
      setConfirmDiscard(true);
      return;
    }
    onClose();
  };

  // ─── 分类徽章（轻量复判：与后端 classify_entry_kind 同规则）───
  const kindOf = (raw: string) => {
    if (raw.startsWith("!")) return "exclude" as const;
    if (raw.startsWith("+")) return "forceInclude" as const;
    if (raw.startsWith("-")) return "forceExclude" as const;
    if (raw.includes("*") || raw.includes("?"))
      return "includeWhitelist" as const;
    if (
      ["npm:", "git:", "github:", "http:", "https:", "ssh:", "builtin:"].some(
        (s) => raw.startsWith(s),
      )
    )
      return "packageScheme" as const;
    return "scanPath" as const;
  };
  const warningsOf = (raw: string): string[] => {
    const kind = kindOf(raw);
    const stripped =
      kind === "scanPath" || kind === "packageScheme" ? raw : raw.slice(1);
    const isPattern = kind !== "scanPath" && kind !== "packageScheme";
    const w: string[] = [];
    if (isPattern && stripped.startsWith("~")) {
      w.push(t("skillsPage.piConfig.warnDeadPattern"));
    }
    if (!isPattern && (raw.includes("*") || raw.includes("?"))) {
      w.push(t("skillsPage.piConfig.warnLiteralWildcard"));
    }
    return w;
  };
  const kindLabel = (kind: ReturnType<typeof kindOf>) =>
    t(`skillsPage.piConfig.kind.${kind}`);

  // ─── 表单操作 ───
  const addEntry = (raw: string) => {
    const trimmed = raw.trim();
    if (!trimmed) return;
    setEntries((prev) => [...prev, trimmed]);
  };
  const removeEntry = (index: number) => {
    setEntries((prev) => prev.filter((_, i) => i !== index));
  };

  // ─── JSON ⇄ 表单 切换（仿 McpFormModal.switchTab）───
  const switchTab = (next: "form" | "json") => {
    if (next === tab) return;
    if (next === "json") {
      setJsonText(JSON.stringify(entries, null, 2));
      setJsonError(null);
      setTab("json");
      return;
    }
    const parsed = parseJsonArray(jsonText);
    if (!parsed.ok) {
      setJsonError(parsed.error);
      return;
    }
    setEntries(parsed.value);
    setJsonError(null);
    setTab("form");
  };
  const handleJsonChange = (text: string) => {
    setJsonText(text);
    const parsed = parseJsonArray(text);
    setJsonError(parsed.ok ? null : parsed.error);
  };

  // ─── 保存 ───
  const handleSave = async () => {
    let finalEntries = entries;
    if (tab === "json") {
      const parsed = parseJsonArray(jsonText);
      if (!parsed.ok) {
        setJsonError(parsed.error);
        toast.error(parsed.error);
        return;
      }
      finalEntries = parsed.value;
    }
    const bad = finalEntries.find((e) => !e.trim());
    if (bad !== undefined) {
      toast.error(t("skillsPage.piConfig.emptyEntryRejected"));
      return;
    }
    savingRef.current = true;
    setSaving(true);
    try {
      await skillsApi.setPiSkillsEntries(finalEntries, config.revision);
      toast.success(t("skillsPage.piConfig.saved"));
      onSaved();
      onClose();
    } catch (e) {
      const msg = String(e);
      if (msg.includes("外部修改") || msg.includes("外部创建")) {
        setRevisionStale(true);
        toast.error(t("skillsPage.piConfig.revisionConflict"));
      } else {
        toast.error(msg);
      }
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  };

  const handleRefresh = () => {
    if (window.confirm(t("skillsPage.piConfig.refreshConfirm"))) {
      load();
    }
  };

  const kindBadgeClass = (kind: ReturnType<typeof kindOf>) =>
    cn(
      "inline-flex shrink-0 items-center whitespace-nowrap rounded-full border border-transparent px-1.5 text-badge font-medium",
      kind === "exclude" && "bg-warning-soft text-warning-text",
      kind === "forceInclude" && "bg-success-soft text-success-text",
      kind === "forceExclude" && "bg-danger-soft text-danger-text",
      kind === "includeWhitelist" && "bg-subtle text-fg-2",
      kind === "packageScheme" && "bg-subtle text-fg-3",
      kind === "scanPath" && "bg-action-soft text-action-text",
    );

  return (
    <Sheet
      open
      modal={false}
      onOpenChange={(open) => {
        if (!open) requestClose();
      }}
    >
      <SheetPageContent
        title={
          <span className="flex min-w-0 items-center gap-2">
            {t("skillsPage.piConfig.title")}
            <span
              title={config.path}
              className="min-w-0 shrink truncate font-mono text-caption font-normal text-fg-3"
            >
              {config.path}
            </span>
          </span>
        }
        closeLabel={t("common.back")}
        onEscapeKeyDown={(event) => {
          // 保存中或有未保存修改时拦截 Esc，走确认流程
          if (savingRef.current || isDirty()) event.preventDefault();
          if (!savingRef.current && isDirty()) setConfirmDiscard(true);
        }}
      >
        <SheetDescription className="sr-only">
          {t("skillsPage.piConfig.description")}
        </SheetDescription>

        {loading ? (
          <div className="flex flex-1 items-center justify-center">
            <Loader2 className="h-5 w-5 animate-spin text-fg-3" />
          </div>
        ) : parseError ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-3 px-6 text-center">
            <p className="text-body text-danger-text">{parseError}</p>
            <p className="text-caption text-fg-2">
              {t("skillsPage.piConfig.parseErrorHint")}
            </p>
            <Button
              type="button"
              variant="neutral"
              size="regular"
              onClick={() => void handleRefresh()}
            >
              <RefreshCw className="h-4 w-4" strokeWidth={1.5} />
              {t("skillsPage.piConfig.refresh")}
            </Button>
          </div>
        ) : (
          <>
            {revisionStale && (
              <div className="flex items-center gap-2 border-b border-border bg-warning-soft px-6 py-2 text-caption text-warning-text">
                <span className="flex-1">
                  {t("skillsPage.piConfig.revisionConflictBanner")}
                </span>
                <Button
                  type="button"
                  variant="neutral"
                  size="compact"
                  onClick={handleRefresh}
                >
                  {t("skillsPage.piConfig.refresh")}
                </Button>
              </div>
            )}

            <div className="grid shrink-0 grid-cols-[1fr_auto]">
              <p className="px-6 pt-3 text-caption text-fg-3">
                {t("skillsPage.piConfig.agentsDirHint")}
              </p>
              <div className="flex items-center justify-end px-6 pt-2">
                <SegmentedControl
                  aria-label={t("skillsPage.piConfig.editMode")}
                  className="h-8 rounded-[8px]"
                  value={tab}
                  onValueChange={switchTab}
                  items={[
                    {
                      value: "form",
                      label: t("skillsPage.piConfig.tabForm"),
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
            </div>

            {tab === "form" ? (
              <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto scroll-stable overscroll-contain px-6 pb-6 pt-2">
                {entries.length === 0 && (
                  <p className="text-caption text-fg-3">
                    {t("skillsPage.piConfig.emptyHint")}
                  </p>
                )}
                <ul className="m-0 flex list-none flex-col gap-1.5 p-0">
                  {entries.map((raw, index) => {
                    const kind = kindOf(raw);
                    const warnings = warningsOf(raw);
                    return (
                      <li
                        key={`${index}-${raw}`}
                        className="flex items-center gap-2 rounded-control border border-border px-3 py-2"
                      >
                        <span
                          className={cn(
                            "min-w-0 flex-1 truncate font-mono text-caption",
                            warnings.length > 0 && "text-warning-text",
                          )}
                          title={warnings.join("\n") || raw}
                        >
                          {raw}
                        </span>
                        <span className={kindBadgeClass(kind)}>
                          {kindLabel(kind)}
                        </span>
                        {warnings.length > 0 && (
                          <span
                            title={warnings.join("\n")}
                            className="shrink-0 text-caption text-warning-text"
                          >
                            ⚠
                          </span>
                        )}
                        <Button
                          type="button"
                          variant="quiet"
                          size="icon-compact"
                          aria-label={t("skillsPage.piConfig.removeEntry", {
                            entry: raw,
                          })}
                          disabled={saving}
                          onClick={() => removeEntry(index)}
                        >
                          <X className="h-3.5 w-3.5" strokeWidth={1.5} />
                        </Button>
                      </li>
                    );
                  })}
                </ul>

                {/* 添加行：输入 + 实时分类预览 */}
                <div className="flex shrink-0 items-center gap-2">
                  <input
                    className="h-8 min-w-0 flex-1 rounded-control border border-border-strong bg-surface px-3 text-caption outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    value={addInput}
                    onChange={(e) => setAddInput(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") {
                        e.preventDefault();
                        addEntry(addInput);
                        setAddInput("");
                      }
                    }}
                    placeholder={t("skillsPage.piConfig.addPlaceholder")}
                    aria-label={t("skillsPage.piConfig.addPlaceholder")}
                    disabled={saving}
                  />
                  {addInput.trim() && (
                    <span className={kindBadgeClass(kindOf(addInput.trim()))}>
                      {t("skillsPage.piConfig.willBe")}{" "}
                      {kindLabel(kindOf(addInput.trim()))}
                    </span>
                  )}
                  <Button
                    type="button"
                    variant="neutral"
                    size="regular"
                    disabled={!addInput.trim() || saving}
                    onClick={() => {
                      addEntry(addInput);
                      setAddInput("");
                    }}
                  >
                    <Plus className="h-4 w-4" strokeWidth={1.5} />
                    {t("common.add")}
                  </Button>
                </div>

                {/* 复选来源弹层 */}
                <div className="relative shrink-0">
                  <Button
                    type="button"
                    variant="neutral"
                    size="regular"
                    disabled={saving}
                    onClick={() => setSourceOpen((v) => !v)}
                  >
                    <Plus className="h-4 w-4" strokeWidth={1.5} />
                    {t("skillsPage.piConfig.addFromApps")}
                  </Button>
                  {sourceOpen && (
                    <>
                      {/* 点击弹层以外任意处关闭 */}
                      <div
                        className="fixed inset-0 z-[5]"
                        onClick={() => setSourceOpen(false)}
                      />
                      <div className="absolute z-10 mt-1 w-full rounded-control border border-border bg-surface p-2 shadow-v7-md">
                        {config.sources.length === 0 ? (
                          <p className="px-2 py-1 text-caption text-fg-3">
                            {t("skillsPage.piConfig.noSources")}
                          </p>
                        ) : (
                          <ul className="m-0 list-none p-0">
                            {config.sources.map((s) => (
                              <li
                                key={s.app}
                                className="flex items-center gap-2 px-2 py-1.5"
                              >
                                <input
                                  type="checkbox"
                                  className="ui-checkbox"
                                  checked={
                                    entries.includes(s.path) ||
                                    (s.listedEntry != null &&
                                      entries.includes(s.listedEntry))
                                  }
                                  disabled={saving}
                                  onChange={(e) => {
                                    if (e.target.checked) {
                                      addEntry(s.path);
                                    } else {
                                      // 取消勾选 = 移除（精确匹配该目录的原条目）
                                      setEntries((prev) =>
                                        prev.filter(
                                          (entry) =>
                                            entry !== s.path &&
                                            entry !== s.listedEntry,
                                        ),
                                      );
                                    }
                                  }}
                                  aria-label={`${APP_DISPLAY_NAME[s.app as AppId] ?? s.app}: ${s.path}`}
                                />
                                <span className="min-w-0 flex-1">
                                  <span className="text-caption font-medium">
                                    {APP_DISPLAY_NAME[s.app as AppId] ?? s.app}
                                  </span>
                                  <span
                                    className="ms-1.5 truncate font-mono text-caption text-fg-3"
                                    title={s.path}
                                  >
                                    {s.path}
                                  </span>
                                </span>
                                {!s.exists && (
                                  <span
                                    className="shrink-0 text-caption text-fg-3"
                                    title={t(
                                      "skillsPage.piConfig.dirNotExistsHint",
                                    )}
                                  >
                                    {t("skillsPage.piConfig.dirNotExists")}
                                  </span>
                                )}
                              </li>
                            ))}
                          </ul>
                        )}
                      </div>
                    </>
                  )}
                </div>
              </div>
            ) : (
              <div className="flex min-h-0 flex-1 flex-col gap-1.5 px-6 pb-6 pt-2">
                <textarea
                  value={jsonText}
                  onChange={(e) => handleJsonChange(e.target.value)}
                  disabled={saving}
                  spellCheck={false}
                  aria-label={t("skillsPage.piConfig.jsonLabel")}
                  aria-invalid={jsonError !== null}
                  className="min-h-[240px] flex-1 resize-none rounded-control border border-border-strong bg-surface px-3 py-2.5 font-mono text-caption leading-[18px] outline-none focus-visible:ring-2 focus-visible:ring-ring"
                  placeholder={'["~/.claude/skills", "!pdf"]'}
                />
                {jsonError && (
                  <p className="text-caption text-danger-text">{jsonError}</p>
                )}
                <p className="text-caption text-fg-3">
                  {t("skillsPage.piConfig.jsonScopeHint")}
                </p>
              </div>
            )}

            <div className="flex h-14 shrink-0 items-center gap-2 border-t border-border px-6">
              <p className="flex-1 text-caption text-fg-3">
                {t("skillsPage.piConfig.reloadHint")}
              </p>
              <Button
                type="button"
                variant="neutral"
                size="regular"
                onClick={requestClose}
                disabled={saving}
              >
                {t("common.cancel")}
              </Button>
              <Button
                type="button"
                variant="solid"
                size="regular"
                onClick={() => void handleSave()}
                disabled={saving || (tab === "json" && jsonError !== null)}
              >
                {saving ? t("common.saving") : t("common.save")}
              </Button>
            </div>
          </>
        )}
      </SheetPageContent>

      <ConfirmDialog
        isOpen={confirmDiscard}
        title={t("common.unsavedLeaveTitle")}
        message={t("common.unsavedLeaveMessage")}
        confirmText={t("common.unsavedLeaveConfirm")}
        cancelText={t("common.unsavedLeaveCancel")}
        zIndex="top"
        onConfirm={() => {
          setConfirmDiscard(false);
          onClose();
        }}
        onCancel={() => setConfirmDiscard(false)}
      />
    </Sheet>
  );
}

/** 解析 JSON 数组文本：必须为字符串数组 */
function parseJsonArray(
  text: string,
): { ok: true; value: string[] } | { ok: false; error: string } {
  try {
    const parsed = JSON.parse(text);
    if (!Array.isArray(parsed)) {
      return {
        ok: false,
        error: '必须是 JSON 数组（如 ["~/.claude/skills"]）',
      };
    }
    for (const item of parsed) {
      if (typeof item !== "string") {
        return { ok: false, error: "数组元素必须全部是字符串" };
      }
    }
    return { ok: true, value: parsed };
  } catch {
    return { ok: false, error: "JSON 解析失败" };
  }
}
