import { useCallback, useEffect, useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import {
  AlertTriangle,
  ArrowRight,
  CheckCircle2,
  FolderInput,
  Info,
  Loader2,
  Plus,
  ShieldCheck,
  Trash2,
  XCircle,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import {
  claudeDesktopMigrationApi,
  type ClaudeDesktopMigrationApplyResult,
  type ClaudeDesktopMigrationAudit,
  type ClaudeDesktopMigrationPlan,
  type ClaudeDesktopMigrationRestoreResult,
  type ClaudeDesktopMigrationVerifyResult,
  type ComponentPlan,
  type MigrationRecord,
  type PathMapping,
} from "@/lib/api/claudeDesktopMigration";
import { extractErrorMessage } from "@/utils/errorUtils";

const COMPONENT_IDS = [
  "code",
  "cowork",
  "schedules",
  "projects",
  "artifacts",
] as const;
type ComponentId = (typeof COMPONENT_IDS)[number];

/** Explicit account-root choices sent alongside plan/apply/verify. */
type RootOverrideSelection = {
  sourceCodeRoot?: string;
  targetCodeRoot?: string;
  sourceCoworkRoot?: string;
  targetCoworkRoot?: string;
};

type Step = "audit" | "plan" | "confirm" | "applying" | "done";

function formatBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return "0 B";
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value >= 100 || unit === 0 ? 0 : 1)} ${units[unit]}`;
}

interface ClaudeDesktopMigrationDialogProps {
  isOpen: boolean;
  onClose: () => void;
}

/**
 * Opt-in 1P -> 3P Claude Desktop data migration.
 *
 * Flow: read-only audit -> read-only plan (with explicit consent gate) ->
 * apply (auto-backup + atomic install) -> verify / ledger-driven undo. The
 * source account is never written; existing 3P records are never overwritten.
 */
export function ClaudeDesktopMigrationDialog({
  isOpen,
  onClose,
}: ClaudeDesktopMigrationDialogProps) {
  const { t } = useTranslation();
  const [step, setStep] = useState<Step>("audit");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [audit, setAudit] = useState<ClaudeDesktopMigrationAudit | null>(null);
  const [plan, setPlan] = useState<ClaudeDesktopMigrationPlan | null>(null);
  const [pathMaps, setPathMaps] = useState<PathMapping[]>([]);
  const [selected, setSelected] = useState<ComponentId[]>([
    "code",
    "cowork",
    "schedules",
    "projects",
    "artifacts",
  ]);
  const [consent, setConsent] = useState(false);
  const [applyResult, setApplyResult] =
    useState<ClaudeDesktopMigrationApplyResult | null>(null);
  const [verifyResult, setVerifyResult] =
    useState<ClaudeDesktopMigrationVerifyResult | null>(null);
  const [restoreResult, setRestoreResult] =
    useState<ClaudeDesktopMigrationRestoreResult | null>(null);
  const [showUndo, setShowUndo] = useState(false);
  const [undoConsent, setUndoConsent] = useState(false);
  const [rootOverrides, setRootOverrides] = useState<RootOverrideSelection>({});
  const [appliedComponents, setAppliedComponents] = useState<string[]>([]);

  // Reset and run the read-only audit each time the dialog opens.
  useEffect(() => {
    if (!isOpen) return;
    setStep("audit");
    setBusy(true);
    setError("");
    setAudit(null);
    setPlan(null);
    setPathMaps([]);
    setConsent(false);
    setApplyResult(null);
    setVerifyResult(null);
    setRestoreResult(null);
    setShowUndo(false);
    setUndoConsent(false);
    setRootOverrides({});
    setAppliedComponents([]);
    setSelected(["code", "cowork", "schedules", "projects", "artifacts"]);
    claudeDesktopMigrationApi
      .audit()
      .then(setAudit)
      .catch((e) => setError(extractErrorMessage(e)))
      .finally(() => setBusy(false));
  }, [isOpen]);

  const buildPlan = useCallback(
    async (maps: PathMapping[], overrides?: RootOverrideSelection) => {
      setBusy(true);
      setError("");
      try {
        const result = await claudeDesktopMigrationApi.plan({
          pathMaps: maps,
          ...(overrides ?? rootOverrides),
        });
        setPlan(result);
        setStep("plan");
      } catch (e) {
        setError(extractErrorMessage(e));
      } finally {
        setBusy(false);
      }
    },
    [rootOverrides],
  );

  const runApply = useCallback(async () => {
    setStep("applying");
    setBusy(true);
    setError("");
    try {
      const result = await claudeDesktopMigrationApi.apply({
        pathMaps,
        components: selected,
        confirmed: true,
        ...rootOverrides,
      });
      setApplyResult(result);
      // Snapshot what actually ran so one-click verification checks only the
      // components that were applied (never the deselected ones).
      setAppliedComponents(selected);
      setStep("done");
    } catch (e) {
      setError(extractErrorMessage(e));
      setStep("confirm");
    } finally {
      setBusy(false);
    }
  }, [pathMaps, selected, rootOverrides]);

  const runVerify = useCallback(async () => {
    setBusy(true);
    setError("");
    try {
      const result = await claudeDesktopMigrationApi.verify(
        { pathMaps, ...rootOverrides },
        appliedComponents,
      );
      setVerifyResult(result);
    } catch (e) {
      setError(extractErrorMessage(e));
    } finally {
      setBusy(false);
    }
  }, [pathMaps, rootOverrides, appliedComponents]);

  const runRestore = useCallback(async () => {
    setBusy(true);
    setError("");
    try {
      // Target the exact ledger that owns this apply's writes. When a later
      // component failed, the newest ledger on disk may be an unrelated
      // skip-only retry; restoring must use the ledger that installed records.
      const result = await claudeDesktopMigrationApi.restore(
        applyResult?.ledgerPath,
      );
      setRestoreResult(result);
      setShowUndo(false);
    } catch (e) {
      setError(extractErrorMessage(e));
    } finally {
      setBusy(false);
    }
  }, [applyResult?.ledgerPath]);

  const toggleComponent = (id: ComponentId, checked: boolean) => {
    setSelected((prev) =>
      checked ? [...new Set([...prev, id])] : prev.filter((c) => c !== id),
    );
  };

  const updatePathMap = (
    index: number,
    field: keyof PathMapping,
    value: string,
  ) => {
    setPathMaps((prev) =>
      prev.map((m, i) => (i === index ? { ...m, [field]: value } : m)),
    );
  };

  const hasBlocking = (plan?.blockingIssues.length ?? 0) > 0;

  const renderStatusBadge = (status: string) => {
    switch (status) {
      case "ready":
        return (
          <Badge variant="default">
            {t("claudeDesktopMigration.statusReady")}
          </Badge>
        );
      case "source-empty":
        return (
          <Badge variant="secondary">
            {t("claudeDesktopMigration.statusSourceEmpty")}
          </Badge>
        );
      case "ambiguous-source":
        return (
          <Badge variant="destructive">
            {t("claudeDesktopMigration.statusAmbiguousSource")}
          </Badge>
        );
      case "ambiguous-target":
        return (
          <Badge variant="destructive">
            {t("claudeDesktopMigration.statusAmbiguousTarget")}
          </Badge>
        );
      case "missing-target-seed":
        return (
          <Badge variant="destructive">
            {t("claudeDesktopMigration.statusMissingTargetSeed")}
          </Badge>
        );
      default:
        return <Badge variant="outline">{status}</Badge>;
    }
  };

  const renderComponentPlan = (component: ComponentPlan) => (
    <div className="mt-2 grid grid-cols-2 gap-x-4 gap-y-1 text-xs text-muted-foreground">
      <span>{t("claudeDesktopMigration.planNewRecords")}</span>
      <span className="text-right font-medium text-foreground">
        {component.newRecords}
      </span>
      <span>{t("claudeDesktopMigration.planConflicts")}</span>
      <span className="text-right font-medium text-foreground">
        {component.conflicts}
      </span>
      {component.component === "cowork" && component.scheduledRecords > 0 && (
        <>
          <span>{t("claudeDesktopMigration.planScheduled")}</span>
          <span className="text-right font-medium text-foreground">
            {component.scheduledRecords}
          </span>
        </>
      )}
      {component.missingSessionDirectories > 0 && (
        <>
          <span>{t("claudeDesktopMigration.planMissingSessionDirs")}</span>
          <span className="text-right font-medium text-amber-600 dark:text-amber-400">
            {component.missingSessionDirectories}
          </span>
        </>
      )}
      {component.missingSharedTranscripts > 0 && (
        <>
          <span>{t("claudeDesktopMigration.planMissingTranscripts")}</span>
          <span className="text-right font-medium text-amber-600 dark:text-amber-400">
            {component.missingSharedTranscripts}
          </span>
        </>
      )}
    </div>
  );

  const renderFailedRecord = (record: MigrationRecord, index: number) => (
    <li key={index} className="text-xs">
      <span className="font-medium">{record.component}</span>
      {record.id ? (
        <span className="text-muted-foreground"> · {record.id}</span>
      ) : null}
      {record.error || record.reason ? (
        <span className="block text-muted-foreground">
          {record.error ?? record.reason}
        </span>
      ) : null}
    </li>
  );

  /** Candidate picker shown when account-root discovery is ambiguous. Picking a
   *  candidate pins it as an explicit override and rebuilds the plan. */
  const renderCandidatePicker = (
    component: ComponentPlan,
    overrideKey: keyof RootOverrideSelection,
  ) => {
    if (
      component.status !== "ambiguous-source" &&
      component.status !== "ambiguous-target"
    ) {
      return null;
    }
    return (
      <div className="mt-2 space-y-1.5 rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2">
        <div className="text-xs font-medium text-amber-900 dark:text-amber-200">
          {t("claudeDesktopMigration.ambiguousPickTitle")}
        </div>
        <p className="text-xs leading-relaxed text-amber-900/90 dark:text-amber-200/90">
          {t("claudeDesktopMigration.ambiguousPickDescription")}
        </p>
        {component.candidates.length > 0 ? (
          component.candidates.map((candidate) => {
            const checked = rootOverrides[overrideKey] === candidate;
            return (
              <label
                key={candidate}
                className="flex cursor-pointer select-none items-start gap-2 rounded border border-border/60 bg-background px-2 py-1.5 text-xs"
              >
                <input
                  type="radio"
                  name={`root-${overrideKey}`}
                  className="mt-0.5"
                  checked={checked}
                  onChange={() => {
                    const next = { ...rootOverrides, [overrideKey]: candidate };
                    setRootOverrides(next);
                    buildPlan(pathMaps, next);
                  }}
                />
                <span className="break-all font-mono">{candidate}</span>
              </label>
            );
          })
        ) : (
          <p className="text-xs text-amber-900/70 dark:text-amber-200/70">
            {t("claudeDesktopMigration.ambiguousPickAgain")}
          </p>
        )}
      </div>
    );
  };

  return (
    <Dialog open={isOpen} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-w-2xl" zIndex="nested">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2 text-lg font-semibold">
            <FolderInput className="h-5 w-5 text-blue-500" />
            {t("claudeDesktopMigration.dialogTitle")}
          </DialogTitle>
          <DialogDescription className="text-sm leading-relaxed">
            {t("claudeDesktopMigration.dialogSubtitle")}
          </DialogDescription>
        </DialogHeader>

        <div className="max-h-[62vh] space-y-4 overflow-y-auto px-6 py-2">
          {error ? (
            <div className="flex items-start gap-2 rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
              <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
              <span className="whitespace-pre-line leading-relaxed">
                {error}
              </span>
            </div>
          ) : null}

          {busy && step !== "applying" ? (
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <Loader2 className="h-4 w-4 animate-spin" />
              {t("claudeDesktopMigration.loading")}
            </div>
          ) : null}

          {/* -- Step: audit (read-only inventory) ------------------------- */}
          {step === "audit" && audit ? (
            <div className="space-y-4">
              {!audit.supported ? (
                <div className="flex items-start gap-2 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm text-amber-900 dark:text-amber-200">
                  <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
                  {t("claudeDesktopMigration.unsupportedDescription")}
                </div>
              ) : (
                <>
                  <div className="space-y-2 rounded-lg border border-border bg-muted/40 px-4 py-3 text-sm">
                    <AuditRow
                      label={t("claudeDesktopMigration.auditSourceRoot")}
                      value={audit.sourceApp.path}
                      missing={!audit.sourceApp.exists}
                      missingLabel={t("claudeDesktopMigration.auditNotFound")}
                    />
                    <AuditRow
                      label={t("claudeDesktopMigration.auditTargetRoot")}
                      value={audit.targetApp.path}
                      missing={!audit.targetApp.exists}
                      missingLabel={t("claudeDesktopMigration.auditNotFound")}
                    />
                    <AuditRow
                      label={t("claudeDesktopMigration.auditCodeSessions")}
                      value={String(
                        audit.sourceApp.codeRoots.reduce(
                          (n, r) => n + r.metadataCount,
                          0,
                        ),
                      )}
                    />
                    <AuditRow
                      label={t("claudeDesktopMigration.auditCoworkTasks")}
                      value={String(
                        audit.sourceApp.coworkRoots.reduce(
                          (n, r) => n + r.metadataCount,
                          0,
                        ),
                      )}
                    />
                    <AuditRow
                      label={t("claudeDesktopMigration.auditSharedTranscripts")}
                      value={String(audit.sharedCodeTranscriptCount)}
                    />
                  </div>
                  <p className="text-xs leading-relaxed text-muted-foreground">
                    {t("claudeDesktopMigration.auditReadOnlyNote")}
                  </p>
                </>
              )}
            </div>
          ) : null}

          {/* -- Step: plan (read-only review) ----------------------------- */}
          {step === "plan" && plan ? (
            <div className="space-y-4">
              {hasBlocking ? (
                <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-4 py-3 text-sm">
                  <div className="flex items-center gap-2 font-medium text-destructive">
                    <AlertTriangle className="h-4 w-4" />
                    {t("claudeDesktopMigration.blockingIssuesTitle")}
                  </div>
                  <ul className="mt-2 list-disc space-y-1 pl-5 text-xs leading-relaxed text-destructive/90">
                    {plan.blockingIssues.map((issue) => (
                      <li key={issue}>{issue}</li>
                    ))}
                  </ul>
                </div>
              ) : null}

              <div className="space-y-3">
                {(["code", "cowork"] as const).map((id) => {
                  const component = id === "code" ? plan.code : plan.cowork;
                  const ready = component.status === "ready";
                  return (
                    <div
                      key={id}
                      className="rounded-lg border border-border px-4 py-3"
                    >
                      <label className="flex items-center justify-between gap-2">
                        <span className="flex items-center gap-2">
                          <Checkbox
                            checked={selected.includes(id)}
                            disabled={!ready}
                            onCheckedChange={(v) =>
                              toggleComponent(id, v === true)
                            }
                          />
                          <span className="text-sm font-medium">
                            {t(`claudeDesktopMigration.component_${id}`)}
                          </span>
                        </span>
                        {renderStatusBadge(component.status)}
                      </label>
                      {ready ? renderComponentPlan(component) : null}
                      {id === "code"
                        ? renderCandidatePicker(component, "sourceCodeRoot")
                        : renderCandidatePicker(component, "sourceCoworkRoot")}
                      {id === "code"
                        ? renderCandidatePicker(component, "targetCodeRoot")
                        : renderCandidatePicker(component, "targetCoworkRoot")}
                    </div>
                  );
                })}

                <div className="rounded-lg border border-border px-4 py-3">
                  <div className="text-xs font-medium text-muted-foreground">
                    {t("claudeDesktopMigration.additionalComponents")}
                  </div>
                  <div className="mt-2 space-y-2">
                    {(["schedules", "projects", "artifacts"] as const).map(
                      (id) => (
                        <label
                          key={id}
                          className="flex items-center gap-2 text-sm"
                        >
                          <Checkbox
                            checked={selected.includes(id)}
                            onCheckedChange={(v) =>
                              toggleComponent(id, v === true)
                            }
                          />
                          {t(`claudeDesktopMigration.component_${id}`)}
                        </label>
                      ),
                    )}
                  </div>
                </div>
              </div>

              {plan.cowork.missingFolderPaths.length > 0 ? (
                <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 px-4 py-3 text-sm">
                  <div className="flex items-center gap-2 font-medium text-amber-900 dark:text-amber-200">
                    <AlertTriangle className="h-4 w-4" />
                    {t("claudeDesktopMigration.missingFoldersTitle")}
                  </div>
                  <p className="mt-1 text-xs leading-relaxed text-amber-900/90 dark:text-amber-200/90">
                    {t("claudeDesktopMigration.missingFoldersDescription")}
                  </p>
                  <ul className="mt-2 space-y-1 text-xs text-amber-900/90 dark:text-amber-200/90">
                    {plan.cowork.missingFolderPaths.map((p) => (
                      <li key={p} className="break-all font-mono">
                        {p}
                      </li>
                    ))}
                  </ul>
                </div>
              ) : null}

              <div className="rounded-lg border border-border px-4 py-3">
                <div className="text-sm font-medium">
                  {t("claudeDesktopMigration.pathMapTitle")}
                </div>
                <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
                  {t("claudeDesktopMigration.pathMapDescription")}
                </p>
                <div className="mt-3 space-y-2">
                  {pathMaps.map((map, index) => (
                    <div key={index} className="flex items-center gap-2">
                      <Input
                        value={map.old}
                        placeholder={t("claudeDesktopMigration.pathMapOld")}
                        onChange={(e) =>
                          updatePathMap(index, "old", e.target.value)
                        }
                        className="font-mono text-xs"
                      />
                      <ArrowRight className="h-4 w-4 shrink-0 text-muted-foreground" />
                      <Input
                        value={map.new}
                        placeholder={t("claudeDesktopMigration.pathMapNew")}
                        onChange={(e) =>
                          updatePathMap(index, "new", e.target.value)
                        }
                        className="font-mono text-xs"
                      />
                      <Button
                        variant="ghost"
                        size="icon"
                        onClick={() =>
                          setPathMaps((prev) =>
                            prev.filter((_, i) => i !== index),
                          )
                        }
                        aria-label={t("claudeDesktopMigration.pathMapRemove")}
                      >
                        <Trash2 className="h-4 w-4" />
                      </Button>
                    </div>
                  ))}
                  <div className="flex items-center gap-2">
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() =>
                        setPathMaps((prev) => [...prev, { old: "", new: "" }])
                      }
                    >
                      <Plus className="mr-1 h-4 w-4" />
                      {t("claudeDesktopMigration.pathMapAdd")}
                    </Button>
                    {pathMaps.length > 0 ? (
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => buildPlan(pathMaps)}
                      >
                        {t("claudeDesktopMigration.rebuildPlan")}
                      </Button>
                    ) : null}
                  </div>
                </div>
              </div>

              <div className="flex items-center gap-2 rounded-lg border border-blue-500/30 bg-blue-500/10 px-4 py-3 text-xs text-blue-900 dark:text-blue-200">
                <Info className="h-4 w-4 shrink-0" />
                {t("claudeDesktopMigration.manualDescription")}
              </div>

              <div className="flex justify-between rounded-lg bg-muted/40 px-4 py-2 text-xs text-muted-foreground">
                <span>
                  {t("claudeDesktopMigration.estimatedCopy", {
                    size: formatBytes(plan.estimatedCopyBytes),
                  })}
                </span>
                <span>
                  {t("claudeDesktopMigration.estimatedBackup", {
                    size: formatBytes(plan.estimatedBackupBytes),
                  })}
                </span>
              </div>
            </div>
          ) : null}

          {/* -- Step: confirm (explicit consent) -------------------------- */}
          {step === "confirm" && plan ? (
            <div className="space-y-4">
              <div className="rounded-lg border border-border bg-muted/40 px-4 py-3 text-sm">
                <div className="font-medium">
                  {t("claudeDesktopMigration.confirmSummaryTitle")}
                </div>
                <ul className="mt-2 space-y-1 text-xs leading-relaxed text-muted-foreground">
                  <li>
                    {t("claudeDesktopMigration.confirmSummaryLine", {
                      code: plan.code.newRecords,
                      cowork: plan.cowork.newRecords,
                      size: formatBytes(plan.estimatedCopyBytes),
                    })}
                  </li>
                  <li className="break-all font-mono">
                    {plan.sourceApp} → {plan.targetApp}
                  </li>
                </ul>
              </div>
              <div className="flex items-start gap-2 rounded-lg border border-amber-500/30 bg-amber-500/10 px-4 py-3 text-sm text-amber-900 dark:text-amber-200">
                <ShieldCheck className="mt-0.5 h-4 w-4 shrink-0" />
                <span className="leading-relaxed">
                  {t("claudeDesktopMigration.confirmWarning")}
                </span>
              </div>
              <label className="flex cursor-pointer select-none items-start gap-2 text-sm">
                <Checkbox
                  checked={consent}
                  onCheckedChange={(v) => setConsent(v === true)}
                  className="mt-0.5"
                />
                <span className="leading-relaxed">
                  {t("claudeDesktopMigration.confirmCheckbox")}
                </span>
              </label>
            </div>
          ) : null}

          {/* -- Step: applying -------------------------------------------- */}
          {step === "applying" ? (
            <div className="flex flex-col items-center gap-3 py-8 text-sm text-muted-foreground">
              <Loader2 className="h-8 w-8 animate-spin text-blue-500" />
              {t("claudeDesktopMigration.applying")}
            </div>
          ) : null}

          {/* -- Step: done ------------------------------------------------- */}
          {step === "done" && applyResult ? (
            <div className="space-y-4">
              {applyResult.applyError ? (
                <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-4 py-3">
                  <div className="flex items-center gap-2 text-sm font-medium text-destructive">
                    <AlertTriangle className="h-4 w-4 shrink-0" />
                    {t("claudeDesktopMigration.applyErrorTitle")}
                  </div>
                  <p className="mt-1 text-xs leading-relaxed text-destructive/90">
                    {t("claudeDesktopMigration.applyErrorDescription")}
                  </p>
                  <p className="mt-1 break-all font-mono text-xs text-destructive/90">
                    {applyResult.applyError}
                  </p>
                </div>
              ) : null}
              <div className="grid grid-cols-3 gap-3 text-center">
                <StatCard
                  label={t("claudeDesktopMigration.doneInstalled")}
                  value={applyResult.installedCount}
                  tone="ok"
                />
                <StatCard
                  label={t("claudeDesktopMigration.doneSkipped")}
                  value={applyResult.skippedCount}
                  tone="muted"
                />
                <StatCard
                  label={t("claudeDesktopMigration.doneFailed")}
                  value={applyResult.failedCount}
                  tone={applyResult.failedCount > 0 ? "bad" : "muted"}
                />
              </div>

              <div className="rounded-lg border border-border bg-muted/40 px-4 py-3 text-xs">
                <div className="font-medium text-foreground">
                  {t("claudeDesktopMigration.doneBackup")}
                </div>
                <div className="mt-1 break-all font-mono text-muted-foreground">
                  {applyResult.backupPath}
                </div>
              </div>

              {applyResult.failed.length > 0 ? (
                <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-4 py-3">
                  <div className="text-sm font-medium text-destructive">
                    {t("claudeDesktopMigration.failedListTitle")}
                  </div>
                  <ul className="mt-2 space-y-1">
                    {applyResult.failed.map(renderFailedRecord)}
                  </ul>
                </div>
              ) : null}

              {verifyResult ? (
                <div
                  className={`rounded-lg border px-4 py-3 ${
                    verifyResult.passed
                      ? "border-emerald-500/30 bg-emerald-500/10"
                      : "border-destructive/30 bg-destructive/10"
                  }`}
                >
                  <div className="flex items-center gap-2 text-sm font-medium">
                    {verifyResult.passed ? (
                      <>
                        <CheckCircle2 className="h-4 w-4 text-emerald-600" />
                        {t("claudeDesktopMigration.verifyPassed")}
                      </>
                    ) : (
                      <>
                        <XCircle className="h-4 w-4 text-destructive" />
                        {t("claudeDesktopMigration.verifyFailed")}
                      </>
                    )}
                  </div>
                  <ul className="mt-2 space-y-1 text-xs">
                    {verifyResult.checks.map((check) => (
                      <li key={check.name} className="flex items-start gap-2">
                        {check.ok ? (
                          <CheckCircle2 className="mt-0.5 h-3.5 w-3.5 shrink-0 text-emerald-600" />
                        ) : (
                          <XCircle className="mt-0.5 h-3.5 w-3.5 shrink-0 text-destructive" />
                        )}
                        <span>
                          <span className="font-medium">{check.name}</span>
                          <span className="text-muted-foreground">
                            {" "}
                            — {check.detail}
                          </span>
                        </span>
                      </li>
                    ))}
                  </ul>
                </div>
              ) : null}

              {restoreResult ? (
                <div className="rounded-lg border border-border bg-muted/40 px-4 py-3 text-sm">
                  <div className="font-medium">
                    {t("claudeDesktopMigration.undoDoneTitle")}
                  </div>
                  <div className="mt-1 text-xs text-muted-foreground">
                    {t("claudeDesktopMigration.undoSummary", {
                      removed: restoreResult.removedCount,
                      reverted: restoreResult.revertedCount,
                      kept: restoreResult.keptCount,
                    })}
                  </div>
                </div>
              ) : null}

              {showUndo && !restoreResult ? (
                <div className="space-y-3 rounded-lg border border-destructive/30 bg-destructive/10 px-4 py-3">
                  <div className="flex items-start gap-2 text-sm text-destructive">
                    <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
                    <span className="leading-relaxed">
                      {t("claudeDesktopMigration.undoWarning")}
                    </span>
                  </div>
                  <label className="flex cursor-pointer select-none items-start gap-2 text-sm">
                    <Checkbox
                      checked={undoConsent}
                      onCheckedChange={(v) => setUndoConsent(v === true)}
                      className="mt-0.5"
                    />
                    <span className="leading-relaxed">
                      {t("claudeDesktopMigration.undoCheckbox")}
                    </span>
                  </label>
                  <div className="flex gap-2">
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => setShowUndo(false)}
                    >
                      {t("claudeDesktopMigration.undoBack")}
                    </Button>
                    <Button
                      variant="destructive"
                      size="sm"
                      disabled={!undoConsent || busy}
                      onClick={runRestore}
                    >
                      {busy ? (
                        <Loader2 className="mr-1 h-4 w-4 animate-spin" />
                      ) : null}
                      {t("claudeDesktopMigration.undoConfirm")}
                    </Button>
                  </div>
                </div>
              ) : null}
            </div>
          ) : null}
        </div>

        <DialogFooter className="flex gap-2 sm:justify-end">
          {step === "audit" ? (
            <>
              <Button variant="outline" onClick={onClose}>
                {t("common.cancel")}
              </Button>
              <Button
                disabled={busy || !audit || !audit.supported}
                onClick={() => buildPlan(pathMaps)}
              >
                {busy ? (
                  <Loader2 className="mr-1 h-4 w-4 animate-spin" />
                ) : null}
                {t("claudeDesktopMigration.buildPlan")}
              </Button>
            </>
          ) : null}

          {step === "plan" ? (
            <>
              <Button variant="outline" onClick={() => setStep("audit")}>
                {t("claudeDesktopMigration.back")}
              </Button>
              <Button
                disabled={hasBlocking || selected.length === 0}
                onClick={() => setStep("confirm")}
              >
                {t("claudeDesktopMigration.continue")}
              </Button>
            </>
          ) : null}

          {step === "confirm" ? (
            <>
              <Button
                variant="outline"
                onClick={() => setStep("plan")}
                disabled={busy}
              >
                {t("claudeDesktopMigration.back")}
              </Button>
              <Button
                variant="destructive"
                disabled={!consent || busy}
                onClick={runApply}
              >
                {busy ? (
                  <Loader2 className="mr-1 h-4 w-4 animate-spin" />
                ) : null}
                {t("claudeDesktopMigration.startMigration")}
              </Button>
            </>
          ) : null}

          {step === "done" ? (
            <>
              <Button variant="outline" onClick={runVerify} disabled={busy}>
                {busy && !verifyResult ? (
                  <Loader2 className="mr-1 h-4 w-4 animate-spin" />
                ) : null}
                {t("claudeDesktopMigration.verifyButton")}
              </Button>
              {!restoreResult ? (
                <Button
                  variant="outline"
                  onClick={() => setShowUndo(true)}
                  disabled={busy}
                >
                  {t("claudeDesktopMigration.undoButton")}
                </Button>
              ) : null}
              <Button onClick={onClose}>{t("common.close")}</Button>
            </>
          ) : null}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function AuditRow({
  label,
  value,
  missing = false,
  missingLabel,
}: {
  label: string;
  value: string;
  missing?: boolean;
  missingLabel?: string;
}) {
  return (
    <div className="flex items-baseline justify-between gap-4">
      <span className="shrink-0 text-muted-foreground">{label}</span>
      <span className="break-all text-right font-mono text-xs">
        {missing ? (
          <span className="font-sans text-amber-600 dark:text-amber-400">
            {missingLabel}
          </span>
        ) : (
          value
        )}
      </span>
    </div>
  );
}

function StatCard({
  label,
  value,
  tone,
}: {
  label: string;
  value: number;
  tone: "ok" | "muted" | "bad";
}) {
  const toneClass =
    tone === "ok"
      ? "text-emerald-600 dark:text-emerald-400"
      : tone === "bad"
        ? "text-destructive"
        : "text-foreground";
  return (
    <div className="rounded-lg border border-border bg-muted/40 px-3 py-3">
      <div className={`text-2xl font-semibold ${toneClass}`}>{value}</div>
      <div className="mt-1 text-xs text-muted-foreground">{label}</div>
    </div>
  );
}
