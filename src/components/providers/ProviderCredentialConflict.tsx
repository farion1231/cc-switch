import { useMemo, useState } from "react";
import { AlertTriangle, RefreshCw } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import type { AppId } from "@/lib/api";
import { providerSecurityApi } from "@/lib/api/providerSecurity";
import type { CredentialDiff } from "@/lib/api/settings";
import type { CredentialField } from "@/types/providerSecurity";

export interface ProviderCredentialConflictProps {
  appId: AppId;
  providerId: string;
  revision: number;
  conflicts: CredentialDiff[];
  onImported?: () => void;
  compact?: boolean;
}

const FIELD_LABEL: Record<string, string> = {
  apiKey: "API Key",
  baseUrl: "Base URL",
};

function normalizeField(field: string): CredentialField | null {
  if (field === "apiKey" || field === "api_key") return "apiKey";
  if (field === "baseUrl" || field === "base_url") return "baseUrl";
  return null;
}

export function ProviderCredentialConflict({
  appId,
  providerId,
  revision,
  conflicts,
  onImported,
  compact = false,
}: ProviderCredentialConflictProps) {
  const [selected, setSelected] = useState<Record<string, boolean>>({});
  const [importing, setImporting] = useState(false);
  const [showImport, setShowImport] = useState(false);

  const normalizedConflicts = useMemo(
    () =>
      conflicts
        .map((c) => ({
          ...c,
          fieldKey: normalizeField(c.field) ?? c.field,
        }))
        .filter((c) => normalizeField(c.field)),
    [conflicts],
  );

  if (!normalizedConflicts.length) {
    return null;
  }

  const toggle = (field: string, checked: boolean) => {
    setSelected((prev) => ({ ...prev, [field]: checked }));
  };

  const handleImport = async () => {
    const fields = normalizedConflicts
      .map((c) => normalizeField(c.field))
      .filter((f): f is CredentialField => !!f && selected[f]);

    if (!fields.length) {
      toast.error("请至少勾选一个要从 Live 导入的字段");
      return;
    }

    setImporting(true);
    try {
      const outcome = await providerSecurityApi.importLiveCredentials({
        appId,
        providerId,
        expectedRevision: revision,
        fields,
      });
      if (outcome.kind === "conflict") {
        toast.error("供应商已被其他操作更新，请重新加载后再导入");
        return;
      }
      toast.success("已从 Live 导入选中凭据");
      setShowImport(false);
      setSelected({});
      onImported?.();
    } catch (error) {
      const message =
        error instanceof Error ? error.message : String(error ?? "");
      if (message.includes("provider_revision_conflict")) {
        toast.error("供应商已被其他操作更新，请重新加载后再导入");
      } else {
        toast.error(message || "导入 Live 凭据失败");
      }
    } finally {
      setImporting(false);
    }
  };

  return (
    <div
      className={
        compact
          ? "rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-sm"
          : "rounded-lg border border-amber-500/40 bg-amber-500/10 p-4 text-sm"
      }
      data-testid="provider-credential-conflict"
    >
      <div className="flex items-start gap-2">
        <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-amber-600" />
        <div className="min-w-0 flex-1 space-y-2">
          <div className="font-medium text-amber-900 dark:text-amber-100">
            Live 凭据冲突
          </div>
          <p className="text-muted-foreground">
            磁盘 Live
            配置与项目数据库中的凭据不一致。表单始终展示数据库值；如需覆盖，请显式从
            Live 导入。
          </p>
          <ul className="space-y-1 text-xs text-muted-foreground">
            {normalizedConflicts.map((c) => (
              <li key={c.field}>
                <span className="font-medium text-foreground">
                  {FIELD_LABEL[normalizeField(c.field) ?? ""] ?? c.field}
                </span>
                ：DB {c.storedMasked || "（空）"} / Live{" "}
                {c.liveMasked || "（空）"}
              </li>
            ))}
          </ul>

          {!showImport ? (
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={() => setShowImport(true)}
            >
              从 Live 导入
            </Button>
          ) : (
            <div className="space-y-3 rounded-md border bg-background/70 p-3">
              <div className="text-xs text-muted-foreground">
                默认不勾选任何字段。勾选后将用 Live 值覆盖数据库对应字段。
              </div>
              {normalizedConflicts.map((c) => {
                const field = normalizeField(c.field)!;
                return (
                  <label
                    key={field}
                    className="flex cursor-pointer items-start gap-2 text-sm"
                  >
                    <Checkbox
                      checked={!!selected[field]}
                      onCheckedChange={(v) => toggle(field, v === true)}
                    />
                    <span>
                      <span className="font-medium">
                        {FIELD_LABEL[field] ?? field}
                      </span>
                      <span className="mt-0.5 block text-xs text-muted-foreground">
                        DB {c.storedMasked || "（空）"} → Live{" "}
                        {c.liveMasked || "（空）"}
                      </span>
                    </span>
                  </label>
                );
              })}
              <div className="flex flex-wrap gap-2">
                <Button
                  type="button"
                  size="sm"
                  disabled={importing}
                  onClick={() => void handleImport()}
                >
                  {importing ? (
                    <>
                      <RefreshCw className="mr-1 h-3.5 w-3.5 animate-spin" />
                      导入中…
                    </>
                  ) : (
                    "确认导入"
                  )}
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  disabled={importing}
                  onClick={() => {
                    setShowImport(false);
                    setSelected({});
                  }}
                >
                  取消
                </Button>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

export function ProviderConflictBadge({ count }: { count: number }) {
  if (count <= 0) return null;
  return (
    <span
      className="inline-flex items-center rounded-full border border-amber-500/50 bg-amber-500/15 px-2 py-0.5 text-[11px] font-medium text-amber-800 dark:text-amber-100"
      data-testid="provider-conflict-badge"
      title="Live 凭据与数据库不一致"
    >
      Live 冲突{count > 1 ? ` · ${count}` : ""}
    </span>
  );
}
