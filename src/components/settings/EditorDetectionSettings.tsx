import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import { isWindows } from "@/lib/platform";
import { editorsApi, type InstalledEditor } from "@/lib/api/editors";
import { Badge } from "@/components/ui/badge";

export function EditorDetectionSettings() {
  const { t } = useTranslation();
  const [items, setItems] = useState<InstalledEditor[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!isWindows()) return;
    let cancelled = false;
    editorsApi
      .listInstalledEditors()
      .then((data) => {
        if (cancelled) return;
        setItems(data);
      })
      .catch((e) => {
        if (cancelled) return;
        console.error("[EditorDetectionSettings] Failed to load editors", e);
        setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const sorted = useMemo(() => {
    if (!items) return null;
    return [...items].sort((a, b) => a.name.localeCompare(b.name));
  }, [items]);

  return (
    <section className="space-y-3">
      <header className="space-y-1">
        <h3 className="text-sm font-medium">{t("settings.editors.title")}</h3>
        <p className="text-xs text-muted-foreground">
          {t("settings.editors.description")}
        </p>
      </header>

      {!isWindows() ? (
        <p className="text-xs text-muted-foreground">
          {t("settings.editors.notSupported")}
        </p>
      ) : error ? (
        <p className="text-xs text-destructive">
          {t("settings.editors.loadFailed", { defaultValue: "检测失败" })}: {error}
        </p>
      ) : !sorted ? (
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <Loader2 className="h-4 w-4 animate-spin" />
          {t("settings.editors.detecting", { defaultValue: "检测中…" })}
        </div>
      ) : (
        <div className="space-y-2">
          {sorted.map((editor) => (
            <div
              key={editor.id}
              className="flex items-start justify-between gap-3 rounded-md border border-border/50 px-3 py-2"
            >
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <span className="text-sm font-medium">{editor.name}</span>
                  {editor.installed ? (
                    <Badge className="bg-emerald-600 hover:bg-emerald-600">
                      {t("settings.editors.status.installed")}
                    </Badge>
                  ) : (
                    <Badge variant="outline">
                      {t("settings.editors.status.notInstalled")}
                    </Badge>
                  )}
                </div>

                {editor.exePath ? (
                  <div className="mt-1 text-xs text-muted-foreground break-all">
                    {t("settings.editors.detectedPath", {
                      defaultValue: "路径：{{path}}",
                      path: editor.exePath,
                    })}
                    {editor.source ? (
                      <span className="ml-2 opacity-70">({editor.source})</span>
                    ) : null}
                  </div>
                ) : null}
              </div>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

