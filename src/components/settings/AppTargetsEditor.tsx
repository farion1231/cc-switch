import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import { FolderSearch, Loader2, RefreshCw, Undo2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { HoverTip } from "@/components/ui/hover-tip";
import { HelpTip } from "@/components/ui/help-tip";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { settingsApi } from "@/lib/api/settings";
import {
  appTargetsApi,
  enabledTargetKeys,
  sameTarget,
  distroOf,
  installOf,
  wslCandidates,
  WINDOWS_TARGET,
  wslTargetKey,
  type AppTargetAppId,
  type AppTargetsConfig,
  type AppTargetsOverview,
  type AppTargetsView,
  type FieldError,
  type Resolution,
  type SaveResult,
  type TargetConflict,
  type TargetKey,
  type WslInventory,
} from "@/lib/api/appTargets";
import {
  reasonLabel,
  statusLabel,
  targetLabel,
  useInvalidateAppTargets,
} from "@/hooks/useAppTargets";
import { cn } from "@/lib/utils";

/** 已保存配置 + 本次检测到、还不在配置里的发行版（未勾选）。 */
/** 已保存配置 + 检测到装了这个客户端、还不在配置里的发行版（未勾选）。没装的不列。 */
function withDetectedDistros(
  config: AppTargetsConfig,
  inventory: WslInventory,
  installs: AppTargetsOverview["installs"],
  app: string,
): AppTargetsConfig {
  const wsl = [...config.wsl];
  for (const distro of wslCandidates(config, { inventory, installs }, app)) {
    if (!wsl.some((t) => t.distro.toLowerCase() === distro.toLowerCase())) {
      wsl.push({ distro, enabled: false, dir: null });
    }
  }
  return { ...config, wsl };
}

const clean = (dir?: string | null) => (dir && dir.trim() ? dir.trim() : null);

/** 主环境不再由用户选（与后端 derived_primary 一致）：勾选了 Windows 就是 Windows，
 * 否则保留原来的 WSL 主环境，它也取消了就用第一个勾选的环境。 */
export function derivePrimary(config: AppTargetsConfig): TargetKey {
  if (config.windows.enabled) return WINDOWS_TARGET;
  const enabled = enabledTargetKeys(config);
  return (
    enabled.find((key) => sameTarget(key, config.primary)) ??
    enabled[0] ??
    config.primary
  );
}

const comparable = (config: AppTargetsConfig) =>
  JSON.stringify({
    windows: {
      enabled: config.windows.enabled,
      dir: clean(config.windows.dir),
    },
    wsl: config.wsl.map((t) => ({
      distro: t.distro,
      enabled: t.enabled,
      dir: clean(t.dir),
    })),
  });

/** 保存时只带真正要保存的东西：未勾选、没有自定义目录、也不在原配置里的检测行不存。 */
function forSave(
  draft: AppTargetsConfig,
  saved: AppTargetsConfig,
): AppTargetsConfig {
  return {
    ...draft,
    windows: {
      enabled: draft.windows.enabled,
      dir: clean(draft.windows.dir),
      route: draft.windows.route,
    },
    wsl: draft.wsl
      .filter(
        (t) =>
          t.enabled ||
          clean(t.dir) ||
          saved.wsl.some(
            (s) => s.distro.toLowerCase() === t.distro.toLowerCase(),
          ),
      )
      .map((t) => ({
        distro: t.distro,
        enabled: t.enabled,
        dir: clean(t.dir),
        route: t.route,
      })),
  };
}

interface ConflictPrompt {
  conflicts: TargetConflict[];
  /** 保存后的主环境：它的冲突只能「导入」或「覆盖」（主环境总要有一家供应商） */
  primary: TargetKey;
  index: number;
  choices: Record<TargetKey, Resolution>;
  current: Resolution;
  resolve: (choices: Record<TargetKey, Resolution> | null) => void;
}

interface AppTargetsEditorProps {
  app: AppTargetAppId;
  appName: string;
  view: AppTargetsView;
  inventory: WslInventory;
  /** 各发行版里客户端的安装情况 */
  installs: AppTargetsOverview["installs"];
  detecting: boolean;
  onDetect: () => void;
  /** 本节的标题区（名称 + 保存 / 撤销按钮由这里渲染） */
  renderBlock: (
    actions: React.ReactNode,
    body: React.ReactNode,
  ) => React.ReactNode;
}

/**
 * 设置 → 应用配置 → 配置目录（多环境）：Windows 和每个 WSL 发行版一行，勾选写入目标，
 * 每个环境在供应商页各自启用供应商。每个应用独立保存 / 撤销，草稿只在本节。
 */
export function AppTargetsEditor({
  app,
  appName,
  view,
  inventory,
  installs,
  detecting,
  onDetect,
  renderBlock,
}: AppTargetsEditorProps) {
  const { t } = useTranslation();
  const invalidate = useInvalidateAppTargets();
  const base = useMemo(
    () => withDetectedDistros(view.config, inventory, installs, app),
    [view.config, inventory, installs, app],
  );
  const [draft, setDraft] = useState<AppTargetsConfig>(base);
  const [errors, setErrors] = useState<FieldError[]>([]);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [prompt, setPrompt] = useState<ConflictPrompt | null>(null);
  const baseKey = comparable(base);
  const lastBase = useRef(baseKey);

  // 已保存配置变了（保存成功、重新检测）时重置草稿；草稿有改动时保留改动。
  useEffect(() => {
    if (lastBase.current === baseKey) return;
    setDraft((current) =>
      comparable(current) === lastBase.current ? base : current,
    );
    lastBase.current = baseKey;
  }, [base, baseKey]);

  const dirty = comparable(draft) !== baseKey;
  const enabled = enabledTargetKeys(draft);

  const rows: { key: TargetKey; enabled: boolean; dir: string | null }[] = [
    {
      key: WINDOWS_TARGET,
      enabled: draft.windows.enabled,
      dir: draft.windows.dir ?? null,
    },
    ...draft.wsl.map((target) => ({
      key: wslTargetKey(target.distro),
      enabled: target.enabled,
      dir: target.dir ?? null,
    })),
  ];

  const update = (
    key: TargetKey,
    patch: { enabled?: boolean; dir?: string | null },
  ) => {
    setErrors([]);
    setDraft((current) => {
      const next: AppTargetsConfig = {
        ...current,
        windows: { ...current.windows },
        wsl: current.wsl.map((t) => ({ ...t })),
      };
      if (key === WINDOWS_TARGET) {
        Object.assign(next.windows, patch);
      } else {
        const target = next.wsl.find((t) =>
          sameTarget(wslTargetKey(t.distro), key),
        );
        if (target) Object.assign(target, patch);
      }
      return next;
    });
  };

  const toggle = (key: TargetKey, checked: boolean) => {
    if (!checked && enabled.length === 1 && sameTarget(enabled[0], key)) {
      setNotice(t("appTargets.keepOne"));
      return;
    }
    setNotice(null);
    let next = {
      ...draft,
      windows: { ...draft.windows },
      wsl: draft.wsl.map((x) => ({ ...x })),
    };
    if (key === WINDOWS_TARGET) next.windows.enabled = checked;
    else {
      const target = next.wsl.find((x) =>
        sameTarget(wslTargetKey(x.distro), key),
      );
      if (target) target.enabled = checked;
    }
    next = { ...next, primary: derivePrimary(next) };
    setErrors([]);
    setDraft(next);
  };

  const browse = async (key: TargetKey, current: string | null) => {
    const picked = await settingsApi.selectConfigDirectory(
      current ?? view.resolvedDirs[key] ?? undefined,
    );
    if (picked) update(key, { dir: picked });
  };

  const firstChoice = (conflict: TargetConflict, primary: TargetKey) =>
    sameTarget(conflict.target, primary) ? "import" : "defer";

  const askConflicts = (conflicts: TargetConflict[], primary: TargetKey) =>
    new Promise<Record<TargetKey, Resolution> | null>((resolve) => {
      setPrompt({
        conflicts,
        primary,
        index: 0,
        choices: {},
        current: firstChoice(conflicts[0], primary),
        resolve,
      });
    });

  const report = (result: SaveResult) => {
    const failed = result.targets.filter((r) => r.status === "error");
    const deferred = result.targets.filter((r) => r.status === "deferred");
    const written = result.targets.filter((r) => r.status === "ok");
    if (result.primary?.status === "error") {
      toast.error(t("appTargets.toast.primaryFailed"), {
        description: result.primary.detail,
        duration: 8000,
      });
    } else if (failed.length) {
      toast.warning(
        t("appTargets.toast.partial", {
          targets: failed.map((r) => targetLabel(r.target)).join("、"),
        }),
        { description: failed[0].detail, duration: 8000 },
      );
    } else if (deferred.length) {
      toast.warning(t("appTargets.toast.deferred"), { duration: 6000 });
    } else if (written.length || result.primary) {
      const keys = [
        ...(result.primary ? [result.primary.target] : []),
        ...written.map((r) => r.target),
      ];
      toast.success(
        t("appTargets.toast.savedAndSynced", {
          targets: keys.map(targetLabel).join("、"),
        }),
      );
    } else {
      toast.success(t("appTargets.toast.saved"));
    }
    const skipped = [
      ...result.targets,
      ...(result.primary ? [result.primary] : []),
    ]
      .flatMap((r) => r.warnings ?? [])
      .filter((w) => w.startsWith("mcp_skipped_windows_only:"));
    if (skipped.length) {
      toast.info(
        t("appTargets.toast.mcpSkipped", {
          servers: skipped.map((w) => w.split(":")[1]).join("、"),
        }),
        { duration: 8000 },
      );
    }
  };

  const save = async () => {
    if (busy || !dirty) return;
    setBusy(true);
    setNotice(null);
    const payload = forSave(draft, view.config);
    try {
      const inspected = await appTargetsApi.inspect(app, payload);
      if (inspected.errors.length) {
        setErrors(inspected.errors);
        toast.error(t("appTargets.toast.fixErrors"));
        return;
      }
      if (inspected.blocked) {
        toast.warning(t(`appTargets.blocked.${inspected.blocked}`), {
          duration: 8000,
        });
        return;
      }
      let resolutions: Record<TargetKey, Resolution> = {};
      if (inspected.conflicts.length) {
        const chosen = await askConflicts(
          inspected.conflicts,
          inspected.primaryTarget,
        );
        if (!chosen) {
          toast.info(t("appTargets.toast.cancelled"));
          return;
        }
        resolutions = chosen;
      }
      const result = await appTargetsApi.save(app, payload, resolutions);
      if (!result.saved) {
        if (result.errors.length) setErrors(result.errors);
        if (result.blocked) {
          toast.warning(t(`appTargets.blocked.${result.blocked}`));
        } else if (result.needsResolution) {
          toast.warning(t("appTargets.toast.conflictChanged"));
        } else {
          toast.error(t("appTargets.toast.fixErrors"));
        }
        return;
      }
      setErrors([]);
      report(result);
      lastBase.current = comparable(draft);
      await invalidate();
    } catch (error) {
      toast.error(t("appTargets.toast.saveFailed"), {
        description: String(error),
      });
    } finally {
      setBusy(false);
    }
  };

  const undo = () => {
    setDraft(base);
    setErrors([]);
    setNotice(null);
  };

  const errorFor = (key: TargetKey) =>
    errors.find((e) => e.target && sameTarget(e.target, key));
  const generalErrors = errors.filter((e) => !e.target);

  const envFor = (key: TargetKey) =>
    inventory.distros.find((d) => sameTarget(wslTargetKey(d.distro), key));

  const detection = (key: TargetKey, rowEnabled: boolean) => {
    if (key === WINDOWS_TARGET) return null;
    const env = envFor(key);
    if (!env) {
      return {
        tone: "bad",
        text: t("appTargets.env.unavailable"),
      };
    }
    const parts = [
      env.running ? t("appTargets.env.running") : t("appTargets.env.stopped"),
    ];
    if (!env.home && !view.resolvedDirs[key])
      parts.push(t("appTargets.env.noHome"));
    else if (env.cached) parts.push(t("appTargets.env.cached"));
    if (rowEnabled && !env.running) parts.push(t("appTargets.env.willStart"));
    if (env.error) parts.push(t("appTargets.env.probeFailed"));
    const distro = distroOf(key);
    const install = distro ? installOf(installs, distro, app) : undefined;
    if (install?.status === "installed") {
      parts.push(
        t("appTargets.env.installed", {
          app: appName,
          version: install.version ?? "",
        }),
      );
    } else if (install?.status === "broken") {
      parts.push(t("appTargets.env.broken", { app: appName }));
    } else if (install?.status === "notInstalled") {
      parts.push(t("appTargets.env.notInstalled", { app: appName }));
    } else {
      parts.push(t("appTargets.env.installUnknown", { app: appName }));
    }
    return { tone: env.running ? "ok" : "off", text: parts.join(" · ") };
  };

  const summary =
    enabled.length === 1
      ? t("appTargets.summaryOne", { target: targetLabel(enabled[0]) })
      : t("appTargets.summaryMany", {
          targets: enabled.map(targetLabel).join("、"),
        });

  const actions = (
    <>
      <span className="text-caption text-fg-2">
        {dirty ? t("appTargets.unsaved") : t("appTargets.saved")}
      </span>
      <Button
        variant="neutral"
        size="compact"
        disabled={!dirty || busy}
        onClick={undo}
      >
        {t("appTargets.undo")}
      </Button>
      <Button
        variant="solid"
        size="compact"
        disabled={!dirty || busy}
        onClick={() => void save()}
      >
        {busy && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
        {busy ? t("appTargets.syncing") : t("common.save")}
      </Button>
    </>
  );

  const conflict = prompt?.conflicts[prompt.index];

  const body = (
    <div className="px-4 py-3">
      <div className="flex min-h-[28px] items-center gap-4">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1">
            <span className="text-body font-medium text-fg-1">
              {t("settings.appConfig.configDir")}
            </span>
            <HelpTip title={t("settings.appConfig.configDir")}>
              {t("appTargets.help")}
            </HelpTip>
          </div>
          <div className="mt-0.5 text-caption text-fg-2">{summary}</div>
        </div>
        <Button
          variant="quiet"
          size="compact"
          disabled={detecting}
          onClick={onDetect}
        >
          <RefreshCw
            className={cn("h-3.5 w-3.5", detecting && "animate-spin")}
          />
          {detecting ? t("appTargets.detecting") : t("appTargets.detect")}
        </Button>
      </div>

      <div className="mt-2.5 space-y-2">
        {rows.map((row) => {
          const error = errorFor(row.key);
          const info = detection(row.key, row.enabled);
          const record = view.records[row.key];
          const isPrimary = sameTarget(row.key, base.primary) && row.enabled;
          const auto = view.resolvedDirs[row.key];
          return (
            <div
              key={row.key}
              className={cn(
                "rounded-control border border-border px-2.5 py-2",
                !row.enabled && "bg-subtle/40",
                error && "border-danger",
              )}
            >
              <div className="flex items-center gap-2">
                <Checkbox
                  checked={row.enabled}
                  disabled={busy}
                  aria-label={t("appTargets.writeTo", {
                    app: appName,
                    target: targetLabel(row.key),
                  })}
                  onCheckedChange={(checked) => toggle(row.key, checked)}
                />
                <HoverTip content={targetLabel(row.key)}>
                  <span className="inline-flex h-[18px] w-[124px] shrink-0 items-center rounded-full border border-border-strong px-1.5 text-badge text-fg-2">
                    <span className="min-w-0 truncate">
                      {targetLabel(row.key)}
                    </span>
                  </span>
                </HoverTip>
                <HoverTip content={row.dir ?? auto} disableHoverableContent>
                  <div className="relative min-w-0 flex-1">
                    <Input
                      value={row.dir ?? ""}
                      placeholder={
                        auto ??
                        (row.key === WINDOWS_TARGET
                          ? t("appTargets.placeholderDefault")
                          : t("appTargets.placeholderUnc"))
                      }
                      disabled={busy}
                      aria-invalid={Boolean(error)}
                      className="pe-14 text-xs"
                      onChange={(event) =>
                        update(row.key, { dir: event.target.value || null })
                      }
                    />
                    <span className="pointer-events-none absolute end-2 top-1/2 -translate-y-1/2 text-badge text-fg-3">
                      {row.dir
                        ? t("appTargets.tag.custom")
                        : row.key === WINDOWS_TARGET
                          ? t("appTargets.tag.default")
                          : t("appTargets.tag.auto")}
                    </span>
                  </div>
                </HoverTip>
                <HoverTip content={t("settings.browseDirectory")}>
                  <Button
                    type="button"
                    variant="neutral"
                    size="icon-compact"
                    disabled={busy}
                    aria-label={t("settings.browseDirectory")}
                    onClick={() => void browse(row.key, row.dir)}
                  >
                    <FolderSearch className="h-4 w-4" />
                  </Button>
                </HoverTip>
                <HoverTip content={t("settings.resetDefault")}>
                  <Button
                    type="button"
                    variant="neutral"
                    size="icon-compact"
                    disabled={busy || !row.dir}
                    aria-label={t("settings.resetDefault")}
                    onClick={() => update(row.key, { dir: null })}
                  >
                    <Undo2 className="h-4 w-4" />
                  </Button>
                </HoverTip>
              </div>
              {(info || record) && (
                <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-0.5 ps-7 text-caption text-fg-2">
                  {info && (
                    <span className="inline-flex items-center gap-1.5">
                      <Dot tone={info.tone} />
                      {info.text}
                    </span>
                  )}
                  {!isPrimary && row.enabled && record && (
                    <span className="inline-flex items-center gap-1.5">
                      <Dot tone={toneOf(record.status)} />
                      {statusLabel(t, record.status)}
                      {record.reason
                        ? ` · ${reasonLabel(t, record.reason)}`
                        : ""}
                    </span>
                  )}
                </div>
              )}
              {error && (
                <div className="mt-1 ps-7 text-caption text-danger-text">
                  {t(`appTargets.errors.${error.code}`, {
                    distro: targetLabel(row.key),
                  })}
                </div>
              )}
            </div>
          );
        })}
      </div>

      {view.source === "legacy" && view.config.primary !== WINDOWS_TARGET && (
        <p className="mt-3 text-caption text-fg-2">
          {t("appTargets.legacyWsl")}
        </p>
      )}
      {view.source === "legacyUnresolved" && (
        <p className="mt-3 text-caption text-warning-text">
          {t("appTargets.legacyUnresolved")}
        </p>
      )}
      {notice && (
        <p className="mt-2 text-caption text-warning-text">{notice}</p>
      )}
      {generalErrors.map((error) => (
        <p key={error.code} className="mt-2 text-caption text-danger-text">
          {t(`appTargets.errors.${error.code}`)}
        </p>
      ))}

      <Dialog
        open={Boolean(prompt)}
        onOpenChange={(open) => {
          if (!open && prompt) {
            prompt.resolve(null);
            setPrompt(null);
          }
        }}
      >
        {prompt && conflict && (
          <DialogContent className="max-w-lg" zIndex="top">
            <DialogHeader>
              <DialogTitle>
                {t("appTargets.conflict.title", {
                  target: targetLabel(conflict.target),
                  app: appName,
                })}
              </DialogTitle>
            </DialogHeader>
            <div className="space-y-3 px-6 py-4">
              <DialogDescription>
                {t("appTargets.conflict.body", {
                  existing:
                    conflict.existingProviderName ??
                    (conflict.existingHost
                      ? t("appTargets.conflict.unknownWithHost", {
                          host: conflict.existingHost,
                        })
                      : t("appTargets.conflict.unknown")),
                  incoming: conflict.incomingProviderName ?? "—",
                })}
              </DialogDescription>
              <code className="block break-all rounded-control bg-subtle px-2 py-1 text-caption">
                {conflict.dir}
              </code>
              <p className="text-caption text-fg-2">
                {t("appTargets.conflict.progress", {
                  index: prompt.index + 1,
                  total: prompt.conflicts.length,
                })}
              </p>
              {(sameTarget(conflict.target, prompt.primary)
                ? (["import", "overwrite"] as Resolution[])
                : (["defer", "overwrite", "import"] as Resolution[])
              ).map((option) => (
                <label
                  key={option}
                  className={cn(
                    "flex cursor-pointer gap-2 rounded-control border border-border p-2.5",
                    prompt.current === option &&
                      "border-border-strong bg-subtle",
                  )}
                >
                  <input
                    type="radio"
                    name="app-target-resolution"
                    checked={prompt.current === option}
                    onChange={() => setPrompt({ ...prompt, current: option })}
                  />
                  <span>
                    <span className="block text-body font-medium text-fg-1">
                      {t(`appTargets.conflict.${option}`)}
                    </span>
                    <span className="block text-caption text-fg-2">
                      {t(`appTargets.conflict.${option}Hint`, {
                        existing:
                          conflict.existingProviderName ??
                          conflict.existingHost ??
                          t("appTargets.conflict.unknown"),
                      })}
                    </span>
                  </span>
                </label>
              ))}
            </div>
            <DialogFooter>
              <Button
                variant="neutral"
                onClick={() => {
                  prompt.resolve(null);
                  setPrompt(null);
                }}
              >
                {t("common.cancel")}
              </Button>
              <Button
                variant="solid"
                onClick={() => {
                  const choices = {
                    ...prompt.choices,
                    [conflict.target]: prompt.current,
                  };
                  if (prompt.index + 1 >= prompt.conflicts.length) {
                    prompt.resolve(choices);
                    setPrompt(null);
                  } else {
                    setPrompt({
                      ...prompt,
                      index: prompt.index + 1,
                      choices,
                      current: firstChoice(
                        prompt.conflicts[prompt.index + 1],
                        prompt.primary,
                      ),
                    });
                  }
                }}
              >
                {prompt.index + 1 >= prompt.conflicts.length
                  ? t("appTargets.conflict.confirm")
                  : t("appTargets.conflict.next")}
              </Button>
            </DialogFooter>
          </DialogContent>
        )}
      </Dialog>
    </div>
  );

  return <>{renderBlock(actions, body)}</>;
}

export const toneOf = (status: string) =>
  status === "ok"
    ? "ok"
    : status === "error"
      ? "bad"
      : status === "skipped"
        ? "off"
        : "warn";

export function Dot({ tone }: { tone: string }) {
  return (
    <span
      aria-hidden
      className={cn(
        "inline-block h-1.5 w-1.5 shrink-0 rounded-full",
        tone === "ok" && "bg-success",
        tone === "bad" && "bg-danger",
        tone === "warn" && "bg-warning",
        tone === "off" && "bg-fg-3",
      )}
    />
  );
}
