import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
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
import { authApi, providersApi, settingsApi } from "@/lib/api";
import { resolveManagedAccountId } from "@/lib/authBinding";
import { isMac } from "@/lib/platform";
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

/** 对话框里说「启动 X」用的产品名 */
const TERMINAL_APP_NAMES: Partial<Record<AppId, string>> = {
  claude: "Claude Code",
  agy: "Antigravity CLI",
};

/**
 * Claude Code：终端经 `claude --settings <临时文件>` 启动，临时文件只放这张卡的 env，没写的
 * 键仍从 ~/.claude/settings.json 读。卡上同时有地址和凭据才压得住全局配置。官方卡绑了授权
 * 中心的账号时，终端用这个账号的 `CLAUDE_CONFIG_DIR` 启动（后端 `claude_official::
 * terminal_account`），也固定在这个号上；没绑的官方卡 env 为空，跟着全局登录走，不能说它隔离。
 *
 * Antigravity：带 Key 的卡用自己的私有 `--gemini_dir` 启动，Key 只给这个 agy 进程；官方
 * 登录卡直接跑原生 agy，跟着全局设置和登录走。
 */
export function terminalIsIsolated(
  provider: Provider,
  appId: AppId = "claude",
): boolean {
  const env = (provider.settingsConfig as { env?: unknown } | undefined)?.env;
  if (!env || typeof env !== "object") return false;
  const filled = (key: string) => {
    const value = (env as Record<string, unknown>)[key];
    return typeof value === "string" && value.trim() !== "";
  };
  if (appId === "agy") {
    return provider.category !== "official" && filled("GEMINI_API_KEY");
  }
  if (provider.category === "official") {
    return Boolean(
      resolveManagedAccountId(provider.meta, "claude_official")?.trim(),
    );
  }
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
 * 「打开终端」：先说明要选的是启动 CLI 的目录，再让用户选。
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
  const queryClient = useQueryClient();

  // Antigravity 官方卡绑了账号：打开前会把 agy 的全局登录切过去，先说清楚
  const boundAgyAccount =
    appId === "agy" && provider && provider.category === "official" && isMac()
      ? resolveManagedAccountId(provider.meta, "agy_oauth")
      : null;
  const { data: agyStatus } = useQuery({
    queryKey: ["managed-auth-status", "agy_oauth"],
    queryFn: () => authApi.authGetStatus("agy_oauth"),
    enabled: boundAgyAccount !== null,
  });
  const agyWillSwitch =
    boundAgyAccount !== null &&
    agyStatus !== undefined &&
    agyStatus.default_account_id !== boundAgyAccount;
  const { data: agyRunning } = useQuery({
    queryKey: ["agy-running", provider?.id],
    queryFn: () => authApi.agyAccountIsRunning(),
    enabled: agyWillSwitch,
    staleTime: 0,
  });
  const agyTargetLogin = agyStatus?.accounts.find(
    (account) => account.id === boundAgyAccount,
  )?.login;

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
      if (agyWillSwitch) {
        void queryClient.invalidateQueries({
          queryKey: ["managed-auth-status", "agy_oauth"],
        });
      }
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
              app: TERMINAL_APP_NAMES[appId] ?? "Claude Code",
            })}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 px-6 py-3">
          {provider && terminalIsIsolated(provider, appId) && (
            <Notice title={t("provider.openTerminalDialog.isolationTitle")}>
              {/* Antigravity 没有「切换」，Claude Code 那句「之后切换不会影响它」对它不成立 */}
              {t(
                appId === "agy"
                  ? "provider.openTerminalDialog.isolationBodyAgy"
                  : "provider.openTerminalDialog.isolationBody",
                { name: provider.name },
              )}
            </Notice>
          )}

          {agyWillSwitch && (
            <Notice
              tone={agyRunning ? "warning" : "neutral"}
              title={t("provider.openTerminalDialog.agySwitchTitle", {
                defaultValue: "会切换 agy 的登录",
              })}
            >
              {t("provider.openTerminalDialog.agySwitchBody", {
                defaultValue:
                  "打开前把 agy 的登录切换到 {{account}}，之后新开的 agy 都用这个号。",
                account:
                  agyTargetLogin ??
                  t("provider.form.agy.accountMissing", {
                    defaultValue: "不在这台电脑上的账号",
                  }),
              })}
              {agyRunning &&
                " " +
                  t("provider.openTerminalDialog.agySwitchRunning", {
                    defaultValue:
                      "检测到 agy 正在运行：已打开的 agy 仍用原账号，还可能把原账号写回去，最好先退出它们。",
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
