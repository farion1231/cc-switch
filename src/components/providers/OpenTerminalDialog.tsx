import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Folder, Loader2, X } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Notice } from "@/components/ui/notice";
import { providersApi, settingsApi } from "@/lib/api";
import type { AppId } from "@/lib/api/types";
import type { Provider } from "@/types";
import { toast } from "@/lib/toast";
import { extractErrorMessage } from "@/utils/errorUtils";
import { cn } from "@/lib/utils";

const STORAGE_KEY = "cc-switch:terminal:recent-dirs";
const MAX_RECENT = 5;

function readRecent(): string[] {
  try {
    const parsed = JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? "[]");
    return Array.isArray(parsed)
      ? parsed.filter((dir): dir is string => typeof dir === "string")
      : [];
  } catch {
    return [];
  }
}

function writeRecent(dirs: string[]) {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(dirs));
  } catch {
    // 只是记住用过的目录，存不了不影响打开终端
  }
}

/** 最后一段当名字、其余当所在位置：路径长时项目名最要紧，不能被截掉 */
function splitPath(dir: string): { name: string; parent: string } {
  const trimmed = dir.replace(/[\\/]+$/, "");
  const index = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
  if (index < 0) return { name: trimmed || dir, parent: "" };
  return {
    name: trimmed.slice(index + 1) || trimmed,
    parent: trimmed.slice(0, index) || trimmed.slice(0, index + 1),
  };
}

function DirLabel({ dir }: { dir: string }) {
  const { name, parent } = splitPath(dir);
  return (
    <span className="flex min-w-0 flex-col text-left" title={dir}>
      <span className="truncate text-body font-medium text-fg-1">{name}</span>
      {parent && (
        <span className="truncate text-caption text-fg-3">{parent}</span>
      )}
    </span>
  );
}

/**
 * 终端经 `claude --settings <临时文件>` 启动，临时文件只放这张卡的 env，没写的键仍从
 * ~/.claude/settings.json 读。卡上同时有地址和凭据才压得住全局配置；官方登录卡 env 为空，
 * 会跟着全局走（账号凭据也是全局一份），不能说它隔离。
 */
export function terminalIsIsolated(provider: Provider): boolean {
  const env = (provider.settingsConfig as { env?: unknown } | undefined)?.env;
  if (!env || typeof env !== "object") return false;
  const filled = (key: string) => {
    const value = (env as Record<string, unknown>)[key];
    return typeof value === "string" && value.trim() !== "";
  };
  return (
    filled("ANTHROPIC_BASE_URL") &&
    (filled("ANTHROPIC_AUTH_TOKEN") || filled("ANTHROPIC_API_KEY"))
  );
}

interface OpenTerminalDialogProps {
  /** 有值即打开 */
  provider: Provider | null;
  appId: AppId;
  onClose: () => void;
}

/**
 * 「打开终端」：先说明要选的是启动 Claude Code 的目录，再让用户选。
 * 默认填上次成功打开的目录，最近用过的几个点一下就能换，不必每次都走系统选文件夹窗口。
 */
export function OpenTerminalDialog({
  provider,
  appId,
  onClose,
}: OpenTerminalDialogProps) {
  const { t } = useTranslation();
  const [recent, setRecent] = useState<string[]>([]);
  const [dir, setDir] = useState<string | null>(null);
  const [opening, setOpening] = useState(false);

  useEffect(() => {
    if (!provider) return;
    const stored = readRecent();
    setRecent(stored);
    setDir(stored[0] ?? null);
    setOpening(false);
  }, [provider]);

  const handleChoose = async () => {
    try {
      const picked = await settingsApi.pickDirectory(dir ?? undefined);
      if (picked) setDir(picked);
    } catch (error) {
      const message = extractErrorMessage(error);
      toast.error(
        t("provider.terminalOpenFailed") + (message ? `: ${message}` : ""),
      );
    }
  };

  const handleForget = (target: string) => {
    const next = recent.filter((item) => item !== target);
    setRecent(next);
    writeRecent(next);
  };

  const handleOpen = async () => {
    if (!provider || !dir || opening) return;
    setOpening(true);
    try {
      await providersApi.openTerminal(provider.id, appId, { cwd: dir });
      writeRecent(
        [dir, ...recent.filter((item) => item !== dir)].slice(0, MAX_RECENT),
      );
      toast.success(t("provider.terminalOpened"));
      onClose();
    } catch (error) {
      console.error("[OpenTerminalDialog] Failed to open terminal", error);
      const message = extractErrorMessage(error);
      toast.error(
        t("provider.terminalOpenFailed") + (message ? `: ${message}` : ""),
      );
      setOpening(false);
    }
  };

  const otherRecent = recent.filter((item) => item !== dir);

  return (
    <Dialog
      open={provider !== null}
      onOpenChange={(open) => {
        if (!open && !opening) onClose();
      }}
    >
      <DialogContent className="max-w-md" zIndex="alert">
        <DialogHeader>
          <DialogTitle>{t("provider.openTerminalDialog.title")}</DialogTitle>
          <DialogDescription>
            {t("provider.openTerminalDialog.description", {
              name: provider?.name ?? "",
            })}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 px-6 py-3">
          {provider && terminalIsIsolated(provider) && (
            <Notice title={t("provider.openTerminalDialog.isolationTitle")}>
              {t("provider.openTerminalDialog.isolationBody", {
                name: provider.name,
              })}
            </Notice>
          )}

          <div className="flex items-center gap-2">
            <div className="flex min-w-0 flex-1 items-center gap-2.5 rounded-control border border-border bg-subtle px-3 py-2">
              <Folder
                className="h-4 w-4 shrink-0 text-fg-3"
                strokeWidth={1.5}
              />
              {dir ? (
                <DirLabel dir={dir} />
              ) : (
                <span className="text-body text-fg-3">
                  {t("provider.openTerminalDialog.noDir")}
                </span>
              )}
            </div>
            <Button
              type="button"
              variant="neutral"
              size="compact"
              disabled={opening}
              onClick={() => void handleChoose()}
            >
              {dir
                ? t("provider.openTerminalDialog.change")
                : t("provider.openTerminalDialog.choose")}
            </Button>
          </div>

          {otherRecent.length > 0 && (
            <div className="space-y-1">
              <div className="text-caption text-fg-3">
                {t("provider.openTerminalDialog.recent")}
              </div>
              <ul className="space-y-0.5">
                {otherRecent.map((item) => (
                  <li key={item} className="group flex items-center gap-1">
                    <button
                      type="button"
                      disabled={opening}
                      onClick={() => setDir(item)}
                      className={cn(
                        "flex min-w-0 flex-1 items-center rounded-control px-2 py-1.5 hover:bg-subtle",
                        "disabled:pointer-events-none",
                      )}
                    >
                      <DirLabel dir={item} />
                    </button>
                    <Button
                      type="button"
                      variant="quiet"
                      size="icon-compact"
                      disabled={opening}
                      aria-label={t("provider.openTerminalDialog.forget")}
                      title={t("provider.openTerminalDialog.forget")}
                      onClick={() => handleForget(item)}
                      className="opacity-0 focus-visible:opacity-100 group-hover:opacity-100"
                    >
                      <X className="h-3.5 w-3.5" strokeWidth={1.5} />
                    </Button>
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>

        <DialogFooter>
          <Button
            type="button"
            variant="outline"
            disabled={opening}
            onClick={onClose}
          >
            {t("common.cancel")}
          </Button>
          <Button
            type="button"
            disabled={!dir || opening}
            onClick={() => void handleOpen()}
          >
            {opening && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
            {t("provider.openTerminalDialog.confirm")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
