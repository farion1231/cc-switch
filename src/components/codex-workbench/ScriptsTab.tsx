import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  useCodexUserScriptsQuery,
  useCodexScriptMarketQuery,
  useRefreshCodexScriptMarket,
  useInstallCodexMarketScript,
  useSetCodexUserScriptEnabled,
  useDeleteCodexUserScript,
  useImportCodexUserScript,
  useOpenCodexScriptsDir,
} from "@/lib/query/codexWorkbench";
import type { MarketScriptEntry, UserScriptInfo } from "@/types/codexWorkbench";

function stateBadge(state: string) {
  const variant =
    state === "loaded"
      ? "default"
      : state === "failed"
        ? "destructive"
        : state === "disabled"
          ? "secondary"
          : "outline";
  return <Badge variant={variant as "default"}>{state}</Badge>;
}

export function ScriptsTab() {
  const { t } = useTranslation();
  const scriptsQ = useCodexUserScriptsQuery();
  const marketQ = useCodexScriptMarketQuery();
  const refreshMarket = useRefreshCodexScriptMarket();
  const installMarket = useInstallCodexMarketScript();
  const setEnabled = useSetCodexUserScriptEnabled();
  const delScript = useDeleteCodexUserScript();
  const importScript = useImportCodexUserScript();
  const openScriptsDir = useOpenCodexScriptsDir();

  const scripts: UserScriptInfo[] = scriptsQ.data ?? [];
  const market: MarketScriptEntry[] = marketQ.data?.scripts ?? [];

  const onImport = async () => {
    // Prefer native dialog if available; fall back to prompt for path.
    let path: string | null = null;
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        multiple: false,
        filters: [{ name: "JavaScript", extensions: ["js"] }],
      });
      if (typeof selected === "string") path = selected;
    } catch {
      path = window.prompt(
        t("codexWorkbench.scripts.importPrompt", {
          defaultValue: "输入本地 .js 脚本绝对路径",
        }),
      );
    }
    if (!path) return;
    await importScript.mutateAsync({ sourcePath: path });
  };

  const onOpenFolder = async () => {
    try {
      await openScriptsDir.mutateAsync();
    } catch (e) {
      console.error(e);
    }
  };

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-2">
        <Button size="sm" onClick={() => void refreshMarket.mutate()} disabled={refreshMarket.isPending}>
          {t("codexWorkbench.scripts.refreshMarket", { defaultValue: "刷新市场" })}
        </Button>
        <Button size="sm" variant="outline" onClick={() => void onImport()} disabled={importScript.isPending}>
          {t("codexWorkbench.scripts.importLocal", { defaultValue: "导入本地脚本" })}
        </Button>
        <Button size="sm" variant="outline" onClick={() => void onOpenFolder()} disabled={openScriptsDir.isPending}>
          {t("codexWorkbench.scripts.openFolder", { defaultValue: "打开脚本目录" })}
        </Button>
      </div>

      {(scriptsQ.error || marketQ.error || refreshMarket.error || installMarket.error) && (
        <div className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive">
          {String(
            (scriptsQ.error as Error)?.message ||
              (marketQ.error as Error)?.message ||
              (refreshMarket.error as Error)?.message ||
              (installMarket.error as Error)?.message ||
              "error",
          )}
        </div>
      )}

      <section className="space-y-2">
        <h3 className="text-sm font-medium">
          {t("codexWorkbench.scripts.installed", { defaultValue: "已安装脚本" })}
        </h3>
        {scripts.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            {t("codexWorkbench.scripts.empty", { defaultValue: "暂无本地脚本" })}
          </p>
        ) : (
          <ul className="space-y-2">
            {scripts.map((s) => (
              <li
                key={s.key}
                className="flex flex-wrap items-center gap-2 rounded-lg border p-3 text-sm"
              >
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="font-medium">{s.name}</span>
                    <Badge variant="outline">{s.source}</Badge>
                    {stateBadge(s.enabled ? s.runtimeState : "disabled")}
                    {s.version && <span className="text-xs text-muted-foreground">v{s.version}</span>}
                  </div>
                  {s.runtimeError && (
                    <p className="mt-1 text-xs text-destructive">{s.runtimeError}</p>
                  )}
                </div>
                <Button
                  size="sm"
                  variant="outline"
                  disabled={setEnabled.isPending}
                  onClick={() =>
                    void setEnabled.mutateAsync({ key: s.key, enabled: !s.enabled })
                  }
                >
                  {s.enabled
                    ? t("codexWorkbench.scripts.disable", { defaultValue: "禁用" })
                    : t("codexWorkbench.scripts.enable", { defaultValue: "启用" })}
                </Button>
                <Button
                  size="sm"
                  variant="destructive"
                  disabled={s.source === "builtin" || delScript.isPending}
                  onClick={() => void delScript.mutateAsync(s.key)}
                >
                  {t("codexWorkbench.scripts.delete", { defaultValue: "删除" })}
                </Button>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="space-y-2">
        <h3 className="text-sm font-medium">
          {t("codexWorkbench.scripts.market", { defaultValue: "远程市场" })}
        </h3>
        {market.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            {t("codexWorkbench.scripts.marketEmpty", {
              defaultValue: "点击“刷新市场”拉取索引（不会自动请求）",
            })}
          </p>
        ) : (
          <ul className="space-y-2">
            {market.map((m) => {
              const installed = scripts.find((s) => s.key === m.id);
              return (
                <li
                  key={m.id}
                  className="flex flex-wrap items-center gap-2 rounded-lg border p-3 text-sm"
                >
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="font-medium">{m.name}</span>
                      <span className="text-xs text-muted-foreground">v{m.version}</span>
                      {installed && <Badge variant="secondary">installed</Badge>}
                    </div>
                    {m.description && (
                      <p className="mt-1 text-xs text-muted-foreground">{m.description}</p>
                    )}
                  </div>
                  <Button
                    size="sm"
                    disabled={installMarket.isPending}
                    onClick={() =>
                      void installMarket.mutateAsync({
                        id: m.id,
                        expectedSha256: m.sha256 || undefined,
                      })
                    }
                  >
                    {installed
                      ? t("codexWorkbench.scripts.update", { defaultValue: "更新" })
                      : t("codexWorkbench.scripts.install", { defaultValue: "安装" })}
                  </Button>
                </li>
              );
            })}
          </ul>
        )}
      </section>
    </div>
  );
}

export default ScriptsTab;
