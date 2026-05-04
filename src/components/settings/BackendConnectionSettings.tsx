import { useCallback, useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Loader2, RefreshCw, Server, Save } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  clearClientBackendConfigCache,
  clearRemoteBackendOverride,
  getClientBackendConnection,
  saveClientBackendConnection,
  setRemoteBackendOverride,
  testRemoteBackendConnection,
  type ClientBackendConnectionSettings,
} from "@/lib/api/transport";
import { runtimeApi } from "@/lib/api";
import { useRuntimeQuery } from "@/lib/query";
import { isCliWebUi } from "@/lib/platform";

type Mode = ClientBackendConnectionSettings["mode"];

export function BackendConnectionSettings() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data: runtimeInfo } = useRuntimeQuery();
  const [mode, setMode] = useState<Mode>("local");
  const [url, setUrl] = useState("");
  const [token, setToken] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const browserWebUi = isCliWebUi();

  useEffect(() => {
    if (browserWebUi) return;

    let active = true;
    setIsLoading(true);
    void getClientBackendConnection()
      .then((connection) => {
        if (!active) return;
        setMode(connection.mode ?? "local");
        setUrl(connection.url ?? "");
        setToken(connection.token ?? "");
      })
      .catch((error) => {
        console.error("[BackendConnectionSettings] load failed", error);
      })
      .finally(() => {
        if (active) setIsLoading(false);
      });

    return () => {
      active = false;
    };
  }, [browserWebUi]);

  const resetLocal = useCallback(() => {
    setMode("local");
    setUrl("");
    setToken("");
  }, []);

  const save = useCallback(async () => {
    const nextUrl = url.trim().replace(/\/+$/, "");
    const nextToken = token.trim();
    if (mode === "remote" && !nextUrl) {
      toast.error(
        t("settings.backendConnection.urlRequired", {
          defaultValue: "请输入远程后端地址",
        }),
      );
      return;
    }

    setIsSaving(true);
    try {
      if (mode === "remote") {
        await testRemoteBackendConnection({
          url: nextUrl,
          token: nextToken || undefined,
        });
      }

      await saveClientBackendConnection({
        mode,
        url: mode === "remote" ? nextUrl : undefined,
        token: mode === "remote" && nextToken ? nextToken : undefined,
      });

      if (mode === "remote") {
        setRemoteBackendOverride({
          url: nextUrl,
          token: nextToken || undefined,
        });
      } else {
        clearRemoteBackendOverride();
      }
      clearClientBackendConfigCache();
      runtimeApi.clearCache();
      await queryClient.invalidateQueries({ queryKey: ["runtime"] });
      toast.success(
        t("settings.backendConnection.saved", {
          defaultValue: "后端连接设置已保存",
        }),
      );
      window.location.reload();
    } catch (error) {
      console.error("[BackendConnectionSettings] save failed", error);
      toast.error(
        mode === "remote"
          ? t("settings.backendConnection.connectionFailed", {
              defaultValue: "无法连接远程后端，请检查地址和 Token",
            })
          : t("settings.backendConnection.saveFailed", {
              defaultValue: "保存后端连接设置失败",
            }),
      );
    } finally {
      setIsSaving(false);
    }
  }, [mode, queryClient, t, token, url]);

  return (
    <section className="space-y-4 rounded-xl glass-card p-5">
      <div className="flex items-start justify-between gap-4">
        <div className="space-y-1">
          <h3 className="text-sm font-medium flex items-center gap-2">
            <Server className="h-4 w-4" />
            {t("settings.backendConnection.title", {
              defaultValue: "后端连接",
            })}
          </h3>
          <p className="text-xs text-muted-foreground">
            {runtimeInfo
              ? t("settings.backendConnection.status", {
                  defaultValue: "当前后端: {{os}} / {{relation}}",
                  os: runtimeInfo.backend.os,
                  relation: runtimeInfo.relation.coLocated
                    ? "co-located"
                    : "remote",
                })
              : t("settings.backendConnection.loading", {
                  defaultValue: "正在读取后端运行信息",
                })}
          </p>
        </div>
        {isLoading ? <Loader2 className="h-4 w-4 animate-spin" /> : null}
      </div>

      {browserWebUi ? (
        <p className="text-xs text-muted-foreground">
          {t("settings.backendConnection.browserHint", {
            defaultValue: "浏览器 WebUI 使用当前页面所在的服务端后端。",
          })}
        </p>
      ) : (
        <div className="space-y-3">
          <div className="grid gap-3 sm:grid-cols-[180px_1fr]">
            <Select
              value={mode}
              onValueChange={(value) => setMode(value as Mode)}
            >
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="local">
                  {t("settings.backendConnection.local", {
                    defaultValue: "本地后端",
                  })}
                </SelectItem>
                <SelectItem value="remote">
                  {t("settings.backendConnection.remote", {
                    defaultValue: "远程后端",
                  })}
                </SelectItem>
              </SelectContent>
            </Select>
            <Input
              value={url}
              disabled={mode === "local"}
              placeholder="http://linux-host:9990"
              onChange={(event) => setUrl(event.target.value)}
            />
          </div>
          <Input
            type="password"
            value={token}
            disabled={mode === "local"}
            placeholder={t("settings.backendConnection.tokenPlaceholder", {
              defaultValue: "Token（远程后端启用 token 验证时填写）",
            })}
            onChange={(event) => setToken(event.target.value)}
          />
          <div className="flex justify-end gap-2">
            <Button type="button" variant="outline" onClick={resetLocal}>
              <RefreshCw className="h-4 w-4" />
              {t("common.reset", { defaultValue: "重置" })}
            </Button>
            <Button type="button" onClick={save} disabled={isSaving}>
              {isSaving ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <Save className="h-4 w-4" />
              )}
              {t("common.save")}
            </Button>
          </div>
        </div>
      )}
    </section>
  );
}
