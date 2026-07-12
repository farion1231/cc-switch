import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { ArrowRightLeft, FolderInput, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { desktopSessionsApi } from "@/lib/api";
import type {
  DesktopSessionAccount,
  MigrateReport,
} from "@/lib/api/desktopSessions";
import { extractErrorMessage } from "@/utils/errorUtils";

/** 复合 key：`rootKind|账号UUID/组织UUID`（rootKind 不含 "|"，两段 UUID 不含 "/"）。 */
function keyOf(a: DesktopSessionAccount): string {
  return `${a.rootKind}|${a.accountUuid}/${a.orgUuid}`;
}

function parseKey(key: string): { root: string; account: string; org: string } {
  const [root, rest] = key.split("|");
  const [account, org] = rest.split("/");
  return { root, account, org };
}

/**
 * Claude Desktop 会话迁移。
 *
 * Claude Desktop 把 Code 会话按账号分目录存放，切换账号后旧账号的会话就「看不到」了
 * （其实只是躺在另一个文件夹里）。这里把来源账号的会话**非破坏性复制**到目标账号，
 * 使其在目标账号下也能显示。只搬历史、不搬用量额度；来源目录始终不变。
 */
export function DesktopSessionMigration() {
  const { t } = useTranslation();
  const [accounts, setAccounts] = useState<DesktopSessionAccount[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [fromKey, setFromKey] = useState<string>("");
  const [toKey, setToKey] = useState<string>("");
  const [preview, setPreview] = useState<MigrateReport | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await desktopSessionsApi.listAccounts();
      setAccounts(list);
      const current = list.find((a) => a.isCurrent) ?? null;
      const defaultTo = current
        ? keyOf(current)
        : list[0]
          ? keyOf(list[0])
          : "";
      const defaultFrom = list.find((a) => keyOf(a) !== defaultTo) ?? null;
      setToKey(defaultTo);
      setFromKey(defaultFrom ? keyOf(defaultFrom) : "");
    } catch (e) {
      setError(extractErrorMessage(e));
      setAccounts([]);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  // 选择变化后清空上一次预览。
  useEffect(() => {
    setPreview(null);
  }, [fromKey, toKey]);

  const sameSelection = fromKey !== "" && fromKey === toKey;
  const canRun = fromKey !== "" && toKey !== "" && !sameSelection && !busy;

  const label = useCallback(
    (a: DesktopSessionAccount) => {
      const short = a.accountUuid.slice(0, 8);
      const count = t("settings.desktopSessionMigration.sessionCount", {
        count: a.sessionCount,
        defaultValue: "{{count}} 条会话",
      });
      const current = a.isCurrent
        ? ` · ${t("settings.desktopSessionMigration.currentTag", {
            defaultValue: "当前登录",
          })}`
        : "";
      const managed =
        a.rootKind === "managed"
          ? ` · ${t("settings.desktopSessionMigration.managedTag", {
              defaultValue: "受管 (3P)",
            })}`
          : "";
      return `${short}… · ${count}${current}${managed}`;
    },
    [t],
  );

  const runPreview = useCallback(async () => {
    if (!canRun) return;
    const from = parseKey(fromKey);
    const to = parseKey(toKey);
    setBusy(true);
    try {
      const report = await desktopSessionsApi.migrate({
        fromRoot: from.root,
        fromAccount: from.account,
        fromOrg: from.org,
        toRoot: to.root,
        toAccount: to.account,
        toOrg: to.org,
        dryRun: true,
      });
      setPreview(report);
    } catch (e) {
      toast.error(extractErrorMessage(e));
    } finally {
      setBusy(false);
    }
  }, [canRun, fromKey, toKey]);

  const runMigrate = useCallback(async () => {
    if (!canRun) return;
    const from = parseKey(fromKey);
    const to = parseKey(toKey);
    setBusy(true);
    try {
      const report = await desktopSessionsApi.migrate({
        fromRoot: from.root,
        fromAccount: from.account,
        fromOrg: from.org,
        toRoot: to.root,
        toAccount: to.account,
        toOrg: to.org,
        dryRun: false,
      });
      setPreview(report);
      toast.success(
        t("settings.desktopSessionMigration.migrateDone", {
          count: report.copied,
          defaultValue: "已迁移 {{count}} 条会话，重开 Claude Desktop 即可看到",
        }),
      );
      await load();
    } catch (e) {
      toast.error(extractErrorMessage(e));
    } finally {
      setBusy(false);
    }
  }, [canRun, fromKey, toKey, t, load]);

  const options = useMemo(
    () =>
      accounts.map((a) => ({
        key: keyOf(a),
        text: label(a),
      })),
    [accounts, label],
  );

  return (
    <section className="rounded-xl border border-border/60 bg-card/60 p-6">
      <div className="mb-4 flex items-start justify-between gap-4">
        <div className="flex items-center gap-3">
          <div className="flex h-10 w-10 items-center justify-center rounded-xl bg-muted">
            <FolderInput className="h-5 w-5" />
          </div>
          <div>
            <h4 className="font-medium">
              {t("settings.desktopSessionMigration.title", {
                defaultValue: "Claude Desktop 会话迁移",
              })}
            </h4>
            <p className="text-sm text-muted-foreground">
              {t("settings.desktopSessionMigration.description", {
                defaultValue:
                  "切换账号后旧账号的会话会「消失」（只是存在另一个文件夹里）。把它们复制到目标账号即可重新看到——只搬历史、不搬用量额度，来源不受影响。",
              })}
            </p>
          </div>
        </div>
        <Button
          variant="ghost"
          size="icon"
          onClick={() => void load()}
          disabled={loading || busy}
          aria-label={t("common.refresh", { defaultValue: "刷新" })}
        >
          <RefreshCw className={`h-4 w-4 ${loading ? "animate-spin" : ""}`} />
        </Button>
      </div>

      {loading ? (
        <p className="text-sm text-muted-foreground">
          {t("settings.desktopSessionMigration.detecting", {
            defaultValue: "正在检测…",
          })}
        </p>
      ) : error ? (
        <p className="text-sm text-muted-foreground">
          {t("settings.desktopSessionMigration.unavailable", {
            defaultValue: "未检测到 Claude Desktop 会话数据。",
          })}
        </p>
      ) : accounts.length < 2 ? (
        <p className="text-sm text-muted-foreground">
          {t("settings.desktopSessionMigration.nothingToMigrate", {
            defaultValue: "仅检测到一个账号的会话，暂无需要迁移的内容。",
          })}
        </p>
      ) : (
        <div className="space-y-4">
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-[1fr_auto_1fr] sm:items-end">
            <div className="space-y-1.5">
              <label className="text-xs font-medium text-muted-foreground">
                {t("settings.desktopSessionMigration.fromLabel", {
                  defaultValue: "来源账号",
                })}
              </label>
              <Select value={fromKey} onValueChange={setFromKey}>
                <SelectTrigger className="h-9">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {options.map((o) => (
                    <SelectItem key={o.key} value={o.key}>
                      {o.text}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            <div className="flex justify-center pb-2 text-muted-foreground">
              <ArrowRightLeft className="h-4 w-4" />
            </div>

            <div className="space-y-1.5">
              <label className="text-xs font-medium text-muted-foreground">
                {t("settings.desktopSessionMigration.toLabel", {
                  defaultValue: "目标账号",
                })}
              </label>
              <Select value={toKey} onValueChange={setToKey}>
                <SelectTrigger className="h-9">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {options.map((o) => (
                    <SelectItem key={o.key} value={o.key}>
                      {o.text}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>

          {sameSelection && (
            <p className="text-sm text-amber-600 dark:text-amber-500">
              {t("settings.desktopSessionMigration.sameSelection", {
                defaultValue: "来源和目标不能是同一个账号。",
              })}
            </p>
          )}

          {preview && (
            <p className="text-sm text-muted-foreground">
              {preview.dryRun
                ? t("settings.desktopSessionMigration.previewResult", {
                    count: preview.pending,
                    defaultValue: "将新增 {{count}} 条会话到目标账号。",
                  })
                : t("settings.desktopSessionMigration.migratedResult", {
                    copied: preview.copied,
                    total: preview.destCountAfter,
                    defaultValue:
                      "已复制 {{copied}} 条，目标账号现有 {{total}} 条会话。",
                  })}
            </p>
          )}

          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => void runPreview()}
              disabled={!canRun}
            >
              {t("settings.desktopSessionMigration.previewBtn", {
                defaultValue: "预览",
              })}
            </Button>
            <Button
              size="sm"
              onClick={() => void runMigrate()}
              disabled={!canRun}
            >
              {t("settings.desktopSessionMigration.migrateBtn", {
                defaultValue: "开始迁移",
              })}
            </Button>
          </div>
        </div>
      )}
    </section>
  );
}
