import { useCallback, useEffect, useMemo, useState } from "react";
import { AlertTriangle, Database, RefreshCw, Wrench } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { providersApi } from "@/lib/api";
import type {
  CodexStateDiagnosis,
  CodexStateRepairResult,
} from "@/lib/api/providers";
import { cn } from "@/lib/utils";

type Action = "diagnose" | "dryRun" | "repair" | null;

function formatOptional(value?: string | null) {
  return value && value.trim() ? value : "openai";
}

export function CodexStateRepairPanel() {
  const [diagnosis, setDiagnosis] = useState<CodexStateDiagnosis | null>(null);
  const [repairResult, setRepairResult] =
    useState<CodexStateRepairResult | null>(null);
  const [action, setAction] = useState<Action>(null);
  const [error, setError] = useState<string | null>(null);

  const loadDiagnosis = useCallback(async () => {
    setAction("diagnose");
    setError(null);
    try {
      const result = await providersApi.diagnoseCodexState();
      setDiagnosis(result);
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setError(message);
      toast.error(message);
    } finally {
      setAction(null);
    }
  }, []);

  useEffect(() => {
    void loadDiagnosis();
  }, [loadDiagnosis]);

  const runRepair = useCallback(async (dryRun: boolean) => {
    setAction(dryRun ? "dryRun" : "repair");
    setError(null);
    try {
      const result = await providersApi.repairCodexState(dryRun);
      setRepairResult(result);
      if (result.diagnosisAfter) {
        setDiagnosis(result.diagnosisAfter);
      }
      toast.success(
        dryRun
          ? `Dry run: ${result.affectedRows} row(s) can be repaired`
          : `Repaired ${result.affectedRows} row(s)`,
      );
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setError(message);
      toast.error(message);
    } finally {
      setAction(null);
    }
  }, []);

  const hasRepairableRows = (diagnosis?.repairableRows ?? 0) > 0;
  const providerCounts = diagnosis?.providerCounts ?? [];
  const statusTone = useMemo(() => {
    if (!diagnosis) return "border-border bg-muted/20";
    return diagnosis.inconsistent
      ? "border-amber-500/40 bg-amber-500/10"
      : "border-emerald-500/40 bg-emerald-500/10";
  }, [diagnosis]);

  return (
    <div className="space-y-4">
      <div className={cn("rounded-lg border p-4", statusTone)}>
        <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
          <div className="flex items-start gap-3">
            {diagnosis?.inconsistent ? (
              <AlertTriangle className="mt-0.5 h-5 w-5 text-amber-600" />
            ) : (
              <Database className="mt-0.5 h-5 w-5 text-emerald-600" />
            )}
            <div className="space-y-2">
              <div className="text-sm font-semibold">
                {diagnosis?.inconsistent
                  ? "Codex state mismatch detected"
                  : "Codex state is aligned"}
              </div>
              {diagnosis ? (
                <div className="grid gap-1 text-sm text-muted-foreground sm:grid-cols-2">
                  <div>
                    Config provider:{" "}
                    <span className="font-mono text-foreground">
                      {formatOptional(diagnosis.configModelProvider)}
                    </span>
                  </div>
                  <div>
                    Effective provider:{" "}
                    <span className="font-mono text-foreground">
                      {diagnosis.effectiveModelProvider}
                    </span>
                  </div>
                  <div>
                    Auth mode:{" "}
                    <span className="font-mono text-foreground">
                      {diagnosis.authMode}
                    </span>
                  </div>
                  <div>
                    Repairable rows:{" "}
                    <span className="font-mono text-foreground">
                      {diagnosis.repairableRows}
                    </span>
                  </div>
                </div>
              ) : (
                <div className="text-sm text-muted-foreground">
                  Reading local Codex config, auth, and thread index.
                </div>
              )}
            </div>
          </div>

          <div className="flex flex-wrap gap-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={loadDiagnosis}
              disabled={action !== null}
            >
              <RefreshCw
                className={cn(
                  "mr-2 h-4 w-4",
                  action === "diagnose" && "animate-spin",
                )}
              />
              Diagnose
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => runRepair(true)}
              disabled={action !== null || !hasRepairableRows}
            >
              Dry Run
            </Button>
            <Button
              type="button"
              size="sm"
              onClick={() => runRepair(false)}
              disabled={action !== null || !hasRepairableRows}
            >
              <Wrench className="mr-2 h-4 w-4" />
              Repair
            </Button>
          </div>
        </div>
      </div>

      {diagnosis?.stateDbPath && (
        <div className="text-xs text-muted-foreground">
          SQLite: <span className="font-mono">{diagnosis.stateDbPath}</span>
        </div>
      )}

      {diagnosis?.configAuthMismatch && (
        <div className="rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2 text-sm text-red-700 dark:text-red-300">
          Config/auth mismatch: Codex may still be using the wrong credential
          mode for the selected provider.
        </div>
      )}

      {providerCounts.length > 0 && (
        <div className="rounded-lg border border-border">
          <div className="border-b border-border px-3 py-2 text-sm font-medium">
            Thread index buckets
          </div>
          <div className="divide-y divide-border">
            {providerCounts.map((row) => (
              <div
                key={row.modelProvider}
                className="flex items-center justify-between px-3 py-2 text-sm"
              >
                <span className="font-mono">{row.modelProvider}</span>
                <span>{row.count}</span>
              </div>
            ))}
          </div>
        </div>
      )}

      {repairResult && (
        <div className="rounded-lg border border-border px-3 py-2 text-sm">
          <div>
            Last result: {repairResult.dryRun ? "dry run" : "repair"} to{" "}
            <span className="font-mono">
              {repairResult.targetModelProvider}
            </span>
            , affected {repairResult.affectedRows} row(s).
          </div>
          {repairResult.backupPath && (
            <div className="mt-1 text-muted-foreground">
              Backup:{" "}
              <span className="font-mono">{repairResult.backupPath}</span>
            </div>
          )}
        </div>
      )}

      {error && (
        <div className="rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2 text-sm text-red-700 dark:text-red-300">
          {error}
        </div>
      )}
    </div>
  );
}
