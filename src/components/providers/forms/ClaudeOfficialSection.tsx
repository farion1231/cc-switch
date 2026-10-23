import React from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { Loader2, Plus } from "lucide-react";
import { toast } from "@/lib/toast";
import { authApi } from "@/lib/api";
import { isMac } from "@/lib/platform";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useManagedAuth } from "./hooks/useManagedAuth";
import { useManagedAccountUsers } from "./hooks/useManagedAccountUsers";
import {
  ManagedAccountRemoveDialog,
  type ManagedAccountRemoveTarget,
} from "./ManagedAccountRemoveDialog";
import {
  ManagedAccountsGroup,
  type GroupAccountRow,
} from "@/components/settings/auth/ManagedAccountsGroup";
import { signedInDate } from "@/components/settings/auth/accountDetails";

const SERVICE_NAME = "Claude";

/**
 * 浏览器授权后回不到本机（远程桌面、浏览器拦了本机地址）时的退路：把浏览器地址栏里的
 * 地址，或登录页上显示的代码粘贴进来，交给正在等待的 `claude auth login`。
 */
function ClaudePasteCode({ flowId }: { flowId: string }) {
  const { t } = useTranslation();
  const [open, setOpen] = React.useState(false);
  const [value, setValue] = React.useState("");
  const [submitting, setSubmitting] = React.useState(false);
  const [submitted, setSubmitted] = React.useState(false);

  const submit = async () => {
    const pasted = value.trim();
    if (!pasted || submitting) return;
    setSubmitting(true);
    try {
      await authApi.claudeAccountSubmitPaste(flowId, pasted);
      setSubmitted(true);
      setValue("");
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    } finally {
      setSubmitting(false);
    }
  };

  if (submitted) {
    return (
      <span className="text-caption text-fg-2">
        {t("authCenter.claude.pasteSubmitted", {
          defaultValue: "已提交，正在完成登录…",
        })}
      </span>
    );
  }

  if (!open) {
    return (
      <button
        type="button"
        className="self-start text-caption text-fg-2 underline underline-offset-2 hover:text-fg-1"
        onClick={() => setOpen(true)}
      >
        {t("authCenter.claude.pasteToggle", {
          defaultValue: "授权后没有回到这里？",
        })}
      </button>
    );
  }

  return (
    <div className="flex flex-col gap-1.5">
      <span className="text-caption text-fg-2">
        {t("authCenter.claude.pasteHint", {
          defaultValue:
            "把授权后浏览器地址栏里的地址，或页面上显示的代码粘贴到这里。",
        })}
      </span>
      <div className="flex items-center gap-2">
        <Input
          value={value}
          onChange={(event) => setValue(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              void submit();
            }
          }}
          spellCheck={false}
          autoComplete="off"
          className="h-8 flex-1 font-mono text-caption"
          aria-label={t("authCenter.claude.pasteLabel", {
            defaultValue: "授权代码或地址",
          })}
        />
        <Button
          type="button"
          variant="neutral"
          size="compact"
          disabled={!value.trim() || submitting}
          onClick={() => void submit()}
        >
          {submitting && (
            <Loader2 className="h-3.5 w-3.5 motion-safe:animate-spin" />
          )}
          {t("authCenter.claude.pasteSubmit", { defaultValue: "提交" })}
        </Button>
      </div>
    </div>
  );
}

interface ClaudeOfficialSectionProps {
  helpSide?: "top" | "bottom";
}

/**
 * 授权中心的 Claude 官方账号组。每个账号是 CC Switch 数据目录下的一个独立 Claude 配置目录，
 * 在里面跑 `claude auth login` 登录；用哪个号由官方卡的绑定决定，所以没有「默认」。
 * `~/.claude` 里原来的登录是「默认登录」，不在这里列出，也永远不会被改动。
 */
export const ClaudeOfficialSection: React.FC<ClaudeOfficialSectionProps> = ({
  helpSide,
}) => {
  const { t, i18n } = useTranslation();
  const {
    accounts,
    isStatusSuccess,
    isStatusError,
    pollingState,
    deviceCode,
    error,
    isPolling,
    isAddingAccount,
    isRemovingAccount,
    addAccount,
    removeAccount,
    cancelAuth,
    logout,
    refetchStatus,
  } = useManagedAuth("claude_official");
  const accountUsers = useManagedAccountUsers("claude_official", null);
  const [removeTarget, setRemoveTarget] =
    React.useState<ManagedAccountRemoveTarget | null>(null);

  const confirmRemove = () => {
    const target = removeTarget;
    setRemoveTarget(null);
    if (!target) return;
    if (target.kind === "all") logout();
    else removeAccount(target.accountId);
  };

  const rows: GroupAccountRow[] = accounts.map((account) => {
    const date = signedInDate(account.authenticated_at, i18n.language);
    return {
      id: account.id,
      login: account.login,
      avatarUrl: account.avatar_url,
      details: account.requires_reauth
        ? [
            t("authCenter.claude.reauthNote", {
              defaultValue: "登录已失效，请重新登录",
            }),
          ]
        : date
          ? [
              t("authCenter.claude.addedOn", {
                defaultValue: "{{date}}添加",
                date,
              }),
            ]
          : [],
      isDefault: false,
      needsReauth: account.requires_reauth,
      users: accountUsers([account.id]),
    };
  });

  return (
    <ManagedAccountsGroup
      slug="claude-official"
      name={SERVICE_NAME}
      iconName="claude"
      help={
        isMac()
          ? t("authCenter.group.claudeHelpMac", {
              defaultValue:
                "Claude Code 的官方订阅账号（Pro / Max / Team）。在 Claude 的官方卡上选一个账号，再从卡片「打开终端」，终端里的 claude 就用这个号。~/.claude 里原来的登录是「默认登录」，不受影响。",
            })
          : t("authCenter.group.claudeHelp", {
              defaultValue:
                "Claude Code 的官方订阅账号（Pro / Max / Team）。在 Claude 的官方卡上选一个账号，切到这张卡后新开的 claude 都用这个号。~/.claude 里原来的登录是「默认登录」，不受影响。",
            })
      }
      helpSide={helpSide}
      accounts={rows}
      status={isStatusError ? "error" : isStatusSuccess ? "ready" : "loading"}
      statusErrorText={t("authCenter.claude.statusLoadFailed", {
        defaultValue: "无法读取 Claude 账号，请重试。",
      })}
      onRetryStatus={() => void refetchStatus()}
      emptyText={t("authCenter.claude.empty", {
        defaultValue:
          "还没有添加 Claude 账号。要在多个订阅账号之间切换时在这里登录。",
      })}
      loginLabel={t("authCenter.claude.login", {
        defaultValue: "登录 Claude 账号",
      })}
      onAdd={addAccount}
      canReauth
      reauthOnlyWhenExpired
      // 用同一个账号再登录一次就会替换掉原来的登录
      onReauth={() => addAccount()}
      settingDefault={false}
      onRemove={(accountId, login) =>
        setRemoveTarget({ kind: "one", accountId, login })
      }
      onRemoveAll={() =>
        setRemoveTarget({
          kind: "all",
          accountIds: accounts.map((account) => account.id),
        })
      }
      removing={isRemovingAccount}
      login={{
        starting: isAddingAccount && !isPolling,
        polling: isPolling,
        pollingState,
        deviceCode,
        error,
        onCancel: cancelAuth,
        onRetry: addAccount,
        extra: deviceCode ? (
          <ClaudePasteCode
            key={deviceCode.device_code}
            flowId={deviceCode.device_code}
          />
        ) : undefined,
      }}
    >
      <ManagedAccountRemoveDialog
        target={removeTarget}
        serviceName={SERVICE_NAME}
        users={
          removeTarget
            ? accountUsers(
                removeTarget.kind === "one"
                  ? [removeTarget.accountId]
                  : removeTarget.accountIds,
              )
            : []
        }
        othersRemain={accounts.length > 1}
        note={t("authCenter.claude.removeNote", {
          defaultValue:
            "只删除 CC Switch 为这个账号保存的登录，不会让它在其他地方退出；默认登录（~/.claude）不受影响。绑定它的卡片会改用默认登录。",
        })}
        pending={isRemovingAccount}
        onConfirm={confirmRemove}
        onCancel={() => setRemoveTarget(null)}
      />
    </ManagedAccountsGroup>
  );
};

export default ClaudeOfficialSection;

const DEFAULT_LOGIN = "__default__";

interface ClaudeAccountSelectProps {
  value: string | null;
  onChange: (accountId: string | null) => void;
}

/**
 * Claude 官方卡的账号：不选就是默认登录（~/.claude）。可以就地登录一个新账号，登录完自动选上。
 */
export function ClaudeAccountSelect({
  value,
  onChange,
}: ClaudeAccountSelectProps) {
  const { t } = useTranslation();
  const { data } = useQuery({
    queryKey: ["managed-auth-status", "claude_official"],
    queryFn: () => authApi.authGetStatus("claude_official"),
    staleTime: 30000,
  });
  const accounts = data?.accounts ?? [];
  // 绑的号不在这台电脑上（别的设备同步来的、或已删掉）时也要显示出来，别悄悄改成默认登录
  const missing = value !== null && !accounts.some((a) => a.id === value);

  const {
    pollingState,
    deviceCode,
    error,
    isPolling,
    isAddingAccount,
    addAccount,
    cancelAuth,
  } = useManagedAuth("claude_official");
  const busy = isAddingAccount || isPolling;

  // 就地登录成功后选上这个账号：等账号列表刷新出新的 id 再选；同一个号重新登录时 id 不变，
  // 列表刷新完（状态回到 idle）还没有新 id，就选最近登录的那个
  const pending = React.useRef<{
    before: Set<string>;
    succeeded: boolean;
  } | null>(null);
  React.useEffect(() => {
    const current = pending.current;
    if (!current) return;
    if (pollingState === "success") current.succeeded = true;
    if (pollingState === "error") {
      pending.current = null;
      return;
    }
    if (!current.succeeded) return;
    const added = accounts.find((a) => !current.before.has(a.id));
    if (added) {
      pending.current = null;
      onChange(added.id);
      return;
    }
    if (pollingState === "idle") {
      pending.current = null;
      const latest = [...accounts].sort(
        (a, b) => b.authenticated_at - a.authenticated_at,
      )[0];
      if (latest) onChange(latest.id);
    }
  }, [pollingState, accounts, onChange]);

  const startLogin = () => {
    pending.current = {
      before: new Set(accounts.map((a) => a.id)),
      succeeded: false,
    };
    addAccount();
  };

  return (
    <div className="space-y-2">
      <Label htmlFor="claude-official-account">
        {t("provider.form.claudeAccount.label", { defaultValue: "账号" })}
      </Label>
      <div className="flex items-center gap-2">
        <Select
          value={value ?? DEFAULT_LOGIN}
          onValueChange={(next) => {
            // Radix Select 在表单里可能回报空串，忽略
            if (!next) return;
            onChange(next === DEFAULT_LOGIN ? null : next);
          }}
        >
          <SelectTrigger id="claude-official-account" className="flex-1">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={DEFAULT_LOGIN}>
              {t("provider.form.claudeAccount.default", {
                defaultValue: "默认登录（~/.claude）",
              })}
            </SelectItem>
            {accounts.map((account) => (
              <SelectItem
                key={account.id}
                value={account.id}
                disabled={account.requires_reauth}
              >
                {account.login}
              </SelectItem>
            ))}
            {missing && (
              <SelectItem value={value}>
                {t("provider.form.claudeAccount.missing", {
                  defaultValue: "不在这台电脑上的账号",
                })}
              </SelectItem>
            )}
          </SelectContent>
        </Select>
        <Button
          type="button"
          variant="neutral"
          size="compact"
          className="gap-1.5 pe-3 ps-2.5"
          disabled={busy}
          onClick={startLogin}
        >
          <Plus className="h-3.5 w-3.5" strokeWidth={2} />
          {t("provider.form.claudeAccount.add", {
            defaultValue: "登录新账号",
          })}
        </Button>
      </div>
      {busy && (
        <div className="flex flex-col gap-2 rounded-[8px] bg-subtle px-3.5 py-3">
          <div className="flex items-center gap-2">
            <Loader2
              aria-hidden="true"
              className="h-[15px] w-[15px] shrink-0 text-fg-2 motion-safe:animate-spin"
            />
            <span className="min-w-0 flex-1 text-caption text-fg-1">
              {t("provider.form.claudeAccount.pending", {
                defaultValue:
                  "已在浏览器打开 Claude 的登录页，登录并授权后会自动选上这个账号。",
              })}
            </span>
            <Button
              type="button"
              variant="neutral"
              size="compact"
              onClick={cancelAuth}
            >
              {t("common.cancel", { defaultValue: "取消" })}
            </Button>
          </div>
          {deviceCode && (
            <ClaudePasteCode
              key={deviceCode.device_code}
              flowId={deviceCode.device_code}
            />
          )}
        </div>
      )}
      {!busy && pollingState === "error" && error && (
        <p className="text-caption text-danger-text [overflow-wrap:anywhere]">
          {error}
        </p>
      )}
      <p className="text-caption text-fg-2">
        {isMac()
          ? t("provider.form.claudeAccount.helpMac", {
              defaultValue:
                "macOS 上请从卡片「打开终端」使用选中的账号：直接切换这张卡不会改变其他终端里 claude 用的号。",
            })
          : t("provider.form.claudeAccount.help", {
              defaultValue:
                "切到这张卡后，新开的 claude 都用选中的账号；已经开着的会话不受影响。",
            })}
      </p>
    </div>
  );
}

/**
 * 绑了账号的 Claude 官方卡底部：写出用的是哪个号（额度查的是默认登录，对不上，先不显示）。
 * macOS 上切卡不会全局换号，提示从卡片打开终端。
 */
export function ClaudeBoundAccountFooter({ accountId }: { accountId: string }) {
  const { t } = useTranslation();
  const { data, isSuccess } = useQuery({
    queryKey: ["managed-auth-status", "claude_official"],
    queryFn: () => authApi.authGetStatus("claude_official"),
    staleTime: 30000,
  });
  const account = data?.accounts.find((a) => a.id === accountId);
  if (!isSuccess) return null;
  const label = account
    ? account.login
    : t("provider.form.claudeAccount.missing", {
        defaultValue: "不在这台电脑上的账号",
      });
  return (
    <span className="min-w-0 truncate text-caption text-fg-2" title={label}>
      {isMac()
        ? t("provider.form.claudeAccount.cardMac", {
            defaultValue: "账号：{{label}} · 从卡片打开终端使用",
            label,
          })
        : t("provider.form.claudeAccount.card", {
            defaultValue: "账号：{{label}}",
            label,
          })}
    </span>
  );
}
